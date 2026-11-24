use super::common::{hex, read_golden, read_json, read_jsonl, str_field, unhex};
use elysiandb::archive::gzip::{self, BEST_SPEED};
use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

const GZIP_VECTORS: usize = 6;
const ARCHIVES: usize = 7;

pub(super) fn archive(rel: &str) -> Vec<u8> {
    read_golden(&format!("ops/archives/{rel}"))
}

pub(super) fn gunzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut r = gzip::new_reader(data)?;
    let mut out = Vec::new();
    r.read_to_end(&mut out)?;
    Ok(out)
}

pub(super) fn gzip_once(data: &[u8]) -> Vec<u8> {
    let mut w = gzip::Writer::new_level(Vec::new(), BEST_SPEED).unwrap();
    w.write_all(data).unwrap();
    w.close().unwrap();
    w.get_ref().clone()
}

fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

fn sha(data: &[u8]) -> String {
    hex(&sha256::sum256(data))
}

fn list_files(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.file_type().unwrap().is_dir() {
            list_files(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name);
        }
    }
}

#[test]
fn manifest_covers_every_archive_file() {
    let manifest = read_json("ops/MANIFEST.json");
    assert_eq!(str_field(&manifest, "go_version"), "go1.24.7");
    assert_eq!(str_field(&manifest, "baseline"), "1a11e5e");
    let mut listed = BTreeSet::new();
    for entry in manifest.get("files").and_then(Value::as_array).unwrap() {
        let path = str_field(entry, "path");
        if !path.starts_with("archives/") {
            continue;
        }
        let data = read_golden(&format!("ops/{path}"));
        assert_eq!(int_field(entry, "size"), data.len() as i64, "{path}");
        assert_eq!(str_field(entry, "sha256"), sha(&data), "{path}");
        listed.insert(path.to_string());
    }
    let mut on_disk = BTreeSet::new();
    list_files(
        &super::common::golden_path("ops/archives"),
        "archives/",
        &mut on_disk,
    );
    assert_eq!(listed, on_disk);
}

fn check_gzip_vector(row: &Value) {
    assert_eq!(int_field(row, "level"), i64::from(BEST_SPEED), "{row:?}");
    let kind = str_field(row, "kind");
    let mut w = gzip::Writer::new_level(Vec::new(), BEST_SPEED).unwrap();
    let mut input = Vec::new();
    match kind {
        "gzip-x" | "gzip-empty" => {
            input = str_field(row, "input").as_bytes().to_vec();
            w.write_all(&input).unwrap();
            w.close().unwrap();
        }
        "gzip-empty-flush" => {
            assert_eq!(str_field(row, "input"), "");
            w.flush().unwrap();
            assert_eq!(hex(w.get_ref()), str_field(row, "flushed_hex"), "{row:?}");
            w.close().unwrap();
        }
        "gzip-flush-between" => {
            let writes = row.get("writes").and_then(Value::as_array).unwrap();
            for step in writes {
                match step.as_str().unwrap() {
                    "<flush>" => w.flush().unwrap(),
                    data => {
                        w.write_all(data.as_bytes()).unwrap();
                        input.extend_from_slice(data.as_bytes());
                    }
                }
            }
            w.close().unwrap();
        }
        "gzip-mixed" | "gzip-mixed-flush" => {
            input = archive(str_field(row, "input"));
            assert_eq!(sha(&input), str_field(row, "input_sha256"));
            if kind == "gzip-mixed" {
                assert_eq!(str_field(row, "writes"), "one write");
                w.write_all(&input).unwrap();
            } else {
                assert_eq!(
                    str_field(row, "writes"),
                    "flush, then each 65536-byte slice followed by a flush, then close"
                );
                w.flush().unwrap();
                for chunk in input.chunks(65536) {
                    w.write_all(chunk).unwrap();
                    w.flush().unwrap();
                }
            }
            w.close().unwrap();
            let want = archive(str_field(row, "output"));
            assert_eq!(sha(&want), str_field(row, "output_sha256"));
            assert!(w.get_ref() == &want, "{kind} differs from Go");
        }
        other => panic!("unknown gzip vector {other}"),
    }
    if let Some(output) = row.get("output_hex").and_then(Value::as_str) {
        assert_eq!(hex(w.get_ref()), output, "{row:?}");
        assert_eq!(unhex(output).as_slice(), w.get_ref().as_slice());
    }
    assert_eq!(
        &w.get_ref()[..10],
        b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x04\xff"
    );
    assert_eq!(gunzip(w.get_ref()).unwrap(), input, "{kind} round trip");
}

#[test]
fn gzip_vectors_match_go() {
    let mut gzip_rows = 0;
    for row in &read_jsonl("ops/archives/vectors.jsonl") {
        if str_field(row, "kind").starts_with("gzip-") {
            check_gzip_vector(row);
            gzip_rows += 1;
        }
    }
    assert_eq!(gzip_rows, GZIP_VECTORS);
}

#[test]
fn archives_gunzip_to_the_go_tar_and_gzip_back_like_go() {
    let mut checked = 0;
    for row in &read_jsonl("ops/archives/archives.jsonl") {
        let gz = archive(str_field(row, "file"));
        let tar = archive(str_field(row, "tar"));
        assert_eq!(int_field(row, "gzip_size"), gz.len() as i64, "{row:?}");
        assert_eq!(int_field(row, "tar_size"), tar.len() as i64, "{row:?}");
        assert_eq!(str_field(row, "tar_sha256"), sha(&tar), "{row:?}");
        assert!(gunzip(&gz).unwrap() == tar, "{row:?}");
        let single = row
            .get("same_as_single_write_gzip")
            .and_then(Value::as_bool)
            .unwrap();
        assert_eq!(gzip_once(&tar) == gz, single, "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, ARCHIVES);

    let seed = archive("seed/seed.tar.gz");
    let seed_tar = archive("seed/seed.tar");
    assert!(gunzip(&seed).unwrap() == seed_tar);
    assert!(gzip_once(&seed_tar) == seed);
}
