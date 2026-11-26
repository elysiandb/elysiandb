#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use elysiandb::datastore::{self, Engine, Options};
use elysiandb::durable::{self, DirEntry, File, Fs, FsError, OpenFlags, filepath};
use elysiandb::gocompat::json::Value;
use elysiandb::journal;

use super::crash_test::{doc, engine_options, run_until_down, text};
use super::damage_test::corrupt;
use super::memfs_test::{MemFs, new_mem_fs};
use super::pagesio_test::GateFs;
use super::tempdir::TempDir;

pub struct FailingState {
    inner: Arc<dyn Fs>,
    pub left: AtomicI64,
    pub counted: AtomicI64,
    failure: FsError,
}

#[derive(Clone)]
pub struct FailingFs(pub Arc<FailingState>);

impl FailingFs {
    pub fn new(inner: Arc<dyn Fs>, failure: FsError) -> FailingFs {
        FailingFs(Arc::new(FailingState {
            inner,
            left: AtomicI64::new(0),
            counted: AtomicI64::new(0),
            failure,
        }))
    }

    fn step(&self) -> Result<(), FsError> {
        self.0.counted.fetch_add(1, Ordering::SeqCst);
        if self.0.left.fetch_sub(1, Ordering::SeqCst) - 1 < 0 {
            return Err(self.0.failure.clone());
        }

        Ok(())
    }
}

impl Fs for FailingFs {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        self.step()?;

        let file = self.0.inner.open_file(path, flags, mode)?;

        Ok(Arc::new(FailingFile {
            file,
            fs: self.clone(),
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.rename(from, to)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        self.0.inner.read_dir(path)
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.mkdir_all(path, mode)
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.sync_dir(path)
    }
}

pub struct FailingFile {
    file: Arc<dyn File>,
    fs: FailingFs,
}

impl File for FailingFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        self.file.read_at(buf, off)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        if let Err(e) = self.fs.step() {
            return (0, Some(e));
        }

        self.file.write_at(buf, off)
    }

    fn sync(&self) -> Result<(), FsError> {
        self.fs.step()?;

        self.file.sync()
    }

    fn truncate(&self, size: u64) -> Result<(), FsError> {
        self.file.truncate(size)
    }

    fn close(&self) -> Result<(), FsError> {
        self.file.close()
    }

    fn size(&self) -> Result<u64, FsError> {
        self.file.size()
    }
}

#[test]
fn test_preallocated_space_changes_nothing_in_a_file() {
    let dir = TempDir::new();
    let path = filepath::join(dir.path(), "segment");

    let f = match durable::OS.open_file(&path, OpenFlags::CREATE | OpenFlags::RDWR, 0o600) {
        Ok(f) => f,
        Err(e) => panic!("{e}"),
    };

    durable::preallocate(&*f, dir.path(), 1 << 20);
    if let Ok(size) = f.size()
        && size != 0
    {
        panic!("the visible size should not change, got {size}");
    }

    if let (_, Some(e)) = f.write_at(b"hello", 0) {
        panic!("{e}");
    }

    durable::trim_preallocated(&*f);

    let size = match f.size() {
        Ok(size) => size,
        Err(e) => panic!("{e}"),
    };
    if size != 5 {
        panic!("trimming should keep the data, size {size}");
    }

    let mut got = [0u8; 5];
    let (_, err) = f.read_at(&mut got, 0);
    if err.is_some() || &got != b"hello" {
        panic!("{:?} {err:?}", String::from_utf8_lossy(&got));
    }

    let _ = f.close();
}

#[derive(Clone, Default)]
struct MixedLedger {
    docs: HashMap<String, String>,
    gone: HashSet<String>,
    in_flight: HashSet<String>,
}

fn mixed_options(dir: &Path, fs: &MemFs, mode: journal::Mode) -> Options {
    let mut o = engine_options(dir, fs);
    o.durability = mode;
    o
}

fn run_mixed_workload(e: &Engine, l: &mut MixedLedger, steps: usize) {
    for i in 0..steps {
        let id = format!("m{:02}", i % 17);
        let value = format!("v{i}");
        l.in_flight.insert(id.clone());

        match i % 7 {
            5 => {
                if e.update(|tx| {
                    tx.delete(b"a", id.as_bytes());
                    Ok(())
                })
                .is_err()
                {
                    return;
                }
                l.docs.remove(&id);
                l.gone.insert(id.clone());
            }
            6 => {
                let other = format!("o{:02}", i % 5);
                if e.update(|tx| {
                    tx.ensure_type(b"a");
                    tx.ensure_type(b"b");
                    tx.put(
                        b"a",
                        id.as_bytes(),
                        &doc(vec![("id", text(id.clone())), ("v", text(value.clone()))]),
                    )?;
                    tx.put(
                        b"b",
                        other.as_bytes(),
                        &doc(vec![
                            ("id", text(other.clone())),
                            ("v", text(value.clone())),
                        ]),
                    )
                })
                .is_err()
                {
                    return;
                }
                l.docs.insert(id.clone(), value.clone());
                l.gone.remove(&id);
            }
            _ => {
                if e.update(|tx| {
                    tx.ensure_type(b"a");
                    tx.put(
                        b"a",
                        id.as_bytes(),
                        &doc(vec![
                            ("id", text(id.clone())),
                            ("v", text(value.clone())),
                            ("pad", text("p".repeat(100))),
                        ]),
                    )
                })
                .is_err()
                {
                    return;
                }
                l.docs.insert(id.clone(), value.clone());
                l.gone.remove(&id);
            }
        }
        l.in_flight.remove(&id);

        if i % 11 == 10 {
            let _ = e.checkpoint();
        } else if i % 19 == 18 {
            let _ = e.compact(2);
        } else if i == 45 {
            let _ = e.compact_pages();
        } else if i % 29 == 28 {
            if e.update(|tx| {
                tx.reset_entity(b"b");
                Ok(())
            })
            .is_err()
            {
                return;
            }
        } else if i % 31 == 30 {
            let point = e.point_in_time();
            if let Some(snap) = point.snapshots.get(b"a".as_slice()) {
                snap.scan_values(|_, raw| raw.is_ok());
            }
            point.release();
        }
    }
}

fn check_mixed(label: &str, e: &Engine, l: &MixedLedger) {
    let snap = e.snapshot(b"a");
    for (id, want) in &l.docs {
        let Some(d) = snap.get(id.as_bytes()) else {
            assert!(
                l.in_flight.contains(id),
                "{label}: confirmed document {id} is lost"
            );
            continue;
        };

        let m = d
            .decode()
            .unwrap_or_else(|err| panic!("{label}: document {id} is unreadable: {err}"));
        let got = m.get("v").and_then(Value::as_str);
        assert!(
            got == Some(want.as_str()) || l.in_flight.contains(id),
            "{label}: document {id} is {got:?}, confirmed {want}"
        );
    }

    for id in &l.gone {
        assert!(
            snap.get(id.as_bytes()).is_none() || l.in_flight.contains(id),
            "{label}: confirmed delete of {id} is lost"
        );
    }

    snap.ascend(0, |d| {
        if let Err(err) = d.decode() {
            panic!("{label}: stored document {:?} is unreadable: {err}", d.id());
        }
        true
    });
}

fn crash_mixed(
    dir: &Path,
    mode: journal::Mode,
    crash_at: i64,
    steps: usize,
) -> (MemFs, MixedLedger) {
    let fs = new_mem_fs();
    fs.set_crash_at(crash_at);
    let e = match datastore::open(mixed_options(dir, &fs, mode)) {
        Ok(e) => e,
        Err(err) => {
            if crash_at > 0 {
                return (fs, MixedLedger::default());
            }
            panic!("{err}");
        }
    };

    let mut l = MixedLedger::default();
    run_until_down(&fs, &e, || {
        let _ = catch_unwind(AssertUnwindSafe(|| run_mixed_workload(&e, &mut l, steps)));
    });
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = e.close();
    }));
    e.abandon();

    (fs, l)
}

fn reopen_mixed(label: &str, dir: &Path, image: &MemFs, mode: journal::Mode, l: &MixedLedger) {
    let e = datastore::open(mixed_options(dir, image, mode))
        .unwrap_or_else(|err| panic!("{label}: cannot start: {err}"));

    check_mixed(label, &e, l);

    e.update(|tx| {
        tx.ensure_type(b"a");
        tx.put(
            b"a",
            b"after",
            &doc(vec![("id", text("after")), ("v", text("after"))]),
        )
    })
    .unwrap_or_else(|err| panic!("{label}: cannot write after recovery: {err}"));

    e.close()
        .unwrap_or_else(|err| panic!("{label}: cannot close after recovery: {err}"));

    let e = datastore::open(mixed_options(dir, image, mode))
        .unwrap_or_else(|err| panic!("{label}: cannot start twice: {err}"));
    check_mixed(&format!("{label} (second start)"), &e, l);
    assert!(
        e.snapshot(b"a").get(b"after").is_some(),
        "{label}: the write after recovery is lost"
    );
    e.abandon();
}

#[test]
fn test_process_crash_at_every_point_keeps_confirmed_writes() {
    const STEPS: usize = 70;
    let tmp = TempDir::new();
    let dir = tmp.path();

    let (clean, _) = crash_mixed(dir, journal::Mode::Write, 0, STEPS);
    let total = clean.ops();
    assert!(
        total >= 150,
        "the workload should touch the disk many times, got {total}"
    );

    let mut at = 1;
    while at <= total {
        let (fs, l) = crash_mixed(dir, journal::Mode::Write, at, STEPS);
        reopen_mixed(
            &format!("crash at {at}/{total}"),
            dir,
            &fs.after_process_crash(),
            journal::Mode::Write,
            &l,
        );
        at += 1 + total / 300;
    }
}

#[test]
fn test_power_cut_at_every_point_keeps_synced_writes() {
    const STEPS: usize = 70;
    let tmp = TempDir::new();
    let dir = tmp.path();

    let (clean, _) = crash_mixed(dir, journal::Mode::Sync, 0, STEPS);
    let total = clean.ops();

    let mut at = 1;
    while at <= total {
        let (fs, l) = crash_mixed(dir, journal::Mode::Sync, at, STEPS);
        reopen_mixed(
            &format!("power cut at {at}/{total}"),
            dir,
            &fs.after_power_loss(),
            journal::Mode::Sync,
            &l,
        );
        at += 1 + total / 300;
    }
}

#[test]
fn test_concurrent_writers_share_their_syncs() {
    let fs = GateFs::new(new_mem_fs());
    fs.set_delay(Duration::from_micros(200));
    let tmp = TempDir::new();
    let shared: Arc<dyn Fs> = Arc::new(fs.clone());
    let e = datastore::open(Options {
        folder: tmp.path().to_path_buf(),
        shards: 4,
        durability: journal::Mode::Sync,
        fs: Some(shared),
        ..Options::default()
    })
    .unwrap_or_else(|err| panic!("{err}"));

    const WRITERS: usize = 32;
    const EACH: usize = 20;

    let before = fs.syncs();
    std::thread::scope(|sc| {
        for w in 0..WRITERS {
            let e = &e;
            sc.spawn(move || {
                for i in 0..EACH {
                    let id = format!("w{w:02}-{i:03}");
                    e.update(|tx| {
                        tx.ensure_type(b"a");
                        tx.put(b"a", id.as_bytes(), &doc(vec![("id", text(id.clone()))]))
                    })
                    .unwrap_or_else(|err| panic!("{err}"));
                }
            });
        }
    });

    let syncs = fs.syncs() - before;
    assert!(
        (syncs as usize) < WRITERS * EACH,
        "writers waiting together should share one sync: {syncs} syncs for {} writes",
        WRITERS * EACH
    );

    assert_eq!(e.snapshot(b"a").len(), WRITERS * EACH);
    e.abandon();
}

fn log_image(dir: &Path) -> (MemFs, MixedLedger) {
    let fs = new_mem_fs();
    let e = datastore::open(mixed_options(dir, &fs, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("{err}"));

    let mut l = MixedLedger::default();
    run_mixed_workload(&e, &mut l, 30);
    let _ = e.checkpoint();
    for i in 0..40 {
        let id = format!("tail{i:02}");
        e.update(|tx| {
            tx.ensure_type(b"a");
            tx.put(
                b"a",
                id.as_bytes(),
                &doc(vec![("id", text(id.clone())), ("v", text(id.clone()))]),
            )
        })
        .unwrap_or_else(|err| panic!("{err}"));
    }
    e.abandon();

    (fs, l)
}

#[test]
fn test_a_torn_log_is_never_read_as_valid() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let (fs, l) = log_image(dir);

    let segments = fs.list(&filepath::join(dir, "log"));
    let last = segments
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("no log segment"));
    let last = Path::new(&last);

    let mut size = 0usize;
    fs.mutate(last, |n| size = n.data.len());

    let mut cut = size as i64;
    while cut > size as i64 - 600 && cut > 0 {
        let c = cut as usize;
        for damage in ["cut", "flip", "zero"] {
            let image = fs.after_process_crash();
            image.mutate(last, |n| {
                match damage {
                    "cut" => n.data.truncate(c),
                    "flip" => {
                        if c < n.data.len() {
                            n.data[c] ^= 0xa5;
                        }
                    }
                    _ => {
                        for b in n.data.iter_mut().skip(c) {
                            *b = 0;
                        }
                    }
                }
                n.synced = n.data.clone();
            });

            let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
                .unwrap_or_else(|err| panic!("{damage} at {c}: cannot start: {err}"));

            let snap = e.snapshot(b"a");
            let mut seen = 0usize;
            let mut gap = false;
            for i in 0..40 {
                let Some(d) = snap.get(format!("tail{i:02}").as_bytes()) else {
                    gap = true;
                    continue;
                };
                assert!(
                    !gap,
                    "{damage} at {c}: tail{i:02} follows a hole, the log was not replayed in order"
                );
                seen += 1;
                if let Err(err) = d.decode() {
                    panic!("{damage} at {c}: document unreadable: {err}");
                }
            }

            for (id, want) in &l.docs {
                let d = snap
                    .get(id.as_bytes())
                    .unwrap_or_else(|| panic!("{damage} at {c}: checkpointed document {id} lost"));
                let m = d.decode().unwrap_or_default();
                assert_eq!(
                    m.get("v").and_then(Value::as_str),
                    Some(want.as_str()),
                    "{damage} at {c}: {id}"
                );
            }

            e.update(|tx| {
                tx.ensure_type(b"a");
                tx.put(
                    b"a",
                    b"again",
                    &doc(vec![("id", text("again")), ("v", text("again"))]),
                )
            })
            .unwrap_or_else(|err| panic!("{damage} at {c}: cannot write: {err}"));
            e.abandon();

            let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
                .unwrap_or_else(|err| panic!("{damage} at {c}: cannot start again: {err}"));
            assert!(
                e.snapshot(b"a").get(b"again").is_some(),
                "{damage} at {c}: the write after the repair is lost"
            );
            assert!(
                e.snapshot(b"a").len() >= seen,
                "{damage} at {c}: documents went missing"
            );
            e.abandon();
        }
        cut -= 7;
    }
}

#[test]
fn test_damage_before_the_end_of_the_log_sets_the_rest_aside() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let fs = new_mem_fs();
    let mut o = mixed_options(dir, &fs, journal::Mode::Sync);
    o.segment_size = 2 << 10;
    let e = datastore::open(o).unwrap_or_else(|err| panic!("{err}"));

    for i in 0..80 {
        let id = format!("d{i:02}");
        e.update(|tx| {
            tx.ensure_type(b"a");
            tx.put(
                b"a",
                id.as_bytes(),
                &doc(vec![
                    ("id", text(id.clone())),
                    ("pad", text("p".repeat(120))),
                ]),
            )
        })
        .unwrap_or_else(|err| panic!("{err}"));
    }
    e.abandon();

    let segments = fs.list(&filepath::join(dir, "log"));
    assert!(
        segments.len() >= 3,
        "expected several segments, got {segments:?}"
    );

    let image = fs.after_power_loss();
    image.mutate(Path::new(&segments[1]), |n| {
        let mid = n.data.len() as i64 / 2;
        corrupt(n, mid)
    });

    let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("a damaged log must not prevent a start: {err}"));

    let n = e.snapshot(b"a").len();
    assert!(
        n != 0 && n < 80,
        "the replay stops at the damage, {n} documents"
    );

    let damaged = image
        .list(&filepath::join(dir, "log"))
        .iter()
        .filter(|name| name.ends_with(".damaged"))
        .count();
    assert!(damaged != 0, "what follows the damage is kept aside");

    e.update(|tx| tx.put(b"a", b"next", &doc(vec![("id", text("next"))])))
        .unwrap_or_else(|err| panic!("{err}"));
    e.abandon();
}

#[test]
fn test_an_interrupted_checkpoint_and_a_partial_page_are_ignored() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let (fs, l) = log_image(dir);

    let image = fs.after_process_crash();
    let pages = image.list(&filepath::join(dir, "pages"));
    let last = pages
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("no page segment"));
    image.mutate(Path::new(&last), |n| {
        n.data
            .extend_from_slice(b"half a page that a checkpoint never finished");
        n.synced = n.data.clone();
    });

    for name in ["MANIFEST.tmp", "kv-00000000000099999.ckpt.tmp"] {
        let f = image
            .open_file(
                &filepath::join(dir, name),
                OpenFlags::CREATE | OpenFlags::WRONLY,
                0o600,
            )
            .unwrap_or_else(|err| panic!("{err}"));
        let _ = f.write_at(b"partial", 0);
        let _ = f.close();
    }

    let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("{err}"));

    check_mixed("interrupted checkpoint", &e, &l);
    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));
    e.close().unwrap_or_else(|err| panic!("{err}"));

    let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("{err}"));
    check_mixed("after the next checkpoint", &e, &l);
    e.abandon();
}

type TxResult = Result<(), elysiandb::gocompat::errors::BoxError>;

#[test]
fn test_a_full_disk_refuses_writes_and_comes_back() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let fs = new_mem_fs();
    let e = datastore::open(mixed_options(dir, &fs, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("{err}"));

    let put = |id: &str| -> TxResult {
        e.update(|tx| {
            tx.ensure_type(b"a");
            tx.put(
                b"a",
                id.as_bytes(),
                &doc(vec![("id", text(id)), ("pad", text("x".repeat(300)))]),
            )
        })
    };
    let disk_full = |res: TxResult| match res {
        Ok(()) => false,
        Err(err) => datastore::is_error(err.as_ref(), datastore::ERR_DISK_FULL),
    };

    put("first").unwrap_or_else(|err| panic!("{err}"));

    fs.set_quota(fs.usage());

    let start = Instant::now();
    assert!(
        disk_full(put("no-room")),
        "a write on a full disk is refused with a clear error"
    );
    assert!(
        start.elapsed() <= Duration::from_secs(10),
        "the refusal took {:?}",
        start.elapsed()
    );

    let start = Instant::now();
    assert!(disk_full(put("next")), "the next write is refused too");
    assert!(
        start.elapsed() <= Duration::from_secs(1),
        "the next refusal should be immediate, took {:?}",
        start.elapsed()
    );

    assert!(
        disk_full(e.kv_put(b"k", b"v", 0)),
        "key writes are refused too"
    );

    assert!(
        e.snapshot(b"a").get(b"first").is_some(),
        "reads are served on a full disk"
    );

    assert!(
        e.refused_writes() >= 3,
        "refusals are counted: {}",
        e.refused_writes()
    );

    fs.set_quota(-1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(refusal) = e.write_refusal() {
        assert!(Instant::now() < deadline, "writes never resume: {refusal}");
        std::thread::sleep(Duration::from_millis(20));
    }

    put("after").unwrap_or_else(|err| panic!("writes resume by themselves: {err}"));

    e.checkpoint()
        .unwrap_or_else(|err| panic!("checkpoints resume: {err}"));
    e.abandon();

    let image = fs.after_power_loss();
    let e = datastore::open(mixed_options(dir, &image, journal::Mode::Sync))
        .unwrap_or_else(|err| panic!("{err}"));

    let snap = e.snapshot(b"a");
    for id in ["first", "after"] {
        assert!(snap.get(id.as_bytes()).is_some(), "{id} is lost");
    }
    assert!(
        snap.get(b"next").is_none(),
        "a refused write must not appear"
    );
    e.abandon();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("{e}"));
    let entries = std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst);
            continue;
        }
        std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("{e}"));
    }
}

fn snapshot_state(e: &Engine) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut state = BTreeMap::new();
    for entity in e.types() {
        let snap = e.snapshot(&entity);
        snap.ascend(0, |d| {
            let raw = d.bytes().unwrap_or_else(|err| panic!("{err}"));
            let mut k = entity.clone();
            k.push(b'/');
            k.extend_from_slice(&d.id());
            state.insert(k, raw.to_vec());
            true
        });
    }

    for key in e.kv_keys() {
        let v = e.kv_get(&key).and_then(|(v, _)| v).unwrap_or_default();
        let mut k = b"kv/".to_vec();
        k.extend_from_slice(&key);
        state.insert(k, v);
    }

    state
}

fn plain_options(dir: &Path, fs: Option<Arc<dyn Fs>>) -> Options {
    Options {
        folder: dir.to_path_buf(),
        shards: 8,
        durability: journal::Mode::Write,
        fs,
        ..Options::default()
    }
}

#[test]
fn test_a_migration_interrupted_at_any_step_completes_at_the_next_start() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/compat/testdata/v0.2.4/full");

    let tmp = TempDir::new();
    let reference = filepath::join(tmp.path(), "reference");
    copy_tree(&fixture, &reference);
    let e = datastore::open(plain_options(&reference, None)).unwrap_or_else(|err| panic!("{err}"));
    let want = snapshot_state(&e);
    assert!(!want.is_empty(), "the fixture holds nothing");
    e.close().unwrap_or_else(|err| panic!("{err}"));

    let probe = filepath::join(tmp.path(), "probe");
    copy_tree(&fixture, &probe);
    let os: Arc<dyn Fs> = Arc::new(durable::OsFs);
    let counter = FailingFs::new(os.clone(), FsError::errno(libc::EIO));
    counter.0.left.store(1 << 40, Ordering::SeqCst);
    let e = datastore::open(plain_options(&probe, Some(Arc::new(counter.clone()))))
        .unwrap_or_else(|err| panic!("{err}"));
    let total = counter.0.counted.load(Ordering::SeqCst);
    e.abandon();
    assert!(
        total >= 20,
        "the migration should touch the disk many times, got {total}"
    );

    let mut at = 0;
    while at < total {
        let run = TempDir::new();
        let folder = filepath::join(run.path(), "folder");
        copy_tree(&fixture, &folder);

        let faulty = FailingFs::new(os.clone(), FsError::errno(libc::EIO));
        faulty.0.left.store(at, Ordering::SeqCst);
        if let Ok(e) = datastore::open(plain_options(&folder, Some(Arc::new(faulty)))) {
            e.abandon();
        }

        let e = datastore::open(plain_options(&folder, None)).unwrap_or_else(|err| {
            panic!("stopped at step {at} of {total}: the next start fails: {err}")
        });

        let got = snapshot_state(&e);
        e.abandon();

        assert_eq!(
            got.len(),
            want.len(),
            "stopped at step {at} of {total}: items"
        );
        for (k, v) in &want {
            assert!(
                got.get(k) == Some(v),
                "stopped at step {at} of {total}: {} differs",
                String::from_utf8_lossy(k)
            );
        }
        at += 1 + total / 80;
    }
}
