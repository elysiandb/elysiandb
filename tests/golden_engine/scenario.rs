use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common;
use crate::keys::{hex_value, obj, same_json};
use crate::tempdir::TempDir;
use elysiandb::datastore::{
    self, Bitset, Engine, INDEX_BITMAP, INDEX_COMPOSITE, Options, Snapshot,
};
use elysiandb::durable::filepath;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::journal;

const LSNS: [u64; 6] = [0, 1, 50, 130, 160, 1 << 40];

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    common::str_field(row, key)
}

fn strs(list: &[Vec<u8>]) -> Value {
    Value::Array(
        list.iter()
            .map(|x| Value::String(String::from_utf8_lossy(x).into_owned()))
            .collect(),
    )
}

fn hexes(list: &[Vec<u8>]) -> Value {
    Value::Array(list.iter().map(|x| hex_value(x)).collect())
}

fn ints_or_null(list: &[u32]) -> Value {
    if list.is_empty() {
        return Value::Null;
    }
    Value::Array(list.iter().map(|&x| Value::Int(i64::from(x))).collect())
}

fn bits_of(set: &Bitset) -> Vec<u32> {
    let mut out = Vec::new();
    set.for_each(|i| {
        out.push(i);
        true
    });
    out
}

fn entries_of(v: &datastore::IndexView) -> Value {
    let mut entries: Vec<Value> = Vec::new();
    v.scan_range(None, None, |key, seq| {
        entries.push(Value::Array(vec![
            hex_value(key),
            Value::Int(i64::from(seq)),
        ]));
        true
    });
    if entries.is_empty() {
        Value::Null
    } else {
        Value::Array(entries)
    }
}

fn wait_indexes(e: &Engine) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let busy = e
            .index_states()
            .iter()
            .any(|st| st.state != "ready" && st.state != "dormant");
        if !busy && e.index_backlog() == 0 {
            return;
        }
        assert!(Instant::now() < deadline, "indexes did not settle");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn dump(e: &Arc<Engine>, step: &str, indexed: bool) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut put = |entity: &[u8], what: &str, v: Value| {
        rows.push(obj(vec![
            ("step", Value::String(step.into())),
            (
                "entity",
                Value::String(String::from_utf8_lossy(entity).into_owned()),
            ),
            ("what", Value::String(what.into())),
            ("value", v),
        ]));
    };

    put(b"", "types", hexes(&e.types()));
    let mut names = e.types();
    names.sort();
    for name in &names {
        let snap: Snapshot = if indexed {
            e.indexed_snapshot(name)
        } else {
            e.snapshot(name)
        };

        let mut docs: Vec<Value> = Vec::new();
        snap.ascend(0, |d| {
            let raw = d.bytes().unwrap_or_else(|err| panic!("{err}"));
            docs.push(Value::Array(vec![
                Value::Int(i64::from(d.seq())),
                hex_value(&d.id()),
                Value::String(d.lsn().to_string()),
                Value::String(STD_ENCODING.encode_to_string(&raw)),
            ]));
            true
        });
        put(
            name,
            "docs",
            if docs.is_empty() {
                Value::Null
            } else {
                Value::Array(docs)
            },
        );

        let refs: Vec<Value> = snap
            .refs()
            .iter()
            .map(|r| {
                Value::Array(vec![
                    Value::String(String::from_utf8_lossy(&r.path).into_owned()),
                    Value::String(String::from_utf8_lossy(&r.target).into_owned()),
                ])
            })
            .collect();
        put(
            name,
            "refs",
            if refs.is_empty() {
                Value::Null
            } else {
                Value::Array(refs)
            },
        );
        put(
            name,
            "counters",
            obj(vec![
                ("version", Value::String(snap.version().to_string())),
                ("generation", Value::String(snap.generation().to_string())),
                ("next_seq", Value::Int(i64::from(snap.next_seq()))),
                ("len", Value::Int(snap.len() as i64)),
            ]),
        );

        let index_names = e.index_names(name);
        put(
            name,
            "index_names",
            match &index_names {
                None => Value::Null,
                Some(list) => strs(list),
            },
        );
        for iname in index_names.unwrap_or_default() {
            let Some(spec) = datastore::parse_index_name(&iname) else {
                continue;
            };
            let what = format!("index:{}", String::from_utf8_lossy(&iname));
            let Some(v) = snap.index(&spec) else {
                put(name, &what, Value::Null);
                continue;
            };

            let mut info = Map::new();
            info.insert("entries".into(), entries_of(&v));
            info.insert("len".into(), Value::Int(v.len()));
            if spec.kind == INDEX_BITMAP {
                let ranks: Vec<Value> = ["", "lclosed", "ldraft", "lopen", "lzzz"]
                    .iter()
                    .map(|p| {
                        let mut k = Vec::new();
                        datastore::append_terminated(&mut k, p.as_bytes());
                        Value::Array(vec![Value::String((*p).into()), Value::Int(v.rank(&k))])
                    })
                    .collect();
                info.insert("ranks".into(), Value::Array(ranks));
                let mut set = Bitset::new(1 << 12);
                v.collect(None, None, &mut set, None);
                info.insert("collect_all".into(), ints_or_null(&bits_of(&set)));
                info.insert(
                    "distinct".into(),
                    Value::Array(vec![
                        Value::Int(v.distinct_values(1)),
                        Value::Int(v.distinct_values(2)),
                        Value::Int(v.distinct_values(100)),
                    ]),
                );
            }
            if spec.kind == INDEX_COMPOSITE {
                info.insert("irregular".into(), Value::Int(v.irregular()));
                let subs: Vec<Value> = ["open", "closed", "draft", "none"]
                    .iter()
                    .map(|status| {
                        let sub = v.sub(&datastore::composite_prefix(status.as_bytes()));
                        obj(vec![
                            ("status", Value::String((*status).into())),
                            ("len", Value::Int(sub.len())),
                            ("entries", entries_of(&sub)),
                        ])
                    })
                    .collect();
                info.insert("subs".into(), Value::Array(subs));
            }
            put(name, &what, Value::Object(info));
        }

        let search: Vec<Value> = ["*text*", "*text 01*", "*xyz*", "ab", "*searchable*"]
            .iter()
            .map(|p| {
                let set = e.search_candidates(name, p.as_bytes());
                let ok = set.is_some();
                let bits = set.map(|s| bits_of(&s)).unwrap_or_default();
                obj(vec![
                    ("pattern", Value::String((*p).into())),
                    ("ok", Value::Bool(ok)),
                    ("seqs", ints_or_null(&bits)),
                ])
            })
            .collect();
        put(name, "search_candidates", Value::Array(search));

        let changed: Vec<Value> = LSNS
            .iter()
            .map(|&l| {
                let got = snap.changed_since(l);
                let ok = got.is_some();
                let mut seqs = got.unwrap_or_default();
                seqs.sort_unstable();
                obj(vec![
                    ("lsn", Value::String(l.to_string())),
                    ("ok", Value::Bool(ok)),
                    ("seqs", ints_or_null(&seqs)),
                ])
            })
            .collect();
        put(name, "changed_since", Value::Array(changed));

        let fv: Vec<Value> = [vec!["open"], vec!["closed", "draft"], vec!["none"]]
            .iter()
            .map(|terms| {
                let tb: Vec<&[u8]> = terms.iter().map(|t| t.as_bytes()).collect();
                let got = snap.field_version(b"status", &tb);
                obj(vec![
                    (
                        "terms",
                        Value::Array(terms.iter().map(|t| Value::String((*t).into())).collect()),
                    ),
                    ("version", Value::String(got.unwrap_or(0).to_string())),
                    ("ok", Value::Bool(got.is_some())),
                ])
            })
            .collect();
        put(name, "field_version", Value::Array(fv));
    }

    let states: Vec<Value> = e
        .index_states()
        .iter()
        .map(|st| {
            obj(vec![
                (
                    "entity",
                    Value::String(String::from_utf8_lossy(&st.entity).into_owned()),
                ),
                (
                    "index",
                    Value::String(String::from_utf8_lossy(&st.index).into_owned()),
                ),
                (
                    "kind",
                    Value::String(String::from_utf8_lossy(&st.kind).into_owned()),
                ),
                (
                    "origin",
                    Value::String(String::from_utf8_lossy(&st.origin).into_owned()),
                ),
                ("state", Value::String(st.state.into())),
                ("pinned", Value::Bool(st.pinned)),
                ("entries", Value::Int(st.entries)),
            ])
        })
        .collect();
    put(
        b"",
        "index_states",
        if states.is_empty() {
            Value::Null
        } else {
            Value::Array(states)
        },
    );

    rows
}

fn run(background: bool) -> Vec<Value> {
    let tmp = TempDir::new();
    let e = datastore::open(Options {
        folder: filepath::join(tmp.path(), "data"),
        shards: 8,
        durability: journal::Mode::Sync,
        background,
        index_workers: 1,
        ..Options::default()
    })
    .unwrap_or_else(|err| panic!("{err}"));

    let ops = common::read_jsonl("engine/scenario_ops.jsonl");
    assert_eq!(ops.len(), 189);

    let mut rows = Vec::new();
    for o in &ops {
        let entity = o
            .get("entity")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes();
        let update = |f: &mut dyn FnMut(&mut datastore::Tx<'_>)| {
            e.update(|tx| {
                f(tx);
                Ok(())
            })
            .unwrap_or_else(|err| panic!("{err}"));
        };
        match s(o, "op") {
            "ensure_type" => update(&mut |tx| tx.ensure_type(entity)),
            "drop_type" => update(&mut |tx| tx.drop_type(entity)),
            "reset_entity" => update(&mut |tx| tx.reset_entity(entity)),
            "clear_all" => update(&mut |tx| tx.clear_all()),
            "put" => {
                let id = s(o, "id").as_bytes();
                let raw = json::marshal(o.get("doc").unwrap_or(&Value::Null))
                    .unwrap_or_else(|err| panic!("{err}"));
                let doc = datastore::decode_map(&raw)
                    .unwrap_or_else(|err| panic!("{err}"))
                    .unwrap_or_default();
                e.update(|tx| tx.put(entity, id, &doc))
                    .unwrap_or_else(|err| panic!("{err}"));
            }
            "delete" => {
                let id = s(o, "id").as_bytes();
                update(&mut |tx| tx.delete(entity, id));
            }
            "ensure_index" => {
                let spec = datastore::parse_index_name(s(o, "index").as_bytes())
                    .unwrap_or_else(|| panic!("bad index {o:?}"));
                e.ensure_index(entity, &spec);
            }
            "watch_field" => e.watch_field(entity, s(o, "field").as_bytes()),
            "dump" => {
                if background {
                    wait_indexes(&e);
                }
                rows.extend(dump(&e, s(o, "step"), background));
            }
            other => panic!("unknown op {other}"),
        }
    }

    e.close().unwrap_or_else(|err| panic!("{err}"));
    rows
}

fn check(label: &str, rows: &[Value]) {
    let want = common::read_jsonl("engine/scenario.jsonl");
    assert_eq!(want.len(), 95);
    assert_eq!(rows.len(), want.len(), "{label}: row count");
    let mut checked = 0;
    for (i, (got, want)) in rows.iter().zip(&want).enumerate() {
        same_json(&format!("{label} row {i}"), got, want);
        checked += 1;
    }
    assert_eq!(checked, 95);
}

#[test]
fn golden_engine_scenario() {
    check("plain", &run(false));
    check("background", &run(true));
}
