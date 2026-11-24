use crate::allowed_vectors::allowed_vectors;
use crate::*;
use elysiandb::archive::tar::internals;
use std::io::{Read, Write};

#[test]
fn sparse_entries() {
    type Entries = Vec<(i64, i64)>;
    let vectors: Vec<(Entries, i64, bool, Entries)> = vec![
        (vec![], 0, true, vec![(0, 0)]),
        (vec![], 5000, true, vec![(0, 5000)]),
        (vec![(0, 5000)], 5000, true, vec![(5000, 0)]),
        (vec![(1000, 4000)], 5000, true, vec![(0, 1000), (5000, 0)]),
        (vec![(0, 3000)], 5000, true, vec![(3000, 2000)]),
        (vec![(3000, 2000)], 5000, true, vec![(0, 3000), (5000, 0)]),
        (
            vec![(2000, 2000)],
            5000,
            true,
            vec![(0, 2000), (4000, 1000)],
        ),
        (
            vec![(0, 2000), (8000, 2000)],
            10000,
            true,
            vec![(2000, 6000), (10000, 0)],
        ),
        (
            vec![
                (0, 2000),
                (2000, 2000),
                (4000, 0),
                (4000, 3000),
                (7000, 1000),
                (8000, 0),
                (8000, 2000),
            ],
            10000,
            true,
            vec![(10000, 0)],
        ),
        (
            vec![
                (0, 0),
                (1000, 0),
                (2000, 0),
                (3000, 0),
                (4000, 0),
                (5000, 0),
            ],
            5000,
            true,
            vec![(0, 5000)],
        ),
        (vec![(1, 0)], 0, false, vec![]),
        (vec![(-1, 0)], 100, false, vec![]),
        (vec![(0, -1)], 100, false, vec![]),
        (vec![(0, 0)], -100, false, vec![]),
        (vec![(i64::MAX, 3), (6, -5)], 35, false, vec![]),
        (vec![(1, 3), (6, -5)], 35, false, vec![]),
        (vec![(i64::MAX, i64::MAX)], i64::MAX, false, vec![]),
        (vec![(3, 3)], 5, false, vec![]),
        (vec![(2, 0), (1, 0), (0, 0)], 3, false, vec![]),
        (vec![(1, 3), (2, 2)], 10, false, vec![]),
    ];

    for (i, (input, size, want_valid, want_inverted)) in vectors.iter().enumerate() {
        let got_valid = internals::validate_sparse_entries(input, *size);
        assert_eq!(got_valid, *want_valid, "test {i}");
        if !want_valid {
            continue;
        }
        let got_inverted = internals::invert_sparse_entries(input, *size);
        assert_eq!(&got_inverted, want_inverted, "test {i}");
    }
}

#[test]
fn round_trip() {
    let data = b"some file contents";

    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"file.txt".to_vec(),
        uid: 1 << 21,
        size: data.len() as i64,
        mod_time: Time::now().round(gocompat::time::Duration::SECOND),
        pax_records: recs(vec![rec(b"uid", b"2097152")]),
        format: tar::FORMAT_PAX,
        typeflag: tar::TYPE_REG,
        ..Header::default()
    };
    tw.write_header(&hdr).unwrap();
    tw.write_all(data).unwrap();
    tw.close().unwrap();
    let b = tw.into_inner();

    let mut tr = tar::Reader::new(b.as_slice());
    let r_hdr = tr.next().unwrap().unwrap();
    assert!(header_eq(&r_hdr, &hdr), "{r_hdr:?}\n{hdr:?}");
    let mut r_data = Vec::new();
    tr.read_to_end(&mut r_data).unwrap();
    assert_eq!(r_data, data);
}

#[test]
fn header_allowed_formats() {
    let vectors = allowed_vectors();
    assert_eq!(vectors.len(), 61);
    for (i, v) in vectors.iter().enumerate() {
        let (formats, pax_hdrs, err) = internals::allowed_formats(&v.header);
        assert_eq!(formats, v.formats, "test {i}");
        if formats.has(tar::FORMAT_PAX) {
            assert_eq!(pax_hdrs, v.pax_hdrs, "test {i}");
        }
        if formats != tar::FORMAT_UNKNOWN {
            assert!(err.is_none(), "test {i}, unexpected error: {err:?}");
        } else {
            assert!(err.is_some(), "test {i}, want an error");
        }
    }
}

#[test]
fn format_names() {
    assert_eq!(tar::FORMAT_UNKNOWN.to_string(), "<unknown>");
    assert_eq!(tar::FORMAT_PAX.to_string(), "PAX");
    assert_eq!(
        (tar::FORMAT_USTAR | tar::FORMAT_PAX | tar::FORMAT_GNU).to_string(),
        "(USTAR | PAX | GNU)"
    );
}
