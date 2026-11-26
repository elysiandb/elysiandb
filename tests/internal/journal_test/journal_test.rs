use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use elysiandb::durable::{DirEntry, FsError, OpenFlags, filepath};
use elysiandb::gocompat::errors;
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::journal::{self, File, Fs, Journal, Location, Position, Record, ReplayStats};

use super::tempdir::TempDir;

fn record(i: usize) -> Record {
    Record {
        kind: journal::KIND_DOC_PUT,
        entity: b"books".to_vec(),
        key: format!("id-{i:05}").into_bytes(),
        r#gen: 1,
        value: format!(r#"{{"n":{i}}}"#).into_bytes(),
        ..Record::default()
    }
}

fn open_journal(dir: &Path, opts: journal::Options) -> Arc<Journal> {
    match journal::open(dir, opts) {
        Ok(j) => j,
        Err(e) => panic!("{e}"),
    }
}

fn replay_all(j: &Journal) -> (Vec<Record>, ReplayStats) {
    let mut out = Vec::new();
    let res = j.replay(
        Position {
            segment: 1,
            offset: 0,
        },
        |recs| {
            out.extend(recs.iter().cloned());
            Ok(())
        },
    );
    match res {
        Ok(stats) => (out, stats),
        Err(e) => panic!("{e}"),
    }
}

fn opts(mode: journal::Mode, segment_size: i64) -> journal::Options {
    journal::Options {
        mode,
        segment_size,
        ..journal::Options::default()
    }
}

fn glob(dir: &Path, suffix: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(suffix))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[test]
fn append_replay_across_segments() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 4096));
    j.start();

    let mut locs: Vec<Location> = Vec::new();
    for i in (0..500).step_by(5) {
        let mut batch = vec![
            record(i),
            record(i + 1),
            record(i + 2),
            record(i + 3),
            record(i + 4),
        ];
        let ticket = match j.append(&mut batch) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };
        if let Err(e) = ticket.wait() {
            panic!("{e}");
        }
        for r in &batch {
            locs.push(r.loc);
        }
    }

    for (i, loc) in locs.iter().enumerate() {
        match j.read_value(*loc) {
            Ok(v) if v == record(i).value => {}
            other => panic!("value {i}: {other:?}"),
        }
    }

    if let Err(e) = j.close() {
        panic!("{e}");
    }

    let segments = j.segments().unwrap_or_default();
    if segments.len() < 5 {
        panic!(
            "expected the log to rotate, got {} segments",
            segments.len()
        );
    }

    let j = open_journal(dir.path(), opts(journal::Mode::Write, 4096));
    let (recs, stats) = replay_all(&j);
    if recs.len() != 500 || stats.batches != 100 {
        panic!(
            "replayed {} records in {} batches",
            recs.len(),
            stats.batches
        );
    }

    for (i, r) in recs.iter().enumerate() {
        if r.key != record(i).key
            || r.value != record(i).value
            || r.lsn != (i + 1) as u64
            || r.loc != locs[i]
        {
            panic!("record {i} differs: {r:?}");
        }
    }

    if j.next_lsn() != 501 {
        panic!("next LSN {}", j.next_lsn());
    }

    j.start();
    let mut batch = vec![record(500)];
    let ticket = j.append(&mut batch);
    match ticket {
        Ok(t) if t.wait().is_ok() => {}
        other => panic!("{:?}", other.err()),
    }
    if let Err(e) = j.close() {
        panic!("{e}");
    }

    let j = open_journal(dir.path(), opts(journal::Mode::Background, 4096));
    let (recs, _) = replay_all(&j);
    if recs.len() != 501 || recs[500].lsn != 501 {
        panic!(
            "appending after a replay continues the log, got {} records",
            recs.len()
        );
    }
    j.abandon();
}

#[test]
fn torn_last_batch_is_dropped() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 0));
    j.start();
    for i in 0..3 {
        let mut batch = vec![record(i)];
        if let Ok(ticket) = j.append(&mut batch) {
            let _ = ticket.wait();
        }
    }
    let _ = j.close();

    let path = filepath::join(dir.path(), journal::segment_name(1));
    let data = std::fs::read(&path).unwrap_or_default();
    let full = data.len();

    for cut in [1, 5, 17, 20] {
        if let Err(e) = std::fs::write(&path, &data[..full - cut]) {
            panic!("{e}");
        }

        let j = open_journal(dir.path(), journal::Options::default());
        let (recs, stats) = replay_all(&j);
        j.abandon();
        if recs.len() != 2 || stats.truncated == 0 {
            panic!(
                "cut {cut}: expected 2 records and a truncation, got {} {stats:?}",
                recs.len()
            );
        }
    }

    let mut garbage = data.clone();
    garbage.extend_from_slice(b"not a batch at all");
    let _ = std::fs::write(&path, &garbage);
    let j = open_journal(dir.path(), journal::Options::default());
    let (recs, stats) = replay_all(&j);
    j.abandon();
    if recs.len() != 3 || stats.truncated != "not a batch at all".len() as i64 {
        panic!(
            "garbage after the last batch is dropped, got {} {stats:?}",
            recs.len()
        );
    }
}

#[test]
fn corruption_in_a_sealed_segment_ends_the_replay_and_keeps_the_rest_aside() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 256));
    j.start();
    for i in 0..20 {
        let mut batch = vec![record(i)];
        if let Ok(ticket) = j.append(&mut batch) {
            let _ = ticket.wait();
        }
    }
    let _ = j.close();

    let path = filepath::join(dir.path(), journal::segment_name(1));
    let mut data = std::fs::read(&path).unwrap_or_default();
    let at = data.len() - 3;
    data[at] ^= 0xff;
    let _ = std::fs::write(&path, &data);

    let before = glob(dir.path(), ".seg");

    let j = open_journal(dir.path(), journal::Options::default());
    let mut replayed = 0;
    let res = j.replay(
        Position {
            segment: 1,
            offset: 0,
        },
        |recs| {
            replayed += recs.len();
            Ok(())
        },
    );
    j.abandon();
    let stats = match res {
        Ok(stats) => stats,
        Err(e) => panic!("damage is not a reason to refuse to start: {e}"),
    };

    if replayed == 0 || replayed >= 20 || stats.truncated == 0 {
        panic!("the replay stops at the damage, {replayed} records, {stats:?}");
    }

    if stats.set_aside.len() != before.len() - 1 {
        panic!(
            "every later segment is set aside, {} of {}: {stats:?}",
            stats.set_aside.len(),
            before.len() - 1
        );
    }

    let damaged = glob(dir.path(), ".damaged");
    let left = glob(dir.path(), ".seg");
    if damaged.len() != stats.set_aside.len() || left.len() != 1 {
        panic!("segments are kept aside, never deleted: {damaged:?}, {left:?}");
    }
}

struct FaultyFs {
    mode: AtomicI32,
}

struct FaultyFile {
    file: Arc<dyn File>,
    fs: Arc<FaultyFs>,
}

impl File for FaultyFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        self.file.read_at(buf, off)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        match self.fs.mode.load(Ordering::SeqCst) {
            1 => return (0, Some(FsError::errno(libc::ENOSPC))),
            2 => {
                let (n, _) = self.file.write_at(&buf[..buf.len() / 2], off);
                return (n, None);
            }
            _ => {}
        }

        self.file.write_at(buf, off)
    }

    fn sync(&self) -> Result<(), FsError> {
        if self.fs.mode.load(Ordering::SeqCst) == 3 {
            return Err(FsError::other("sync failed"));
        }

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

#[derive(Clone)]
struct FaultyHandle(Arc<FaultyFs>);

impl Fs for FaultyHandle {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        let file = journal::OSFS.open_file(path, flags, mode)?;

        Ok(Arc::new(FaultyFile {
            file,
            fs: self.0.clone(),
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        journal::OSFS.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        journal::OSFS.rename(from, to)
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        journal::OSFS.sync_dir(path)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        journal::OSFS.read_dir(path)
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        journal::OSFS.mkdir_all(path, mode)
    }
}

#[test]
fn write_faults_are_reported_and_retried() {
    for (name, mode) in [("failed sync", 3), ("no space", 1), ("short write", 2)] {
        let dir = TempDir::new();
        let fs = FaultyHandle(Arc::new(FaultyFs {
            mode: AtomicI32::new(0),
        }));
        let reported = Arc::new(AtomicI32::new(0));
        let counter = reported.clone();
        let j = open_journal(
            dir.path(),
            journal::Options {
                mode: journal::Mode::Sync,
                fs: Some(Arc::new(fs.clone())),
                on_write_error: Some(Arc::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                })),
                ..journal::Options::default()
            },
        );
        j.start();

        let mut first = vec![record(0)];
        match j.append(&mut first) {
            Ok(t) => {
                if let Err(e) = t.wait() {
                    panic!("{name}: {e}");
                }
            }
            Err(e) => panic!("{name}: {e}"),
        }

        fs.0.mode.store(mode, Ordering::SeqCst);
        let mut batch = vec![record(1)];
        let ticket = j.append(&mut batch).unwrap_or_default();
        if ticket.wait().is_ok() {
            panic!("{name}: the fault must reach the writer");
        }
        if reported.load(Ordering::SeqCst) == 0 {
            panic!("{name}: the fault must be reported");
        }

        match j.read_value(batch[0].loc) {
            Ok(v) if v == record(1).value => {}
            other => panic!("{name}: an unwritten value stays readable: {other:?}"),
        }

        fs.0.mode.store(0, Ordering::SeqCst);
        if let Err(e) = j.flush() {
            panic!("{name}: the write is retried once the disk works: {e}");
        }
        if let Err(e) = j.close() {
            panic!("{name}: {e}");
        }

        let j = open_journal(dir.path(), journal::Options::default());
        let (recs, _) = replay_all(&j);
        j.abandon();
        if recs.len() != 2 || recs[1].key != record(1).key {
            panic!("{name}: both records reach the disk, got {}", recs.len());
        }
    }
}

#[test]
fn group_commit_under_concurrent_writers() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 0));
    j.start();

    std::thread::scope(|s| {
        for g in 0..32 {
            let j = &j;
            s.spawn(move || {
                for i in 0..100 {
                    let mut batch = vec![record(g * 1000 + i)];
                    let ticket = match j.append(&mut batch) {
                        Ok(t) => t,
                        Err(e) => panic!("{e}"),
                    };
                    if let Err(e) = ticket.wait() {
                        panic!("{e}");
                    }
                    match j.read_value(batch[0].loc) {
                        Ok(v) if v == batch[0].value => {}
                        other => panic!("read back {other:?}"),
                    }
                }
            });
        }
    });
    let _ = j.close();

    let j = open_journal(dir.path(), journal::Options::default());
    let (recs, stats) = replay_all(&j);
    j.abandon();
    if recs.len() != 3200 || stats.batches != 3200 {
        panic!("replayed {} records", recs.len());
    }

    for (i, r) in recs.iter().enumerate() {
        if r.lsn != (i + 1) as u64 {
            panic!("LSNs follow the log order, record {i} has {}", r.lsn);
        }
    }
}

#[test]
fn background_mode_writes_on_flush() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Background, 0));
    j.start();

    let mut batch = vec![record(0)];
    match j.append(&mut batch) {
        Ok(t) => {
            if let Err(e) = t.wait() {
                panic!("{e}");
            }
        }
        Err(e) => panic!("{e}"),
    }

    if j.pending_bytes() == 0 {
        panic!("background mode keeps the batch in memory until a flush");
    }

    match j.read_value(batch[0].loc) {
        Ok(v) if v == record(0).value => {}
        other => panic!("a pending value is readable: {other:?}"),
    }

    if let Err(e) = j.flush() {
        panic!("{e}");
    }
    if j.pending_bytes() != 0 {
        panic!("a flush writes everything");
    }
    let _ = j.close();
}

fn replay_input(data: &[u8]) {
    let d = TempDir::new();
    let _ = std::fs::create_dir_all(d.path());
    let _ = std::fs::write(filepath::join(d.path(), journal::segment_name(1)), data);
    let Ok(j) = journal::open(d.path(), journal::Options::default()) else {
        return;
    };
    let _ = j.replay(
        Position {
            segment: 1,
            offset: 0,
        },
        |_| Ok(()),
    );
    j.abandon();
}

fn mutate(rng: &mut Rand, seed: &[u8]) -> Vec<u8> {
    let mut data = seed.to_vec();
    let edits = 1 + rng.intn(4);
    for _ in 0..edits {
        match rng.intn(6) {
            0 if !data.is_empty() => {
                let i = rng.intn(data.len() as i64) as usize;
                data[i] ^= 1 << rng.intn(8);
            }
            1 if !data.is_empty() => {
                let i = rng.intn(data.len() as i64) as usize;
                data[i] = rng.intn(256) as u8;
            }
            2 => {
                let n = rng.intn(data.len() as i64 + 1) as usize;
                data.truncate(n);
            }
            3 => {
                let i = rng.intn(data.len() as i64 + 1) as usize;
                let n = 1 + rng.intn(32) as usize;
                let extra: Vec<u8> = (0..n).map(|_| rng.intn(256) as u8).collect();
                data.splice(i..i, extra);
            }
            4 if data.len() >= 4 => {
                let i = rng.intn(data.len() as i64 - 3) as usize;
                let v = rng.uint32().to_le_bytes();
                data[i..i + 4].copy_from_slice(&v);
            }
            _ => {
                let i = rng.intn(data.len() as i64 + 1) as usize;
                data.splice(i..i, 0x3142_4c45u32.to_le_bytes());
            }
        }
    }
    data
}

#[test]
fn fuzz_replay_never_panics() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 0));
    j.start();
    let mut batch = vec![record(1), record(2)];
    if let Ok(ticket) = j.append(&mut batch) {
        let _ = ticket.wait();
    }
    let _ = j.close();
    let seed =
        std::fs::read(filepath::join(dir.path(), journal::segment_name(1))).unwrap_or_default();
    let seeds = [seed.clone(), seed[..20].to_vec()];

    for data in &seeds {
        replay_input(data);
    }

    let mut rng = Rand::new(rand::new_source(206));
    for i in 0..2000 {
        let base = &seeds[i % seeds.len()];
        let data = mutate(&mut rng, base);
        replay_input(&data);
    }
}

#[test]
fn abandoned_journal_never_writes_its_pending_batches() {
    let dir = TempDir::new();
    let j = open_journal(dir.path(), opts(journal::Mode::Write, 0));
    j.start();

    let mut first = vec![record(0)];
    match j.append(&mut first) {
        Ok(t) => {
            if let Err(e) = t.wait() {
                panic!("{e}");
            }
        }
        Err(e) => panic!("{e}"),
    }

    let other = TempDir::new();
    let background = open_journal(other.path(), opts(journal::Mode::Background, 0));
    background.start();
    if let Err(e) = background.append(&mut [record(1)]) {
        panic!("{e}");
    }
    background.abandon();

    if background.flush().is_ok() {
        panic!("an abandoned journal must refuse to flush");
    }

    match background.append(&mut [record(2)]) {
        Err(e) if errors::is(&e, &journal::ERR_CLOSED) => {}
        other => panic!(
            "an abandoned journal must refuse new batches, got {:?}",
            other.err()
        ),
    }

    if let Ok(segments) = background.segments()
        && !segments.is_empty()
    {
        panic!("an abandoned journal must not write its pending batch, got {segments:?}");
    }

    j.abandon();

    let reopened = open_journal(dir.path(), opts(journal::Mode::Write, 0));
    let (recs, _) = replay_all(&reopened);
    if recs.len() != 1 {
        panic!("expected the written record only, got {}", recs.len());
    }

    reopened.start();
    for i in 1..50 {
        let mut batch = vec![record(i)];
        let ticket = match reopened.append(&mut batch) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };
        if let Err(e) = ticket.wait() {
            panic!("{e}");
        }
    }
    if let Err(e) = reopened.close() {
        panic!("{e}");
    }

    let last = open_journal(dir.path(), journal::Options::default());
    let (recs, stats) = replay_all(&last);
    last.abandon();
    if recs.len() != 50 || stats.truncated != 0 {
        panic!(
            "the log written after a reopening is intact, got {} records and {} truncated bytes",
            recs.len(),
            stats.truncated
        );
    }
}
