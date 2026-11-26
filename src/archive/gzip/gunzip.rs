use crate::archive::flate::Decompressor;
use crate::archive::{Fail, read_byte, read_full};
use crate::hash::crc32;
use gocompat::time::Time;
use std::io::{self, BufRead, BufReader, Read};

pub(super) const GZIP_ID1: u8 = 0x1f;
pub(super) const GZIP_ID2: u8 = 0x8b;
pub(super) const GZIP_DEFLATE: u8 = 8;
const FLAG_HDR_CRC: u8 = 1 << 1;
pub(super) const FLAG_EXTRA: u8 = 1 << 2;
pub(super) const FLAG_NAME: u8 = 1 << 3;
pub(super) const FLAG_COMMENT: u8 = 1 << 4;

const BUFIO_SIZE: usize = 4096;

const INVALID_HEADER: &str = "invalid gzip header";
const INVALID_CHECKSUM: &str = "invalid gzip checksum";

fn err_header() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, INVALID_HEADER.to_string())
}

fn err_checksum() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, INVALID_CHECKSUM.to_string())
}

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub comment: String,
    pub extra: Option<Vec<u8>>,
    pub mod_time: Time,
    pub name: String,
    pub os: u8,
}

pub struct Reader<B> {
    pub header: Header,
    r: B,
    decompressor: Decompressor,
    digest: u32,
    size: u32,
    buf: [u8; 512],
    err: Option<Fail>,
    multistream: bool,
}

pub fn new_reader<R: Read>(r: R) -> io::Result<Reader<BufReader<R>>> {
    Reader::new(BufReader::with_capacity(BUFIO_SIZE, r))
}

impl<B: BufRead> Reader<B> {
    pub fn new(r: B) -> io::Result<Reader<B>> {
        let mut z = Reader {
            header: Header::default(),
            r,
            decompressor: Decompressor::new(),
            digest: 0,
            size: 0,
            buf: [0; 512],
            err: None,
            multistream: true,
        };
        z.read_first_header()?;
        Ok(z)
    }

    pub fn reset(&mut self, r: B) -> io::Result<()> {
        self.r = r;
        self.read_first_header()
    }

    fn read_first_header(&mut self) -> io::Result<()> {
        self.header = Header::default();
        self.digest = 0;
        self.size = 0;
        self.err = None;
        self.multistream = true;
        let (header, err) = self.read_header();
        self.header = header;
        self.err = err;
        match &self.err {
            None => Ok(()),
            Some(err) => Err(err.to_io()),
        }
    }

    pub fn multistream(&mut self, ok: bool) {
        self.multistream = ok;
    }

    pub fn close(&mut self) -> io::Result<()> {
        match self.decompressor.close() {
            None => Ok(()),
            Some(err) => Err(err.to_io()),
        }
    }

    pub fn get_ref(&self) -> &B {
        &self.r
    }

    pub fn get_mut(&mut self) -> &mut B {
        &mut self.r
    }

    pub fn into_inner(self) -> B {
        self.r
    }

    fn read_string(&mut self) -> Result<String, Fail> {
        let mut need_conv = false;
        let mut i = 0;
        loop {
            if i >= self.buf.len() {
                return Err(err_header());
            }
            self.buf[i] = read_byte(&mut self.r)?;
            if self.buf[i] > 0x7f {
                need_conv = true;
            }
            if self.buf[i] == 0 {
                self.digest = crc32::update(self.digest, &self.buf[..i + 1]);

                if need_conv {
                    return Ok(self.buf[..i].iter().map(|&v| char::from(v)).collect());
                }
                return Ok(String::from_utf8_lossy(&self.buf[..i]).into_owned());
            }
            i += 1;
        }
    }

    fn read_header(&mut self) -> (Header, Option<Fail>) {
        let mut hdr = Header::default();
        if let (_, Some(err)) = read_full(&mut self.r, &mut self.buf[..10]) {
            return (hdr, Some(err));
        }
        if self.buf[0] != GZIP_ID1 || self.buf[1] != GZIP_ID2 || self.buf[2] != GZIP_DEFLATE {
            return (hdr, Some(err_header()));
        }
        let flg = self.buf[3];
        let t = i64::from(u32::from_le_bytes([
            self.buf[4],
            self.buf[5],
            self.buf[6],
            self.buf[7],
        ]));
        if t > 0 {
            hdr.mod_time = Time::from_unix(t, 0);
        }
        hdr.os = self.buf[9];
        self.digest = crc32::checksum_ieee(&self.buf[..10]);

        if flg & FLAG_EXTRA != 0 {
            if let (_, Some(err)) = read_full(&mut self.r, &mut self.buf[..2]) {
                return (hdr, Some(err.no_eof()));
            }
            self.digest = crc32::update(self.digest, &self.buf[..2]);
            let mut data = vec![0u8; usize::from(u16::from_le_bytes([self.buf[0], self.buf[1]]))];
            if let (_, Some(err)) = read_full(&mut self.r, &mut data) {
                return (hdr, Some(err.no_eof()));
            }
            self.digest = crc32::update(self.digest, &data);
            hdr.extra = Some(data);
        }

        if flg & FLAG_NAME != 0 {
            match self.read_string() {
                Ok(s) => hdr.name = s,
                Err(err) => return (hdr, Some(err.no_eof())),
            }
        }

        if flg & FLAG_COMMENT != 0 {
            match self.read_string() {
                Ok(s) => hdr.comment = s,
                Err(err) => return (hdr, Some(err.no_eof())),
            }
        }

        if flg & FLAG_HDR_CRC != 0 {
            if let (_, Some(err)) = read_full(&mut self.r, &mut self.buf[..2]) {
                return (hdr, Some(err.no_eof()));
            }
            let digest = u16::from_le_bytes([self.buf[0], self.buf[1]]);
            if digest != self.digest as u16 {
                return (hdr, Some(err_header()));
            }
        }

        self.digest = 0;
        self.decompressor.reset(None);
        (hdr, None)
    }

    fn read_go(&mut self, p: &mut [u8]) -> (usize, Option<Fail>) {
        if let Some(err) = &self.err {
            return (0, Some(err.clone()));
        }

        let mut n = 0;
        while n == 0 {
            let (m, err) = self.decompressor.read(&mut self.r, p);
            n = m;
            self.err = err;
            self.digest = crc32::update(self.digest, &p[..n]);
            self.size = self.size.wrapping_add(n as u32);
            if self.err != Some(Fail::Eof) {
                return (n, self.err.clone());
            }

            if let (_, Some(err)) = read_full(&mut self.r, &mut self.buf[..8]) {
                self.err = Some(err.no_eof());
                return (n, self.err.clone());
            }
            let digest = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
            let size = u32::from_le_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]]);
            if digest != self.digest || size != self.size {
                self.err = Some(err_checksum());
                return (n, self.err.clone());
            }
            self.digest = 0;
            self.size = 0;

            if !self.multistream {
                return (n, Some(Fail::Eof));
            }
            self.err = None;

            let (_, err) = self.read_header();
            if err.is_some() {
                self.err = err;
                return (n, self.err.clone());
            }
        }

        (n, None)
    }
}

impl<B: BufRead> Read for Reader<B> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (n, err) = self.read_go(buf);
        if n > 0 {
            return Ok(n);
        }
        match err {
            None | Some(Fail::Eof) => Ok(0),
            Some(err) => Err(err.to_io()),
        }
    }
}
