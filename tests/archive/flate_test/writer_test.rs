use crate::Shared;
use elysiandb::archive::flate::{BEST_SPEED, BUFFER_RESET, MAX_MATCH_OFFSET, Writer};
use gocompat::rand::{Rand, new_source};
use std::io::{self, Read, Write};

struct ErrorWriter {
    n: i64,
    failing: bool,
}

impl Write for ErrorWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if !self.failing {
            return Ok(b.len());
        }
        if self.n <= 0 {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.n -= 1;
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn copy_buffer<W: Write>(w: &mut W, mut input: &[u8], size: usize) -> io::Result<u64> {
    let mut buf = vec![0u8; size];
    let mut written = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            return Ok(written);
        }
        w.write_all(&buf[..n])?;
        written += n as u64;
    }
}

#[test]
fn write_error() {
    let mut buf = Vec::new();
    let n = 65536 * 4;
    for i in 0..n {
        writeln!(buf, "asdasfasf{i}{i}fghfgujyut{i}yutyu").unwrap();
    }
    let input = buf;
    let mut fail = 1;
    while fail <= 256 {
        let ew = ErrorWriter {
            n: fail,
            failing: true,
        };
        let mut w = Writer::new(ew, BEST_SPEED).unwrap();
        assert!(
            copy_buffer(&mut w, &input, 128).is_err(),
            "expected an error, fail = {fail}"
        );
        assert!(w.write(&[1, 2, 2, 3, 4, 5]).is_err(), "expected an error");
        assert!(w.flush().is_err(), "expected an error on flush");
        assert!(w.close().is_err(), "expected an error on close");

        w.reset(ErrorWriter {
            n: 0,
            failing: false,
        });
        let n2 = w
            .write(&[1, 2, 3, 4, 5, 6])
            .unwrap_or_else(|e| panic!("got unexpected error after reset: {e}"));
        assert!(n2 > 0, "got 0 length write, expected > 0");
        fail *= 2;
    }
}

#[test]
fn deterministic() {
    let length = 65535 * 30 + 500;

    let mut rng = Rand::new(new_source(1));
    let t1: Vec<u8> = (0..length).map(|_| (rng.int63() & 7) as u8).collect();

    let mut w = Writer::new(Vec::new(), BEST_SPEED).unwrap();
    copy_buffer(&mut w, &t1, 787).unwrap();
    w.close().unwrap();

    let mut w2 = Writer::new(Vec::new(), BEST_SPEED).unwrap();
    copy_buffer(&mut w2, &t1, 81761).unwrap();
    w2.close().unwrap();

    let b1b = w.get_ref();
    let b2b = w2.get_ref();
    assert!(
        b1b == b2b,
        "level 1 did not produce deterministic result, len(a) = {}, len(b) = {}",
        b1b.len(),
        b2b.len()
    );
}

#[test]
fn deflate_fast_reset() {
    let mut buf = Vec::new();
    let n = 65536;
    for i in 0..n {
        writeln!(buf, "asdfasdfasdfasdf{i}{i}fghfgujyut{i}yutyu").unwrap();
    }
    let input = buf;
    let mut offset = 1usize;

    let mut w = Writer::new(Vec::new(), BEST_SPEED).unwrap();
    w.write_all(&input).unwrap();
    w.write_all(&input).unwrap();
    w.write_all(&input).unwrap();
    w.close().unwrap();
    let want = w.get_ref().clone();

    while offset <= 256 {
        let mut w = Writer::new(Shared::default(), BEST_SPEED).unwrap();

        let resets =
            (BUFFER_RESET as usize - input.len() - offset - MAX_MATCH_OFFSET) / MAX_MATCH_OFFSET;
        for _ in 0..resets {
            w.reset_state();
        }
        let got = Shared::default();
        w.reset(got.clone());

        for _ in 0..3 {
            w.write_all(&input).unwrap();
        }
        w.close().unwrap();
        assert!(
            got.bytes() == want,
            "output did not match at wraparound, len(want) = {}, len(got) = {}",
            want.len(),
            got.size()
        );
        offset *= 2;
    }
}
