use super::archives::{archive, gunzip};
use super::common::{hex, read_jsonl, str_field, unhex};
use elysiandb::archive::gzip;
use elysiandb::archive::tar::{self, Header, TYPE_REG};
use elysiandb::gocompat::json::Value;
use gocompat::time::{RFC3339, parse};
use std::io::{Read, Write};

const EXPECTED_ROWS: usize = 87;
const REBACKUPS: [&str; 8] = [
    "valid/kv-expired-and-live.rebackup.tar",
    "valid/documents-without-separators.rebackup.tar",
    "valid/typeflag-nul.rebackup.tar",
    "valid/kv-case-insensitive.rebackup.tar",
    "valid/empty-entities.rebackup.tar",
    "valid/two-gzip-members.rebackup.tar",
    "valid/manifest-extra-keys.rebackup.tar",
    "valid/raw-documents.rebackup.tar",
];
const GO_TARS: [&str; 8] = [
    "seed/seed.tar",
    "api/backup.tar",
    "api/hot.tar",
    "api/periodic.tar",
    "seed-only/backup.tar",
    "seed-only/hot.tar",
    "large/backup.tar",
    "large/hot-beats.tar",
];

fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

pub(super) fn check_tar_vector(row: &Value) {
    let name = str_field(row, "name");
    assert_eq!(int_field(row, "name_len"), name.len() as i64, "{row:?}");
    let size = int_field(row, "size");
    let mtime = parse(RFC3339, str_field(row, "mtime")).unwrap();
    let hdr = Header {
        name: name.as_bytes().to_vec(),
        mode: 0o600,
        size,
        mod_time: mtime.clone(),
        typeflag: TYPE_REG,
        ..Header::default()
    };
    let mut tw = tar::Writer::new(Vec::new());
    tw.write_header(&hdr).unwrap();
    if let Some(want) = row.get("header_hex").and_then(Value::as_str) {
        assert_eq!(hex(tw.get_ref()), want, "{row:?}");
        let mut tr = tar::Reader::new(tw.get_ref().as_slice());
        let got = tr.next().unwrap().unwrap();
        assert_eq!(got.name, name.as_bytes());
        assert_eq!(got.size, size);
        assert_eq!(got.mod_time.unix(), mtime.unix());
        return;
    }
    let data = str_field(row, "data");
    tw.write_all(data.as_bytes()).unwrap();
    tw.close().unwrap();
    let want = unhex(str_field(row, "archive_hex"));
    assert_eq!(int_field(row, "archive_len"), want.len() as i64);
    assert!(tw.get_ref() == &want, "{row:?}");

    let mut tr = tar::Reader::new(want.as_slice());
    let got = tr.next().unwrap().unwrap();
    assert_eq!(got.name, name.as_bytes(), "{row:?}");
    assert_eq!(got.size, size);
    assert_eq!(got.mode, 0o600);
    assert_eq!(got.typeflag, TYPE_REG);
    assert_eq!(got.mod_time.unix(), mtime.unix());
    let mut content = Vec::new();
    tr.read_to_end(&mut content).unwrap();
    assert_eq!(content, data.as_bytes());
    assert!(tr.next().unwrap().is_none());
}

struct Entry {
    header: Header,
    data: Vec<u8>,
}

fn read_entries(tar_bytes: &[u8]) -> Vec<Entry> {
    let mut tr = tar::Reader::new(tar_bytes);
    let mut out = Vec::new();
    while let Some(header) = tr.next().unwrap() {
        let mut data = Vec::new();
        tr.read_to_end(&mut data).unwrap();
        out.push(Entry { header, data });
    }
    out
}

fn rewrite(entries: &[Entry]) -> Vec<u8> {
    let mut tw = tar::Writer::new(Vec::new());
    for e in entries {
        let hdr = Header {
            name: e.header.name.clone(),
            mode: e.header.mode,
            size: e.header.size,
            mod_time: e.header.mod_time.clone(),
            typeflag: e.header.typeflag,
            ..Header::default()
        };
        tw.write_header(&hdr).unwrap();
        tw.write_all(&e.data).unwrap();
    }
    tw.close().unwrap();
    tw.into_inner()
}

#[test]
fn go_tar_streams_read_and_rewrite_byte_for_byte() {
    let mut checked = 0;
    for rel in GO_TARS.iter().chain(REBACKUPS.iter()) {
        let original = archive(rel);
        let entries = read_entries(&original);
        assert!(!entries.is_empty(), "{rel}");
        for e in &entries {
            assert_eq!(e.header.typeflag, TYPE_REG, "{rel}");
            assert_eq!(e.header.mode, 0o600, "{rel}");
            assert_eq!(e.header.uid, 0, "{rel}");
            assert_eq!(e.data.len() as i64, e.header.size, "{rel}");
        }
        assert!(
            rewrite(&entries) == original,
            "{rel} differs after a rewrite"
        );
        checked += 1;
    }
    assert_eq!(checked, GO_TARS.len() + REBACKUPS.len());
}

#[test]
fn archives_list_their_entries_like_go() {
    let mut checked = 0;
    for row in &read_jsonl("ops/archives/archives.jsonl") {
        let entries = read_entries(&archive(str_field(row, "tar")));
        let names: Vec<String> = entries
            .iter()
            .map(|e| String::from_utf8(e.header.name.clone()).unwrap())
            .collect();
        let want: Vec<String> = row
            .get("entries")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, want, "{row:?}");

        let gz = archive(str_field(row, "file"));
        let mut tr = tar::Reader::new(gzip::new_reader(gz.as_slice()).unwrap());
        let mut streamed = Vec::new();
        while let Some(h) = tr.next().unwrap() {
            streamed.push(String::from_utf8(h.name).unwrap());
        }
        assert_eq!(streamed, want, "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, 7);
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Gzip(String),
    Next(String),
    Read(String, String),
    Complete(Vec<String>),
}

fn walk(data: &[u8]) -> Outcome {
    let gz = match gzip::new_reader(data) {
        Ok(gz) => gz,
        Err(e) => return Outcome::Gzip(e.to_string()),
    };
    let mut tr = tar::Reader::new(gz);
    let mut names = Vec::new();
    loop {
        let header = match tr.next() {
            Ok(Some(h)) => h,
            Ok(None) => return Outcome::Complete(names),
            Err(e) => return Outcome::Next(e.to_string()),
        };
        let name = String::from_utf8(header.name).unwrap();
        let mut sink = Vec::new();
        if let Err(e) = tr.read_to_end(&mut sink) {
            return Outcome::Read(name, e.to_string());
        }
        names.push(name);
    }
}

fn place(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Gzip(detail) => Some(format!("invalid backup archive: not a gzip file: {detail}")),
        Outcome::Next(detail) => Some(format!(
            "invalid backup archive: unreadable tar stream: {detail}"
        )),
        Outcome::Read(name, detail) => Some(format!(
            "invalid backup archive: {name} cannot be read: {detail}"
        )),
        Outcome::Complete(_) => None,
    }
}

const GO_GZIP: &str = "\u{27e8}go:gzip\u{27e9}";
const GO_TAR: &str = "\u{27e8}go:tar\u{27e9}";

fn expected_detail(row: &Value, data: &[u8], reason: &str) -> String {
    if let Some(offset) = row
        .get("go_values")
        .and_then(|v| v.get("flate_offset"))
        .and_then(Value::as_i64)
    {
        return format!("corrupt compressed data at byte {offset}");
    }
    if reason.contains("not a gzip file") {
        if data.len() >= 10 {
            return "invalid gzip header".to_string();
        }
        return "unexpected end of data".to_string();
    }
    if reason.contains("unreadable tar stream") {
        return "invalid tar header".to_string();
    }
    "unexpected end of data".to_string()
}

#[test]
fn expected_archives_fail_or_pass_at_the_archive_layer_like_go() {
    let mut checked = 0;
    let mut archive_layer = 0;
    for row in &read_jsonl("ops/archives/expected.jsonl") {
        let file = str_field(row, "file");
        let data = archive(file);
        let outcome = walk(&data);
        match row.get("reason").and_then(Value::as_str) {
            Some(reason) if reason.contains(GO_GZIP) || reason.contains(GO_TAR) => {
                let detail = expected_detail(row, &data, reason);
                let want = reason.replace(GO_GZIP, &detail).replace(GO_TAR, &detail);
                assert_eq!(place(&outcome).as_deref(), Some(want.as_str()), "{file}");
                archive_layer += 1;
            }
            _ => assert!(
                matches!(outcome, Outcome::Complete(_)),
                "{file}: {outcome:?}"
            ),
        }
        checked += 1;
    }
    assert_eq!(checked, EXPECTED_ROWS);
    assert_eq!(archive_layer, 7);
}

#[test]
fn trailer_damage_is_only_seen_by_a_full_read() {
    let cases = [
        (
            "valid/gzip-trailer-missing.tar.gz",
            "unexpected end of data",
        ),
        ("valid/gzip-bad-crc.tar.gz", "invalid gzip checksum"),
        ("valid/gzip-bad-size.tar.gz", "invalid gzip checksum"),
    ];
    for (file, want) in cases {
        let data = archive(file);
        assert!(matches!(walk(&data), Outcome::Complete(_)), "{file}");
        let err = gunzip(&data).unwrap_err();
        assert_eq!(err.to_string(), want, "{file}");
    }
}

#[test]
fn edge_archives_read_like_go() {
    let two = walk(&archive("valid/two-gzip-members.tar.gz"));
    let Outcome::Complete(names) = two else {
        panic!("{two:?}")
    };
    assert_eq!(names[0], "manifest.json");

    let data = archive("valid/typeflag-nul.tar.gz");
    let mut tr = tar::Reader::new(gzip::new_reader(data.as_slice()).unwrap());
    let mut count = 0;
    while let Some(h) = tr.next().unwrap() {
        assert_eq!(h.typeflag, TYPE_REG);
        count += 1;
    }
    assert!(count >= 5);

    let data = archive("invalid/manifest-not-regular.tar.gz");
    let mut tr = tar::Reader::new(gzip::new_reader(data.as_slice()).unwrap());
    let h = tr.next().unwrap().unwrap();
    assert_eq!(h.name, b"manifest.json");
    assert_eq!(h.typeflag, tar::TYPE_DIR);

    for file in [
        "invalid/tar-empty-stream.tar.gz",
        "invalid/tar-only-end-blocks.tar.gz",
    ] {
        let data = archive(file);
        let mut tr = tar::Reader::new(gzip::new_reader(data.as_slice()).unwrap());
        assert!(tr.next().unwrap().is_none(), "{file}");
        assert!(tr.next().unwrap().is_none(), "{file}");
    }
}

#[test]
fn write_errors_carry_no_go_wording() {
    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"a".to_vec(),
        mode: 0o600,
        size: 2,
        typeflag: TYPE_REG,
        ..Header::default()
    };
    tw.write_header(&hdr).unwrap();
    let err = tw.write_all(b"abc").unwrap_err();
    assert_eq!(err.to_string(), "write longer than the tar entry size");
    tw.write_header(&hdr).unwrap();
    tw.write_all(b"a").unwrap();
    let err = tw.write_header(&hdr).unwrap_err();
    assert_eq!(err.to_string(), "missed writing 1 bytes of the tar entry");
    let err = tw.close().unwrap_err();
    assert_eq!(err.to_string(), "missed writing 1 bytes of the tar entry");
    let err = tw.write_all(b"b").unwrap_err();
    assert_eq!(err.to_string(), "write after the tar stream was closed");
    tw.close().unwrap();
    let err = tw.write_header(&hdr).unwrap_err();
    assert_eq!(err.to_string(), "write after the tar stream was closed");
}
