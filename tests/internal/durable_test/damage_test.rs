use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use elysiandb::datastore;
use elysiandb::durable::{Fs, filepath};

use super::crash_test::{Ledger, check_ledger, doc, engine_options, run_workload, text};
use super::memfs_test::{MemFs, MemNode, new_mem_fs};
use super::tempdir::TempDir;

fn filled_disk(dir: &Path) -> (MemFs, Ledger) {
    let fs = new_mem_fs();
    let e = datastore::open(engine_options(dir, &fs)).unwrap_or_else(|err| panic!("{err}"));

    let mut l = Ledger::default();
    run_workload(&e, &mut l, 60);
    e.checkpoint_now().unwrap_or_else(|err| panic!("{err}"));

    for i in 0..8 {
        let id = format!("late{i}");
        e.update(|tx| tx.put(b"a", id.as_bytes(), &doc(vec![("v", text(id.clone()))])))
            .unwrap_or_else(|err| panic!("{err}"));
        l.docs.insert(id.clone(), id);
    }
    e.checkpoint_now().unwrap_or_else(|err| panic!("{err}"));
    e.abandon();

    (fs, l)
}

pub fn corrupt(n: &mut MemNode, at: i64) {
    let at = if at < 0 { at + n.data.len() as i64 } else { at } as usize;
    let mut i = at;
    while i < at + 16 && i < n.data.len() {
        n.data[i] ^= 0x5a;
        i += 1;
    }
    n.synced = n.data.clone();
    n.shared = None;
}

fn last(fs: &MemFs, prefix: &Path) -> String {
    let list = fs.list(prefix);
    list.last()
        .cloned()
        .unwrap_or_else(|| panic!("nothing under {}", prefix.display()))
}

type Damage = fn(&MemFs, &Path);

#[test]
fn test_damaged_files_never_prevent_a_start() {
    let cases: Vec<(&str, Damage, bool)> = vec![
        (
            "unreadable manifest",
            |fs, dir| fs.mutate(&filepath::join(dir, "MANIFEST"), |n| corrupt(n, 2)),
            true,
        ),
        (
            "truncated manifest",
            |fs, dir| {
                fs.mutate(&filepath::join(dir, "MANIFEST"), |n| {
                    let half = n.data.len() / 2;
                    n.data.truncate(half);
                    n.alias_synced();
                })
            },
            true,
        ),
        (
            "missing key value checkpoint",
            |fs, dir| {
                let kv = last(fs, &filepath::join(dir, "kv-"));
                let _ = fs.remove(Path::new(&kv));
            },
            true,
        ),
        (
            "corrupt key value checkpoint",
            |fs, dir| {
                let kv = last(fs, &filepath::join(dir, "kv-"));
                fs.mutate(Path::new(&kv), |n| {
                    let mid = n.data.len() as i64 / 2;
                    corrupt(n, mid)
                });
            },
            true,
        ),
        (
            "corrupt latest page segment",
            |fs, dir| {
                let p = last(fs, &filepath::join(dir, "pages"));
                fs.mutate(Path::new(&p), |n| corrupt(n, -40));
            },
            true,
        ),
        (
            "torn end of the log",
            |fs, dir| {
                let p = last(fs, &filepath::join(dir, "log"));
                fs.mutate(Path::new(&p), |n| {
                    let len = n.data.len() - 5;
                    n.data.truncate(len);
                    n.alias_synced();
                });
            },
            false,
        ),
        (
            "garbage after the log",
            |fs, dir| {
                let p = last(fs, &filepath::join(dir, "log"));
                fs.mutate(Path::new(&p), |n| {
                    for _ in 0..40 {
                        n.data.extend_from_slice(b"\xde\xad");
                    }
                    n.alias_synced();
                });
            },
            true,
        ),
        (
            "corrupt record inside the log",
            |fs, dir| {
                let p = last(fs, &filepath::join(dir, "log"));
                fs.mutate(Path::new(&p), |n| {
                    let mid = n.data.len() as i64 / 2;
                    corrupt(n, mid)
                });
            },
            false,
        ),
    ];

    let mut checked = 0;
    for (name, damage, keeps) in cases {
        let tmp = TempDir::new();
        let dir = tmp.path();
        let (fs, l) = filled_disk(dir);
        damage(&fs, dir);

        let e = datastore::open(engine_options(dir, &fs))
            .unwrap_or_else(|err| panic!("{name}: the engine must start: {err}"));

        if keeps {
            check_ledger(name, &e, &l);
        }

        e.update(|tx| tx.put(b"a", b"fresh", &doc(vec![("v", text("fresh"))])))
            .unwrap_or_else(|err| panic!("{name}: the engine must accept writes: {err}"));
        e.checkpoint_now()
            .unwrap_or_else(|err| panic!("{name}: the engine must checkpoint again: {err}"));
        e.close().unwrap_or_else(|err| panic!("{name}: {err}"));

        let e = datastore::open(engine_options(dir, &fs)).unwrap_or_else(|err| {
            panic!("{name}: the engine must start again after repairing: {err}")
        });
        assert!(
            e.snapshot(b"a").get(b"fresh").is_some(),
            "{name}: a write after the repair must survive"
        );
        e.abandon();
        checked += 1;
    }
    assert_eq!(checked, 8);
}

#[test]
fn test_a_full_disk_never_acknowledges_a_lost_write() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let fs = new_mem_fs();
    let e = datastore::open(engine_options(dir, &fs)).unwrap_or_else(|err| panic!("{err}"));

    let mut l = Ledger::default();
    run_workload(&e, &mut l, 20);

    fs.set_quota(fs.usage());
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let writer = {
        let e = e.clone();
        std::thread::spawn(move || {
            let res = e
                .update(|t| {
                    t.put(
                        b"a",
                        b"blocked",
                        &doc(vec![("v", text("blocked")), ("pad", text("x".repeat(300)))]),
                    )
                })
                .map_err(|err| err.to_string());
            let _ = tx.send(res);
        })
    };

    if let Ok(res) = rx.recv_timeout(Duration::from_millis(300)) {
        panic!("a write must not be acknowledged while the disk is full, got {res:?}");
    }

    assert!(
        e.checkpoint_now().is_err(),
        "a checkpoint cannot succeed on a full disk"
    );

    assert!(
        e.snapshot(b"a").get(b"d00").is_some(),
        "reads keep working while the disk is full"
    );

    fs.set_quota(-1);
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => panic!("the write must go through once the disk has room: {err}"),
        Err(_) => panic!("the write never went through after the disk was freed"),
    }
    let _ = writer.join();
    l.docs.insert("blocked".into(), "blocked".into());

    e.checkpoint_now()
        .unwrap_or_else(|err| panic!("checkpoints resume once the disk has room: {err}"));
    e.abandon();

    let image = fs.after_power_loss();
    let e = datastore::open(engine_options(dir, &image)).unwrap_or_else(|err| panic!("{err}"));
    check_ledger("after a full disk", &e, &l);
    e.abandon();
}

#[test]
fn test_a_previous_manifest_from_another_chain_is_ignored() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let (fs, l) = filled_disk(dir);

    fs.mutate(&filepath::join(dir, "MANIFEST.prev"), |n| {
        n.data = br#"{"format": 2, "lsn": 1, "segment": 1, "offset": 16, "entities": [{"name": "a", "slots": {"id": 999999999999, "size": 64}, "ids": {"id": 999999999999, "size": 64}}]}"#.to_vec();
        n.alias_synced();
    });

    for round in 0..3 {
        let e = datastore::open(engine_options(dir, &fs))
            .unwrap_or_else(|err| panic!("round {round}: {err}"));
        check_ledger(&format!("round {round}"), &e, &l);
        e.compact_pages().unwrap_or_else(|err| {
            panic!("round {round}: cleaning must ignore the foreign manifest: {err}")
        });
        e.close().unwrap_or_else(|err| panic!("{err}"));
    }
}
