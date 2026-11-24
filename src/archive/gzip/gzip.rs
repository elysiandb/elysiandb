use super::gunzip::{
    FLAG_COMMENT, FLAG_EXTRA, FLAG_NAME, GZIP_DEFLATE, GZIP_ID1, GZIP_ID2, Header,
};
use crate::archive::Fail;
use crate::archive::flate::{self, BEST_SPEED, write_once};
use crate::hash::crc32;
use gocompat::time::Time;
use std::io::{self, Write};

pub struct Writer<W: Write> {
    pub header: Header,
    level: i32,
    wrote_header: bool,
    closed: bool,
    buf: [u8; 10],
    compressor: flate::Writer<W>,
    digest: u32,
    size: u32,
    err: Option<Fail>,
}

fn default_header() -> Header {
    Header {
        os: 255,
        ..Header::default()
    }
}

impl<W: Write> Writer<W> {
    pub fn new_level(w: W, level: i32) -> io::Result<Writer<W>> {
        let compressor = flate::Writer::new(w, level)?;
        Ok(Writer {
            header: default_header(),
            level,
            wrote_header: false,
            closed: false,
            buf: [0; 10],
            compressor,
            digest: 0,
            size: 0,
            err: None,
        })
    }

    pub fn reset(&mut self, w: W) {
        self.compressor.reset(w);
        self.header = default_header();
        self.wrote_header = false;
        self.closed = false;
        self.buf = [0; 10];
        self.digest = 0;
        self.size = 0;
        self.err = None;
    }

    pub fn get_ref(&self) -> &W {
        self.compressor.get_ref()
    }

    pub fn get_mut(&mut self) -> &mut W {
        self.compressor.get_mut()
    }

    fn raw_write(&mut self, b: &[u8]) -> Result<(), Fail> {
        write_once(self.compressor.get_mut(), b).map_err(|e| Fail::from_io(&e))
    }

    fn write_bytes(&mut self, b: &[u8]) -> Result<(), Fail> {
        if b.len() > 0xffff {
            return Err(Fail::Error(
                io::ErrorKind::InvalidInput,
                "the gzip extra field is longer than 65535 bytes".to_string(),
            ));
        }
        let len = (b.len() as u16).to_le_bytes();
        self.raw_write(&len)?;
        self.raw_write(b)
    }

    fn write_string(&mut self, s: &str) -> Result<(), Fail> {
        let mut need_conv = false;
        for v in s.chars() {
            if v == '\0' || u32::from(v) > 0xff {
                return Err(Fail::Error(
                    io::ErrorKind::InvalidInput,
                    "a gzip header text must be Latin-1 without NUL".to_string(),
                ));
            }
            if u32::from(v) > 0x7f {
                need_conv = true;
            }
        }
        if need_conv {
            let b: Vec<u8> = s.chars().map(|v| u32::from(v) as u8).collect();
            self.raw_write(&b)?;
        } else {
            self.raw_write(s.as_bytes())?;
        }
        self.raw_write(&[0])
    }

    fn write_header(&mut self) -> Result<(), Fail> {
        self.buf = [GZIP_ID1, GZIP_ID2, GZIP_DEFLATE, 0, 0, 0, 0, 0, 0, 0];
        if self.header.extra.is_some() {
            self.buf[3] |= FLAG_EXTRA;
        }
        if !self.header.name.is_empty() {
            self.buf[3] |= FLAG_NAME;
        }
        if !self.header.comment.is_empty() {
            self.buf[3] |= FLAG_COMMENT;
        }
        if self.header.mod_time.after(&Time::from_unix(0, 0)) {
            let t = (self.header.mod_time.unix() as u32).to_le_bytes();
            self.buf[4..8].copy_from_slice(&t);
        }
        if self.level == BEST_SPEED {
            self.buf[8] = 4;
        }
        self.buf[9] = self.header.os;
        let buf = self.buf;
        self.raw_write(&buf)?;
        if let Some(extra) = self.header.extra.clone() {
            self.write_bytes(&extra)?;
        }
        if !self.header.name.is_empty() {
            let name = self.header.name.clone();
            self.write_string(&name)?;
        }
        if !self.header.comment.is_empty() {
            let comment = self.header.comment.clone();
            self.write_string(&comment)?;
        }
        Ok(())
    }

    fn write_go(&mut self, p: &[u8]) -> io::Result<usize> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        if !self.wrote_header {
            self.wrote_header = true;
            if let Err(err) = self.write_header() {
                let e = err.to_io();
                self.err = Some(err);
                return Err(e);
            }
        }
        self.size = self.size.wrapping_add(p.len() as u32);
        self.digest = crc32::update(self.digest, p);
        match self.compressor.write(p) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.err = Some(Fail::from_io(&e));
                Err(e)
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        if self.closed {
            return Ok(());
        }
        if !self.wrote_header {
            let _ = self.write_go(&[]);
            if let Some(err) = &self.err {
                return Err(err.to_io());
            }
        }
        if let Err(e) = self.compressor.flush() {
            self.err = Some(Fail::from_io(&e));
            return Err(e);
        }
        Ok(())
    }

    pub fn close(&mut self) -> io::Result<()> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        if !self.wrote_header {
            let _ = self.write_go(&[]);
            if let Some(err) = &self.err {
                return Err(err.to_io());
            }
        }
        if let Err(e) = self.compressor.close() {
            self.err = Some(Fail::from_io(&e));
            return Err(e);
        }
        self.buf[..4].copy_from_slice(&self.digest.to_le_bytes());
        self.buf[4..8].copy_from_slice(&self.size.to_le_bytes());
        let trailer = self.buf;
        if let Err(err) = self.raw_write(&trailer[..8]) {
            let e = err.to_io();
            self.err = Some(err);
            return Err(e);
        }
        Ok(())
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_go(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Writer::flush(self)
    }
}
