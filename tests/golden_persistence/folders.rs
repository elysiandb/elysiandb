use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use elysiandb::datastore::{
    self, Engine, INDEX_BITMAP, INDEX_COMPOSITE, INDEX_FILTER, INDEX_REFS, INDEX_SEARCH,
    INDEX_SORT, IndexSpec, ORIGIN_ADVISOR, ORIGIN_HOOK, ORIGIN_QUERY, Options,
};
use elysiandb::durable::filepath;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::journal;

use super::common;
use super::support::copy_tree;
use super::tempdir::TempDir;

pub fn folder_options(dir: &Path) -> Options {
    Options {
        folder: dir.to_path_buf(),
        shards: 8,
        durability: journal::Mode::Sync,
        segment_size: 4 << 10,
        page_segment_size: 16 << 10,
        page_cache_bytes: 8 << 10,
        cache_bytes: 4 << 10,
        legacy_recovery: true,
        ..Options::default()
    }
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

fn hex(b: &[u8]) -> Value {
    Value::String(common::hex(b))
}

fn text(b: &[u8]) -> Value {
    Value::String(String::from_utf8_lossy(b).into_owned())
}

pub fn folder_dump(e: &Engine) -> Value {
    let types = e.types();
    let mut ents = Vec::new();
    for name in &types {
        let s = e.snapshot(name);
        let mut docs = Vec::new();
        s.ascend(0, |d| {
            let mut row = Map::new();
            row.insert("id_hex".into(), hex(&d.id()));
            row.insert("seq".into(), Value::Int(i64::from(d.seq())));
            row.insert("lsn".into(), Value::String(d.lsn().to_string()));
            match d.bytes() {
                Ok(raw) => {
                    row.insert(
                        "raw_b64".into(),
                        Value::String(STD_ENCODING.encode_to_string(&raw)),
                    );
                }
                Err(_) => {
                    row.insert("read_error".into(), Value::Bool(true));
                }
            }
            docs.push(Value::Object(row));
            true
        });
        let refs = s
            .refs()
            .iter()
            .map(|r| {
                obj(vec![
                    ("path_hex", hex(&r.path)),
                    ("target_hex", hex(&r.target)),
                ])
            })
            .collect();
        let mut idx = Vec::new();
        for iname in e.index_names(name).unwrap_or_default() {
            let mut row = Map::new();
            row.insert("name".into(), text(&iname));
            let Some(spec) = datastore::parse_index_name(&iname) else {
                row.insert("parsed".into(), Value::Bool(false));
                idx.push(Value::Object(row));
                continue;
            };
            row.insert("dormant".into(), Value::Bool(s.index_dormant(&spec)));
            if let Some(v) = s.index(&spec) {
                let mut keys = Vec::new();
                v.scan_range(None, None, |key, seq| {
                    keys.push(Value::Array(vec![hex(key), Value::Int(i64::from(seq))]));
                    true
                });
                row.insert("entries".into(), Value::Array(keys));
                row.insert("len".into(), Value::Int(v.len()));
            }
            idx.push(Value::Object(row));
        }
        ents.push(obj(vec![
            ("name_hex", hex(name)),
            ("gen", Value::String(s.generation().to_string())),
            ("next_seq", Value::Int(i64::from(s.next_seq()))),
            ("version", Value::String(s.version().to_string())),
            ("len", Value::Int(s.len() as i64)),
            ("docs", Value::Array(docs)),
            ("refs", Value::Array(refs)),
            ("indexes", Value::Array(idx)),
        ]));
    }

    let mut states: Vec<(String, String, Value)> = e
        .index_states()
        .iter()
        .map(|st| {
            (
                common::hex(&st.entity),
                String::from_utf8_lossy(&st.index).into_owned(),
                obj(vec![
                    ("entity_hex", hex(&st.entity)),
                    ("index", text(&st.index)),
                    ("kind", text(&st.kind)),
                    ("origin", text(&st.origin)),
                    ("state", Value::String(st.state.into())),
                    ("pinned", Value::Bool(st.pinned)),
                    ("entries", Value::Int(st.entries)),
                ]),
            )
        })
        .collect();
    states.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut keys = e.kv_keys();
    keys.sort();
    let kv = keys
        .iter()
        .map(|k| {
            let got = e.kv_get(k);
            let mut row = Map::new();
            row.insert("key_hex".into(), hex(k));
            row.insert("found".into(), Value::Bool(got.is_some()));
            let (v, exp) = got.unwrap_or((None, 0));
            row.insert("value_hex".into(), hex(&v.unwrap_or_default()));
            row.insert("expires_at".into(), Value::String(exp.to_string()));
            if let Some(lsn) = e.kv_lsn(k) {
                row.insert("lsn".into(), Value::String(lsn.to_string()));
            }
            Value::Object(row)
        })
        .collect();

    obj(vec![
        (
            "types_hex",
            Value::Array(types.iter().map(|t| hex(t)).collect()),
        ),
        ("entities", Value::Array(ents)),
        (
            "index_states",
            Value::Array(states.into_iter().map(|s| s.2).collect()),
        ),
        ("kv", Value::Array(kv)),
        ("replayed_records", Value::Int(e.replayed_records())),
    ])
}

fn same(label: &str, got: &Value, want: &Value) {
    let a = json::marshal(got).unwrap_or_else(|e| panic!("{e}"));
    let b = json::marshal(want).unwrap_or_else(|e| panic!("{e}"));
    if a != b {
        let (ga, wa) = (String::from_utf8_lossy(&a), String::from_utf8_lossy(&b));
        let at = ga
            .bytes()
            .zip(wa.bytes())
            .position(|(x, y)| x != y)
            .unwrap_or(ga.len().min(wa.len()));
        let from = at.saturating_sub(200);
        panic!(
            "{label} differs at byte {at}:\n got  {}\n want {}",
            &ga[from..(at + 200).min(ga.len())],
            &wa[from..(at + 200).min(wa.len())]
        );
    }
}

fn open(dir: &Path) -> Arc<Engine> {
    datastore::open(folder_options(dir)).unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
}

#[test]
fn data_folders_written_by_go_open_identically() {
    let tmp = TempDir::new();
    let mut checked = 0;
    for case in ["clean", "crash", "damaged", "format1-upgraded", "migrated"] {
        let work = filepath::join(tmp.path(), case);
        copy_tree(
            &common::golden_path(&format!("persistence/folders/{case}/before")),
            &work,
        );
        let e = open(&work);
        let mut got = folder_dump(&e);
        if let Some(m) = e.migration()
            && let Value::Object(o) = &mut got
        {
            o.insert(
                "migration".into(),
                obj(vec![
                    ("entities", Value::Int(m.entities as i64)),
                    ("documents", Value::Int(m.documents as i64)),
                    ("kv_keys", Value::Int(m.kv_keys as i64)),
                    ("replayed", Value::Int(m.replayed as i64)),
                ]),
            );
        }
        e.close().unwrap_or_else(|err| panic!("{case}: {err}"));
        let want = common::read_json(&format!("persistence/folders/{case}/expected.json"));
        same(case, &got, &want);
        checked += 1;
    }
    assert_eq!(checked, 5);
}

#[test]
fn a_024_folder_migrates_to_the_same_bytes() {
    let tmp = TempDir::new();
    let work = filepath::join(tmp.path(), "data");
    copy_tree(
        &common::golden_path("persistence/migration/numbers/input"),
        &work,
    );
    let e = open(&work);
    let got = folder_dump(&e);
    e.close().unwrap_or_else(|err| panic!("{err}"));
    let want = common::read_json("persistence/migration/numbers/expected.json");
    same("migration/numbers", &got, &want);
}

pub fn doc_id(i: usize) -> Vec<u8> {
    if i % 50 == 7 {
        let mut id = b"id-\xff".to_vec();
        id.extend_from_slice(format!("{i:04}").as_bytes());
        return id;
    }
    format!("id-{i:04}").into_bytes()
}

fn id_text(i: usize) -> String {
    String::from_utf8_lossy(&doc_id(i)).into_owned()
}

pub fn workload_doc(i: usize) -> Map {
    let mut doc = Map::new();
    doc.insert("title".into(), Value::String(format!("T{:03}", i % 37)));
    doc.insert("n".into(), Value::Number(i as f64));
    doc.insert(
        "status".into(),
        Value::String(["open", "closed", "draft"][i % 3].into()),
    );
    doc.insert(
        "date".into(),
        Value::String(format!("2026-01-{:02}T10:00:00Z", 1 + i % 28)),
    );
    doc.insert(
        "tags".into(),
        Value::Array(
            ["x", "y", "z"][..i % 4]
                .iter()
                .map(|t| Value::String((*t).into()))
                .collect(),
        ),
    );
    if i % 10 == 3 {
        let mut a = Map::new();
        a.insert("@entity".into(), Value::String("a".into()));
        a.insert("id".into(), Value::String(id_text(i % 30)));
        doc.insert("author".into(), Value::Object(a));
    }
    if i % 25 == 4 {
        let mut b = Map::new();
        b.insert("@entity".into(), Value::String("b".into()));
        b.insert("id".into(), Value::String(id_text(i % 20)));
        doc.insert(
            "notes".into(),
            Value::Array(vec![
                Value::Object(b),
                Value::String("plain".into()),
                Value::Number(1.5),
            ]),
        );
    }
    doc
}

fn must(
    e: &Engine,
    mut f: impl FnMut(&mut datastore::Tx<'_>) -> Result<(), elysiandb::gocompat::errors::BoxError>,
) {
    e.update(|tx| f(tx)).unwrap_or_else(|err| panic!("{err}"));
}

pub fn run_workload(dir: &Path) {
    let e = open(dir);
    let types: [&[u8]; 3] = [b"a", b"b", b"c<&>"];
    must(&e, |tx| {
        for t in types.iter().chain([&b"d"[..]].iter()) {
            tx.ensure_type(t);
        }
        Ok(())
    });
    let mut start = 0;
    while start < 300 {
        must(&e, |tx| {
            for i in start..start + 25 {
                tx.put(types[i % 3], &doc_id(i), &workload_doc(i))?;
            }
            Ok(())
        });
        start += 25;
    }
    must(&e, |tx| {
        let mut m = Map::new();
        m.insert("x".into(), Value::Number(1.0));
        tx.put(b"d", b"only", &m)
    });

    let specs: Vec<(&[u8], IndexSpec, &str)> = vec![
        (b"a", IndexSpec::new(INDEX_FILTER, "title"), ORIGIN_QUERY),
        (b"a", IndexSpec::new(INDEX_SORT, "n"), ORIGIN_QUERY),
        (b"a", IndexSpec::new(INDEX_REFS, "author"), ORIGIN_QUERY),
        (b"a", IndexSpec::new(INDEX_SEARCH, "title"), ORIGIN_QUERY),
        (b"a", IndexSpec::new(INDEX_BITMAP, "status"), ORIGIN_ADVISOR),
        (
            b"a",
            IndexSpec::new(INDEX_COMPOSITE, "status,n"),
            ORIGIN_ADVISOR,
        ),
        (b"b", IndexSpec::new(INDEX_FILTER, "title"), ORIGIN_HOOK),
        (b"b", IndexSpec::new(INDEX_SORT, "date"), ORIGIN_QUERY),
        (b"c<&>", IndexSpec::new(INDEX_FILTER, "tags"), ORIGIN_QUERY),
    ];
    for (ent, spec, origin) in &specs {
        e.ensure_index_with(ent, spec, origin.as_bytes());
    }
    let dormant = IndexSpec::new(INDEX_SORT, "n");
    let keep = |ent: &[u8], spec: &IndexSpec| !(ent == b"a" && *spec == dormant);
    for (ent, spec, _) in &specs {
        if keep(ent, spec) {
            e.pin_index(ent, spec);
        }
    }
    e.drop_idle_indexes(GoDuration(0));
    std::thread::sleep(Duration::from_millis(5));
    e.drop_idle_indexes(GoDuration(0));
    for (ent, spec, _) in &specs {
        if keep(ent, spec) {
            e.unpin_index(ent, spec);
        }
    }
    must(&e, |tx| {
        let mut i = 0;
        while i < 300 {
            tx.delete(types[i % 3], &doc_id(i));
            i += 11;
        }
        Ok(())
    });
    e.kv_put(b"plain", b"value", 0)
        .unwrap_or_else(|err| panic!("{err}"));
    e.kv_put(b"ttl-2100", b"expires in 2100", 4_102_444_800)
        .unwrap_or_else(|err| panic!("{err}"));
    e.kv_put(b"binary\xff", &[0, 1, 2, 0xff], 0)
        .unwrap_or_else(|err| panic!("{err}"));
    e.kv_put(b"gone", b"x", 0)
        .unwrap_or_else(|err| panic!("{err}"));
    e.kv_delete(b"gone");
    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    must(&e, |tx| {
        tx.reset_entity(b"b");
        Ok(())
    });
    must(&e, |tx| {
        for i in 0..10 {
            tx.put(
                b"b",
                format!("again-{i:02}").as_bytes(),
                &workload_doc(i + 1000),
            )?;
        }
        Ok(())
    });
    must(&e, |tx| {
        tx.drop_type(b"d");
        Ok(())
    });
    for i in 0..40 {
        must(&e, |tx| tx.put(b"a", &doc_id(i), &workload_doc(i + 2000)));
    }
    e.compact(2).unwrap_or_else(|err| panic!("{err}"));
    e.compact_pages().unwrap_or_else(|err| panic!("{err}"));
    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    e.close().unwrap_or_else(|err| panic!("{err}"));
}
