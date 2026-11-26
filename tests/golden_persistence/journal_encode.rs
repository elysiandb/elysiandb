use elysiandb::durable::filepath;
use elysiandb::gocompat::json::Value;
use elysiandb::journal::{self, Kind, Position, Record};

use super::common;
use super::support::{
    array, assert_same_tree, copy_tree, dec_i64, dec_u64, field, hex_bytes, string, uint,
};
use super::tempdir::TempDir;

fn script() -> (Value, Vec<Vec<Record>>) {
    let doc = common::read_json("persistence/journal/encode/script.json");
    let options = field(&doc, "options");
    assert_eq!(string(options, "mode"), "sync");
    assert_eq!(uint(options, "segment_size"), 4096);
    let batches = array(&doc, "batches")
        .iter()
        .map(|b| {
            b.as_array()
                .expect("a batch is an array")
                .iter()
                .map(|r| Record {
                    kind: Kind(uint(r, "kind") as u8),
                    r#gen: dec_u64(r, "gen"),
                    expires_at: dec_i64(r, "expires_at"),
                    entity: hex_bytes(r, "entity_hex"),
                    key: hex_bytes(r, "key_hex"),
                    value: hex_bytes(r, "value_hex"),
                    ..Record::default()
                })
                .collect()
        })
        .collect();
    (doc, batches)
}

fn options() -> journal::Options {
    journal::Options {
        mode: journal::Mode::Sync,
        segment_size: 4096,
        ..journal::Options::default()
    }
}

fn check_record(want: &Value, batch: usize, index: usize, got: &Record) {
    assert_eq!(uint(want, "batch") as usize, batch, "{want:?}");
    assert_eq!(uint(want, "record") as usize, index, "{want:?}");
    assert_eq!(got.lsn, dec_u64(want, "lsn"), "{want:?}");
    assert_eq!(got.loc.segment, uint(want, "segment") as u32, "{want:?}");
    assert_eq!(got.loc.offset, uint(want, "offset") as u32, "{want:?}");
    assert_eq!(got.loc.length, uint(want, "length") as u32, "{want:?}");
}

fn check_position(want: &Value, got: Position) {
    assert_eq!(got.segment, uint(want, "segment") as u32);
    assert_eq!(got.offset, uint(want, "offset") as u32);
}

#[test]
fn journal_encoding_matches_go() {
    let (_, batches) = script();
    let expected = common::read_json("persistence/journal/encode/expected.json");
    let want_records = array(&expected, "records");
    let tmp = TempDir::new();
    let dir = filepath::join(tmp.path(), "log");

    let j = journal::open(&dir, options()).unwrap_or_else(|e| panic!("{e}"));
    j.start();
    let mut checked = 0;
    for (bi, batch) in batches.iter().enumerate() {
        let mut recs = batch.clone();
        let ticket = j.append(&mut recs).unwrap_or_else(|e| panic!("{e}"));
        ticket.wait().unwrap_or_else(|e| panic!("{e}"));
        for (ri, r) in recs.iter().enumerate() {
            check_record(&want_records[checked], bi, ri, r);
            checked += 1;
        }
    }
    assert_eq!(checked, want_records.len());
    assert_eq!(checked, 84);

    assert_eq!(j.next_lsn(), dec_u64(&expected, "next_lsn"));
    check_position(field(&expected, "position"), j.position());
    let segments = j.segments().unwrap_or_else(|e| panic!("{e}"));
    let want_segments = array(&expected, "segments");
    assert_eq!(segments.len(), want_segments.len());
    for (got, want) in segments.iter().zip(want_segments) {
        assert_eq!(got.id, uint(want, "id") as u32);
        assert_eq!(got.size, uint(want, "size") as i64);
    }
    j.close().unwrap_or_else(|e| panic!("{e}"));

    assert_same_tree(
        &dir,
        &common::golden_path("persistence/journal/encode/log"),
        "journal/encode",
    );
}

#[test]
fn journal_replay_of_go_segments_matches_go() {
    let (_, batches) = script();
    let expected = common::read_json("persistence/journal/encode/expected.json");
    let want_records = array(&expected, "records");
    let golden = common::golden_path("persistence/journal/encode/log");
    let tmp = TempDir::new();
    let dir = filepath::join(tmp.path(), "log");
    copy_tree(&golden, &dir);

    let j = journal::open(&dir, options()).unwrap_or_else(|e| panic!("{e}"));
    let mut seen: Vec<Vec<Record>> = Vec::new();
    let stats = j
        .replay(
            Position {
                segment: 1,
                offset: 0,
            },
            |recs| {
                seen.push(recs.to_vec());
                Ok(())
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(seen.len(), batches.len());
    assert_eq!(stats.batches, batches.len());
    let mut checked = 0;
    for (bi, (got, want)) in seen.iter().zip(&batches).enumerate() {
        assert_eq!(got.len(), want.len(), "batch {bi}");
        for (ri, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(g.kind, w.kind);
            assert_eq!(g.r#gen, w.r#gen);
            assert_eq!(g.expires_at, w.expires_at);
            assert_eq!(g.entity, w.entity);
            assert_eq!(g.key, w.key);
            assert_eq!(g.value, w.value);
            check_record(&want_records[checked], bi, ri, g);
            checked += 1;
        }
    }
    assert_eq!(checked, want_records.len());
    assert_eq!(stats.records, checked);
    assert_eq!(stats.truncated, 0);
    assert!(stats.set_aside.is_empty());
    assert!(stats.damaged.is_empty());
    assert_eq!(stats.last_lsn + 1, dec_u64(&expected, "next_lsn"));
    assert_eq!(j.next_lsn(), dec_u64(&expected, "next_lsn"));
    check_position(field(&expected, "position"), j.position());
    j.close().unwrap_or_else(|e| panic!("{e}"));

    assert_same_tree(&dir, &golden, "journal/encode replay");
}
