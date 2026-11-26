mod common;

use common::{b, i, opt_s, read_jsonl, s};
use gocompat::json::Value;
use gocompat::unicode::{self, RangeTable};
use talos::deps::xtext::rangetable::merge;
use talos::parser::{WHITESPACE_CHARS, is_identifier, transform_reg_exp};
use talos::token::{self, Token};

fn table_json(t: &RangeTable) -> (Vec<[i64; 3]>, Vec<[i64; 3]>, usize) {
    (
        t.r16
            .iter()
            .map(|r| [i64::from(r.lo), i64::from(r.hi), i64::from(r.stride)])
            .collect(),
        t.r32
            .iter()
            .map(|r| [i64::from(r.lo), i64::from(r.hi), i64::from(r.stride)])
            .collect(),
        t.latin_offset,
    )
}

fn triples(v: &Value) -> Vec<[i64; 3]> {
    v.as_array()
        .expect("ranges")
        .iter()
        .map(|r| {
            let r = r.as_array().expect("range");
            [
                r[0].as_i64().expect("lo"),
                r[1].as_i64().expect("hi"),
                r[2].as_i64().expect("stride"),
            ]
        })
        .collect()
}

#[test]
fn rangetable_merge_follows_x_text() {
    let id_start = merge(&[&unicode::LETTER, &unicode::NL, &unicode::OTHER_ID_START]);
    let tables: Vec<(&str, RangeTable)> = vec![
        (
            "id_neg",
            merge(&[&unicode::PATTERN_SYNTAX, &unicode::PATTERN_WHITE_SPACE]),
        ),
        (
            "id_cont",
            merge(&[
                &id_start,
                &unicode::MN,
                &unicode::MC,
                &unicode::ND,
                &unicode::PC,
                &unicode::OTHER_ID_CONTINUE,
            ]),
        ),
        ("empty", merge(&[])),
        (
            "latin_greek",
            merge(&[&unicode::LATIN, &unicode::GREEK, &unicode::ND]),
        ),
        ("upper_lower", merge(&[&unicode::UPPER, &unicode::LOWER])),
        ("self", merge(&[&unicode::LU, &unicode::LU])),
        ("id_start", id_start),
    ];
    let rows = read_jsonl("talos-front/rangetable/merge.jsonl");
    assert_eq!(rows.len(), tables.len());
    for row in &rows {
        let name = s(row, "name");
        let t = &tables.iter().find(|(n, _)| *n == name).expect("table").1;
        let want = row.get("table").expect("table");
        let (r16, r32, latin) = table_json(t);
        assert_eq!(r16, triples(want.get("r16").expect("r16")), "{name} r16");
        assert_eq!(r32, triples(want.get("r32").expect("r32")), "{name} r32");
        assert_eq!(latin as i64, i(want, "latin_offset"), "{name} latin offset");
    }
}

#[test]
fn identifier_characters_follow_go() {
    for row in &read_jsonl("talos-front/rangetable/identifiers.jsonl") {
        let kind = s(row, "kind");
        let mut got: Vec<[i64; 2]> = Vec::new();
        let mut count = 0;
        for r in 0x80u32..=0x10FFFF {
            let c = char::from_u32(r).unwrap_or(char::REPLACEMENT_CHARACTER);
            let mut buf = String::new();
            if kind == "part" {
                buf.push('a');
            }
            buf.push(c);
            if is_identifier(buf.as_bytes()) {
                count += 1;
                match got.last_mut() {
                    Some(last) if last[1] + 1 == i64::from(r) => last[1] = i64::from(r),
                    _ => got.push([i64::from(r), i64::from(r)]),
                }
            }
        }
        assert_eq!(count, i(row, "count"), "{kind}");
        let want: Vec<[i64; 2]> = row
            .get("ranges")
            .and_then(Value::as_array)
            .expect("ranges")
            .iter()
            .map(|p| {
                let p = p.as_array().expect("pair");
                [p[0].as_i64().expect("lo"), p[1].as_i64().expect("hi")]
            })
            .collect();
        assert_eq!(got, want, "{kind}");
    }
}

#[test]
fn tokens_follow_go() {
    for row in &read_jsonl("talos-front/token/tokens.jsonl") {
        if let Some(word) = opt_s(row, "word") {
            let (t, strict) = token::is_keyword(word.as_bytes());
            assert_eq!(t.0, i(row, "t"), "{word:?}");
            assert_eq!(strict, b(row, "strict"), "{word:?}");
            continue;
        }
        let t = Token(i(row, "t"));
        assert_eq!(t.string(), s(row, "s"), "{t:?}");
        assert_eq!(token::is_id(t), b(row, "is_id"), "{t:?}");
        assert_eq!(token::is_unreserved_word(t), b(row, "unreserved"), "{t:?}");
    }
}

#[test]
fn transform_reg_exp_follows_go() {
    for row in &read_jsonl("talos-front/regexp/transform.jsonl") {
        let pattern = s(row, "pattern");
        let dot_all = b(row, "dot_all");
        let uni = b(row, "unicode");
        let ctx = format!("{pattern:?} dotAll={dot_all} unicode={uni}");
        match transform_reg_exp(pattern.as_bytes(), dot_all, uni) {
            Ok(res) => assert_eq!(
                Some(String::from_utf8_lossy(&res).as_ref()),
                opt_s(row, "result"),
                "{ctx}"
            ),
            Err(e) if e.is_incompatible() => {
                assert_eq!(Some(e.message()), opt_s(row, "incompatible"), "{ctx}")
            }
            Err(e) => assert_eq!(Some(e.message()), opt_s(row, "syntax"), "{ctx}"),
        }
    }
    assert!(WHITESPACE_CHARS.starts_with(' '));
}
