use crate::{Shared, new_writer, read_all};
use elysiandb::archive::gzip::{self, BEST_SPEED, Header};
use gocompat::time::Time;
use std::io::{self, Write};

fn header_eq(a: &Header, b: &Header) -> bool {
    a.comment == b.comment
        && a.extra == b.extra
        && a.mod_time.equal(&b.mod_time)
        && a.mod_time.is_zero() == b.mod_time.is_zero()
        && a.name == b.name
        && a.os == b.os
}

#[test]
fn empty() {
    let mut w = new_writer(Vec::new());
    w.close().unwrap_or_else(|e| panic!("writer close: {e}"));
    let buf = w.get_ref().clone();

    let mut r = gzip::Reader::new(buf.as_slice()).unwrap_or_else(|e| panic!("new reader: {e}"));
    let want = Header {
        os: 255,
        ..Header::default()
    };
    assert!(
        header_eq(&r.header, &want),
        "header mismatch: {:?}",
        r.header
    );
    let (b, res) = read_all(&mut r);
    res.unwrap();
    assert!(b.is_empty(), "got {} bytes, want 0", b.len());
    r.close().unwrap();
}

#[test]
fn round_trip() {
    let mut w = new_writer(Vec::new());
    w.header.comment = "comment".to_string();
    w.header.extra = Some(b"extra".to_vec());
    w.header.mod_time = Time::from_unix(100_000_000, 0);
    w.header.name = "name".to_string();
    w.write_all(b"payload").unwrap();
    w.close().unwrap();
    let buf = w.get_ref().clone();

    let mut r = gzip::Reader::new(buf.as_slice()).unwrap();
    let (b, res) = read_all(&mut r);
    res.unwrap();
    assert_eq!(b, b"payload");
    assert_eq!(r.header.comment, "comment");
    assert_eq!(r.header.extra.as_deref(), Some(&b"extra"[..]));
    assert_eq!(r.header.mod_time.unix(), 100_000_000);
    assert_eq!(r.header.name, "name");
    r.close().unwrap();
}

#[test]
fn latin1() {
    let latin1 = [0xc4u8, b'u', 0xdf, b'e', b'r', b'u', b'n', b'g', 0];
    let utf8 = "Äußerung";

    let mut w = new_writer(Vec::new());
    w.header.name = utf8.to_string();
    w.close().unwrap();
    let out = w.get_ref();
    assert_eq!(out[3] & 0x08, 0x08);
    assert_eq!(&out[10..10 + latin1.len()], &latin1);

    let r = gzip::Reader::new(out.as_slice()).unwrap();
    assert_eq!(r.header.name, utf8);
}

#[test]
fn latin1_round_trip() {
    let test_cases = [
        ("", true),
        ("ASCII is OK", true),
        ("unless it contains a NUL\x00", false),
        ("no matter where \x00 occurs", false),
        ("\x00\x00\x00", false),
        ("Látin-1 also passes (U+00E1)", true),
        ("but LĀtin Extended-A (U+0100) does not", false),
        ("neither does 日本語", false),
        ("\x00 as does Látin-1 with NUL", false),
    ];
    for (name, ok) in test_cases {
        let mut w = new_writer(Vec::new());
        w.header.name = name.to_string();
        let res = w.close();
        assert_eq!(
            res.is_ok(),
            ok,
            "writer close: name = {name:?}, res = {res:?}"
        );
        if !ok {
            assert_eq!(
                res.unwrap_err().to_string(),
                "a gzip header text must be Latin-1 without NUL"
            );
            continue;
        }
        let buf = w.get_ref().clone();

        let mut r = gzip::Reader::new(buf.as_slice()).unwrap();
        let (_, res) = read_all(&mut r);
        res.unwrap();
        assert_eq!(r.header.name, name);
        r.close().unwrap();
    }
}

#[test]
fn writer_flush() {
    let buf = Shared::default();
    let mut w = new_writer(buf.clone());
    w.header.comment = "comment".to_string();
    w.header.extra = Some(b"extra".to_vec());
    w.header.mod_time = Time::from_unix(100_000_000, 0);
    w.header.name = "name".to_string();

    let n0 = buf.size();
    assert_eq!(n0, 0, "buffer size = {n0} before writes; want 0");

    w.flush().unwrap();

    let n1 = buf.size();
    assert_ne!(n1, 0, "no data after first flush");

    w.write_all(b"x").unwrap();

    let n2 = buf.size();
    assert_eq!(n1, n2, "after writing a single byte, size changed");

    w.flush().unwrap();

    let n3 = buf.size();
    assert_ne!(n2, n3, "flush didn't flush any data");

    w.close().unwrap();
}

#[test]
fn concat() {
    let buf = Shared::default();
    let mut w = new_writer(buf.clone());
    w.write_all(b"hello ").unwrap();
    w.close().unwrap();
    let mut w = new_writer(buf.clone());
    w.write_all(b"world\n").unwrap();
    w.close().unwrap();

    let data = buf.bytes();
    let mut r = gzip::Reader::new(data.as_slice()).unwrap();
    let (data, res) = read_all(&mut r);
    assert!(
        data == b"hello world\n" && res.is_ok(),
        "read all = {data:?}, {res:?}"
    );
}

#[test]
fn writer_reset() {
    let buf = Shared::default();
    let buf2 = Shared::default();
    let mut z = new_writer(buf.clone());
    let msg = b"hello world";
    z.write_all(msg).unwrap();
    z.close().unwrap();
    z.reset(buf2.clone());
    z.write_all(msg).unwrap();
    z.close().unwrap();
    assert_eq!(buf.bytes(), buf2.bytes());
}

struct LimitedWriter {
    n: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        if self.n < p.len() {
            let n = self.n;
            self.n = 0;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            return Ok(n);
        }
        self.n -= p.len();
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn limited_write() {
    let msg = b"a";

    for lim in 2..20 {
        let mut z = gzip::Writer::new_level(LimitedWriter { n: lim }, BEST_SPEED).unwrap();
        if let Ok(n) = z.write(msg) {
            assert!(n <= msg.len(), "write() = {n}, want {} or less", msg.len());
        }

        z.reset(LimitedWriter { n: lim });
        z.header = Header {
            comment: "comment".to_string(),
            extra: Some(b"extra".to_vec()),
            mod_time: Time::now(),
            name: "name".to_string(),
            os: 1,
        };
        if let Ok(n) = z.write(msg) {
            assert!(n <= msg.len(), "write() = {n}, want {} or less", msg.len());
        }
    }
}

#[test]
fn errors_are_sticky() {
    let mut z = gzip::Writer::new_level(LimitedWriter { n: 4 }, BEST_SPEED).unwrap();
    let first = z.write(b"a").unwrap_err().to_string();
    assert_eq!(z.write(b"b").unwrap_err().to_string(), first);
    assert_eq!(z.flush().unwrap_err().to_string(), first);
    assert_eq!(z.close().unwrap_err().to_string(), first);
}

#[test]
fn unsupported_level() {
    let err = gzip::Writer::new_level(Vec::new(), 9).err().unwrap();
    assert_eq!(err.to_string(), "unsupported compression level 9");
}

#[test]
fn write_after_close_fails_and_flush_succeeds() {
    let mut z = new_writer(Vec::new());
    z.write_all(b"data").unwrap();
    z.close().unwrap();
    let len = z.get_ref().len();
    z.flush().unwrap();
    z.close().unwrap();
    let err = z.write(b"more").unwrap_err();
    assert_eq!(err.to_string(), "the compressed stream is already closed");
    assert_eq!(z.flush().unwrap_err().to_string(), err.to_string());
    assert_eq!(z.get_ref().len(), len);
}
