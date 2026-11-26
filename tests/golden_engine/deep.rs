use crate::tempdir::TempDir;
use elysiandb::datastore::{
    self, INDEX_BITMAP, INDEX_COMPOSITE, INDEX_FILTER, INDEX_REFS, INDEX_SORT, IndexSpec, Options,
};
use elysiandb::durable::filepath;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::journal;

const LEVELS: usize = 9998;

fn deep_doc() -> Map {
    let mut v = Value::String("leaf text".into());
    for i in 0..LEVELS {
        v = if i % 2 == 0 {
            Value::Array(vec![v])
        } else {
            let mut m = Map::new();
            m.insert("k".into(), v);
            Value::Object(m)
        };
    }
    let mut doc = Map::new();
    doc.insert("x".into(), v);
    doc.insert("title".into(), Value::String("deep document".into()));
    doc
}

fn depth(v: &Value) -> usize {
    let mut n = 0;
    let mut cur = v;
    loop {
        match cur {
            Value::Array(a) if !a.is_empty() => cur = &a[0],
            Value::Object(m) => match m.get("k") {
                Some(next) => cur = next,
                None => return n,
            },
            _ => return n,
        }
        n += 1;
    }
}

#[test]
fn deep_documents_are_indexed_without_recursion() {
    let tmp = TempDir::new();
    let opts = Options {
        folder: filepath::join(tmp.path(), "data"),
        shards: 4,
        durability: journal::Mode::Write,
        ..Options::default()
    };
    let specs = [
        IndexSpec::new(INDEX_FILTER, "x"),
        IndexSpec::new(INDEX_SORT, "x"),
        IndexSpec::new(INDEX_REFS, ""),
        datastore::search_spec(),
        IndexSpec::new(INDEX_BITMAP, "x"),
        IndexSpec::new(INDEX_COMPOSITE, "x,x"),
    ];

    let e = datastore::open(opts.clone()).unwrap_or_else(|err| panic!("{err}"));
    for spec in &specs {
        e.ensure_index(b"deep", spec);
    }
    e.watch_field(b"deep", b"x");

    let doc = deep_doc();
    e.update(|tx| {
        tx.ensure_type(b"deep");
        tx.put(b"deep", b"d1", &doc)
    })
    .unwrap_or_else(|err| panic!("{err}"));
    e.update(|tx| tx.put(b"deep", b"d1", &doc))
        .unwrap_or_else(|err| panic!("{err}"));

    let got = e
        .snapshot(b"deep")
        .get(b"d1")
        .and_then(|d| d.decode().ok())
        .unwrap_or_else(|| panic!("the deep document reads back"));
    assert_eq!(depth(&got["x"]), LEVELS);

    let set = e
        .search_candidates(b"deep", b"*leaf*")
        .unwrap_or_else(|| panic!("the search index answers"));
    assert_eq!(set.count(), 1);
    for spec in &specs {
        assert!(e.snapshot(b"deep").index(spec).is_some(), "{spec:?}");
    }

    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    e.close().unwrap_or_else(|err| panic!("{err}"));

    let e = datastore::open(opts).unwrap_or_else(|err| panic!("{err}"));
    let got = e
        .snapshot(b"deep")
        .get(b"d1")
        .and_then(|d| d.decode().ok())
        .unwrap_or_else(|| panic!("the deep document survives a restart"));
    assert_eq!(depth(&got["x"]), LEVELS);
    e.update(|tx| {
        tx.delete(b"deep", b"d1");
        Ok(())
    })
    .unwrap_or_else(|err| panic!("{err}"));
    assert!(e.snapshot(b"deep").is_empty());
    e.close().unwrap_or_else(|err| panic!("{err}"));
}
