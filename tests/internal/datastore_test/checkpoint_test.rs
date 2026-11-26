use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use elysiandb::datastore::{self, INDEX_FILTER, IndexSpec, Options};
use elysiandb::durable::filepath;
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::journal;

use super::build_test::{put_docs, work_doc};
use super::tempdir::TempDir;
use super::{Closer, doc, open, options, s};

#[test]
fn stats_answer_during_a_checkpoint() {
    let tmp = TempDir::new();
    let mut opts = options(tmp.path());
    opts.segment_size = 0;
    let e = Closer(open(&opts));
    let e = &e.0;

    put_docs(e, b"big", 0, 60000, work_doc);
    for f in ["a", "b", "c", "text"] {
        e.ensure_index(b"big", &IndexSpec::new(INDEX_FILTER, f));
    }

    let mut slowest = Duration::ZERO;
    let mut longest = Duration::ZERO;
    for round in 0..3 {
        put_docs(e, b"big", 60000 + round, 60001 + round, work_doc);

        let (tx, rx) = mpsc::channel::<Duration>();
        std::thread::scope(|sc| {
            sc.spawn(|| {
                let t0 = Instant::now();
                if let Err(err) = e.checkpoint() {
                    panic!("{err}");
                }
                let _ = tx.send(t0.elapsed());
            });

            loop {
                match rx.try_recv() {
                    Ok(d) => {
                        longest = longest.max(d);
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {
                        let t0 = Instant::now();
                        let _ = e.stats();
                        slowest = slowest.max(t0.elapsed());
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        });
    }

    if longest < Duration::from_millis(20) {
        println!("checkpoints too fast to observe ({longest:?})");
        return;
    }
    assert!(
        slowest <= longest / 4,
        "Stats waited {slowest:?} during a checkpoint of {longest:?}"
    );
}

fn dir_bytes(dir: &Path) -> i64 {
    let mut total = 0i64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        if let Ok(info) = entry.metadata() {
            total += info.len() as i64;
        }
    }
    total
}

#[test]
fn disk_is_given_back_after_an_entity_is_emptied() {
    let tmp = TempDir::new();
    let dir = tmp.path();
    let e = Closer(
        datastore::open(Options {
            folder: dir.to_path_buf(),
            shards: 4,
            durability: journal::Mode::Background,
            page_segment_size: 1 << 20,
            segment_size: 1 << 20,
            background: true,
            ..Options::default()
        })
        .unwrap_or_else(|err| panic!("{err}")),
    );
    let e = &e.0;

    e.ensure_index(b"a", &IndexSpec::new(INDEX_FILTER, "v"));
    let mut i = 0;
    while i < 40000 {
        e.update(|tx| {
            tx.ensure_type(b"a");
            for j in i..i + 500 {
                tx.put(
                    b"a",
                    format!("doc-{j:06}").as_bytes(),
                    &doc(vec![
                        ("v", s(format!("value-{j}"))),
                        ("pad", s("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")),
                    ]),
                )?;
            }
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
        i += 500;
    }

    e.checkpoint().unwrap_or_else(|err| panic!("{err}"));

    let pages = filepath::join(dir, "pages");
    let full = dir_bytes(&pages);
    assert!(
        full >= 1 << 20,
        "the pages should hold several segments, got {full} bytes"
    );

    e.update(|tx| {
        tx.reset_entity(b"a");
        Ok(())
    })
    .unwrap_or_else(|err| panic!("{err}"));

    let deadline = Instant::now() + Duration::from_secs(30);
    while dir_bytes(&pages) > full / 4 {
        assert!(
            Instant::now() < deadline,
            "the pages of an emptied entity should be removed without further writes: {} bytes, {full} before",
            dir_bytes(&pages)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_reset_keeps_every_known_index_following_the_fill() {
    let tmp = TempDir::new();
    let e = Closer(
        datastore::open(Options {
            folder: tmp.path().to_path_buf(),
            shards: 4,
            durability: journal::Mode::Background,
            ..Options::default()
        })
        .unwrap_or_else(|err| panic!("{err}")),
    );
    let e = &e.0;

    let put = |id: &str, v: &str| {
        e.update(|tx| {
            tx.ensure_type(b"a");
            tx.put(b"a", id.as_bytes(), &doc(vec![("v", s(v))]))
        })
        .unwrap_or_else(|err| panic!("{err}"));
    };

    let spec = IndexSpec::new(INDEX_FILTER, "v");
    put("x", "one");
    e.ensure_index(b"a", &spec);
    let hour = GoDuration(-GoDuration::HOUR.0);
    e.drop_idle_indexes(hour);
    e.drop_idle_indexes(hour);

    assert!(
        e.snapshot(b"a").index_dormant(&spec),
        "the index should be released"
    );

    e.update(|tx| {
        tx.reset_entity(b"a");
        Ok(())
    })
    .unwrap_or_else(|err| panic!("{err}"));

    put("y", "two");
    let view = e.snapshot(b"a").index(&spec);
    assert!(
        view.as_ref().is_some_and(|v| !v.is_empty()),
        "the index should have followed the insert after the reset: {}",
        view.is_some()
    );
}
