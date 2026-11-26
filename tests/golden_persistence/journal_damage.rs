use elysiandb::durable::filepath;
use elysiandb::journal::{self, Position};

use super::common;
use super::support::{array, assert_same_tree, copy_tree, dec_u64, field, int, uint};
use super::tempdir::TempDir;

fn u32s(v: &[elysiandb::gocompat::json::Value]) -> Vec<u32> {
    v.iter()
        .map(|x| x.as_u64().expect("an unsigned integer") as u32)
        .collect()
}

#[test]
fn journal_damage_replays_like_go() {
    let root = common::golden_path("persistence/journal/damage");
    let mut cases: Vec<String> = std::fs::read_dir(&root)
        .expect("damage cases")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    cases.sort();
    assert_eq!(
        cases,
        [
            "cut-1",
            "cut-17",
            "cut-20",
            "cut-5",
            "flip-mid-first",
            "garbage-tail",
            "intact",
            "ten-byte-file",
            "wrong-header-id",
            "zero-tail",
            "zeroed-last-bytes",
        ]
    );

    let mut checked = 0;
    for case in &cases {
        let stats_doc = common::read_json(&format!("persistence/journal/damage/{case}/stats.json"));
        let tmp = TempDir::new();
        let dir = filepath::join(tmp.path(), "log");
        copy_tree(&root.join(case).join("before"), &dir);

        let j = journal::open(
            &dir,
            journal::Options {
                mode: journal::Mode::Sync,
                segment_size: 4096,
                ..journal::Options::default()
            },
        )
        .unwrap_or_else(|e| panic!("{case}: {e}"));
        let mut applied: Vec<u64> = Vec::new();
        let stats = j
            .replay(
                Position {
                    segment: 1,
                    offset: 0,
                },
                |recs| {
                    applied.extend(recs.iter().map(|r| r.lsn));
                    Ok(())
                },
            )
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        let next = j.next_lsn();
        let pos = j.position();
        j.close().unwrap_or_else(|e| panic!("{case}: {e}"));

        assert_eq!(
            stats.set_aside,
            u32s(array(&stats_doc, "set_aside")),
            "{case}"
        );
        assert_eq!(stats.damaged, u32s(array(&stats_doc, "damaged")), "{case}");
        assert_eq!(stats.batches as i64, int(&stats_doc, "batches"), "{case}");
        assert_eq!(stats.records as i64, int(&stats_doc, "records"), "{case}");
        assert_eq!(stats.truncated, int(&stats_doc, "truncated"), "{case}");
        assert_eq!(stats.last_lsn, dec_u64(&stats_doc, "last_lsn"), "{case}");
        let want_applied: Vec<u64> = array(&stats_doc, "applied")
            .iter()
            .map(|v| v.as_str().expect("decimal").parse().expect("u64"))
            .collect();
        assert_eq!(applied, want_applied, "{case}");
        assert_eq!(next, dec_u64(&stats_doc, "next_lsn"), "{case}");
        let want_pos = field(&stats_doc, "position");
        assert_eq!(pos.segment, uint(want_pos, "segment") as u32, "{case}");
        assert_eq!(pos.offset, uint(want_pos, "offset") as u32, "{case}");

        assert_same_tree(&dir, &root.join(case).join("after"), case);
        checked += 1;
    }

    assert_eq!(checked, cases.len());
    assert_eq!(checked, 11);
}
