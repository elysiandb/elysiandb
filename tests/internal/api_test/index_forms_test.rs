use elysiandb::api::{self, ListQuery};
use elysiandb::datastore::{INDEX_BITMAP, IndexSpec};
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::query::{FilterNode, Query};
use elysiandb::storage;

use super::index_test::{obj, random_leaf, random_value, s};
use super::{ids, init_test_store, setup};

pub fn random_diff_doc(r: &mut Rand, id: &str) -> Map {
    let mut d = obj(vec![("id", s(id))]);
    for f in ["a", "b", "m", "list"] {
        if r.intn(6) == 0 {
            continue;
        }
        let v = match f {
            "m" => {
                let x = random_value(r, 1);
                let y = random_value(r, 1);
                Value::Object(obj(vec![("x", x), ("y", y)]))
            }
            "list" => {
                let a = random_value(r, 1);
                let b = random_value(r, 1);
                Value::Array(vec![
                    Value::Object(obj(vec![("x", a)])),
                    Value::Object(obj(vec![("x", b)])),
                ])
            }
            _ => random_value(r, 0),
        };
        d.insert(f.to_string(), v);
    }
    d
}

pub fn check_planner_against_scan(r: &mut Rand, entity: &str, rounds: usize) {
    let reference = api::list_entities(entity.as_bytes(), 0, 0, b"", true, None, b"", b"");
    let scan = |m: &dyn Fn(&Map) -> bool| -> Vec<String> {
        ids(&reference
            .iter()
            .filter(|d| m(d))
            .cloned()
            .collect::<Vec<_>>())
    };

    for _ in 0..rounds {
        let f = random_leaf(r);
        let want = scan(&|d| api::filters_match_entity(d, &f));
        let got = ids(&api::list_entities(
            entity.as_bytes(),
            0,
            0,
            b"",
            true,
            Some(&f),
            b"",
            b"",
        ));
        if got != want {
            panic!("filters {f:?}: planner {got:?}, scan {want:?}");
        }
        let res = api::plan(ListQuery {
            entity: entity.as_bytes(),
            filters: Some(&f),
            count_only: true,
            ..ListQuery::default()
        });
        if res.total != want.len() {
            panic!("filters {f:?}: count {}, scan {}", res.total, want.len());
        }
    }

    for _ in 0..rounds / 5 {
        let first = random_leaf(r);
        let second = random_leaf(r);
        let third = random_leaf(r);
        let node = FilterNode::with_or(vec![
            FilterNode::with_leaf(first),
            FilterNode::with_and(vec![
                FilterNode::with_leaf(second),
                FilterNode::with_leaf(third),
            ]),
        ]);
        let want = scan(&|d| api::query_filter_matches_entity(&node, d));
        let q = Query {
            entity: entity.as_bytes().to_vec(),
            filter: node,
            ..Query::default()
        };
        let got = ids(&api::plan_query(&q, false).decode());
        if got != want {
            panic!("node: planner {got:?}, scan {want:?}");
        }
    }
}

#[test]
fn test_bitmap_indexes_answer_like_ordered_indexes() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(7));
    let mut docs: Vec<Map> = (0..9000)
        .map(|i| random_diff_doc(&mut r, &format!("q{i:05}")))
        .collect();
    api::write_list_of_entities(b"forms", &mut docs);
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    for f in ["a", "b", "m.x", "m.y", "list.x"] {
        if !e.ensure_index(b"forms", &IndexSpec::new(INDEX_BITMAP, f)) {
            panic!("bitmap index on {f} not built");
        }
    }
    check_planner_against_scan(&mut r, "forms", 150);
    for _ in 0..3000 {
        let id = format!("q{:05}", r.intn(9000));
        match r.intn(3) {
            0 => api::delete_entity_by_id(b"forms", id.as_bytes()),
            _ => {
                let mut d = random_diff_doc(&mut r, &id);
                api::write_entity(b"forms", &mut d);
            }
        }
    }
    check_planner_against_scan(&mut r, "forms", 150);
    for name in e.index_names(b"forms").unwrap_or_default() {
        if name.first() == Some(&b'f') {
            panic!(
                "the planner should read the bitmap indexes, yet built {}",
                String::from_utf8_lossy(&name)
            );
        }
    }
}
