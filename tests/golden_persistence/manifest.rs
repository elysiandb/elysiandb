use std::collections::BTreeMap;

use elysiandb::datastore::{
    self, MANIFEST_FORMAT, MANIFEST_FORMAT_PAGES, Manifest, ManifestEntity, ManifestIndex,
    PageRoot, RefPath,
};
use elysiandb::gocompat::json::{self, Value};

use super::common;
use super::support::{field, string};

fn root(id: u64, size: u32) -> PageRoot {
    PageRoot { id, size }
}

fn index(name: &str, r: PageRoot, dormant: bool) -> ManifestIndex {
    ManifestIndex {
        name: name.as_bytes().to_vec(),
        root: r,
        dormant,
    }
}

fn live(pairs: &[(&str, i64)]) -> BTreeMap<Vec<u8>, i64> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), *v))
        .collect()
}

fn full() -> Manifest {
    let mut origins = BTreeMap::new();
    origins.insert(b"f:title".to_vec(), b"hook".to_vec());
    origins.insert(b"s:year".to_vec(), b"query".to_vec());
    Manifest {
        format: MANIFEST_FORMAT_PAGES,
        checkpoint: b"checkpoint-0000000000000192.ckpt".to_vec(),
        kv: b"kv-0000000000000200.ckpt".to_vec(),
        lsn: 200,
        segment: 3,
        offset: 4096,
        created_at: b"<created_at>".to_vec(),
        version: b"0.3.0".to_vec(),
        types: vec![
            b"book".to_vec(),
            b"c<&>".to_vec(),
            "a\u{2028}b\u{2029}".as_bytes().to_vec(),
        ],
        entities: vec![ManifestEntity {
            name: b"book".to_vec(),
            gen_: 1 << 40,
            next_seq: 301,
            version: 77,
            refs: vec![
                RefPath {
                    path: b"author".to_vec(),
                    target: b"person".to_vec(),
                },
                RefPath {
                    path: b"tags.*".to_vec(),
                    target: b"tag".to_vec(),
                },
            ],
            slots: root((1 << 32) | 16, 4000),
            ids: root((2 << 32) | 32, 3000),
            indexes: vec![
                index("f:title", root((3 << 32) | 16, 100), false),
                index("s:year", root(0, 0), true),
            ],
            derived: vec![index("c:status,year", root((4 << 32) | 16, 80), false)],
            origins,
        }],
        live: live(&[("1", 100), ("2", 2000), ("10", 5)]),
        previous: 150,
    }
}

fn encode_cases() -> Vec<(&'static str, Manifest)> {
    let mut controls: Vec<u8> = (1u8..0x20).collect();
    controls.push(0x7f);
    vec![
        ("full", full()),
        ("empty", Manifest::default()),
        (
            "format-1",
            Manifest {
                format: MANIFEST_FORMAT,
                checkpoint: b"checkpoint-0000000000000001.ckpt".to_vec(),
                lsn: 1,
                created_at: b"<created_at>".to_vec(),
                ..Manifest::default()
            },
        ),
        (
            "escapes",
            Manifest {
                format: MANIFEST_FORMAT_PAGES,
                created_at: b"<created_at>".to_vec(),
                types: vec![
                    b"<script>&".to_vec(),
                    "\u{2028}\u{2029}".as_bytes().to_vec(),
                    controls,
                    b"\xff\xfe".to_vec(),
                    b"\"q\\".to_vec(),
                ],
                ..Manifest::default()
            },
        ),
        (
            "live-order",
            Manifest {
                format: MANIFEST_FORMAT_PAGES,
                created_at: b"<created_at>".to_vec(),
                live: live(&[("10", 1), ("2", 2), ("1", 3), ("100", 4), ("9", 5)]),
                ..Manifest::default()
            },
        ),
    ]
}

#[test]
fn manifest_encoding_matches_go() {
    let mut checked = 0;
    for (name, m) in encode_cases() {
        let want = common::read_golden(&format!("persistence/manifest/encode/{name}.json"));
        let got = m.encode().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            got == want,
            "{name}:\n{}\n{}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want)
        );
        checked += 1;
    }
    assert_eq!(checked, 5);
}

#[test]
fn manifest_parsing_matches_go() {
    let rows = common::read_json("persistence/manifest/parse.json");
    let rows = rows.as_array().expect("parse.json is an array");
    assert_eq!(rows.len(), 20);

    let mut checked = 0;
    for row in rows {
        let name = string(row, "name");
        let input = common::unhex(string(row, "hex"));
        let got = datastore::parse_manifest(&input);
        if let Some(Value::String(accepted)) = row.get("accepted") {
            let m = got.unwrap_or_else(|e| panic!("{name}: {e}"));
            let again = json::marshal(&m).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(String::from_utf8_lossy(&again), *accepted, "{name}");
        } else {
            let rejected = field(row, "rejected");
            let err = match got {
                Ok(m) => panic!("{name}: accepted {m:?}"),
                Err(err) => err.to_string(),
            };
            if let Some(Value::String(msg)) = rejected.get("message") {
                assert_eq!(err, *msg, "{name}");
            } else {
                let prefix = string(rejected, "literal");
                assert!(err.starts_with(prefix), "{name}: {err}");
                let rest = &err[prefix.len()..];
                assert!(rest.starts_with("invalid JSON"), "{name}: {err}");
                match string(rejected, "wrapped") {
                    "json_syntax" => {
                        assert!(!rest.contains("must be"), "{name}: {err}");
                        assert!(!rest.contains("out of range"), "{name}: {err}");
                    }
                    "json_type" => {
                        let f = string(rejected, "field");
                        let head = format!("invalid JSON: \"{f}");
                        assert!(rest.starts_with(&head), "{name}: {err}");
                        let next = rest.as_bytes()[head.len()];
                        assert!(matches!(next, b'"' | b'.' | b'['), "{name}: {err}");
                    }
                    other => panic!("{name}: unknown kind {other}"),
                }
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 20);

    let exact = [
        (
            &b"{\"format\":1.0}"[..],
            "cannot read MANIFEST: invalid JSON: \"format\" must be an integer, got a number",
        ),
        (
            &b"{\"format\":2,\"lsn\":-1}"[..],
            "cannot read MANIFEST: invalid JSON: \"lsn\": -1 is out of range",
        ),
        (
            &b"{\"format\":2,\"lsn\":"[..],
            "cannot read MANIFEST: invalid JSON: unexpected end of input",
        ),
        (
            &b"{\"format\":\"2\"}"[..],
            "cannot read MANIFEST: invalid JSON: \"format\" must be an integer, got a string",
        ),
    ];
    for (input, want) in exact {
        let err = datastore::parse_manifest(input).expect_err("rejected");
        assert_eq!(err.to_string(), want);
    }
}
