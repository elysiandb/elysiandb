use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::datastore::{Engine, INDEX_FILTER, INDEX_SORT, IndexSpec};
use elysiandb::durable::filepath;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;

use super::tempdir::TempDir;
use super::{Closer, doc, n, open, options, s};

pub struct Background {
    pub e: Closer,
    _tmp: TempDir,
}

pub fn open_background() -> Background {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.background = true;
    opts.index_workers = 2;
    opts.segment_size = 0;
    Background {
        e: Closer(open(&opts)),
        _tmp: tmp,
    }
}

pub fn put_docs(e: &Engine, entity: &[u8], from: usize, to: usize, f: impl Fn(usize) -> Map) {
    const BATCH: usize = 500;
    let mut start = from;
    while start < to {
        e.update(|tx| {
            tx.ensure_type(entity);
            for i in start..(start + BATCH).min(to) {
                let d = f(i);
                let id = d
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                tx.put(entity, id.as_bytes(), &d)?;
            }
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
        start += BATCH;
    }
}

pub fn entries(e: &Engine, entity: &[u8], spec: &IndexSpec) -> Option<Vec<Vec<u8>>> {
    let view = e.snapshot(entity).index(spec)?;
    let mut out = Vec::new();
    view.scan_range(None, None, |key, _| {
        out.push(key.to_vec());
        true
    });
    Some(out)
}

pub fn wait_ready(e: &Engine, entity: &[u8], specs: &[IndexSpec]) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let mut ready = e.index_backlog() == 0;
        for spec in specs {
            if e.snapshot(entity).index(spec).is_none() {
                ready = false;
            }
        }

        if ready {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "the indexes of {entity:?} never became ready, backlog {}",
            e.index_backlog()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn work_doc(i: usize) -> Map {
    doc(vec![
        ("id", s(format!("w{i:05}"))),
        ("a", n((i % 100) as f64)),
        ("b", s(format!("tag{}", i % 13))),
        ("c", Value::Bool(i.is_multiple_of(2))),
        ("text", s(format!("alpha{i:05}"))),
    ])
}

fn minutes(m: i64) -> GoDuration {
    GoDuration(m * GoDuration::MINUTE.0)
}

#[test]
fn batched_index_build_follows_concurrent_writes() {
    let bg = open_background();
    let e: &Arc<Engine> = &bg.e.0;
    put_docs(e, b"w", 0, 20000, work_doc);

    let specs = vec![
        IndexSpec::new(INDEX_SORT, "a"),
        IndexSpec::new(INDEX_FILTER, "b"),
        IndexSpec::new(INDEX_FILTER, "c"),
        IndexSpec::new(INDEX_SORT, "text"),
    ];

    std::thread::scope(|sc| {
        sc.spawn(|| {
            for i in 0..300usize {
                let _ = e.update(|tx| {
                    let id = format!("w{:05}", i * 37 % 20000);
                    match i % 3 {
                        0 => tx.put(
                            b"w",
                            id.as_bytes(),
                            &doc(vec![
                                ("id", s(id.clone())),
                                ("a", n((1000 + i) as f64)),
                                ("b", s("moved")),
                                ("c", Value::Bool(i % 2 == 1)),
                                ("text", s("omega")),
                            ]),
                        ),
                        1 => {
                            tx.delete(b"w", id.as_bytes());
                            Ok(())
                        }
                        _ => {
                            let d = work_doc(20000 + i);
                            let did = d
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            tx.put(b"w", did.as_bytes(), &d)
                        }
                    }
                });
            }
        });

        for spec in &specs {
            e.ensure_index(b"w", spec);
        }
        e.search_candidates(b"w", b"*omega*");
    });
    wait_ready(e, b"w", &specs);

    let mut built: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();
    for spec in &specs {
        built.insert(spec.name(), entries(e, b"w", spec).unwrap_or_default());
    }

    let searched = e
        .search_candidates(b"w", b"*omega*")
        .unwrap_or_else(|| panic!("the search index should be ready after the batch"));
    e.snapshot(b"w").ascend(0, |d| {
        let doc = d.decode().unwrap_or_default();
        if doc.get("text").and_then(Value::as_str) == Some("omega") && !searched.has(d.seq()) {
            panic!("the search index misses {:?}", d.id());
        }
        true
    });

    let n = e.drop_idle_indexes(minutes(-1));
    assert_eq!(
        n,
        specs.len() + 1,
        "expected released indexes with the search index"
    );
    for spec in &specs {
        e.ensure_index(b"w", spec);
    }
    wait_ready(e, b"w", &specs);

    for spec in &specs {
        let quiet = entries(e, b"w", spec).unwrap_or_default();
        let before = &built[&spec.name()];
        assert!(
            *before == quiet,
            "{:?} built under writes differs from a quiet rebuild ({} vs {} entries)",
            String::from_utf8_lossy(&spec.name()),
            before.len(),
            quiet.len()
        );
    }
}

#[test]
fn idle_index_is_released_and_rebuilt_on_use() {
    let bg = open_background();
    let e = &bg.e.0;
    put_docs(e, b"idle", 0, 3000, work_doc);

    let spec = IndexSpec::new(INDEX_SORT, "a");
    e.ensure_index(b"idle", &spec);
    wait_ready(e, b"idle", std::slice::from_ref(&spec));

    assert_eq!(
        e.drop_idle_indexes(GoDuration::HOUR),
        0,
        "an index used just now is kept"
    );
    assert_eq!(
        e.drop_idle_indexes(minutes(-1)),
        1,
        "an idle index is released"
    );

    let snap = e.snapshot(b"idle");
    assert!(
        snap.has_index(&spec) && snap.index_dormant(&spec) && e.index_backlog() == 0,
        "a released index stays registered, dormant, and out of the backlog"
    );
    assert!(
        snap.index(&spec).is_none(),
        "a released index is not served"
    );

    put_docs(e, b"idle", 3000, 3001, |_| {
        doc(vec![("id", s("late")), ("a", n(-5.0))])
    });

    e.ensure_index(b"idle", &spec);
    wait_ready(e, b"idle", std::slice::from_ref(&spec));

    let got = entries(e, b"idle", &spec).unwrap_or_default();
    assert_eq!(got.len(), 3001, "the rebuilt index has all entries");

    let mut first = Vec::new();
    let view = e
        .snapshot(b"idle")
        .index(&spec)
        .unwrap_or_else(|| panic!("ready"));
    view.ascend_at(0, |_, seq| {
        if let Some(d) = e.snapshot(b"idle").by_seq(seq) {
            first = d.id().into_owned();
        }
        false
    });
    assert_eq!(
        first, b"late",
        "the rebuilt index includes the write made while it was released"
    );
}

#[test]
fn small_entity_index_is_built_at_once() {
    let bg = open_background();
    let e = &bg.e.0;
    put_docs(e, b"small", 0, 200, work_doc);

    let spec = IndexSpec::new(INDEX_FILTER, "b");
    assert!(
        e.ensure_index(b"small", &spec),
        "a small entity gets its index without waiting for the background builders"
    );

    let got = entries(e, b"small", &spec);
    let joined: Vec<u8> = got.clone().unwrap_or_default().concat();
    assert!(
        got.as_ref().is_some_and(|g| !g.is_empty()) && joined.windows(4).any(|w| w == b"tag1"),
        "the index of a small entity is ready and filled"
    );

    let set = e.search_candidates(b"small", b"*alpha*");
    assert!(
        set.is_some_and(|s| s.count() == 200),
        "the search index of a small entity is ready at the first search"
    );
}

#[test]
fn restart_rebuilds_search_indexes_and_keeps_released_indexes() {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.background = true;
    opts.segment_size = 0;

    let e = open(&opts);
    put_docs(&e, b"texts", 0, 3000, work_doc);

    let released = IndexSpec::new(INDEX_SORT, "a");
    let kept = IndexSpec::new(INDEX_FILTER, "b");
    e.ensure_index(b"texts", &released);
    e.ensure_index(b"texts", &kept);
    e.search_candidates(b"texts", b"*alpha*");
    wait_ready(&e, b"texts", &[released.clone(), kept.clone()]);
    e.drop_idle_indexes(minutes(-1));
    e.ensure_index(b"texts", &kept);
    wait_ready(&e, b"texts", std::slice::from_ref(&kept));
    e.close().unwrap_or_else(|err| panic!("{err}"));

    let e = Closer(open(&opts));
    let e = &e.0;

    let snap = e.snapshot(b"texts");
    assert!(
        snap.has_index(&released) && snap.index_dormant(&released),
        "a released index is still registered after a restart"
    );
    assert!(
        snap.index(&kept).is_some(),
        "a ready index is restored from the checkpoint"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(set) = e.search_candidates(b"texts", b"*alpha*") {
            assert_eq!(set.count(), 3000, "the rebuilt search index");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the search index of a searched entity is rebuilt at startup"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reset_releases_the_cached_values() {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.background = true;
    opts.segment_size = 0;
    opts.cache_bytes = 256 << 20;
    let e = Closer(open(&opts));
    let e = &e.0;

    put_docs(e, b"cached", 0, 20000, work_doc);
    put_docs(e, b"kept", 0, 1000, work_doc);
    let before = e.stats().value_cache_bytes;
    assert!(
        before >= 1 << 20,
        "the values should be cached, got {before} bytes"
    );

    e.update(|tx| {
        tx.reset_entity(b"cached");
        Ok(())
    })
    .unwrap_or_else(|err| panic!("{err}"));

    let deadline = Instant::now() + Duration::from_secs(5);
    while e.stats().value_cache_bytes > before / 10 {
        assert!(
            Instant::now() < deadline,
            "the values of a reset entity leave the cache, {} of {before} bytes left",
            e.stats().value_cache_bytes
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(
        e.stats().value_cache_bytes != 0,
        "the values of other entities stay cached"
    );
}

fn pages_on_disk(folder: &Path) -> i64 {
    let mut total = 0i64;
    if let Ok(entries) = std::fs::read_dir(filepath::join(folder, "pages")) {
        for entry in entries.flatten() {
            if let Ok(info) = entry.metadata() {
                total += info.len() as i64;
            }
        }
    }
    total
}

#[test]
fn fragmented_page_segments_are_cleaned() {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.segment_size = 0;
    opts.page_segment_size = 256 << 10;
    opts.page_cache_bytes = 1 << 20;
    let e = open(&opts);

    let spec = IndexSpec::new(INDEX_SORT, "text");
    put_docs(&e, b"frag", 0, 20000, work_doc);
    e.ensure_index(b"frag", &spec);
    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));

    for round in 0..3 {
        put_docs(&e, b"frag", 0, 20000, |i| {
            let mut d = work_doc(i);
            d.insert("text".into(), s(format!("round{round}-{:05}", 20000 - i)));
            d
        });
        e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    }

    let before = pages_on_disk(&opts.folder);
    let removed = e.compact_pages().unwrap_or_else(|err| panic!("{err}"));
    let after = pages_on_disk(&opts.folder);
    let live = e.stats().live_page_bytes;
    assert!(
        removed != 0 && live != 0 && after <= 2 * live,
        "fragmented page segments are cleaned: {removed} removed, {before} bytes before, {after} after, {live} live"
    );

    let check = |e: &Engine| {
        let snap = e.snapshot(b"frag");
        assert_eq!(snap.len(), 20000, "expected 20000 documents");
        let d = snap.get(b"w00007");
        let text = d
            .as_ref()
            .and_then(|d| d.decode().ok())
            .and_then(|m| m.get("text").and_then(Value::as_str).map(str::to_string));
        assert_eq!(
            text.as_deref(),
            Some("round2-19993"),
            "the last version of a document survives the cleaning"
        );
        let got = entries(e, b"frag", &spec).unwrap_or_default();
        assert_eq!(got.len(), 20000, "the index survives the cleaning");
    };

    check(&e);
    e.close().unwrap_or_else(|err| panic!("{err}"));

    let e = Closer(open(&opts));
    check(&e.0);
}
