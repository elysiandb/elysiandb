use elysiandb::api;
use elysiandb::gocompat::json::{self, Map, Value};

use super::common::{read_jsonl, str_field};
use super::{array, go_filters, go_maps, open_store, text};

const DOCS: &str = r#"[{"id":"d1","author":{"@entity":"person","id":"p1"},"co":[{"@entity":"person","id":"p2"},"text",1,{"plain":true},{"@entity":"person","id":"zz"}],"org":{"@entity":"org","id":"o1"},"core":{"@entity":"_elysiandb_core_user","id":"admin"},"tags":["a","b"],"n":5},{"id":"d2","author":{"@entity":"person","id":"missing"},"chain":{"@entity":"chain","id":"c0"}},{"id":"d3"}]"#;

fn link(entity: &str, id: &str) -> Value {
    let mut m = Map::new();
    m.insert("@entity".to_string(), Value::String(entity.to_string()));
    m.insert("id".to_string(), Value::String(id.to_string()));
    Value::Object(m)
}

fn write(entity: &str, pairs: Vec<(&str, Value)>) {
    let mut m: Map = pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    api::write_entity(entity.as_bytes(), &mut m);
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

#[test]
fn include_corpus() {
    let rows = read_jsonl("api/include_corpus.jsonl");
    let _store = open_store(false, false, false);
    write(
        "person",
        vec![
            ("id", s("p1")),
            ("name", s("Ann")),
            ("friend", link("person", "p2")),
        ],
    );
    write(
        "person",
        vec![
            ("id", s("p2")),
            ("name", s("Bob")),
            ("friend", link("person", "p1")),
        ],
    );
    write(
        "chain",
        vec![("id", s("c0")), ("next", link("chain", "c1"))],
    );
    for i in 1..40 {
        write(
            "chain",
            vec![
                ("id", s(&format!("c{i}"))),
                ("next", link("chain", &format!("c{}", i + 1))),
            ],
        );
    }
    write(
        "org",
        vec![
            ("id", s("o1")),
            ("name", s("Org")),
            ("owner", link("person", "p1")),
        ],
    );

    let mut checked = 0;
    for row in &rows {
        match str_field(row, "fn") {
            "ApplyIncludes" => {
                let mut data = go_maps(DOCS.as_bytes());
                api::apply_includes(&mut data, str_field(row, "includes").as_bytes());
                let out = String::from_utf8(json::marshal(&data).expect("encode")).expect("utf-8");
                if out != str_field(row, "out") {
                    panic!("includes {:?}: got {out}", str_field(row, "includes"));
                }
            }
            "ExtractAutoIncludes" => {
                let filters = go_filters(row.get("filters").expect("filters"));
                let auto = api::extract_auto_includes(&filters);
                let mut parts: Vec<String> = auto
                    .split(|&c| c == b',')
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect();
                parts.sort();
                let want: Vec<&str> = array(row, "out_set").iter().map(text).collect();
                if parts != want {
                    panic!("{row:?}: got {parts:?}");
                }
            }
            "MergeIncludes" => {
                let out = api::merge_includes(
                    str_field(row, "a").as_bytes(),
                    str_field(row, "b").as_bytes(),
                );
                if out != str_field(row, "out").as_bytes() {
                    panic!("{row:?}: got {}", String::from_utf8_lossy(&out));
                }
            }
            other => panic!("unknown function {other}"),
        }
        checked += 1;
    }

    assert_eq!(checked, 33);
    assert_eq!(checked, rows.len());
}
