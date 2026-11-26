use elysiandb::api::{self, ListQuery};
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::{self, Time};
use elysiandb::query::{Filters, Query, Sort};
use elysiandb::storage;
use elysiandb::transport::http::api::parse_filter_node;

use super::common::{read_jsonl, str_field, unhex};
use super::{boolean, integer, open_store, text};

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn planner_doc(i: i64) -> Map {
    let tags: Vec<Value> = ["red", "green", "blue", "x"][..(i % 5 % 4) as usize]
        .iter()
        .map(|t| s(t))
        .collect();
    let mut author = Map::new();
    author.insert("@entity".to_string(), s("writer"));
    author.insert("id".to_string(), s(&format!("w{:02}", i % 23)));
    let date = Time::from_unix(1_700_000_000 + i * 3600, 0)
        .utc()
        .format(time::RFC3339);
    let mut d = Map::new();
    d.insert("id".to_string(), s(&format!("d{i:05}")));
    d.insert("title".to_string(), s(&format!("Title {:03}", i % 200)));
    d.insert("num".to_string(), Value::Number((i % 97) as f64));
    d.insert(
        "status".to_string(),
        s(["open", "closed", "draft", "review"][(i % 4) as usize]),
    );
    d.insert("tags".to_string(), Value::Array(tags));
    d.insert("date".to_string(), s(&date));
    d.insert("author".to_string(), Value::Object(author));
    d
}

struct Spec {
    limit: i64,
    offset: i64,
    filters: Option<Filters>,
    sort: Option<(&'static str, bool)>,
    sorts: Option<Vec<Sort>>,
    search: &'static str,
    includes: &'static str,
    count: bool,
    node: Option<&'static str>,
}

const BASE: Spec = Spec {
    limit: 0,
    offset: 0,
    filters: None,
    sort: None,
    sorts: None,
    search: "",
    includes: "",
    count: false,
    node: None,
};

fn f(spec: &[(&str, &[(&str, &str)])]) -> Option<Filters> {
    Some(
        spec.iter()
            .map(|(field, ops)| {
                (
                    field.as_bytes().to_vec(),
                    ops.iter()
                        .map(|(o, v)| (o.as_bytes().to_vec(), v.as_bytes().to_vec()))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn specs() -> Vec<Spec> {
    vec![
        BASE,
        Spec { limit: 10, ..BASE },
        Spec {
            offset: 5,
            limit: 7,
            ..BASE
        },
        Spec { limit: -1, ..BASE },
        Spec { offset: -3, ..BASE },
        Spec {
            filters: f(&[("status", &[("eq", "open")])]),
            ..BASE
        },
        Spec {
            filters: f(&[("status", &[("eq", "open")]), ("num", &[("gt", "50")])]),
            sort: Some(("num", false)),
            limit: 20,
            ..BASE
        },
        Spec {
            filters: f(&[("title", &[("eq", "title 01*")])]),
            ..BASE
        },
        Spec {
            filters: f(&[("tags", &[("any", "red,blue")])]),
            count: true,
            ..BASE
        },
        Spec {
            filters: f(&[("date", &[("gte", "2023-11-20")])]),
            sort: Some(("date", true)),
            limit: 15,
            ..BASE
        },
        Spec {
            filters: f(&[("author.country", &[("eq", "jp")])]),
            limit: 25,
            ..BASE
        },
        Spec {
            search: "*title 1*",
            limit: 30,
            ..BASE
        },
        Spec {
            sort: Some(("title", true)),
            offset: 10,
            limit: 10,
            ..BASE
        },
        Spec {
            sorts: Some(vec![Sort::new("status", true), Sort::new("num", false)]),
            limit: 40,
            ..BASE
        },
        Spec {
            includes: "author",
            limit: 5,
            ..BASE
        },
        Spec {
            count: true,
            ..BASE
        },
        Spec {
            count: true,
            limit: 3,
            ..BASE
        },
        Spec {
            node: Some(r#"{"or":[{"status":{"eq":"draft"}},{"num":{"lt":"3"}}]}"#),
            limit: 50,
            ..BASE
        },
        Spec {
            node: Some(
                r#"{"and":[{"status":{"eq":"open"}},{"or":[{"num":{"eq":"4"}},{"num":{"eq":"8"}}]}]}"#,
            ),
            ..BASE
        },
    ]
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(items)) => items.iter().map(|i| text(i).to_string()).collect(),
        _ => Vec::new(),
    }
}

fn utf8(ids: Vec<Vec<u8>>) -> Vec<String> {
    ids.into_iter()
        .map(|id| String::from_utf8_lossy(&id).into_owned())
        .collect()
}

fn map_ids(docs: &[Map]) -> Vec<String> {
    docs.iter()
        .map(|d| {
            d.get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn out_bytes(row: &Value) -> Vec<u8> {
    match (row.get("out"), row.get("out_hex")) {
        (Some(Value::String(o)), _) => o.as_bytes().to_vec(),
        (_, Some(Value::String(h))) => unhex(h),
        _ => panic!("{row:?}: no output"),
    }
}

#[test]
fn planner_corpus() {
    let rows = read_jsonl("api/planner_corpus.jsonl");
    let mut next = 0;
    let mut checked = 0;
    for size in [100i64, 3000, 9000] {
        let _store = open_store(false, false, false);
        for i in 0..23 {
            let mut w = Map::new();
            w.insert("id".to_string(), s(&format!("w{i:02}")));
            w.insert("name".to_string(), s(&format!("Writer {i}")));
            w.insert("country".to_string(), s(["fr", "us", "jp"][i % 3]));
            api::write_entity(b"writer", &mut w);
        }
        let entity = format!("e{size}");
        let mut batch: Vec<Map> = Vec::new();
        for i in 0..size {
            batch.push(planner_doc(i));
            if batch.len() == 500 || i == size - 1 {
                api::write_list_of_entities(entity.as_bytes(), &mut batch);
                batch = Vec::new();
            }
        }

        for (qi, spec) in specs().into_iter().enumerate() {
            let row = &rows[next];
            next += 1;
            if integer(row, "size") != size || integer(row, "case") != qi as i64 {
                panic!("unexpected row order at {qi}: {row:?}");
            }
            let node = spec.node.map(|raw| {
                let mut v = json::unmarshal_value(raw.as_bytes()).expect("node");
                let Value::Object(m) = &mut v else {
                    panic!("node must be an object");
                };
                parse_filter_node(Some(m)).expect("valid node")
            });
            let (sort_field, sort_asc) = spec.sort.unwrap_or(("", false));
            let res = api::plan(ListQuery {
                entity: entity.as_bytes(),
                limit: spec.limit,
                offset: spec.offset,
                filters: spec.filters.as_ref(),
                sort_field: sort_field.as_bytes(),
                sort_asc,
                sorts: spec.sorts.clone().unwrap_or_default(),
                search: spec.search.as_bytes(),
                includes: spec.includes.as_bytes(),
                count_only: spec.count,
                node: node.as_ref(),
                ..ListQuery::default()
            });
            let got = utf8(res.ids());
            if got != strings(row.get("ids")) || res.total as i64 != integer(row, "total") {
                panic!("size {size} case {qi}: ids {got:?} total {}", res.total);
            }
            if node.is_none() && spec.sorts.is_none() {
                let listed = map_ids(&api::list_entities(
                    entity.as_bytes(),
                    spec.limit,
                    spec.offset,
                    sort_field.as_bytes(),
                    sort_asc,
                    spec.filters.as_ref(),
                    spec.search.as_bytes(),
                    spec.includes.as_bytes(),
                ));
                if listed != strings(row.get("list_entities")) {
                    panic!("size {size} case {qi}: ListEntities {listed:?}");
                }
            }
            if let Some(node) = node {
                let q = Query {
                    entity: entity.as_bytes().to_vec(),
                    offset: spec.offset,
                    limit: spec.limit,
                    filter: node,
                    ..Query::default()
                };
                let (docs, err) = api::execute_query(&q);
                if map_ids(&docs) != strings(row.get("execute_query"))
                    || err.is_some() != row.get("execute_query_error").is_some_and(boolean)
                {
                    panic!("size {size} case {qi}: ExecuteQuery {:?}", map_ids(&docs));
                }
            }
            checked += 1;
        }

        for (field, asc) in [
            ("", true),
            ("num", true),
            ("num", false),
            ("title", true),
            ("missing", true),
        ] {
            let row = &rows[next];
            next += 1;
            if str_field(row, "fn") != "GetListOfIds" || str_field(row, "field") != field {
                panic!("unexpected row {row:?}");
            }
            let res = api::get_list_of_ids(entity.as_bytes(), field.as_bytes(), asc);
            if res.is_err() != row.get("error").is_some_and(boolean) {
                panic!("{row:?}: {:?}", res.is_err());
            }
            if let Ok(out) = res
                && out != out_bytes(row)
            {
                panic!("{row:?}: {}", String::from_utf8_lossy(&out));
            }
            checked += 1;
        }

        for key in [
            "api:entity:types:list".to_string(),
            format!("api:entity:{entity}:internal:index:id"),
            format!("api:entity:{entity}:internal:index:fields:all"),
            format!("api:entity:{entity}:id:d00001"),
            "api:entity:writer:id:w01".to_string(),
            "api:entity:nothing:id:x".to_string(),
            "plain-key".to_string(),
        ] {
            let row = &rows[next];
            next += 1;
            if str_field(row, "fn") != "GetByKey" || str_field(row, "key") != key {
                panic!("unexpected row {row:?}");
            }
            let res = storage::get_by_key(key.as_bytes());
            if res.is_err() != row.get("error").is_some_and(boolean) {
                panic!("{row:?}: {:?}", res.is_err());
            }
            if let Ok(out) = res
                && out != out_bytes(row)
            {
                panic!("{row:?}: {}", String::from_utf8_lossy(&out));
            }
            checked += 1;
        }
    }

    assert_eq!(checked, 93);
    assert_eq!(checked, rows.len());
}
