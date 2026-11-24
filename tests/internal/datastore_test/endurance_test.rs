use std::path::Path;
use std::time::{Duration, Instant};

use elysiandb::datastore::{INDEX_FILTER, INDEX_SORT, IndexSpec};
use elysiandb::durable::filepath;
use elysiandb::gocompat::json::Map;
use elysiandb::gocompat::rand;
use elysiandb::memory;

use super::tempdir::TempDir;
use super::{Closer, doc, n, open, options, s};

#[derive(Clone, Copy, Default)]
struct Sample {
    threads: usize,
    files: usize,
    log_bytes: i64,
    page_bytes: i64,
    heap: i64,
    dirty: i64,
}

#[cfg(target_os = "linux")]
fn threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|d| d.count())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn threads() -> usize {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if got != size {
        return 0;
    }
    info.pti_threadnum.max(0) as usize
}

fn open_descriptors() -> usize {
    std::fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0)
}

fn folder_bytes(dir: &Path) -> i64 {
    let mut total = 0i64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(info) = entry.metadata() {
                total += info.len() as i64;
            }
        }
    }
    total
}

#[test]
fn test_nothing_drifts_at_constant_volume() {
    let tmp = TempDir::new();
    let folder = tmp.path();
    let mut opts = options(folder);
    opts.background = true;
    opts.segment_size = 1 << 20;
    opts.page_segment_size = 1 << 20;
    opts.page_cache_bytes = 4 << 20;
    opts.cache_bytes = 4 << 20;
    opts.dirty_flush_bytes = 2 << 20;
    opts.max_log_bytes = 2 << 20;
    opts.checkpoint_every = Duration::from_millis(300);
    opts.retired_file_grace = Duration::from_secs(3);

    let e = Closer(open(&opts));
    let e = &e.0;

    const DOCUMENTS: usize = 1000;
    const COLD_DOCUMENTS: usize = 50;
    let r = std::cell::RefCell::new(rand::Rand::new(rand::new_source(5)));
    let make = |i: usize, round: usize| -> Map {
        let tag = r.borrow_mut().intn(40);
        let rank = r.borrow_mut().intn(100_000);
        doc(vec![
            ("id", s(format!("doc-{i:05}"))),
            ("tag", s(format!("t{tag}"))),
            ("rank", n(rank as f64)),
            ("round", n(round as f64)),
            ("pad", s(format!("{i:0300}"))),
        ])
    };

    let write_all = |round: usize| {
        let mut from = 0;
        while from < DOCUMENTS {
            let docs: Vec<Map> = (from..from + 200).map(|i| make(i, round)).collect();
            e.update(|tx| {
                tx.ensure_type(b"steady");
                for (k, d) in docs.iter().enumerate() {
                    tx.put(b"steady", format!("doc-{:05}", from + k).as_bytes(), d)?;
                }
                if round == 0 {
                    tx.ensure_type(b"cold");
                    for i in from / 20..from / 20 + 10 {
                        let id = format!("cold-{i:05}");
                        tx.put(
                            b"cold",
                            id.as_bytes(),
                            &doc(vec![("id", s(id.clone())), ("pad", s(format!("{i:0300}")))]),
                        )?;
                    }
                }
                Ok(())
            })
            .unwrap_or_else(|err| panic!("{err}"));
            from += 200;
        }

        for _ in 0..100 {
            let at = r.borrow_mut().intn(DOCUMENTS as i64) as usize;
            let d = make(at, round);
            e.update(|tx| tx.put(b"steady", format!("doc-{at:05}").as_bytes(), &d))
                .unwrap_or_else(|err| panic!("{err}"));
        }

        for i in 0..50 {
            e.kv_put(
                format!("key-{i}").as_bytes(),
                format!("{round:0200}").as_bytes(),
                0,
            )
            .unwrap_or_else(|err| panic!("{err}"));
        }
    };

    write_all(0);
    let specs = [
        IndexSpec::new(INDEX_FILTER, "tag"),
        IndexSpec::new(INDEX_SORT, "rank"),
    ];
    for spec in &specs {
        while !e.ensure_index(b"steady", spec) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let measure = || -> Sample {
        std::thread::sleep(Duration::from_millis(50));
        Sample {
            threads: threads(),
            files: open_descriptors(),
            log_bytes: e.stats().log_bytes,
            page_bytes: folder_bytes(&filepath::join(folder, "pages")),
            heap: memory::allocated(),
            dirty: e.dirty_bytes(),
        }
    };

    const ROUNDS: usize = 45;
    let mut samples: Vec<Sample> = Vec::with_capacity(ROUNDS);
    for round in 1..=ROUNDS {
        let began = Instant::now();
        write_all(round);

        let snap = e.indexed_snapshot(b"steady");
        assert_eq!(snap.len(), DOCUMENTS, "round {round}");
        for spec in &specs {
            if snap.index(spec).is_none() {
                while !e.ensure_index(b"steady", spec) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        if round % 5 == 0 {
            let view = e.point_in_time();
            let mut count = 0;
            view.snapshots[&b"steady".to_vec()].scan_values(|_, raw| {
                if let Err(err) = raw {
                    panic!("{err}");
                }
                count += 1;
                true
            });
            view.release();
            assert_eq!(count, DOCUMENTS, "round {round}: the view");
        }

        samples.push(measure());
        if let Some(rest) = Duration::from_secs(1).checked_sub(began.elapsed()) {
            std::thread::sleep(rest);
        }
    }

    let average = |part: &[Sample], pick: &dyn Fn(&Sample) -> f64| -> f64 {
        part.iter().map(pick).sum::<f64>() / part.len() as f64
    };

    let middle = &samples[ROUNDS / 3..2 * ROUNDS / 3];
    let last = &samples[2 * ROUNDS / 3..];
    type Pick = Box<dyn Fn(&Sample) -> f64>;
    let checks: Vec<(&str, Pick, f64, f64)> = vec![
        ("threads", Box::new(|s| s.threads as f64), 1.1, 4.0),
        ("file descriptors", Box::new(|s| s.files as f64), 1.1, 6.0),
        (
            "log size",
            Box::new(|s| s.log_bytes as f64),
            1.3,
            (4 << 20) as f64,
        ),
        (
            "page files size",
            Box::new(|s| s.page_bytes as f64),
            1.3,
            (4 << 20) as f64,
        ),
        ("heap", Box::new(|s| s.heap as f64), 1.3, (8 << 20) as f64),
    ];

    let mut drift = Vec::new();
    for (name, pick, ratio, slack) in &checks {
        let before = average(middle, pick.as_ref());
        let after = average(last, pick.as_ref());
        println!("{name}: {before:.0} then {after:.0}");
        if after > before * ratio + slack {
            drift.push(format!("{name} drifts: {before:.0} then {after:.0}"));
        }
    }
    println!(
        "dirty: {:.0} then {:.0}",
        average(middle, &|s| s.dirty as f64),
        average(last, &|s| s.dirty as f64)
    );

    let st = e.stats();
    if st.compacted_segments == 0 {
        drift.push(format!(
            "segments that keep a little live data should be compacted: {st:?}"
        ));
    }
    assert!(drift.is_empty(), "{}", drift.join("\n"));

    let cold = e.snapshot(b"cold");
    assert_eq!(
        cold.len(),
        COLD_DOCUMENTS,
        "documents that were never rewritten"
    );
    let mut count = 0;
    cold.scan_values(|_, raw| {
        if let Err(err) = raw {
            panic!("{err}");
        }
        count += 1;
        true
    });
    assert_eq!(
        count, COLD_DOCUMENTS,
        "documents can be read after compaction"
    );
}
