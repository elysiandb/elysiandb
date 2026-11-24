use std::collections::BTreeMap;

use elysiandb::api;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::strings;

use super::evaluate_test::wait_for_indexes;
use super::{filters, ids, init_test_store, setup};

#[test]
fn test_searches_and_search_counts_match_the_scan() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(5));
    let mut all: Vec<Map> = Vec::new();
    for i in 0..4000 {
        let mut d = random_search_doc(&mut r, 1);
        d.insert(b"id".to_vec(), Gen::Str(format!("s{i:04}").into_bytes()));
        d.insert(
            b"kind".to_vec(),
            Gen::Str(["film", "serie"][i % 2].as_bytes().to_vec()),
        );
        let mut raw = Vec::new();
        marshal_gen(&Gen::Obj(d), &mut raw);
        all.push(decode(&raw));
    }
    api::write_list_of_entities(b"searched", &mut all);
    let reference = api::list_entities(b"searched", 0, 0, b"", true, None, b"", b"");
    api::list_entities(b"searched", 1, 0, b"", true, None, b"alpha", b"");
    let kind = filters(&[("kind", &[("eq", "film")])]);
    api::list_entities(b"searched", 1, 0, b"", true, Some(&kind), b"", b"");
    wait_for_indexes();

    for search in [
        "alpha",
        "été",
        "ÉTÉ",
        "<b>",
        "a&b",
        "smile",
        "dune",
        "nnn42",
        "*",
        "zzz",
        "back\\\\slash",
    ] {
        for f in [None, Some(&kind)] {
            let want: Vec<String> = ids(&reference
                .iter()
                .filter(|d| {
                    api::search_matches_entity(d, search.as_bytes())
                        && f.is_none_or(|f| api::filters_match_entity(d, f))
                })
                .cloned()
                .collect::<Vec<_>>());
            let got = ids(&api::list_entities(
                b"searched",
                0,
                0,
                b"",
                true,
                f,
                search.as_bytes(),
                b"",
            ));
            if got != want {
                panic!(
                    "search {search:?} filters {f:?}: planner {} documents, scan {}",
                    got.len(),
                    want.len()
                );
            }
            let res = api::plan(api::ListQuery {
                entity: b"searched",
                search: search.as_bytes(),
                filters: f,
                count_only: true,
                ..api::ListQuery::default()
            });
            if res.total != want.len() {
                panic!(
                    "search {search:?} filters {f:?}: count {}, scan {}",
                    res.total,
                    want.len()
                );
            }
        }
    }
}

pub const SEARCH_FRAGMENTS: [&[u8]; 18] = [
    b"alpha",
    "ÉTÉ".as_bytes(),
    "été".as_bytes(),
    b"<b>",
    b"a&b",
    b"quo\"te",
    b"back\\slash",
    b"tab\there",
    "line\u{2028}sep".as_bytes(),
    "😀 smile".as_bytes(),
    b"x\x01y",
    b"\xff\xfe",
    b"nnn",
    b"Dune",
    b"dune",
    b"*",
    b"",
    b"42",
];

pub enum Gen {
    Str(Vec<u8>),
    Num(f64),
    Bool(bool),
    Nil,
    Arr(Vec<Gen>),
    Obj(BTreeMap<Vec<u8>, Gen>),
}

enum GenFrame<'a> {
    Arr(std::slice::Iter<'a, Gen>, bool),
    Obj(std::collections::btree_map::Iter<'a, Vec<u8>, Gen>, bool),
}

fn open_gen<'a>(g: &'a Gen, out: &mut Vec<u8>, stack: &mut Vec<GenFrame<'a>>) {
    match g {
        Gen::Str(s) => json::append_string(out, s, true),
        Gen::Num(f) => out.extend_from_slice(&json::marshal(&Value::Number(*f)).expect("number")),
        Gen::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Gen::Nil => out.extend_from_slice(b"null"),
        Gen::Arr(items) => {
            out.push(b'[');
            stack.push(GenFrame::Arr(items.iter(), true));
        }
        Gen::Obj(m) => {
            out.push(b'{');
            stack.push(GenFrame::Obj(m.iter(), true));
        }
    }
}

pub fn marshal_gen(g: &Gen, out: &mut Vec<u8>) {
    let mut stack: Vec<GenFrame<'_>> = Vec::new();
    open_gen(g, out, &mut stack);
    while let Some(top) = stack.last_mut() {
        let next = match top {
            GenFrame::Arr(it, first) => match it.next() {
                Some(item) => {
                    if !*first {
                        out.push(b',');
                    }
                    *first = false;
                    Some(item)
                }
                None => {
                    out.push(b']');
                    None
                }
            },
            GenFrame::Obj(it, first) => match it.next() {
                Some((k, v)) => {
                    if !*first {
                        out.push(b',');
                    }
                    *first = false;
                    json::append_string(out, k, true);
                    out.push(b':');
                    Some(v)
                }
                None => {
                    out.push(b'}');
                    None
                }
            },
        };
        match next {
            Some(item) => open_gen(item, out, &mut stack),
            None => {
                stack.pop();
            }
        }
    }
}

fn concat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = a.to_vec();
    out.extend_from_slice(b);
    out
}

fn random_search_value(r: &mut Rand, depth: i64) -> Gen {
    match r.intn(7) {
        0..=2 => {
            let a = SEARCH_FRAGMENTS[r.intn(SEARCH_FRAGMENTS.len() as i64) as usize];
            let b = SEARCH_FRAGMENTS[r.intn(SEARCH_FRAGMENTS.len() as i64) as usize];
            Gen::Str(concat(a, b))
        }
        3 => Gen::Num(r.intn(100) as f64),
        4 => {
            if depth > 2 {
                return Gen::Bool(true);
            }
            let mut arr = Vec::new();
            let mut i = r.intn(3);
            while i > 0 {
                arr.push(random_search_value(r, depth + 1));
                i -= 1;
            }
            Gen::Arr(arr)
        }
        5 => {
            if depth > 2 {
                return Gen::Nil;
            }
            Gen::Obj(random_search_doc(r, depth + 1))
        }
        _ => Gen::Nil,
    }
}

pub fn random_search_doc(r: &mut Rand, depth: i64) -> BTreeMap<Vec<u8>, Gen> {
    let mut doc = BTreeMap::new();
    let keys: [&[u8]; 8] = [
        b"id",
        b"@entity",
        b"_elysiandb_core_owner",
        b"title",
        b"notes",
        b"tags",
        b"Dune",
        b"k\"ey",
    ];
    let mut i = 1 + r.intn(4);
    while i > 0 {
        let key = keys[r.intn(keys.len() as i64) as usize];
        let v = random_search_value(r, depth);
        doc.insert(key.to_vec(), v);
        i -= 1;
    }
    doc
}

fn random_search_pattern(r: &mut Rand) -> Vec<u8> {
    let mut frag = SEARCH_FRAGMENTS[r.intn(SEARCH_FRAGMENTS.len() as i64) as usize].to_vec();
    if frag.len() > 2 && r.intn(2) == 0 {
        frag = frag[1..frag.len() - 1].to_vec();
    }
    match r.intn(5) {
        0 => concat(&concat(b"*", &frag), b"*"),
        1 => concat(&frag, b"*"),
        2 => strings::to_upper(&frag),
        3 => concat(b"\\", &frag),
        _ => frag,
    }
}

fn decode(raw: &[u8]) -> Map {
    match &mut json::unmarshal_value(raw) {
        Ok(Value::Object(m)) => std::mem::take(m),
        other => panic!("{}: {other:?}", String::from_utf8_lossy(raw)),
    }
}

#[test]
fn test_search_on_stored_bytes_matches_the_decoded_search() {
    let _s = setup();
    let mut r = Rand::new(rand::new_source(11));
    for _ in 0..20000 {
        let mut raw = Vec::new();
        marshal_gen(&Gen::Obj(random_search_doc(&mut r, 0)), &mut raw);
        let doc = decode(&raw);

        let pattern = random_search_pattern(&mut r);
        let (got, ok) = api::search_matches_bytes(&raw, &pattern);
        let want = api::search_matches_entity(&doc, &pattern);
        if !ok || got != want {
            panic!(
                "search {:?} on {}: bytes {got} (ok {ok}), decoded {want}",
                String::from_utf8_lossy(&pattern),
                String::from_utf8_lossy(&raw)
            );
        }
    }
}

#[test]
fn test_search_on_hand_written_json() {
    let _s = setup();
    let cases: [(&str, &str); 10] = [
        (r#"{"a":"\/path\/to"}"#, "/path/to"),
        (r#"{"a":"café"}"#, "café"),
        (r#"{"a":"CAFÉ"}"#, "café"),
        (r#"{"a":"😀 smile"}"#, "😀"),
        (r#"{"a":"lone \ud800 surrogate"}"#, "lone \u{FFFD}"),
        (r#"{"a":"lone \ud800A"}"#, "\u{FFFD}A"),
        (
            r#" { "a" : [ 1 , "x\tz" , { "b" : "deep" } ] , "id" : "deep" } "#,
            "deep",
        ),
        (r#"{"id":"only id"}"#, "only"),
        (r#"{"a":{"@entity":"hidden","n":null}}"#, "hidden"),
        (r#"{"a":"x\u0000y"}"#, "x\u{0}y"),
    ];
    for (raw, search) in cases {
        let doc = decode(raw.as_bytes());
        let (got, ok) = api::search_matches_bytes(raw.as_bytes(), search.as_bytes());
        let want = api::search_matches_entity(&doc, search.as_bytes());
        if !ok || got != want {
            panic!("search {search:?} on {raw}: bytes {got} (ok {ok}), decoded {want}");
        }
    }

    let broken: [&[u8]; 6] = [
        br#"{"a":"x"#,
        br#"{"a":"\x"}"#,
        br#"{"a" "x"}"#,
        br#"["a"]"#,
        br#"{"a":"\u12"}"#,
        br#"{"a":"it\'s"}"#,
    ];
    for raw in broken {
        let (_, ok) = api::search_matches_bytes(raw, b"a");
        if ok {
            panic!(
                "invalid JSON {} falls back to decoding",
                String::from_utf8_lossy(raw)
            );
        }
    }
}
