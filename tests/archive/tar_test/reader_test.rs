use crate::reader_vectors::reader_vectors;
use crate::*;
use elysiandb::archive::tar::internals;
use elysiandb::hash::crc32;
use std::io::{self, Read};

#[test]
fn reader() {
    let vectors = reader_vectors();
    assert_eq!(vectors.len(), 31);
    for v in &vectors {
        let data = testdata(v.file);
        let mut tr = tar::Reader::new(data.as_slice());
        let mut hdrs: Vec<Header> = Vec::new();
        let mut chksums: Vec<String> = Vec::new();
        let mut rdbuf = [0u8; 8];
        let mut err: Option<io::Error> = None;
        loop {
            match tr.next() {
                Ok(None) => break,
                Err(e) => {
                    err = Some(e);
                    break;
                }
                Ok(Some(hdr)) => hdrs.push(hdr),
            }
            if v.chksums.is_none() {
                continue;
            }
            let mut crc = 0u32;
            let mut failed = None;
            loop {
                match tr.read(&mut rdbuf) {
                    Ok(0) => break,
                    Ok(n) => crc = crc32::update(crc, &rdbuf[..n]),
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = failed {
                err = Some(e);
                break;
            }
            chksums.push(format!("{crc:08x}"));
        }

        for (i, hdr) in hdrs.iter().enumerate() {
            assert!(
                i < v.headers.len(),
                "{}: entry {i}: unexpected header {hdr:?}",
                v.file
            );
            assert!(
                header_eq(hdr, &v.headers[i]),
                "{}: entry {i}: incorrect header:\ngot  {hdr:?}\nwant {:?}",
                v.file,
                v.headers[i]
            );
        }
        assert_eq!(hdrs.len(), v.headers.len(), "{}", v.file);

        if let Some(want) = &v.chksums {
            for (i, sum) in chksums.iter().enumerate() {
                assert_eq!(sum, want[i], "{}: entry {i}", v.file);
            }
        }

        assert!(
            error_matches(err.as_ref(), v.err),
            "{}: unexpected error: got {err:?}, want {:?}",
            v.file,
            v.err
        );
    }
}

#[test]
fn partial_read() {
    type Cases = Vec<(usize, &'static [u8])>;
    let vectors: [(&str, Cases); 2] = [
        ("gnu.tar", vec![(4, b"Kilt"), (6, b"Google")]),
        (
            "sparse-formats.tar",
            vec![
                (2, b"\x00G"),
                (4, b"\x00G\x00o"),
                (6, b"\x00G\x00o\x00G"),
                (8, b"\x00G\x00o\x00G\x00o"),
                (4, b"end\n"),
            ],
        ),
    ];

    for (file, cases) in vectors {
        let data = testdata(file);
        let mut tr = tar::Reader::new(data.as_slice());
        for (i, (cnt, output)) in cases.iter().enumerate() {
            let hdr = tr.next().unwrap_or_else(|e| panic!("entry {i}, next: {e}"));
            assert!(hdr.is_some(), "entry {i}");
            let mut buf = vec![0u8; *cnt];
            tr.read_exact(&mut buf)
                .unwrap_or_else(|e| panic!("entry {i}, read: {e}"));
            assert_eq!(buf, *output, "entry {i}");
        }
        assert!(tr.next().unwrap().is_none(), "{file}: next, want the end");
    }
}

#[test]
fn uninitialized_read() {
    let data = testdata("gnu.tar");
    let mut tr = tar::Reader::new(data.as_slice());
    let mut buf = [0u8; 1];
    assert_eq!(tr.read(&mut buf).unwrap(), 0);
}

fn count_headers(input: &[u8], manual: bool) -> (usize, Option<io::Error>) {
    let mut tr = tar::Reader::new(input);
    let mut cnt = 0;
    loop {
        match tr.next() {
            Ok(None) => return (cnt, None),
            Err(e) => return (cnt, Some(e)),
            Ok(Some(_)) => {}
        }
        cnt += 1;
        if manual && let Err(e) = io::copy(&mut tr, &mut io::sink()) {
            return (cnt, Some(e));
        }
    }
}

#[test]
fn read_truncation() {
    let data1 = testdata("gnu.tar");
    let mut data2 = testdata("ustar-file-reg.tar");
    let pax = testdata("pax-path-hdr.tar");
    let sparse = testdata("sparse-formats.tar");
    data2.extend(std::iter::repeat_n(0u8, 10 * 512));
    let trash = b"garbage ".repeat(64);

    let cat = |a: &[u8], b: &[u8]| [a, b].concat();
    let vectors: Vec<(Vec<u8>, usize, Option<&str>)> = vec![
        (Vec::new(), 0, None),
        (data1[..511].to_vec(), 0, Some(UNEXPECTED)),
        (data1[..512].to_vec(), 1, Some(UNEXPECTED)),
        (data1[..1024].to_vec(), 1, None),
        (data1[..1536].to_vec(), 2, Some(UNEXPECTED)),
        (data1[..2048].to_vec(), 2, None),
        (data1.clone(), 2, None),
        (cat(&data1[..2048], &data2[..1536]), 3, None),
        (data2[..511].to_vec(), 0, Some(UNEXPECTED)),
        (data2[..512].to_vec(), 1, Some(UNEXPECTED)),
        (data2[..1195].to_vec(), 1, Some(UNEXPECTED)),
        (data2[..1196].to_vec(), 1, None),
        (data2[..1200].to_vec(), 1, None),
        (data2[..1535].to_vec(), 1, None),
        (data2[..1536].to_vec(), 1, None),
        (cat(&data2[..1536], &trash[..1]), 1, Some(UNEXPECTED)),
        (cat(&data2[..1536], &trash[..511]), 1, Some(UNEXPECTED)),
        (cat(&data2[..1536], &trash), 1, Some(HEADER)),
        (data2[..2048].to_vec(), 1, None),
        (cat(&data2[..2048], &trash[..1]), 1, Some(UNEXPECTED)),
        (cat(&data2[..2048], &trash[..511]), 1, Some(UNEXPECTED)),
        (cat(&data2[..2048], &trash), 1, Some(HEADER)),
        (data2[..2560].to_vec(), 1, None),
        (cat(&data2[..2560], &trash[..1]), 1, None),
        (cat(&data2[..2560], &trash[..511]), 1, None),
        (cat(&data2[..2560], &trash), 1, None),
        (data2[..3072].to_vec(), 1, None),
        (pax.clone(), 0, None),
        (cat(&pax, &trash[..1]), 0, Some(UNEXPECTED)),
        (cat(&pax, &trash[..511]), 0, Some(UNEXPECTED)),
        (sparse[..511].to_vec(), 0, Some(UNEXPECTED)),
        (sparse[..512].to_vec(), 0, Some(UNEXPECTED)),
        (sparse[..3584].to_vec(), 1, None),
        (sparse[..9200].to_vec(), 1, None),
        (sparse[..9216].to_vec(), 1, None),
        (sparse[..9728].to_vec(), 2, Some(UNEXPECTED)),
        (sparse[..10240].to_vec(), 2, None),
        (sparse[..11264].to_vec(), 2, Some(UNEXPECTED)),
        (sparse.clone(), 5, None),
        (cat(&sparse, &trash), 5, None),
    ];

    for (i, (input, cnt, want)) in vectors.iter().enumerate() {
        for manual in [false, true] {
            let (got_cnt, err) = count_headers(input, manual);
            assert!(
                error_matches(err.as_ref(), *want),
                "test {i}, manual {manual}: got {err:?}, want {want:?}"
            );
            assert_eq!(got_cnt, *cnt, "test {i}, manual {manual}");
        }
    }
}

#[test]
fn read_header_only() {
    let data = testdata("hdr-only.tar");
    let mut hdrs = Vec::new();
    let mut tr = tar::Reader::new(data.as_slice());
    loop {
        let hdr = match tr.next() {
            Ok(None) => break,
            Ok(Some(h)) => h,
            Err(e) => panic!("next: {e}"),
        };
        let mut one = [0u8; 1];
        let cnt = tr.read(&mut one).unwrap_or(0);
        assert!(
            cnt == 0 || hdr.typeflag == tar::TYPE_REG,
            "read {cnt} bytes of a header-only entry"
        );
        hdrs.push(hdr);
    }

    assert_eq!(hdrs.len(), 16);
    for i in 0..8 {
        let mut hdr1 = hdrs[i].clone();
        let mut hdr2 = hdrs[i + 8].clone();
        hdr1.size = 0;
        hdr2.size = 0;
        assert!(header_eq(&hdr1, &hdr2), "incorrect header {i}");
    }
}

#[test]
fn merge_pax() {
    let mut got = Header::default();
    let ok = internals::merge_pax(
        &mut got,
        recs(vec![
            rec(b"path", b"a/b/c"),
            rec(b"uid", b"1000"),
            rec(b"mtime", b"1350244992.023960108"),
        ]),
    );
    assert!(ok);
    let want = Header {
        name: b"a/b/c".to_vec(),
        uid: 1000,
        mod_time: unix(1350244992, 23960108),
        pax_records: recs(vec![
            rec(b"path", b"a/b/c"),
            rec(b"uid", b"1000"),
            rec(b"mtime", b"1350244992.023960108"),
        ]),
        ..Header::default()
    };
    assert!(header_eq(&got, &want), "{got:?}");

    let mut got = Header::default();
    assert!(!internals::merge_pax(
        &mut got,
        recs(vec![rec(b"gid", b"gtgergergersagersgers")])
    ));

    let mut got = Header::default();
    let ok = internals::merge_pax(
        &mut got,
        recs(vec![
            rec(b"missing", b"missing"),
            rec(b"SCHILY.xattr.key", b"value"),
        ]),
    );
    assert!(ok);
    let want = Header {
        xattrs: recs(vec![rec(b"key", b"value")]),
        pax_records: recs(vec![
            rec(b"missing", b"missing"),
            rec(b"SCHILY.xattr.key", b"value"),
        ]),
        ..Header::default()
    };
    assert!(header_eq(&got, &want), "{got:?}");
}

#[test]
fn parse_pax() {
    let vectors: Vec<(&[u8], Option<Records>)> = vec![
        (b"", Some(recs(vec![]))),
        (b"6 k=1\n", Some(recs(vec![rec(b"k", b"1")]))),
        (b"10 a=name\n", Some(recs(vec![rec(b"a", b"name")]))),
        (b"9 a=name\n", Some(recs(vec![rec(b"a", b"name")]))),
        (
            b"30 mtime=1350244992.023960108\n",
            Some(recs(vec![rec(b"mtime", b"1350244992.023960108")])),
        ),
        (b"3 somelongkey=\n", None),
        (b"50 tooshort=\n", None),
        (
            b"13 key1=haha\n13 key2=nana\n13 key3=kaka\n",
            Some(recs(vec![
                rec(b"key1", b"haha"),
                rec(b"key2", b"nana"),
                rec(b"key3", b"kaka"),
            ])),
        ),
        (
            b"13 key1=val1\n13 key2=val2\n8 key1=\n",
            Some(recs(vec![rec(b"key1", b""), rec(b"key2", b"val2")])),
        ),
        (
            b"22 GNU.sparse.size=10\n26 GNU.sparse.numblocks=2\n23 GNU.sparse.offset=1\n25 GNU.sparse.numbytes=2\n23 GNU.sparse.offset=3\n25 GNU.sparse.numbytes=4\n",
            Some(recs(vec![
                rec(b"GNU.sparse.size", b"10"),
                rec(b"GNU.sparse.numblocks", b"2"),
                rec(b"GNU.sparse.map", b"1,2,3,4"),
            ])),
        ),
        (
            b"22 GNU.sparse.size=10\n26 GNU.sparse.numblocks=1\n25 GNU.sparse.numbytes=2\n23 GNU.sparse.offset=1\n",
            None,
        ),
        (
            b"22 GNU.sparse.size=10\n26 GNU.sparse.numblocks=1\n25 GNU.sparse.offset=1,2\n25 GNU.sparse.numbytes=2\n",
            None,
        ),
    ];

    for (i, (input, want)) in vectors.iter().enumerate() {
        let got = internals::parse_pax(input);
        assert_eq!(got.is_some(), want.is_some(), "test {i}");
        if let (Some(got), Some(want)) = (got, want) {
            assert_eq!(&got, want, "test {i}");
        }
    }
}

#[test]
fn next_errors_are_sticky() {
    let data = testdata("neg-size.tar");
    let mut tr = tar::Reader::new(data.as_slice());
    let first = loop {
        match tr.next() {
            Ok(Some(_)) => continue,
            Ok(None) => panic!("neg-size.tar must fail"),
            Err(e) => break e.to_string(),
        }
    };
    assert_eq!(first, HEADER);
    assert_eq!(tr.next().unwrap_err().to_string(), HEADER);
    let mut buf = [0u8; 4];
    assert_eq!(tr.read(&mut buf).unwrap_err().to_string(), HEADER);
}
