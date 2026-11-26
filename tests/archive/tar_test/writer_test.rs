use crate::writer_vectors::writer_vectors;
use crate::*;
use elysiandb::archive::tar::internals;
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::rc::Rc;

#[derive(Clone)]
struct TruncateWriter {
    buf: Rc<RefCell<Vec<u8>>>,
    max: usize,
}

impl Write for TruncateWriter {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        let mut buf = self.buf.borrow_mut();
        let room = self.max.saturating_sub(buf.len());
        let n = room.min(p.len());
        buf.extend_from_slice(&p[..n]);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_go<W: Write>(tw: &mut tar::Writer<W>, b: &[u8]) -> (usize, Option<io::Error>) {
    match tw.write(b) {
        Ok(n) if n < b.len() => match tw.write(&b[n..]) {
            Ok(m) => (n + m, None),
            Err(e) => (n, Some(e)),
        },
        Ok(n) => (n, None),
        Err(e) => (0, Some(e)),
    }
}

#[test]
fn writer() {
    let vectors = writer_vectors();
    assert_eq!(vectors.len(), 13);
    for v in &vectors {
        const MAX_SIZE: usize = 10 << 10;
        let buf = Rc::new(RefCell::new(Vec::new()));
        let mut tw = tar::Writer::new(TruncateWriter {
            buf: buf.clone(),
            max: MAX_SIZE,
        });

        for (i, op) in v.ops.iter().enumerate() {
            match op {
                Op::Header(hdr, want) => {
                    let err = tw.write_header(hdr).err();
                    assert!(
                        error_matches(err.as_ref(), *want),
                        "{}: test {i}, write_header() = {err:?}, want {want:?}",
                        v.file
                    );
                }
                Op::Write(data, want_cnt, want) => {
                    let (got, err) = write_go(&mut tw, data);
                    assert!(
                        got == *want_cnt && error_matches(err.as_ref(), *want),
                        "{}: test {i}, write() = ({got}, {err:?}), want ({want_cnt}, {want:?})",
                        v.file
                    );
                }
                Op::Close(want) => {
                    let err = tw.close().err();
                    assert!(
                        error_matches(err.as_ref(), *want),
                        "{}: test {i}, close() = {err:?}, want {want:?}",
                        v.file
                    );
                }
            }
        }

        if !v.file.is_empty() {
            let want = testdata(v.file);
            let got = buf.borrow().clone();
            assert!(got == want, "{}: incorrect result", v.file);
        }
    }
}

fn small_header() -> Header {
    Header {
        name: b"small.txt".to_vec(),
        mode: 0o644,
        size: 5,
        mod_time: unix(1244428340, 0),
        typeflag: tar::TYPE_REG,
        ..Header::default()
    }
}

fn write_one(hdr: &Header, contents: &[u8]) -> Vec<u8> {
    let mut writer = tar::Writer::new(Vec::new());
    writer.write_header(hdr).unwrap();
    writer.write_all(contents).unwrap();
    writer.close().unwrap();
    writer.into_inner()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn index(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}

#[test]
fn pax() {
    let mut hdr = small_header();
    let long_name = b"ab".repeat(100);
    let contents = vec![b' '; hdr.size as usize];
    hdr.name = long_name.clone();
    let buf = write_one(&hdr, &contents);
    assert!(contains(&buf, b"PaxHeaders.0"), "expected a PAX header");
    let mut reader = tar::Reader::new(buf.as_slice());
    let hdr = reader.next().unwrap().unwrap();
    assert_eq!(hdr.name, long_name, "couldn't recover long file name");
}

#[test]
fn pax_symlink() {
    let mut hdr = small_header();
    hdr.typeflag = tar::TYPE_SYMLINK;
    let long_linkname = b"1234567890/1234567890".repeat(10);
    hdr.linkname = long_linkname.clone();
    hdr.size = 0;
    let buf = write_one(&hdr, b"");
    assert!(contains(&buf, b"PaxHeaders.0"), "expected a PAX header");
    let mut reader = tar::Reader::new(buf.as_slice());
    let hdr = reader.next().unwrap().unwrap();
    assert_eq!(
        hdr.linkname, long_linkname,
        "couldn't recover long link name"
    );
}

#[test]
fn pax_non_ascii() {
    let mut hdr = small_header();
    let chinese_filename = "文件名".as_bytes().to_vec();
    let chinese_groupname = "組".as_bytes().to_vec();
    let chinese_username = "用戶名".as_bytes().to_vec();
    hdr.name = chinese_filename.clone();
    hdr.gname = chinese_groupname.clone();
    hdr.uname = chinese_username.clone();
    let contents = vec![b' '; hdr.size as usize];
    let buf = write_one(&hdr, &contents);
    assert!(contains(&buf, b"PaxHeaders.0"), "expected a PAX header");
    let mut reader = tar::Reader::new(buf.as_slice());
    let hdr = reader.next().unwrap().unwrap();
    assert_eq!(hdr.name, chinese_filename);
    assert_eq!(hdr.gname, chinese_groupname);
    assert_eq!(hdr.uname, chinese_username);
}

#[test]
fn pax_xattrs() {
    let xattrs = recs(vec![rec(b"user.key", b"value")]);
    let mut hdr = small_header();
    hdr.xattrs = xattrs.clone();
    let buf = write_one(&hdr, b"Kilts");
    let mut reader = tar::Reader::new(buf.as_slice());
    let hdr = reader.next().unwrap().unwrap();
    assert_eq!(hdr.xattrs, xattrs, "xattrs did not survive round trip");
}

#[test]
fn pax_headers_sorted() {
    let mut hdr = small_header();
    let contents = vec![b' '; hdr.size as usize];
    hdr.xattrs = recs(vec![
        rec(b"foo", b"foo"),
        rec(b"bar", b"bar"),
        rec(b"baz", b"baz"),
        rec(b"qux", b"qux"),
    ]);
    let buf = write_one(&hdr, &contents);
    assert!(contains(&buf, b"PaxHeaders.0"));
    let indices = [
        index(&buf, b"bar=bar"),
        index(&buf, b"baz=baz"),
        index(&buf, b"foo=foo"),
        index(&buf, b"qux=qux"),
    ];
    assert!(indices.is_sorted(), "PAX headers are not sorted");
}

#[test]
fn ustar_long_name() {
    let mut hdr = small_header();
    hdr.typeflag = tar::TYPE_DIR;
    let long_name = b"/0000_0000000/00000-000000000/0000_0000000/00000-0000000000000/0000_0000000/00000-0000000-00000000/0000_0000000/00000000/0000_0000000/000/0000_0000000/00000000v00/0000_0000000/000000/0000_0000000/0000000/0000_0000000/00000y-00/0000/0000/00000000/0x000000/".to_vec();
    hdr.name = long_name.clone();
    hdr.size = 0;
    let mut writer = tar::Writer::new(Vec::new());
    writer.write_header(&hdr).unwrap();
    writer.close().unwrap();
    let buf = writer.into_inner();
    let mut reader = tar::Reader::new(buf.as_slice());
    let hdr = reader.next().unwrap().unwrap();
    assert_eq!(hdr.name, long_name, "couldn't recover long name");
}

#[test]
fn valid_typeflag_with_pax_header() {
    let file_name = b"ab".repeat(100);
    let hdr = Header {
        name: file_name,
        size: 4,
        typeflag: 0,
        ..Header::default()
    };
    let buf = write_one(&hdr, b"fooo");
    let mut tr = tar::Reader::new(buf.as_slice());
    while let Some(header) = tr.next().unwrap() {
        assert_eq!(header.typeflag, tar::TYPE_REG);
    }
}

struct FailWriter;

impl Write for FailWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::WriteZero))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_errors() {
    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"dir/".to_vec(),
        typeflag: tar::TYPE_DIR,
        ..Header::default()
    };
    tw.write_header(&hdr).unwrap();
    assert_eq!(
        tw.write(&[0x00]).unwrap_err().to_string(),
        WRITE_TOO_LONG,
        "header only"
    );

    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"small.txt".to_vec(),
        size: -1,
        ..Header::default()
    };
    assert!(tw.write_header(&hdr).is_err(), "negative size");

    let mut tw = tar::Writer::new(Vec::new());
    assert_eq!(
        tw.write(b"Kilts").unwrap_err().to_string(),
        WRITE_TOO_LONG,
        "before header"
    );

    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"small.txt".to_vec(),
        ..Header::default()
    };
    tw.write_header(&hdr).unwrap();
    tw.close().unwrap();
    assert_eq!(
        tw.write(b"Kilts").unwrap_err().to_string(),
        WRITE_AFTER_CLOSE
    );
    assert_eq!(tw.flush().unwrap_err().to_string(), WRITE_AFTER_CLOSE);
    tw.close().unwrap();

    let mut tw = tar::Writer::new(Vec::new());
    let hdr = Header {
        name: b"small.txt".to_vec(),
        size: 5,
        ..Header::default()
    };
    tw.write_header(&hdr).unwrap();
    assert!(tw.flush().is_err(), "premature flush");

    let mut tw = tar::Writer::new(Vec::new());
    tw.write_header(&hdr).unwrap();
    assert!(tw.close().is_err(), "premature close");

    let mut tw = tar::Writer::new(FailWriter);
    let err = tw.write_header(&Header::default()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::WriteZero);
    assert!(
        tw.write_header(&Header {
            name: b"small.txt".to_vec(),
            ..Header::default()
        })
        .is_err()
    );
    assert!(tw.write(&[]).is_err());
    assert!(tw.flush().is_err());
    assert!(tw.close().is_err());
}

#[test]
fn split_ustar_path() {
    let sr = |s: &str, n: usize| s.repeat(n);
    const NAME_SIZE: usize = 100;
    const PREFIX_SIZE: usize = 155;
    let vectors: Vec<(String, String, String, bool)> = vec![
        (String::new(), String::new(), String::new(), false),
        ("abc".into(), String::new(), String::new(), false),
        ("用戶名".into(), String::new(), String::new(), false),
        (sr("a", NAME_SIZE), String::new(), String::new(), false),
        (
            sr("a", NAME_SIZE) + "/",
            String::new(),
            String::new(),
            false,
        ),
        (
            sr("a", NAME_SIZE) + "/a",
            sr("a", NAME_SIZE),
            "a".into(),
            true,
        ),
        (
            sr("a", PREFIX_SIZE) + "/",
            String::new(),
            String::new(),
            false,
        ),
        (
            sr("a", PREFIX_SIZE) + "/a",
            sr("a", PREFIX_SIZE),
            "a".into(),
            true,
        ),
        (sr("a", NAME_SIZE + 1), String::new(), String::new(), false),
        (
            sr("/", NAME_SIZE + 1),
            sr("/", NAME_SIZE - 1),
            "/".into(),
            true,
        ),
        (
            sr("a", PREFIX_SIZE) + "/" + &sr("b", NAME_SIZE),
            sr("a", PREFIX_SIZE),
            sr("b", NAME_SIZE),
            true,
        ),
        (
            sr("a", PREFIX_SIZE) + "//" + &sr("b", NAME_SIZE),
            String::new(),
            String::new(),
            false,
        ),
        (sr("a/", NAME_SIZE), sr("a/", 77) + "a", sr("a/", 22), true),
    ];

    for (input, prefix, suffix, ok) in vectors {
        let got = internals::split_ustar_path(input.as_bytes());
        match got {
            Some((p, s)) => {
                assert!(ok, "{input:?}");
                assert_eq!(p, prefix.as_bytes(), "{input:?}");
                assert_eq!(s, suffix.as_bytes(), "{input:?}");
            }
            None => assert!(!ok, "{input:?}"),
        }
    }
}

#[test]
fn issue_12594() {
    let names = [
        "0/1/2/3/4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19/20/21/22/23/24/25/26/27/28/29/30/file.txt",
        "0/1/2/3/4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19/20/21/22/23/24/25/26/27/28/29/30/31/32/33/file.txt",
        "0/1/2/3/4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19/20/21/22/23/24/25/26/27/28/29/30/31/32/333/file.txt",
        "0/1/2/3/4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19/20/21/22/23/24/25/26/27/28/29/30/31/32/33/34/35/36/37/38/39/40/file.txt",
        "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000/file.txt",
        "/home/support/.openoffice.org/3/user/uno_packages/cache/registry/com.sun.star.comp.deployment.executable.PackageRegistryBackend",
    ];

    for (i, name) in names.iter().enumerate() {
        let mut tw = tar::Writer::new(Vec::new());
        tw.write_header(&Header {
            name: name.as_bytes().to_vec(),
            uid: 1 << 25,
            ..Header::default()
        })
        .unwrap_or_else(|e| panic!("test {i}, write_header: {e}"));
        tw.close().unwrap();
        let b = tw.into_inner();

        let is_gnu = &b[257..263] == b"ustar " && &b[263..265] == b" \x00";
        let prefix_field = &b[345..500];
        let prefix_len = prefix_field.iter().position(|&c| c == 0).unwrap_or(155);
        let prefix = &prefix_field[..prefix_len];
        assert!(
            !(is_gnu && !prefix.is_empty() && name.as_bytes().starts_with(prefix)),
            "test {i}, found prefix in GNU format"
        );

        let mut tr = tar::Reader::new(b.as_slice());
        let hdr = tr.next().unwrap().unwrap();
        assert_eq!(hdr.name, name.as_bytes(), "test {i}");
    }
}

#[test]
fn write_long_header() {
    const MAX: usize = 1 << 20;
    let long = vec![b'a'; MAX];
    let headers = [
        Header {
            name: long.clone(),
            ..Header::default()
        },
        Header {
            linkname: long.clone(),
            ..Header::default()
        },
        Header {
            uname: long.clone(),
            ..Header::default()
        },
        Header {
            gname: long.clone(),
            ..Header::default()
        },
        Header {
            pax_records: recs(vec![(b"VENDOR.x".to_vec(), long.clone())]),
            ..Header::default()
        },
    ];
    for (i, h) in headers.iter().enumerate() {
        let mut w = tar::Writer::new(io::sink());
        let err = w.write_header(h).unwrap_err();
        assert_eq!(err.to_string(), FIELD_TOO_LONG, "test {i}");
    }
}

#[test]
fn backup_entries_round_trip() {
    let hdr = Header {
        name: b"manifest.json".to_vec(),
        mode: 0o600,
        size: 5,
        mod_time: unix(1795429230, 0),
        typeflag: tar::TYPE_REG,
        ..Header::default()
    };
    let buf = write_one(&hdr, b"hello");
    assert_eq!(buf.len(), 2048);
    let mut tr = tar::Reader::new(buf.as_slice());
    let got = tr.next().unwrap().unwrap();
    assert_eq!(got.format, tar::FORMAT_USTAR);
    let mut data = Vec::new();
    tr.read_to_end(&mut data).unwrap();
    assert_eq!(data, b"hello");
}
