use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use elysiandb::datastore::{self, Engine, Options};
use elysiandb::durable::Fs;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::journal;

use super::memfs_test::{MemFs, new_mem_fs};
use super::tempdir::TempDir;

pub fn engine_options(dir: &Path, fs: &MemFs) -> Options {
    let fs: Arc<dyn Fs> = Arc::new(fs.clone());
    Options {
        folder: dir.to_path_buf(),
        shards: 4,
        durability: journal::Mode::Sync,
        segment_size: 4 << 10,
        page_cache_bytes: 8 << 10,
        page_segment_size: 16 << 10,
        cache_bytes: 4 << 10,
        fs: Some(fs),
        ..Options::default()
    }
}

pub fn doc(pairs: Vec<(&str, Value)>) -> Map {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    m
}

pub fn text(v: impl Into<String>) -> Value {
    Value::String(v.into())
}

#[derive(Clone, Default)]
pub struct Ledger {
    pub docs: HashMap<String, String>,
    pub kv: HashMap<String, String>,
    pub pending: HashMap<String, String>,
}

pub const DELETED: &str = "\x00deleted";

pub fn run_workload(e: &Engine, l: &mut Ledger, steps: usize) {
    for i in 0..steps {
        let id = format!("d{:02}", (i * 7) % 23);
        let value = format!("v{i}-{id}");

        if i % 5 == 3 {
            l.pending.insert(id.clone(), DELETED.into());
            if e.update(|tx| {
                tx.delete(b"a", id.as_bytes());
                Ok(())
            })
            .is_err()
            {
                return;
            }
            l.docs.insert(id.clone(), DELETED.into());
            l.pending.remove(&id);
        } else {
            l.pending.insert(id.clone(), value.clone());
            let d = doc(vec![
                ("v", text(value.clone())),
                ("pad", text(format!("{i:0120}"))),
            ]);
            if e.update(|tx| {
                tx.ensure_type(b"a");
                tx.put(b"a", id.as_bytes(), &d)
            })
            .is_err()
            {
                return;
            }
            l.docs.insert(id.clone(), value.clone());
            l.pending.remove(&id);
        }

        if i % 4 == 0 {
            let key = format!("k{}", i % 9);
            let pk = format!("kv:{key}");
            l.pending.insert(pk.clone(), value.clone());
            if e.kv_put(key.as_bytes(), value.as_bytes(), 0).is_err() {
                return;
            }
            l.kv.insert(key, value.clone());
            l.pending.remove(&pk);
        }

        if i % 13 == 12 {
            let _ = e.checkpoint_now();
        }

        if i % 29 == 28 {
            let _ = e.compact(2);
        }
    }
}

pub fn check_ledger(label: &str, e: &Engine, l: &Ledger) {
    let snap = e.snapshot(b"a");
    for (id, want) in &l.docs {
        let mut allowed: HashSet<&str> = HashSet::new();
        allowed.insert(want);
        if let Some(p) = l.pending.get(id) {
            allowed.insert(p);
        }

        let got = match snap.get(id.as_bytes()) {
            None => DELETED.to_string(),
            Some(d) => {
                let m = d
                    .decode()
                    .unwrap_or_else(|err| panic!("{label}: document {id} unreadable: {err}"));
                m.get("v")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            }
        };

        assert!(
            allowed.contains(got.as_str()),
            "{label}: document {id} is {got:?}, acknowledged {want:?}"
        );
    }

    for (key, want) in &l.kv {
        let mut allowed: HashSet<&str> = HashSet::new();
        allowed.insert(want);
        if let Some(p) = l.pending.get(&format!("kv:{key}")) {
            allowed.insert(p);
        }

        let got = e.kv_get(key.as_bytes());
        let ok = got.as_ref().is_some_and(|(v, _)| {
            allowed.contains(String::from_utf8_lossy(v.as_deref().unwrap_or_default()).as_ref())
        });
        assert!(ok, "{label}: key {key} is {got:?}, acknowledged {want:?}");
    }
}

pub fn run_until_down(fs: &MemFs, e: &Engine, work: impl FnOnce() + Send) {
    let finished = AtomicBool::new(false);
    std::thread::scope(|sc| {
        let h = sc.spawn(|| {
            work();
            finished.store(true, Ordering::SeqCst);
        });
        loop {
            if finished.load(Ordering::SeqCst) {
                break;
            }
            if fs.down().wait_timeout(Duration::from_millis(2)) {
                break;
            }
        }
        if !finished.load(Ordering::SeqCst) {
            e.abandon();
        }
        if let Err(payload) = h.join() {
            std::panic::resume_unwind(payload);
        }
    });
}

fn crash_run(dir: &Path, crash_at: i64, steps: usize) -> (MemFs, Ledger) {
    let fs = new_mem_fs();
    fs.set_crash_at(crash_at);
    let e = match datastore::open(engine_options(dir, &fs)) {
        Ok(e) => e,
        Err(err) => {
            if crash_at > 0 {
                return (fs, Ledger::default());
            }
            panic!("{err}");
        }
    };

    let mut l = Ledger::default();
    run_until_down(&fs, &e, || run_workload(&e, &mut l, steps));
    e.abandon();

    (fs, l)
}

fn reopen_and_check(label: &str, dir: &Path, image: &MemFs, acked: &Ledger) {
    let mut l = acked.clone();

    let e = datastore::open(engine_options(dir, image))
        .unwrap_or_else(|err| panic!("{label}: cannot start after the crash: {err}"));

    check_ledger(label, &e, &l);

    e.update(|tx| {
        tx.ensure_type(b"a");
        tx.put(b"a", b"after", &doc(vec![("v", text("after"))]))
    })
    .unwrap_or_else(|err| panic!("{label}: cannot write after recovery: {err}"));
    e.checkpoint_now()
        .unwrap_or_else(|err| panic!("{label}: cannot checkpoint after recovery: {err}"));
    e.close()
        .unwrap_or_else(|err| panic!("{label}: cannot close after recovery: {err}"));

    let e = datastore::open(engine_options(dir, image))
        .unwrap_or_else(|err| panic!("{label}: cannot start a second time: {err}"));
    l.docs.insert("after".into(), "after".into());
    check_ledger(&format!("{label} (second start)"), &e, &l);
    e.abandon();
}

#[test]
fn test_every_crash_point_keeps_acknowledged_writes() {
    const STEPS: usize = 70;
    let tmp = TempDir::new();
    let dir = tmp.path();

    let (clean, _) = crash_run(dir, 0, STEPS);
    let total = clean.ops();
    assert!(
        total >= 200,
        "the workload should touch the disk many times, got {total} operations"
    );

    for crash_at in 1..=total {
        let (fs, l) = crash_run(dir, crash_at, STEPS);
        reopen_and_check(
            &format!("power cut at operation {crash_at}/{total}"),
            dir,
            &fs.after_power_loss(),
            &l,
        );
        reopen_and_check(
            &format!("process crash at operation {crash_at}/{total}"),
            dir,
            &fs.after_process_crash(),
            &l,
        );
    }
}

#[test]
fn test_state_survives_repeated_restarts() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let mut fs = new_mem_fs();
    let mut l = Ledger::default();

    for round in 0..40usize {
        let e = datastore::open(engine_options(dir, &fs))
            .unwrap_or_else(|err| panic!("round {round}: {err}"));
        check_ledger(&format!("round {round}"), &e, &l);

        for i in 0..15usize {
            let id = format!("r{:03}", (round * 15 + i) % 97);
            let value = format!("round{round}-{i}");
            e.update(|tx| {
                tx.ensure_type(b"a");
                tx.put(b"a", id.as_bytes(), &doc(vec![("v", text(value.clone()))]))
            })
            .unwrap_or_else(|err| panic!("{err}"));
            l.docs.insert(id, value);
        }

        match round % 3 {
            0 => {
                let _ = e.close();
            }
            1 => {
                let _ = e.checkpoint_now();
                e.abandon();
                fs = fs.after_power_loss();
            }
            _ => {
                e.abandon();
                fs = fs.after_process_crash();
            }
        }
    }

    let files = fs.list(dir);
    assert!(
        files.len() <= 120,
        "repeated restarts should not pile up files, {} left: {files:?}",
        files.len()
    );
}
