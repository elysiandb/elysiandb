use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use elysiandb::datastore::{self, Engine};
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::rand;

use super::tempdir::TempDir;
use super::{Closer, doc, n, open, options, s};

#[derive(Default)]
struct Model {
    types: Vec<Vec<u8>>,
    docs: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    data: HashMap<Vec<u8>, HashMap<Vec<u8>, String>>,
    kv: BTreeMap<Vec<u8>, String>,
}

impl Model {
    fn ensure_type(&mut self, entity: &[u8]) {
        if !self.types.iter().any(|t| t == entity) {
            self.types.push(entity.to_vec());
        }
    }

    fn put(&mut self, entity: &[u8], id: &[u8], value: String) {
        let data = self.data.entry(entity.to_vec()).or_default();
        if !data.contains_key(id) {
            self.docs
                .entry(entity.to_vec())
                .or_default()
                .push(id.to_vec());
        }
        data.insert(id.to_vec(), value);
    }

    fn del(&mut self, entity: &[u8], id: &[u8]) {
        let Some(data) = self.data.get_mut(entity) else {
            return;
        };
        if data.remove(id).is_none() {
            return;
        }
        if let Some(list) = self.docs.get_mut(entity) {
            list.retain(|d| d != id);
        }
    }

    fn reset(&mut self, entity: &[u8]) {
        self.data.remove(entity);
        self.docs.remove(entity);
    }
}

fn compare(step: i64, e: &Engine, m: &Model) {
    assert_eq!(e.types(), m.types, "step {step}: types");

    for entity in [&b"a"[..], b"b", b"c"] {
        let snap = e.snapshot(entity);
        let mut got: Vec<Vec<u8>> = Vec::new();
        snap.ascend(0, |d| {
            let b = d.bytes().unwrap_or_else(|err| panic!("step {step}: {err}"));
            let v = datastore::decode_map(&b).ok().flatten().unwrap_or_default();
            let value = match v.get("v") {
                Some(Value::String(x)) => x.clone(),
                _ => String::new(),
            };
            let want_value = m
                .data
                .get(entity)
                .and_then(|data| data.get(d.id().as_ref()))
                .cloned()
                .unwrap_or_default();
            assert_eq!(value, want_value, "step {step}: {:?}", d.id());
            got.push(d.id().into_owned());
            true
        });

        let want = m.docs.get(entity).cloned().unwrap_or_default();
        assert_eq!(got, want, "step {step}: order of {entity:?}");
        assert_eq!(snap.len(), want.len(), "step {step}: count of {entity:?}");
    }

    let keys = e.kv_keys();
    let want: Vec<Vec<u8>> = m.kv.keys().cloned().collect();
    assert_eq!(keys, want, "step {step}: kv keys");

    for (k, v) in &m.kv {
        let got = e
            .kv_get(k)
            .unwrap_or_else(|| panic!("step {step}: kv {k:?} missing"));
        assert_eq!(
            got.0.unwrap_or_default(),
            v.as_bytes(),
            "step {step}: kv {k:?}"
        );
    }
}

#[test]
fn random_operations_against_a_model() {
    for (cache, pages, segment) in [(0, 0, 0), (4 << 10, 0, 0), (4 << 10, 16 << 10, 64 << 10)] {
        let tmp = TempDir::new();
        let mut opts = options(tmp.path());
        opts.cache_bytes = cache;
        opts.page_cache_bytes = pages;
        opts.page_segment_size = segment;
        let mut e = open(&opts);

        let mut r = rand::Rand::new(rand::new_source(11));
        let mut m = Model::default();
        let entities: [&[u8]; 3] = [b"a", b"b", b"c"];

        for step in 0..4000i64 {
            let entity = entities[r.intn(entities.len() as i64) as usize];
            let id = format!("d{:02}", r.intn(60)).into_bytes();
            let op = r.intn(100);
            let result: Result<(), elysiandb::gocompat::errors::BoxError> = if op < 45 {
                let value = format!("{step}-{}", "x".repeat(r.intn(300) as usize));
                let d = doc(vec![("v", s(value.clone()))]);
                let res = e.update(|tx| {
                    tx.ensure_type(entity);
                    tx.put(entity, &id, &d)
                });
                m.ensure_type(entity);
                m.put(entity, &id, value);
                res
            } else if op < 65 {
                let res = e.update(|tx| {
                    tx.delete(entity, &id);
                    Ok(())
                });
                m.del(entity, &id);
                res
            } else if op < 67 {
                let res = e.update(|tx| {
                    tx.reset_entity(entity);
                    Ok(())
                });
                m.reset(entity);
                res
            } else if op < 80 {
                let key = format!("k{:02}", r.intn(40)).into_bytes();
                let value = format!("{step}");
                let res = e.kv_put(&key, value.as_bytes(), 0);
                m.kv.insert(key, value);
                res
            } else if op < 88 {
                let key = format!("k{:02}", r.intn(40)).into_bytes();
                e.kv_delete(&key);
                m.kv.remove(&key);
                Ok(())
            } else if op < 92 {
                if step % 3 == 0 {
                    e.compact_pages().map(|_| ())
                } else {
                    e.checkpoint()
                }
            } else if op < 95 {
                e.compact(4).map(|_| ())
            } else if op < 98 {
                let res = e.close();
                if res.is_ok() {
                    e = open(&opts);
                }
                res
            } else {
                e.abandon();
                e = open(&opts);
                Ok(())
            };

            if let Err(err) = result {
                panic!("step {step}: {err}");
            }

            if step % 250 == 0 {
                compare(step, &e, &m);
            }
        }

        compare(-1, &e, &m);
        e.close().unwrap_or_else(|err| panic!("{err}"));

        let e = open(&opts);
        compare(-2, &e, &m);

        let stats = e.stats();
        assert!(stats.segments != 0, "expected log segments");
        let _ = e.close();
    }
}

#[test]
fn compaction_removes_dead_segments() {
    let tmp = TempDir::new();
    let e = Closer(open(&options(tmp.path())));
    let e = &e.0;

    for round in 0..30 {
        e.update(|tx| {
            tx.ensure_type(b"a");
            for i in 0..20 {
                let d = doc(vec![("v", s("y".repeat(100))), ("round", n(round))]);
                tx.put(b"a", format!("d{i:02}").as_bytes(), &d)?;
            }
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
    }

    e.kv_put(b"kept", b"value", 0)
        .unwrap_or_else(|err| panic!("{err}"));

    let before = e.stats();
    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));

    for _ in 0..10 {
        e.compact(10).unwrap_or_else(|err| panic!("{err}"));
        e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    }

    let after = e.stats();
    assert!(
        after.log_bytes < before.log_bytes / 2,
        "compaction should reclaim most of the log: {} bytes before, {} after",
        before.log_bytes,
        after.log_bytes
    );

    let snap = e.snapshot(b"a");
    assert_eq!(snap.len(), 20, "documents survive compaction");

    let data = snap
        .get(b"d05")
        .and_then(|d| d.decode().ok())
        .unwrap_or_default();
    assert_eq!(
        data.get("round").and_then(Value::as_f64),
        Some(29.0),
        "the latest version survives compaction"
    );

    let kept = e.kv_get(b"kept");
    assert!(
        kept.is_some_and(|(v, _)| v.unwrap_or_default() == b"value"),
        "kv keys survive compaction"
    );
}

#[test]
fn snapshots_are_stable_while_writes_continue() {
    let tmp = TempDir::new();
    let e = Closer(open(&options(tmp.path())));
    let e = &e.0;

    for i in 0..100 {
        let _ = e.update(|tx| tx.put(b"a", format!("d{i:03}").as_bytes(), &doc(vec![("v", n(i))])));
    }

    let snap = e.snapshot(b"a");
    for i in 0..50 {
        let _ = e.update(|tx| {
            tx.delete(b"a", format!("d{i:03}").as_bytes());
            tx.put(b"a", format!("n{i:03}").as_bytes(), &doc(vec![("v", n(i))]))
        });
    }

    assert_eq!(snap.len(), 100, "an old snapshot keeps its documents");
    assert!(
        snap.get(b"d000").is_some(),
        "an old snapshot still sees a deleted document"
    );
    assert_eq!(
        e.snapshot(b"a").len(),
        100,
        "the new snapshot has 50 old and 50 new documents"
    );
}

#[test]
fn conflicting_transactions_are_retried() {
    let tmp = TempDir::new();
    let e = Closer(open(&options(tmp.path())));
    let e = &e.0;

    let _ = e.update(|tx| tx.put(b"a", b"counter", &doc(vec![("n", n(0.0))])));

    std::thread::scope(|sc| {
        for _ in 0..8 {
            sc.spawn(|| {
                for _ in 0..50 {
                    let _ = e.update(|tx| {
                        let mut d = tx.read(b"a", b"counter").unwrap_or_default();
                        let cur = d.get("n").and_then(Value::as_f64).unwrap_or(0.0);
                        d.insert("n".into(), n(cur + 1.0));
                        tx.put(b"a", b"counter", &d)
                    });
                }
            });
        }
    });

    let data = e
        .snapshot(b"a")
        .get(b"counter")
        .and_then(|d| d.decode().ok())
        .unwrap_or_default();
    assert_eq!(
        data.get("n").and_then(Value::as_f64),
        Some(400.0),
        "read-modify-write transactions lose no update"
    );
}

fn child(test: &str, var: &str, dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap_or_else(|e| panic!("{e}")));
    cmd.args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(var, dir);
    cmd
}

#[test]
fn lock_file_rejects_a_second_process_on_the_folder() {
    if let Some(dir) = std::env::var_os("ELYSIAN_LOCK_CHILD") {
        match datastore::open(options(std::path::Path::new(&dir))) {
            Err(err) => println!("LOCKED {err}"),
            Ok(_) => println!("OPENED"),
        }
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    let tmp = TempDir::new();
    let e = open(&options(tmp.path()));

    let out = child(
        "model_test::lock_file_rejects_a_second_process_on_the_folder",
        "ELYSIAN_LOCK_CHILD",
        tmp.path(),
    )
    .output()
    .unwrap_or_else(|err| panic!("{err}"));
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("LOCKED"),
        "a second process must not open the folder: {text}"
    );

    let _ = e.close();
}

fn crash_child(dir: &std::path::Path) -> ! {
    let e = match datastore::open(options(dir)) {
        Ok(e) => e,
        Err(err) => {
            println!("ERR {err}");
            std::process::exit(1);
        }
    };

    let mut w = std::io::stdout().lock();
    let start = e.snapshot(b"a").next_seq();
    let mut i: i64 = 0;
    loop {
        let id = format!("p{start}-{i}");
        let prev = format!("p{start}-{}", i - 1);
        let res = e.update(|tx| {
            tx.ensure_type(b"a");
            if i % 7 == 3 {
                tx.delete(b"a", prev.as_bytes());
            }
            tx.put(b"a", id.as_bytes(), &doc(vec![("i", n(i as f64))]))
        });
        if let Err(err) = res {
            let _ = writeln!(w, "ERR {err}");
            std::process::exit(1);
        }

        if e.kv_put(format!("kv-{id}").as_bytes(), i.to_string().as_bytes(), 0)
            .is_err()
        {
            std::process::exit(1);
        }

        let _ = writeln!(w, "ACK {id} {}", i % 7);
        let _ = w.flush();

        if i % 200 == 199 {
            let _ = e.checkpoint();
        }
        i += 1;
    }
}

fn parse_ack(line: &str) -> Option<String> {
    let mut parts = line.split(' ');
    if parts.next() != Some("ACK") {
        return None;
    }
    let id = parts.next()?;
    parts.next()?.parse::<i64>().ok()?;
    Some(id.to_string())
}

#[test]
fn kill9_loses_no_acknowledged_write() {
    if let Some(dir) = std::env::var_os("ELYSIAN_CRASH_CHILD") {
        crash_child(std::path::Path::new(&dir));
    }

    let runs = std::env::var("ELYSIAN_KILL9_RUNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(10);

    let tmp = TempDir::new();
    let dir = tmp.path();
    let mut acked: HashSet<String> = HashSet::new();
    let mut deleted: HashSet<String> = HashSet::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(1);
    let mut r = rand::Rand::new(rand::new_source(seed));

    for run in 0..runs {
        let mut proc_ = child(
            "model_test::kill9_loses_no_acknowledged_write",
            "ELYSIAN_CRASH_CHILD",
            dir,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| panic!("{err}"));
        let stdout = proc_.stdout.take().unwrap_or_else(|| panic!("no stdout"));

        let (tx, rx) = mpsc::channel::<String>();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let deadline = Instant::now() + Duration::from_millis(50 + r.intn(400) as u64);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(line) => {
                    if let Some(id) = parse_ack(&line) {
                        acked.insert(id);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = proc_.kill();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let _ = proc_.wait();
        let _ = reader.join();
        for line in rx.try_iter() {
            if let Some(id) = parse_ack(&line) {
                acked.insert(id);
            }
        }

        let e = datastore::open(options(dir))
            .unwrap_or_else(|err| panic!("run {run}: cannot reopen after kill -9: {err}"));

        let snap = e.snapshot(b"a");
        let mut present: HashSet<String> = HashSet::new();
        snap.ascend(0, |d| {
            present.insert(String::from_utf8_lossy(&d.id()).into_owned());
            true
        });

        for id in &acked {
            let Some((start, i)) = id
                .strip_prefix('p')
                .and_then(|x| x.split_once('-'))
                .and_then(|(a, b)| Some((a.parse::<i64>().ok()?, b.parse::<i64>().ok()?)))
            else {
                continue;
            };
            let next = format!("p{start}-{}", i + 1);
            if (i + 1) % 7 == 3 && present.contains(&next) {
                deleted.insert(id.clone());
            }
        }

        for id in &acked {
            if deleted.contains(id) {
                continue;
            }
            assert!(
                present.contains(id),
                "run {run}: acknowledged document {id} is missing after kill -9"
            );
            assert!(
                e.kv_get(format!("kv-{id}").as_bytes()).is_some(),
                "run {run}: acknowledged key kv-{id} is missing after kill -9"
            );
        }

        assert_eq!(
            snap.len(),
            present.len(),
            "run {run}: count differs from the listed documents"
        );

        e.close().unwrap_or_else(|err| panic!("{err}"));
    }

    assert!(!acked.is_empty(), "the child wrote nothing");
}

#[test]
fn old_snapshots_read_pages_of_cleaned_segments() {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.cache_bytes = 4 << 10;
    opts.page_cache_bytes = 16 << 10;
    opts.page_segment_size = 64 << 10;
    let e = Closer(open(&opts));
    let e = &e.0;

    for i in 0..3000 {
        let _ = e.update(|tx| {
            tx.put(
                b"a",
                format!("d{i:05}").as_bytes(),
                &doc(vec![("v", n(i)), ("pad", s("x".repeat(40)))]),
            )
        });
    }

    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));

    let old = e.snapshot(b"a");
    let _ = e.update(|tx| {
        tx.reset_entity(b"a");
        tx.put(b"a", b"fresh", &doc(vec![("v", n(-1.0))]))
    });

    e.compact_pages().unwrap_or_else(|err| panic!("{err}"));

    let mut seen = 0;
    old.ascend(0, |d| {
        if let Err(err) = d.decode() {
            panic!("an old snapshot cannot read {:?}: {err}", d.id());
        }
        seen += 1;
        true
    });

    assert_eq!(
        seen, 3000,
        "an old snapshot keeps all its documents after the pages are cleaned"
    );
    assert_eq!(
        e.snapshot(b"a").len(),
        1,
        "the reset entity holds only the new document"
    );
}
