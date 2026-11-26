use std::fmt;
use std::io::{self, Read};

use super::bufio::{BufReader, ReadErr, SliceErr};

const MAX_LINE_LENGTH: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkedError {
    Malformed,
    BareLf,
    InvalidCr,
    LineTooLong,
    EmptyLength,
    InvalidLengthByte,
    LengthTooLarge,
    TooMuchNonData,
}

impl fmt::Display for ChunkedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid chunked body")
    }
}

impl std::error::Error for ChunkedError {}

impl ChunkedError {
    fn into_io(self) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, self)
    }
}

pub fn is_chunked_error(err: &io::Error) -> bool {
    err.get_ref()
        .is_some_and(|e| e.downcast_ref::<ChunkedError>().is_some())
}

enum State {
    Ok,
    Eof,
    Err(io::Error),
}

pub struct ChunkedReader<R> {
    r: BufReader<R>,
    n: u64,
    state: State,
    check_end: bool,
    excess: i64,
}

fn clone_io_error(e: &io::Error) -> io::Error {
    if let Some(code) = e.raw_os_error() {
        return io::Error::from_raw_os_error(code);
    }
    if let Some(c) = e.get_ref().and_then(|i| i.downcast_ref::<ChunkedError>()) {
        return c.into_io();
    }
    if e.get_ref().is_none() {
        return io::Error::from(e.kind());
    }
    io::Error::new(e.kind(), e.to_string())
}

fn read_err(e: ReadErr) -> io::Error {
    match e {
        ReadErr::Eof => io::Error::from(io::ErrorKind::UnexpectedEof),
        ReadErr::Io(e) => e,
    }
}

fn read_chunk_line<R: Read>(b: &mut BufReader<R>) -> Result<Vec<u8>, io::Error> {
    let (mut p, err) = b.read_slice(b'\n');
    match err {
        None => {}
        Some(SliceErr::BufferFull) => return Err(ChunkedError::LineTooLong.into_io()),
        Some(SliceErr::Read(e)) => return Err(read_err(e)),
    }

    match p.iter().position(|&c| c == b'\r') {
        None => return Err(ChunkedError::BareLf.into_io()),
        Some(idx) if idx != p.len() - 2 => return Err(ChunkedError::InvalidCr.into_io()),
        Some(_) => {}
    }
    p.truncate(p.len() - 2);

    if p.len() >= MAX_LINE_LENGTH {
        return Err(ChunkedError::LineTooLong.into_io());
    }

    Ok(p)
}

fn trim_trailing_whitespace(mut b: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = b.split_last() {
        if !matches!(last, b' ' | b'\t' | b'\n' | b'\r') {
            break;
        }
        b = rest;
    }
    b
}

fn remove_chunk_extension(p: &[u8]) -> &[u8] {
    match p.iter().position(|&c| c == b';') {
        Some(i) => &p[..i],
        None => p,
    }
}

fn parse_hex_uint(v: &[u8]) -> Result<u64, ChunkedError> {
    if v.is_empty() {
        return Err(ChunkedError::EmptyLength);
    }

    let mut n: u64 = 0;
    for (i, &b) in v.iter().enumerate() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(ChunkedError::InvalidLengthByte),
        };
        if i == 16 {
            return Err(ChunkedError::LengthTooLarge);
        }
        n <<= 4;
        n |= u64::from(d);
    }

    Ok(n)
}

impl<R: Read> ChunkedReader<R> {
    pub fn new(r: BufReader<R>) -> ChunkedReader<R> {
        ChunkedReader {
            r,
            n: 0,
            state: State::Ok,
            check_end: false,
            excess: 0,
        }
    }

    fn fail(&mut self, e: io::Error) {
        self.state = State::Err(e);
    }

    fn begin_chunk(&mut self) {
        let line = match read_chunk_line(&mut self.r) {
            Ok(line) => line,
            Err(e) => return self.fail(e),
        };
        self.excess += line.len() as i64 + 2;
        let line = remove_chunk_extension(trim_trailing_whitespace(&line));
        self.n = match parse_hex_uint(line) {
            Ok(n) => n,
            Err(e) => return self.fail(e.into_io()),
        };
        self.excess -= 16 + 2 * (self.n as i64);
        self.excess = self.excess.max(0);
        if self.excess > 16 * 1024 {
            return self.fail(ChunkedError::TooMuchNonData.into_io());
        }
        if self.n == 0 {
            self.state = State::Eof;
        }
    }

    fn chunk_header_available(&mut self) -> bool {
        let n = self.r.buffered();
        if n > 0 {
            return self.r.buffer().contains(&b'\n');
        }
        false
    }

    fn read_full_crlf(&mut self) -> Result<[u8; 2], io::Error> {
        let mut buf = [0u8; 2];
        let mut got = 0;
        while got < 2 {
            match self.r.read_buf(&mut buf[got..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(n) => got += n,
                Err(ReadErr::Eof) => {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
                Err(ReadErr::Io(e)) => return Err(e),
            }
        }
        Ok(buf)
    }
}

impl<R: Read> Read for ChunkedReader<R> {
    fn read(&mut self, mut b: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        while matches!(self.state, State::Ok) {
            if self.check_end {
                if n > 0 && self.r.buffered() < 2 {
                    break;
                }
                match self.read_full_crlf() {
                    Ok(buf) => {
                        if &buf != b"\r\n" {
                            self.fail(ChunkedError::Malformed.into_io());
                            break;
                        }
                    }
                    Err(e) => {
                        self.fail(e);
                        break;
                    }
                }
                self.check_end = false;
            }
            if self.n == 0 {
                if n > 0 && !self.chunk_header_available() {
                    break;
                }
                self.begin_chunk();
                continue;
            }
            if b.is_empty() {
                break;
            }
            let want = b.len().min(usize::try_from(self.n).unwrap_or(usize::MAX));
            match self.r.read_buf(&mut b[..want]) {
                Ok(n0) => {
                    n += n0;
                    b = &mut b[n0..];
                    self.n -= n0 as u64;
                    if self.n == 0 {
                        self.check_end = true;
                    }
                }
                Err(ReadErr::Eof) => {
                    self.fail(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
                Err(ReadErr::Io(e)) => self.fail(e),
            }
        }

        if n > 0 {
            return Ok(n);
        }

        match &self.state {
            State::Ok | State::Eof => Ok(0),
            State::Err(e) => Err(clone_io_error(e)),
        }
    }
}
