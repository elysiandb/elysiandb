use super::*;

pub fn writer_vectors() -> Vec<WriterVector> {
    vec![
    WriterVector {
        file: "writer.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"small.txt".to_vec(),
            size: 5,
            mode: 416,
            uid: 73025,
            gid: 5000,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            mod_time: unix(1246508266, 0),
            ..Header::default()
        }), None),
            Op::Write(b"Kilts".to_vec(), 5, None),
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"small2.txt".to_vec(),
            size: 11,
            mode: 416,
            uid: 73025,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            gid: 5000,
            mod_time: unix(1245217492, 0),
            ..Header::default()
        }), None),
            Op::Write(b"Google.com\n".to_vec(), 11, None),
            Op::Header(Box::new(Header {
            typeflag: 50,
            name: b"link.txt".to_vec(),
            linkname: b"small.txt".to_vec(),
            mode: 511,
            uid: 1000,
            gid: 1000,
            uname: b"strings".to_vec(),
            gname: b"strings".to_vec(),
            mod_time: unix(1314603082, 0),
            ..Header::default()
        }), None),
            Op::Write(b"".to_vec(), 0, None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "writer-big.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"tmp/16gig.txt".to_vec(),
            size: 17179869184,
            mode: 416,
            uid: 73025,
            gid: 5000,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            mod_time: unix(1254699560, 0),
            format: Format(8),
            ..Header::default()
        }), None),
        ],
    },
    WriterVector {
        file: "writer-big-long.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/16gig.txt".to_vec(),
            size: 17179869184,
            mode: 420,
            uid: 1000,
            gid: 1000,
            uname: b"guillaume".to_vec(),
            gname: b"guillaume".to_vec(),
            mod_time: unix(1399583047, 0),
            ..Header::default()
        }), None),
        ],
    },
    WriterVector {
        file: "ustar.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/longname/file.txt".to_vec(),
            size: 6,
            mode: 420,
            uid: 501,
            gid: 20,
            uname: b"shane".to_vec(),
            gname: b"staff".to_vec(),
            mod_time: unix(1360135598, 0),
            ..Header::default()
        }), None),
            Op::Write(b"hello\n".to_vec(), 6, None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "hardlink.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file.txt".to_vec(),
            size: 15,
            mode: 420,
            uid: 1000,
            gid: 100,
            uname: b"vbatts".to_vec(),
            gname: b"users".to_vec(),
            mod_time: unix(1425484303, 0),
            ..Header::default()
        }), None),
            Op::Write(b"Slartibartfast\n".to_vec(), 15, None),
            Op::Header(Box::new(Header {
            typeflag: 49,
            name: b"hard.txt".to_vec(),
            linkname: b"file.txt".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 100,
            uname: b"vbatts".to_vec(),
            gname: b"users".to_vec(),
            mod_time: unix(1425484303, 0),
            ..Header::default()
        }), None),
            Op::Write(b"".to_vec(), 0, None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"bad-null.txt".to_vec(),
            xattrs: recs(vec![rec(b"null\x00null\x00", b"fizzbuzz")]),
            ..Header::default()
        }), Some(HEADER_ERROR)),
        ],
    },
    WriterVector {
        file: "",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"null\x00.txt".to_vec(),
            ..Header::default()
        }), Some(HEADER_ERROR)),
        ],
    },
    WriterVector {
        file: "pax-records.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file".to_vec(),
            uname: b"longlonglonglonglonglonglonglonglonglong".to_vec(),
            pax_records: recs(vec![rec(b"path", b"FILE"), rec(b"GNU.sparse.map", b"0,0"), rec(b"comment", b"Hello, \xe4\xb8\x96\xe7\x95\x8c"), rec(b"VENDOR.pkg", b"tar")]),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "pax-global-records.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 103,
            pax_records: recs(vec![rec(b"path", b"global1"), rec(b"mtime", b"1500000000.0")]),
            ..Header::default()
        }), None),
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file1".to_vec(),
            ..Header::default()
        }), None),
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file2".to_vec(),
            pax_records: recs(vec![rec(b"path", b"file2")]),
            ..Header::default()
        }), None),
            Op::Header(Box::new(Header {
            typeflag: 103,
            pax_records: recs(vec![rec(b"path", b"")]),
            ..Header::default()
        }), None),
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file3".to_vec(),
            ..Header::default()
        }), None),
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"file4".to_vec(),
            mod_time: unix(1400000000, 0),
            pax_records: recs(vec![rec(b"mtime", b"1400000000")]),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "gnu-utf8.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            uname: b"\xe2\x98\xba".to_vec(),
            gname: b"\xe2\x9a\xb9".to_vec(),
            mod_time: unix(0, 0),
            format: Format(8),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "gnu-not-utf8.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            typeflag: 48,
            name: b"hi\x80\x81\x82\x83bye".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            mod_time: unix(0, 0),
            format: Format(8),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "trailing-slash.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            name: b"123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/".to_vec(),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    WriterVector {
        file: "file-and-dir.tar",
        ops: vec![
            Op::Header(Box::new(Header {
            name: b"small.txt".to_vec(),
            size: 5,
            ..Header::default()
        }), None),
            Op::Write(b"Kilts".to_vec(), 5, None),
            Op::Header(Box::new(Header {
            name: b"dir/".to_vec(),
            ..Header::default()
        }), None),
            Op::Close(None),
        ],
    },
    ]
}
