pub mod flate;

use std::io::{self, BufRead, Read};

pub(crate) const UNEXPECTED_END: &str = "unexpected end of data";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    Eof,
    Corrupt(i64),
    Error(io::ErrorKind, String),
}

impl Fail {
    pub(crate) fn unexpected_eof() -> Fail {
        Fail::Error(io::ErrorKind::UnexpectedEof, UNEXPECTED_END.to_string())
    }

    pub(crate) fn from_io(e: &io::Error) -> Fail {
        Fail::Error(e.kind(), e.to_string())
    }

    pub(crate) fn no_eof(self) -> Fail {
        match self {
            Fail::Eof => Fail::unexpected_eof(),
            other => other,
        }
    }

    pub(crate) fn to_io(&self) -> io::Error {
        match self {
            Fail::Eof => io::Error::new(io::ErrorKind::UnexpectedEof, UNEXPECTED_END),
            Fail::Corrupt(offset) => io::Error::new(
                io::ErrorKind::InvalidData,
                flate::CorruptInputError(*offset),
            ),
            Fail::Error(kind, msg) => io::Error::new(*kind, msg.clone()),
        }
    }
}

pub(crate) fn read_full<R: Read + ?Sized>(r: &mut R, buf: &mut [u8]) -> (usize, Option<Fail>) {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => {
                if n == 0 {
                    return (0, Some(Fail::Eof));
                }
                return (n, Some(Fail::unexpected_eof()));
            }
            Ok(m) => n += m,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return (n, Some(Fail::from_io(&e))),
        }
    }
    (n, None)
}

pub(crate) fn read_byte<B: BufRead + ?Sized>(r: &mut B) -> Result<u8, Fail> {
    loop {
        match r.fill_buf() {
            Ok([]) => return Err(Fail::Eof),
            Ok(buf) => {
                let c = buf[0];
                r.consume(1);
                return Ok(c);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Fail::from_io(&e)),
        }
    }
}
