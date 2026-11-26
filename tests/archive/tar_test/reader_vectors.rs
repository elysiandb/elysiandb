use super::*;

pub fn reader_vectors() -> Vec<ReaderVector> {
    vec![
    ReaderVector {
        file: "gnu.tar",
        headers: vec![
        Header {
            name: b"small.txt".to_vec(),
            mode: 416,
            uid: 73025,
            gid: 5000,
            size: 5,
            mod_time: unix(1244428340, 0),
            typeflag: 48,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            format: Format(8),
            ..Header::default()
        },
        Header {
            name: b"small2.txt".to_vec(),
            mode: 416,
            uid: 73025,
            gid: 5000,
            size: 11,
            mod_time: unix(1244436044, 0),
            typeflag: 48,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: Some(vec!["6cbd88fc", "ddac04b3"]),
        err: None,
    },
    ReaderVector {
        file: "sparse-formats.tar",
        headers: vec![
        Header {
            name: b"sparse-gnu".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            size: 200,
            mod_time: unix(1392395740, 0),
            typeflag: 83,
            linkname: b"".to_vec(),
            uname: b"david".to_vec(),
            gname: b"david".to_vec(),
            devmajor: 0,
            devminor: 0,
            format: Format(8),
            ..Header::default()
        },
        Header {
            name: b"sparse-posix-0.0".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            size: 200,
            mod_time: unix(1392342187, 0),
            typeflag: 48,
            linkname: b"".to_vec(),
            uname: b"david".to_vec(),
            gname: b"david".to_vec(),
            devmajor: 0,
            devminor: 0,
            pax_records: recs(vec![rec(b"GNU.sparse.size", b"200"), rec(b"GNU.sparse.numblocks", b"95"), rec(b"GNU.sparse.map", b"1,1,3,1,5,1,7,1,9,1,11,1,13,1,15,1,17,1,19,1,21,1,23,1,25,1,27,1,29,1,31,1,33,1,35,1,37,1,39,1,41,1,43,1,45,1,47,1,49,1,51,1,53,1,55,1,57,1,59,1,61,1,63,1,65,1,67,1,69,1,71,1,73,1,75,1,77,1,79,1,81,1,83,1,85,1,87,1,89,1,91,1,93,1,95,1,97,1,99,1,101,1,103,1,105,1,107,1,109,1,111,1,113,1,115,1,117,1,119,1,121,1,123,1,125,1,127,1,129,1,131,1,133,1,135,1,137,1,139,1,141,1,143,1,145,1,147,1,149,1,151,1,153,1,155,1,157,1,159,1,161,1,163,1,165,1,167,1,169,1,171,1,173,1,175,1,177,1,179,1,181,1,183,1,185,1,187,1,189,1")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            name: b"sparse-posix-0.1".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            size: 200,
            mod_time: unix(1392340456, 0),
            typeflag: 48,
            linkname: b"".to_vec(),
            uname: b"david".to_vec(),
            gname: b"david".to_vec(),
            devmajor: 0,
            devminor: 0,
            pax_records: recs(vec![rec(b"GNU.sparse.size", b"200"), rec(b"GNU.sparse.numblocks", b"95"), rec(b"GNU.sparse.map", b"1,1,3,1,5,1,7,1,9,1,11,1,13,1,15,1,17,1,19,1,21,1,23,1,25,1,27,1,29,1,31,1,33,1,35,1,37,1,39,1,41,1,43,1,45,1,47,1,49,1,51,1,53,1,55,1,57,1,59,1,61,1,63,1,65,1,67,1,69,1,71,1,73,1,75,1,77,1,79,1,81,1,83,1,85,1,87,1,89,1,91,1,93,1,95,1,97,1,99,1,101,1,103,1,105,1,107,1,109,1,111,1,113,1,115,1,117,1,119,1,121,1,123,1,125,1,127,1,129,1,131,1,133,1,135,1,137,1,139,1,141,1,143,1,145,1,147,1,149,1,151,1,153,1,155,1,157,1,159,1,161,1,163,1,165,1,167,1,169,1,171,1,173,1,175,1,177,1,179,1,181,1,183,1,185,1,187,1,189,1"), rec(b"GNU.sparse.name", b"sparse-posix-0.1")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            name: b"sparse-posix-1.0".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            size: 200,
            mod_time: unix(1392337404, 0),
            typeflag: 48,
            linkname: b"".to_vec(),
            uname: b"david".to_vec(),
            gname: b"david".to_vec(),
            devmajor: 0,
            devminor: 0,
            pax_records: recs(vec![rec(b"GNU.sparse.major", b"1"), rec(b"GNU.sparse.minor", b"0"), rec(b"GNU.sparse.realsize", b"200"), rec(b"GNU.sparse.name", b"sparse-posix-1.0")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            name: b"end".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            size: 4,
            mod_time: unix(1392398319, 0),
            typeflag: 48,
            linkname: b"".to_vec(),
            uname: b"david".to_vec(),
            gname: b"david".to_vec(),
            devmajor: 0,
            devminor: 0,
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: Some(vec!["5375e1d2", "5375e1d2", "5375e1d2", "5375e1d2", "8eb179ba"]),
        err: None,
    },
    ReaderVector {
        file: "star.tar",
        headers: vec![
        Header {
            name: b"small.txt".to_vec(),
            mode: 416,
            uid: 73025,
            gid: 5000,
            size: 5,
            mod_time: unix(1244592783, 0),
            typeflag: 48,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            access_time: unix(1244592783, 0),
            change_time: unix(1244592783, 0),
            ..Header::default()
        },
        Header {
            name: b"small2.txt".to_vec(),
            mode: 416,
            uid: 73025,
            gid: 5000,
            size: 11,
            mod_time: unix(1244592783, 0),
            typeflag: 48,
            uname: b"dsymonds".to_vec(),
            gname: b"eng".to_vec(),
            access_time: unix(1244592783, 0),
            change_time: unix(1244592783, 0),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "v7.tar",
        headers: vec![
        Header {
            name: b"small.txt".to_vec(),
            mode: 292,
            uid: 73025,
            gid: 5000,
            size: 5,
            mod_time: unix(1244593104, 0),
            typeflag: 48,
            ..Header::default()
        },
        Header {
            name: b"small2.txt".to_vec(),
            mode: 292,
            uid: 73025,
            gid: 5000,
            size: 11,
            mod_time: unix(1244593104, 0),
            typeflag: 48,
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax.tar",
        headers: vec![
        Header {
            name: b"a/123456789101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100".to_vec(),
            mode: 436,
            uid: 1000,
            gid: 1000,
            uname: b"shane".to_vec(),
            gname: b"shane".to_vec(),
            size: 7,
            mod_time: unix(1350244992, 23960108),
            change_time: unix(1350244992, 23960108),
            access_time: unix(1350244992, 23960108),
            typeflag: 48,
            pax_records: recs(vec![rec(b"path", b"a/123456789101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100"), rec(b"mtime", b"1350244992.023960108"), rec(b"atime", b"1350244992.023960108"), rec(b"ctime", b"1350244992.023960108")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            name: b"a/b".to_vec(),
            mode: 511,
            uid: 1000,
            gid: 1000,
            uname: b"shane".to_vec(),
            gname: b"shane".to_vec(),
            size: 0,
            mod_time: unix(1350266320, 910238425),
            change_time: unix(1350266320, 910238425),
            access_time: unix(1350266320, 910238425),
            typeflag: 50,
            linkname: b"123456789101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100".to_vec(),
            pax_records: recs(vec![rec(b"linkpath", b"123456789101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100"), rec(b"mtime", b"1350266320.910238425"), rec(b"atime", b"1350266320.910238425"), rec(b"ctime", b"1350266320.910238425")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-bad-hdr-file.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "pax-bad-mtime-file.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "pax-pos-size-file.tar",
        headers: vec![
        Header {
            name: b"foo".to_vec(),
            mode: 416,
            uid: 319973,
            gid: 5000,
            size: 999,
            mod_time: unix(1442282516, 0),
            typeflag: 48,
            uname: b"joetsai".to_vec(),
            gname: b"eng".to_vec(),
            pax_records: recs(vec![rec(b"size", b"000000000000000000000999")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: Some(vec!["5fd7e86a"]),
        err: None,
    },
    ReaderVector {
        file: "pax-records.tar",
        headers: vec![
        Header {
            typeflag: 48,
            name: b"file".to_vec(),
            uname: b"longlonglonglonglonglonglonglonglonglong".to_vec(),
            mod_time: unix(0, 0),
            pax_records: recs(vec![rec(b"VENDOR.pkg", b"tar"), rec(b"comment", b"Hello, \xe4\xb8\x96\xe7\x95\x8c"), rec(b"uname", b"longlonglonglonglonglonglonglonglonglong")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-global-records.tar",
        headers: vec![
        Header {
            typeflag: 103,
            name: b"global1".to_vec(),
            pax_records: recs(vec![rec(b"path", b"global1"), rec(b"mtime", b"1500000000.0")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            typeflag: 48,
            name: b"file1".to_vec(),
            mod_time: unix(0, 0),
            format: Format(2),
            ..Header::default()
        },
        Header {
            typeflag: 48,
            name: b"file2".to_vec(),
            pax_records: recs(vec![rec(b"path", b"file2")]),
            mod_time: unix(0, 0),
            format: Format(4),
            ..Header::default()
        },
        Header {
            typeflag: 103,
            name: b"GlobalHead.0.0".to_vec(),
            pax_records: recs(vec![rec(b"path", b"")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            typeflag: 48,
            name: b"file3".to_vec(),
            mod_time: unix(0, 0),
            format: Format(2),
            ..Header::default()
        },
        Header {
            typeflag: 48,
            name: b"file4".to_vec(),
            mod_time: unix(1400000000, 0),
            pax_records: recs(vec![rec(b"mtime", b"1400000000")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "nil-uid.tar",
        headers: vec![
        Header {
            name: b"P1050238.JPG.log".to_vec(),
            mode: 436,
            uid: 0,
            gid: 0,
            size: 14,
            mod_time: unix(1365454838, 0),
            typeflag: 48,
            linkname: b"".to_vec(),
            uname: b"eyefi".to_vec(),
            gname: b"eyefi".to_vec(),
            devmajor: 0,
            devminor: 0,
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "xattrs.tar",
        headers: vec![
        Header {
            name: b"small.txt".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 10,
            size: 5,
            mod_time: unix(1386065770, 448252320),
            typeflag: 48,
            uname: b"alex".to_vec(),
            gname: b"wheel".to_vec(),
            access_time: unix(1389782991, 419875220),
            change_time: unix(1389782956, 794414986),
            xattrs: recs(vec![rec(b"user.key", b"value"), rec(b"user.key2", b"value2"), rec(b"security.selinux", b"unconfined_u:object_r:default_t:s0\x00")]),
            pax_records: recs(vec![rec(b"mtime", b"1386065770.44825232"), rec(b"atime", b"1389782991.41987522"), rec(b"ctime", b"1389782956.794414986"), rec(b"SCHILY.xattr.user.key", b"value"), rec(b"SCHILY.xattr.user.key2", b"value2"), rec(b"SCHILY.xattr.security.selinux", b"unconfined_u:object_r:default_t:s0\x00")]),
            format: Format(4),
            ..Header::default()
        },
        Header {
            name: b"small2.txt".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 10,
            size: 11,
            mod_time: unix(1386065770, 449252304),
            typeflag: 48,
            uname: b"alex".to_vec(),
            gname: b"wheel".to_vec(),
            access_time: unix(1389782991, 419875220),
            change_time: unix(1386065770, 449252304),
            xattrs: recs(vec![rec(b"security.selinux", b"unconfined_u:object_r:default_t:s0\x00")]),
            pax_records: recs(vec![rec(b"mtime", b"1386065770.449252304"), rec(b"atime", b"1389782991.41987522"), rec(b"ctime", b"1386065770.449252304"), rec(b"SCHILY.xattr.security.selinux", b"unconfined_u:object_r:default_t:s0\x00")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-multi-hdrs.tar",
        headers: vec![
        Header {
            name: b"GNU2/GNU2/long-path-name".to_vec(),
            linkname: b"GNU4/GNU4/long-linkpath-name".to_vec(),
            mod_time: unix(0, 0),
            typeflag: 50,
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-incremental.tar",
        headers: vec![
        Header {
            name: b"test2/".to_vec(),
            mode: 16877,
            uid: 1000,
            gid: 1000,
            size: 14,
            mod_time: unix(1441973427, 0),
            typeflag: 68,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            access_time: unix(1441974501, 0),
            change_time: unix(1441973436, 0),
            format: Format(8),
            ..Header::default()
        },
        Header {
            name: b"test2/foo".to_vec(),
            mode: 33188,
            uid: 1000,
            gid: 1000,
            size: 64,
            mod_time: unix(1441973363, 0),
            typeflag: 48,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            access_time: unix(1441974501, 0),
            change_time: unix(1441973436, 0),
            format: Format(8),
            ..Header::default()
        },
        Header {
            name: b"test2/sparse".to_vec(),
            mode: 33188,
            uid: 1000,
            gid: 1000,
            size: 536870912,
            mod_time: unix(1441973427, 0),
            typeflag: 83,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            access_time: unix(1441991948, 0),
            change_time: unix(1441973436, 0),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-multi-hdrs.tar",
        headers: vec![
        Header {
            name: b"bar".to_vec(),
            linkname: b"PAX4/PAX4/long-linkpath-name".to_vec(),
            mod_time: unix(0, 0),
            typeflag: 50,
            pax_records: recs(vec![rec(b"linkpath", b"PAX4/PAX4/long-linkpath-name")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-long-nul.tar",
        headers: vec![
        Header {
            name: b"0123456789".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            mod_time: unix(1486082191, 0),
            typeflag: 48,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-utf8.tar",
        headers: vec![
        Header {
            name: b"\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9\xe2\x98\xba\xe2\x98\xbb\xe2\x98\xb9".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            mod_time: unix(0, 0),
            typeflag: 48,
            uname: b"\xe2\x98\xba".to_vec(),
            gname: b"\xe2\x9a\xb9".to_vec(),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-not-utf8.tar",
        headers: vec![
        Header {
            name: b"hi\x80\x81\x82\x83bye".to_vec(),
            mode: 420,
            uid: 1000,
            gid: 1000,
            mod_time: unix(0, 0),
            typeflag: 48,
            uname: b"rawr".to_vec(),
            gname: b"dsnet".to_vec(),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-nul-xattrs.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "pax-nul-path.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "neg-size.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "issue10968.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "issue11169.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "issue12435.tar",
        headers: vec![
        ],
        chksums: None,
        err: Some(HEADER),
    },
    ReaderVector {
        file: "invalid-go17.tar",
        headers: vec![
        Header {
            name: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/foo".to_vec(),
            uid: 2097152,
            mod_time: unix(0, 0),
            typeflag: 48,
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "ustar-file-devs.tar",
        headers: vec![
        Header {
            name: b"file".to_vec(),
            mode: 420,
            typeflag: 48,
            mod_time: unix(0, 0),
            devmajor: 1,
            devminor: 1,
            format: Format(2),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-nil-sparse-data.tar",
        headers: vec![
        Header {
            name: b"sparse.db".to_vec(),
            typeflag: 83,
            size: 1000,
            mod_time: unix(0, 0),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "gnu-nil-sparse-hole.tar",
        headers: vec![
        Header {
            name: b"sparse.db".to_vec(),
            typeflag: 83,
            size: 1000,
            mod_time: unix(0, 0),
            format: Format(8),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-nil-sparse-data.tar",
        headers: vec![
        Header {
            name: b"sparse.db".to_vec(),
            typeflag: 48,
            size: 1000,
            mod_time: unix(0, 0),
            pax_records: recs(vec![rec(b"size", b"1512"), rec(b"GNU.sparse.major", b"1"), rec(b"GNU.sparse.minor", b"0"), rec(b"GNU.sparse.realsize", b"1000"), rec(b"GNU.sparse.name", b"sparse.db")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "pax-nil-sparse-hole.tar",
        headers: vec![
        Header {
            name: b"sparse.db".to_vec(),
            typeflag: 48,
            size: 1000,
            mod_time: unix(0, 0),
            pax_records: recs(vec![rec(b"size", b"512"), rec(b"GNU.sparse.major", b"1"), rec(b"GNU.sparse.minor", b"0"), rec(b"GNU.sparse.realsize", b"1000"), rec(b"GNU.sparse.name", b"sparse.db")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ReaderVector {
        file: "trailing-slash.tar",
        headers: vec![
        Header {
            typeflag: 53,
            name: b"123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/".to_vec(),
            mod_time: unix(0, 0),
            pax_records: recs(vec![rec(b"path", b"123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/")]),
            format: Format(4),
            ..Header::default()
        },
        ],
        chksums: None,
        err: None,
    },
    ]
}
