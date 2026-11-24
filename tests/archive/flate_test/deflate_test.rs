use crate::{Shared, inflate_all, testdata};
use elysiandb::archive::flate::{
    self, BEST_SPEED, BUFFER_RESET, DeflateFast, INPUT_MARGIN, MAX_MATCH_LENGTH, MAX_MATCH_OFFSET,
    Writer, reverse_bits,
};
use gocompat::rand::{Rand, new_source};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::rc::Rc;

fn large_data_chunk() -> Vec<u8> {
    (0..100000usize).map(|i| ((i * i) & 0xFF) as u8).collect()
}

fn deflate_inflate_tests() -> Vec<Vec<u8>> {
    vec![
        vec![],
        vec![0x11],
        vec![0x11, 0x12],
        vec![0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11],
        vec![
            0x11, 0x10, 0x13, 0x41, 0x21, 0x21, 0x41, 0x13, 0x87, 0x78, 0x13,
        ],
        large_data_chunk(),
    ]
}

#[test]
fn unsupported_levels_are_rejected() {
    for level in [-2, -1, 0, 2, 5, 9, 10] {
        let err = Writer::new(Vec::new(), level).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            err.to_string(),
            format!("unsupported compression level {level}")
        );
    }
}

#[test]
fn writer_close() {
    let mut zw = Writer::new(Shared::default(), BEST_SPEED).unwrap();

    match zw.write(b"Test") {
        Ok(4) => {}
        other => panic!("write to not closed writer: {other:?}"),
    }

    zw.close().unwrap();

    let after_close = zw.get_ref().size();

    assert!(zw.write(b"Test").is_err(), "write to closed writer");
    let err = zw.flush().unwrap_err();
    assert_eq!(err.to_string(), "the compressed stream is already closed");

    zw.close().unwrap();

    assert_eq!(
        after_close,
        zw.get_ref().size(),
        "writer wrote data after close"
    );
}

struct SparseReader {
    l: i64,
    cur: i64,
}

impl Read for SparseReader {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.cur >= self.l {
            return Ok(0);
        }
        let mut n = b.len();
        let mut cur = self.cur + n as i64;
        if cur > self.l {
            n -= (cur - self.l) as usize;
            cur = self.l;
        }
        for (i, v) in b[..n].iter_mut().enumerate() {
            *v = u8::from(self.cur + i as i64 >= self.l - (1 << 16));
        }
        self.cur = cur;
        Ok(n)
    }
}

#[test]
fn very_long_sparse_chunk() {
    let mut w = Writer::new(io::sink(), BEST_SPEED).unwrap();
    let mut r = SparseReader {
        l: 2_300_000_000,
        cur: 0,
    };
    io::copy(&mut r, &mut w).expect("compress failed");
}

#[derive(Default)]
struct SyncState {
    data: Vec<u8>,
    pos: usize,
}

#[derive(Clone, Default)]
struct SyncBuffer(Rc<RefCell<SyncState>>);

impl SyncBuffer {
    fn remaining(&self) -> usize {
        let s = self.0.borrow();
        s.data.len() - s.pos
    }
}

impl Read for SyncBuffer {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        let mut s = self.0.borrow_mut();
        let avail = s.data.len() - s.pos;
        assert!(avail > 0, "the reader needs data that was not written yet");
        let n = avail.min(p.len());
        let pos = s.pos;
        p[..n].copy_from_slice(&s.data[pos..pos + n]);
        s.pos += n;
        Ok(n)
    }
}

#[derive(Clone)]
struct Tee(SyncBuffer, Shared);

impl Write for Tee {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.0.borrow_mut().data.extend_from_slice(b);
        self.1.write(b)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_at_least<R: Read>(r: &mut R, buf: &mut [u8], min: usize) -> io::Result<usize> {
    let mut n = 0;
    while n < min {
        match r.read(&mut buf[n..])? {
            0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            m => n += m,
        }
    }
    Ok(n)
}

fn test_sync(level: i32, input: &[u8], name: &str) {
    if input.is_empty() {
        return;
    }

    let buf = SyncBuffer::default();
    let buf1 = Shared::default();
    let mut w = Writer::new(Tee(buf.clone(), buf1.clone()), level).unwrap();
    let mut r = flate::new_reader(buf.clone());

    for i in 0..2 {
        let (lo, hi) = if i == 0 {
            (0, input.len().div_ceil(2))
        } else {
            (input.len().div_ceil(2), input.len())
        };
        w.write_all(&input[lo..hi])
            .unwrap_or_else(|e| panic!("test_sync: write: {e}"));
        if i == 0 {
            w.flush()
                .unwrap_or_else(|e| panic!("test_sync: flush: {e}"));
        } else {
            w.close()
                .unwrap_or_else(|e| panic!("test_sync: close: {e}"));
        }
        let mut out = vec![0u8; hi - lo + 1];
        let m = read_at_least(&mut r, &mut out, hi - lo)
            .unwrap_or_else(|e| panic!("test_sync/{i} ({level}, {}, {name}): {e}", input.len()));
        assert_eq!(
            m,
            hi - lo,
            "test_sync/{i} ({level}, {}, {name})",
            input.len()
        );
        assert_eq!(
            &input[lo..hi],
            &out[..hi - lo],
            "test_sync/{i}: read wrong bytes"
        );
    }
    let mut out = [0u8; 10];
    let n = r.read(&mut out).unwrap();
    assert_eq!(
        n,
        0,
        "test_sync ({level}, {}, {name}): final read",
        input.len()
    );
    assert_eq!(
        buf.remaining(),
        0,
        "test_sync ({level}, {}, {name}): extra data at end",
        input.len()
    );
    r.close().unwrap();

    let out = inflate_all(&buf1.bytes()).unwrap_or_else(|e| panic!("test_sync: read: {e}"));
    assert!(
        input == out.as_slice(),
        "test_sync: decompress(compress(data)) != data: level={level} input={name}"
    );
}

fn test_to_from_with_level_and_limit(level: i32, input: &[u8], name: &str, limit: usize) {
    let mut w = Writer::new(Vec::new(), level).unwrap();
    w.write_all(input).unwrap();
    w.close().unwrap();
    let buffer = w.get_ref().clone();
    if limit > 0 {
        assert!(
            buffer.len() <= limit,
            "level: {level}, len(compress(data)) = {} > limit = {limit}",
            buffer.len()
        );
    }
    let out = inflate_all(&buffer).unwrap_or_else(|e| panic!("read: {e}"));
    assert!(
        input == out.as_slice(),
        "decompress(compress(data)) != data: level={level} input={name}"
    );
    test_sync(level, input, name);
}

#[test]
fn deflate_inflate() {
    for (i, input) in deflate_inflate_tests().iter().enumerate() {
        test_to_from_with_level_and_limit(BEST_SPEED, input, &format!("#{i}"), 0);
    }
}

#[test]
fn reverse_bits_matches_go() {
    let tests: [(u16, u8, u16); 8] = [
        (1, 1, 1),
        (1, 2, 2),
        (1, 3, 4),
        (1, 4, 8),
        (1, 5, 16),
        (17, 5, 17),
        (257, 9, 257),
        (29, 5, 23),
    ];
    for (input, bit_count, out) in tests {
        assert_eq!(reverse_bits(input, bit_count), out, "{input} {bit_count}");
    }
}

#[test]
fn deflate_inflate_string() {
    let tests = [
        ("e.text", "2.718281828...", 50650usize),
        ("Isaac.Newton-Opticks.text", "Isaac.Newton-Opticks", 218338),
    ];
    for (file, label, limit) in tests {
        let gold = testdata(file);
        test_to_from_with_level_and_limit(BEST_SPEED, &gold, label, limit);
    }
}

#[test]
fn regression_2508() {
    let mut w = Writer::new(io::sink(), BEST_SPEED).unwrap();
    let buf = [0u8; 1024];
    for _ in 0..131072 {
        w.write_all(&buf)
            .unwrap_or_else(|e| panic!("writer failed: {e}"));
    }
    w.close().unwrap();
}

fn test_reset_output() {
    let write_data = |w: &mut Writer<Shared>| {
        w.write_all(b"now is the time for all good gophers")
            .unwrap();
        w.flush().unwrap();

        for _ in 0..1024 {
            w.write_all(b"hello world").unwrap();
        }

        w.write_all(&b"x".repeat(65000)).unwrap();
    };

    let buf = Shared::default();
    let mut w = Writer::new(buf.clone(), BEST_SPEED).unwrap();
    write_data(&mut w);
    w.close().unwrap();
    let out1 = buf.bytes();

    let buf2 = Shared::default();
    w.reset(buf2.clone());
    write_data(&mut w);
    w.close().unwrap();
    let out2 = buf2.bytes();

    assert_eq!(out1.len(), out2.len());
    assert_eq!(out1, out2);
}

#[test]
fn writer_reset() {
    let buf = Shared::default();
    let mut w = Writer::new(buf.clone(), BEST_SPEED).unwrap();
    for _ in 0..1024 {
        w.write_all(b"hello world").unwrap();
    }
    let fresh = Shared::default();
    w.reset(fresh.clone());
    let wref_buf = Shared::default();
    let mut wref = Writer::new(wref_buf.clone(), BEST_SPEED).unwrap();
    w.write_all(b"after reset").unwrap();
    wref.write_all(b"after reset").unwrap();
    w.close().unwrap();
    wref.close().unwrap();
    assert_eq!(fresh.bytes(), wref_buf.bytes());

    test_reset_output();
}

#[test]
fn best_speed() {
    let abc: Vec<u8> = (0..128u8).collect();
    let abcabc = abc.repeat(131072 / abc.len());
    let mut want: Vec<u8> = Vec::new();

    let mut test_cases: Vec<Vec<usize>> = vec![
        vec![65536, 0],
        vec![65536, 1],
        vec![65536, 1, 256],
        vec![65536, 1, 65536],
        vec![65536, 14],
        vec![65536, 15],
        vec![65536, 16],
        vec![65536, 16, 256],
        vec![65536, 16, 65536],
        vec![65536, 127],
        vec![65536, 128],
        vec![65536, 128, 256],
        vec![65536, 128, 65536],
        vec![65536, 129],
        vec![65536, 65536, 256],
        vec![65536, 65536, 65536],
    ];

    for (i, tc) in test_cases.iter_mut().enumerate() {
        for first_n in [1usize, 65534, 65535, 65536, 65537, 131072] {
            tc[0] = first_n;
            for flush in [false, true] {
                want.clear();
                let mut w = Writer::new(Vec::new(), BEST_SPEED).unwrap();
                for &n in tc.iter() {
                    want.extend_from_slice(&abcabc[..n]);
                    w.write_all(&abcabc[..n]).unwrap();
                    if flush {
                        w.flush().unwrap();
                    }
                }
                w.close().unwrap();

                let got = inflate_all(w.get_ref()).unwrap_or_else(|e| {
                    panic!("i={i}, first_n={first_n}, flush={flush}: read: {e}")
                });
                assert!(
                    got == want,
                    "i={i}, first_n={first_n}, flush={flush}: corruption during deflate-then-inflate"
                );
            }
        }
    }
}

struct FailWriter {
    n: i32,
}

fn err_io() -> io::Error {
    io::Error::other("IO error")
}

impl Write for FailWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.n -= 1;
        if self.n == -1 {
            return Err(err_io());
        }
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct SharedFail(Rc<RefCell<FailWriter>>);

impl Write for SharedFail {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(b)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn is_err_io(r: &io::Result<()>) -> bool {
    matches!(r, Err(e) if e.to_string() == "IO error")
}

#[test]
fn writer_persistent_write_error() {
    let d = testdata("Isaac.Newton-Opticks.text");
    let d = &d[..10000];

    let mut zw = Writer::new(
        SharedFail(Rc::new(RefCell::new(FailWriter { n: 0 }))),
        BEST_SPEED,
    )
    .unwrap();

    for i in 0..1000 {
        let fw = SharedFail(Rc::new(RefCell::new(FailWriter { n: i })));
        zw.reset(fw.clone());

        let werr = zw.write_all(d);
        let cerr = zw.close();
        let ferr = zw.flush();
        let n = fw.0.borrow().n;
        assert!(
            werr.is_ok() || is_err_io(&werr),
            "test {i}, mismatching write error: {werr:?}"
        );
        if n < 0 {
            assert!(
                is_err_io(&cerr),
                "test {i}, mismatching close error: {cerr:?}"
            );
            assert!(
                is_err_io(&ferr),
                "test {i}, mismatching flush error: {ferr:?}"
            );
        }
        if n >= 0 {
            return;
        }
    }
}

#[test]
fn writer_persistent_flush_error() {
    let mut zw = Writer::new(FailWriter { n: 0 }, BEST_SPEED).unwrap();
    let flush_err = zw.flush();
    let close_err = zw.close();
    let write_err = zw.write(b"Test").map(|_| ());
    for err in [close_err, flush_err, write_err] {
        assert!(is_err_io(&err), "{err:?}");
    }
}

#[test]
fn writer_persistent_close_error() {
    let mut zw = Writer::new(
        SharedFail(Rc::new(RefCell::new(FailWriter { n: 0 }))),
        BEST_SPEED,
    )
    .unwrap();
    let close_err = zw.close();
    let flush_err = zw.flush();
    let write_err = zw.write(b"Test").map(|_| ());
    for err in [close_err, flush_err, write_err] {
        assert!(is_err_io(&err), "{err:?}");
    }

    zw.reset(SharedFail(Rc::new(RefCell::new(FailWriter { n: 1000 }))));
    zw.close()
        .unwrap_or_else(|e| panic!("first call to close returned error: {e}"));
    zw.close()
        .unwrap_or_else(|e| panic!("second call to close returned error: {e}"));

    let flush_err = zw.flush();
    let write_err = zw.write(b"Test").map(|_| ());
    for err in [flush_err, write_err] {
        match err {
            Err(e) => assert_eq!(e.to_string(), "the compressed stream is already closed"),
            Ok(()) => panic!("expected the closed error"),
        }
    }
}

#[test]
fn best_speed_match() {
    struct Case {
        previous: Vec<u8>,
        current: Vec<u8>,
        t: i32,
        s: i32,
        want: i32,
    }
    let m = (MAX_MATCH_LENGTH - 4) as i32;
    let cases = vec![
        Case {
            previous: vec![0, 0, 0, 1, 2],
            current: vec![3, 4, 5, 0, 1, 2, 3, 4, 5],
            t: -3,
            s: 3,
            want: 6,
        },
        Case {
            previous: vec![0, 0, 0, 1, 2],
            current: vec![2, 4, 5, 0, 1, 2, 3, 4, 5],
            t: -3,
            s: 3,
            want: 3,
        },
        Case {
            previous: vec![0, 0, 0, 1, 1],
            current: vec![3, 4, 5, 0, 1, 2, 3, 4, 5],
            t: -3,
            s: 3,
            want: 2,
        },
        Case {
            previous: vec![0, 0, 0, 1, 2],
            current: vec![2, 2, 2, 2, 1, 2, 3, 4, 5],
            t: -1,
            s: 0,
            want: 4,
        },
        Case {
            previous: vec![0, 0, 0, 1, 2, 3, 4, 5, 2, 2],
            current: vec![2, 2, 2, 2, 1, 2, 3, 4, 5],
            t: -7,
            s: 4,
            want: 5,
        },
        Case {
            previous: vec![9, 9, 9, 9, 9],
            current: vec![2, 2, 2, 2, 1, 2, 3, 4, 5],
            t: -1,
            s: 0,
            want: 0,
        },
        Case {
            previous: vec![9, 9, 9, 9, 9],
            current: vec![9, 2, 2, 2, 1, 2, 3, 4, 5],
            t: 0,
            s: 1,
            want: 0,
        },
        Case {
            previous: vec![],
            current: vec![9, 2, 2, 2, 1, 2, 3, 4, 5],
            t: -5,
            s: 1,
            want: 0,
        },
        Case {
            previous: vec![],
            current: vec![9, 2, 2, 2, 1, 2, 3, 4, 5],
            t: -1,
            s: 1,
            want: 0,
        },
        Case {
            previous: vec![],
            current: vec![2, 2, 2, 2, 1, 2, 3, 4, 5],
            t: 0,
            s: 1,
            want: 3,
        },
        Case {
            previous: vec![3, 4, 5],
            current: vec![3, 4, 5],
            t: -3,
            s: 0,
            want: 3,
        },
        Case {
            previous: vec![0; 1000],
            current: vec![0; 1000],
            t: -1000,
            s: 0,
            want: m,
        },
        Case {
            previous: vec![0; 200],
            current: vec![0; 500],
            t: -200,
            s: 0,
            want: m,
        },
        Case {
            previous: vec![0; 200],
            current: vec![0; 500],
            t: 0,
            s: 1,
            want: m,
        },
        Case {
            previous: vec![0; MAX_MATCH_LENGTH - 4],
            current: vec![0; 500],
            t: -m,
            s: 0,
            want: m,
        },
        Case {
            previous: vec![0; 200],
            current: vec![0; 500],
            t: -200,
            s: 400,
            want: 100,
        },
        Case {
            previous: vec![0; 10],
            current: vec![0; 500],
            t: 200,
            s: 400,
            want: 100,
        },
    ];
    for (i, c) in cases.iter().enumerate() {
        let e = DeflateFast::with_prev(&c.previous);
        let got = e.match_len(c.s, c.t, &c.current);
        assert_eq!(got, c.want, "test {i}: match length");
    }
}

#[test]
fn best_speed_max_match_offset() {
    let abc = b"abcdefgh";
    let xyz = b"stuvwxyz";
    for match_before in [false, true] {
        for extra in [
            0,
            INPUT_MARGIN - 1,
            INPUT_MARGIN,
            INPUT_MARGIN + 1,
            2 * INPUT_MARGIN,
        ] {
            for offset_adj in -5i64..=5 {
                let offset = (MAX_MATCH_OFFSET as i64 + offset_adj) as usize;

                let mut src = vec![0u8; offset + abc.len() + extra];
                src[..abc.len()].copy_from_slice(abc);
                if !match_before {
                    src[offset - xyz.len()..offset].copy_from_slice(xyz);
                }
                src[offset..offset + abc.len()].copy_from_slice(abc);

                let mut w = Writer::new(Vec::new(), BEST_SPEED).unwrap();
                w.write_all(&src).unwrap();
                w.close().unwrap();

                let dst = inflate_all(w.get_ref()).unwrap_or_else(|e| {
                    panic!(
                        "match_before={match_before}, extra={extra}, offset_adj={offset_adj}: {e}"
                    )
                });
                assert!(
                    dst == src,
                    "match_before={match_before}, extra={extra}, offset_adj={offset_adj}: bytes differ after round-tripping"
                );
            }
        }
    }
}

#[test]
fn best_speed_shift_offsets() {
    let mut enc = DeflateFast::new();

    let mut test_data = [0u8; 32];
    let mut rng = Rand::new(new_source(0));
    for b in test_data.iter_mut() {
        *b = rng.uint32() as u8;
    }

    let encode = |enc: &mut DeflateFast| {
        let mut dst = Vec::new();
        enc.encode(&mut dst, &test_data);
        dst.len()
    };

    let want_first_tokens = encode(&mut enc);
    let want_second_tokens = encode(&mut enc);

    assert!(
        want_first_tokens > want_second_tokens,
        "test needs matches between inputs to be generated"
    );
    enc.set_cur(BUFFER_RESET - test_data.len() as i32);

    assert_eq!(encode(&mut enc), want_first_tokens);

    assert_eq!(enc.cur(), BUFFER_RESET);

    assert_eq!(encode(&mut enc), want_second_tokens);

    assert!(enc.cur() < BUFFER_RESET);

    enc.set_cur(BUFFER_RESET);
    enc.shift_offsets();

    assert_eq!(encode(&mut enc), want_first_tokens);
}

#[test]
fn reading_after_the_end_returns_zero() {
    let data = b"hello hello hello hello hello hello";
    let compressed = crate::deflate_once(data);
    let mut r = flate::new_reader(compressed.as_slice());
    let mut out = Vec::new();
    r.read_to_end(&mut out).unwrap();
    assert_eq!(out, data);
    r.close().unwrap();
    let mut more = [0u8; 4];
    assert_eq!(r.read(&mut more).unwrap(), 0);
}
