use super::*;

pub fn allowed_vectors() -> Vec<AllowedVector> {
    vec![
    AllowedVector {
        header: Header {
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                size: 8589934591,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                size: 8589934591,
                format: Format(2),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(2),
    },
    AllowedVector {
        header: Header {
                size: 8589934591,
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(6),
    },
    AllowedVector {
        header: Header {
                size: 8589934591,
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                size: 8589934592,
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"size", b"8589934592")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                size: 8589934592,
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"size", b"8589934592")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                size: 8589934592,
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"size", b"8589934592")]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                mode: 2097151,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                mode: 2097152,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                devmajor: -123,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                devmajor: 72057594037927935,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                devmajor: 72057594037927936,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                devmajor: -72057594037927936,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                devmajor: -72057594037927937,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                name: b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d".to_vec(),
                devmajor: -72057594037927936,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                size: 9223372036854775807,
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"size", b"9223372036854775807")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                size: -9223372036854775808,
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"size", b"-9223372036854775808")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                uname: b"0123456789abcdef0123456789abcdef".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                uname: b"0123456789abcdef0123456789abcdefx".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"uname", b"0123456789abcdef0123456789abcdefx")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                name: b"foobar".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                name: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                name: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"path", b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                linkname: b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"linkpath", b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                linkname: b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"linkpath", b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d\x00")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                linkname: b"\x00hello".to_vec(),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"linkpath", b"\x00hello")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                uid: 2097151,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                uid: 2097152,
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"uid", b"2097152")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                xattrs: recs(vec![rec(b"foo", b"bar")]),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"SCHILY.xattr.foo", b"bar")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                xattrs: recs(vec![rec(b"foo", b"bar")]),
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"SCHILY.xattr.foo", b"bar")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                xattrs: recs(vec![rec(b"\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d", b"\x00hello")]),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"SCHILY.xattr.\xe7\x94\xa8\xe6\x88\xb6\xe5\x90\x8d", b"\x00hello")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                xattrs: recs(vec![rec(b"foo=bar", b"baz")]),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                xattrs: recs(vec![rec(b"foo", b"")]),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"SCHILY.xattr.foo", b"")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(0, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(8589934591, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(8589934592, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"8589934592")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(9223372036854775807, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"9223372036854775807")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(9223372036854775807, 0),
                format: Format(2),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"9223372036854775807")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(-1, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"-1")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 500),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"1.0000005")]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 0),
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(6),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 500),
                format: Format(2),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"1.0000005")]),
        formats: Format(2),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 500),
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"1.0000005")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(1, 500),
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"1.0000005")]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(-1, 500),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"-0.9999995")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                mod_time: unix(-1, 500),
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"mtime", b"-0.9999995")]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                access_time: unix(0, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"0")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                access_time: unix(0, 0),
                format: Format(2),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"0")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                access_time: unix(0, 0),
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"0")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                access_time: unix(0, 0),
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"0")]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                access_time: unix(-123, 0),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"-123")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                access_time: unix(-123, 0),
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"atime", b"-123")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                change_time: unix(123, 456),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"ctime", b"123.000000456")]),
        formats: Format(12),
    },
    AllowedVector {
        header: Header {
                change_time: unix(123, 456),
                format: Format(2),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"ctime", b"123.000000456")]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                change_time: unix(123, 456),
                format: Format(8),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"ctime", b"123.000000456")]),
        formats: Format(8),
    },
    AllowedVector {
        header: Header {
                change_time: unix(123, 456),
                format: Format(4),
                ..Header::default()
            },
        pax_hdrs: recs(vec![rec(b"ctime", b"123.000000456")]),
        formats: Format(4),
    },
    AllowedVector {
        header: Header {
                name: b"foo/".to_vec(),
                typeflag: 53,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    AllowedVector {
        header: Header {
                name: b"foo/".to_vec(),
                typeflag: 48,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(0),
    },
    AllowedVector {
        header: Header {
                name: b"foo/".to_vec(),
                typeflag: 50,
                ..Header::default()
            },
        pax_hdrs: recs(vec![]),
        formats: Format(14),
    },
    ]
}
