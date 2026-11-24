use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

use elysiandb::api::{self, ListQuery};
use elysiandb::datastore::{
    self, Doc, Engine, INDEX_BITMAP, INDEX_FILTER, INDEX_SEARCH, INDEX_SORT, IndexSpec, KEY_LOWER,
    Options, append_escaped, composite_spec,
};
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::journal;
use elysiandb::storage;

use super::index_forms_test::{check_planner_against_scan, random_diff_doc};
use super::index_test::{obj, s};
use super::{TestStore, filters, init_test_store, setup};

fn open_background_store() -> (TestStore, Arc<Engine>) {
    let db = init_test_store();
    if let Err(err) = storage::open(storage::MODE_SERVER) {
        panic!("{err}");
    }
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    (db, e)
}

fn ensure(e: &Engine, entity: &[u8], spec: &IndexSpec) {
    while !e.ensure_index(entity, spec) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn bulk(r: &mut Rand, from: usize, n: usize) {
    let mut docs: Vec<Map> = (from..from + n)
        .map(|i| random_diff_doc(r, &format!("q{i:05}")))
        .collect();
    api::write_list_of_entities(b"forms", &mut docs);
}

#[test]
fn test_bulk_writes_answer_like_a_scan() {
    let _s = setup();
    let (_db, e) = open_background_store();
    let mut r = Rand::new(rand::new_source(11));

    bulk(&mut r, 0, 2000);
    for f in ["a", "b", "m.x"] {
        ensure(&e, b"forms", &IndexSpec::new(INDEX_FILTER, f));
    }
    ensure(&e, b"forms", &IndexSpec::new(INDEX_BITMAP, "list.x"));
    ensure(&e, b"forms", &IndexSpec::new(INDEX_SORT, "a"));
    ensure(&e, b"forms", &IndexSpec::new(INDEX_SEARCH, b"".as_slice()));
    ensure(&e, b"forms", &composite_spec(b"b", b"a"));
    check_planner_against_scan(&mut r, "forms", 30);

    let mut saw_lag = false;
    for round in 0..12 {
        bulk(&mut r, 2000 + round * 300, 300);
        saw_lag = saw_lag || e.index_backlog_bytes() > 0;
        for i in 0..80 {
            let id = format!("q{:05}", r.intn(2000));
            if i % 5 == 0 {
                api::delete_entity_by_id(b"forms", id.as_bytes());
            } else {
                let mut d = random_diff_doc(&mut r, &id);
                api::write_entity(b"forms", &mut d);
            }
        }
        let mut docs: Vec<Map> = Vec::with_capacity(200);
        for _ in 0..200 {
            let id = format!("q{:05}", r.intn(2000));
            docs.push(random_diff_doc(&mut r, &id));
        }
        api::write_list_of_entities(b"forms", &mut docs);
        check_planner_against_scan(&mut r, "forms", 25);
        if round % 4 == 3
            && let Err(err) = e.checkpoint()
        {
            panic!("{err}");
        }
        if round % 4 == 1 {
            std::thread::sleep(Duration::from_millis(450));
            if e.index_backlog_bytes() != 0 {
                panic!(
                    "the backlog should drain when writes stop: {} bytes",
                    e.index_backlog_bytes()
                );
            }
        }
    }
    if !saw_lag {
        panic!("bulk writes should go through the backlog");
    }
}

#[test]
fn test_reads_during_bulk_writes_never_see_an_old_index() {
    let _s = setup();
    let (_db, e) = open_background_store();
    ensure(&e, b"live", &IndexSpec::new(INDEX_FILTER, "tag"));

    let acked = AtomicI64::new(0);
    let stop = AtomicBool::new(false);
    let f = filters(&[("tag", &[("eq", "x")])]);
    let mut checks = 0;
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let mut b = 0;
            while !stop.load(Ordering::SeqCst) {
                let mut docs: Vec<Map> = (0..100)
                    .map(|i| obj(vec![("id", s(&format!("d{b}-{i}"))), ("tag", s("x"))]))
                    .collect();
                api::write_list_of_entities(b"live", &mut docs);
                acked.fetch_add(100, Ordering::SeqCst);
                b += 1;
            }
        });

        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            let before = acked.load(Ordering::SeqCst);
            let res = api::plan(ListQuery {
                entity: b"live",
                filters: Some(&f),
                count_only: true,
                ..ListQuery::default()
            });
            let after = acked.load(Ordering::SeqCst);
            if (res.total as i64) < before {
                stop.store(true, Ordering::SeqCst);
                panic!(
                    "a confirmed write is missing from a filter: {} found, {before} confirmed",
                    res.total
                );
            }
            if res.total as i64 > after + 100 {
                stop.store(true, Ordering::SeqCst);
                panic!(
                    "the filter found too much: {} found, {after} confirmed",
                    res.total
                );
            }
            checks += 1;
        }
        stop.store(true, Ordering::SeqCst);
    });

    if checks == 0 {
        panic!("no read ran");
    }
    let res = api::plan(ListQuery {
        entity: b"live",
        filters: Some(&f),
        ..ListQuery::default()
    });
    let got: Vec<Vec<u8>> = res.ids();
    let mut want: Vec<Vec<u8>> = Vec::new();
    e.snapshot(b"live").ascend(0, |d: Doc| {
        want.push(d.id().into_owned());
        true
    });
    if got != want {
        panic!("{} found, {} stored", got.len(), want.len());
    }
}

#[test]
fn test_an_index_in_backlog_is_rebuilt_after_a_crash() {
    let _s = setup();
    let _db = init_test_store();
    let Some(current) = storage::engine() else {
        panic!("no engine");
    };
    let cfg_folder = current.folder().to_path_buf();
    drop(current);
    let _ = storage::close();

    let open = || -> Arc<Engine> {
        match datastore::open(Options {
            folder: cfg_folder.clone(),
            shards: 4,
            background: true,
            durability: journal::Mode::Write,
            ..Options::default()
        }) {
            Ok(e) => e,
            Err(err) => panic!("{err}"),
        }
    };
    let e = open();
    let spec = IndexSpec::new(INDEX_FILTER, "tag");
    e.ensure_index(b"a", &spec);
    ensure(&e, b"a", &spec);
    let put = |e: &Engine, from: usize| {
        let res = e.update(|tx| {
            tx.ensure_type(b"a");
            for i in from..from + 200 {
                let id = format!("d{i:05}");
                let mut m = Map::new();
                m.insert("id".to_string(), Value::String(id.clone()));
                m.insert("tag".to_string(), Value::String("x".to_string()));
                tx.put(b"a", id.as_bytes(), &m)?;
            }
            Ok(())
        });
        if let Err(err) = res {
            panic!("{err}");
        }
    };
    put(&e, 0);
    if let Err(err) = e.checkpoint() {
        panic!("{err}");
    }
    put(&e, 200);
    e.abandon();

    let e = open();
    let n = e.snapshot(b"a").len();
    if n != 400 {
        e.abandon();
        panic!("{n} documents");
    }
    ensure(&e, b"a", &spec);
    let Some(view) = e.indexed_snapshot(b"a").index(&spec) else {
        e.abandon();
        panic!("the index is not usable");
    };
    let mut found = 0;
    let mut prefix = vec![KEY_LOWER];
    append_escaped(&mut prefix, b"x");
    prefix.extend_from_slice(&[0, 1]);
    view.scan_prefix(&prefix, |_, _| {
        found += 1;
        true
    });
    e.abandon();
    if found != 400 {
        panic!("the index holds {found} of 400 documents");
    }
}

#[test]
fn test_log_replay_keeps_indexes_exact() {
    let _s = setup();
    let (_db, e) = open_background_store();
    let mut r = Rand::new(rand::new_source(23));
    let mut docs: Vec<Map> = (0..3000)
        .map(|i| random_diff_doc(&mut r, &format!("q{i:05}")))
        .collect();
    api::write_list_of_entities(b"forms", &mut docs);
    let specs = [
        IndexSpec::new(INDEX_FILTER, "a"),
        IndexSpec::new(INDEX_FILTER, "b"),
        IndexSpec::new(INDEX_BITMAP, "list.x"),
        IndexSpec::new(INDEX_SORT, "a"),
        IndexSpec::new(INDEX_SEARCH, b"".as_slice()),
        composite_spec(b"b", b"a"),
    ];
    for spec in &specs {
        ensure(&e, b"forms", spec);
    }
    if e.indexed_snapshot(b"forms").index(&specs[0]).is_none() {
        panic!("the index is not usable");
    }
    if let Err(err) = e.checkpoint() {
        panic!("{err}");
    }
    for i in 0..600 {
        let id = format!("q{:05}", r.intn(3000));
        if i % 6 == 0 {
            api::delete_entity_by_id(b"forms", id.as_bytes());
        } else {
            let mut d = random_diff_doc(&mut r, &id);
            api::write_entity(b"forms", &mut d);
        }
    }
    let mut more: Vec<Map> = (3000..3500)
        .map(|i| random_diff_doc(&mut r, &format!("q{i:05}")))
        .collect();
    api::write_list_of_entities(b"forms", &mut more);
    if let Err(err) = e.flush_log() {
        panic!("{err}");
    }
    e.abandon();
    drop(e);
    if let Err(err) = storage::open(storage::MODE_SERVER) {
        panic!("{err}");
    }
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    if e.replayed_records() == 0 {
        panic!("this test should replay the log");
    }
    if e.index_backlog_bytes() != 0 {
        panic!(
            "a replay should leave no backlog: {} bytes",
            e.index_backlog_bytes()
        );
    }
    for spec in &specs {
        ensure(&e, b"forms", spec);
    }
    check_planner_against_scan(&mut r, "forms", 60);
}
