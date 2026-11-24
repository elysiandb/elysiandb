use std::fmt;
use std::path::Path;

use super::fs::{Fs, FsError, read_file, write_file_atomic};
use crate::gocompat::binary;
use crate::hash::crc32c;

const KV_FILE_MAGIC: &[u8; 8] = b"ELYKVCK1";

#[derive(Clone, Debug)]
pub enum Error {
    Fs(FsError),
    CorruptFile,
}

pub const ERR_CORRUPT_FILE: Error = Error::CorruptFile;

impl Error {
    pub fn is_corrupt_file(&self) -> bool {
        matches!(self, Error::CorruptFile)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Fs(e) => fmt::Display::fmt(e, f),
            Error::CorruptFile => f.write_str("durable: file is corrupt"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Fs(e) => Some(e),
            Error::CorruptFile => None,
        }
    }
}

impl From<FsError> for Error {
    fn from(e: FsError) -> Error {
        Error::Fs(e)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvRecord {
    pub key: Vec<u8>,
    pub lsn: u64,
    pub expires_at: i64,
    pub segment: u32,
    pub offset: u32,
    pub length: u32,
}

pub fn write_kv_file(f: &dyn Fs, path: &Path, records: &[KvRecord]) -> Result<(), FsError> {
    write_file_atomic(f, path, |w| {
        let mut crc = crc32c::new();
        let mut buf: Vec<u8> = Vec::with_capacity(64);

        w.write(KV_FILE_MAGIC)?;
        crc.write(KV_FILE_MAGIC);

        binary::append_uvarint(&mut buf, records.len() as u64);
        w.write(&buf)?;
        crc.write(&buf);

        for r in records {
            buf.clear();
            binary::append_uvarint(&mut buf, r.key.len() as u64);
            buf.extend_from_slice(&r.key);
            binary::append_uvarint(&mut buf, r.lsn);
            binary::append_varint(&mut buf, r.expires_at);
            binary::append_uvarint(&mut buf, u64::from(r.segment));
            binary::append_uvarint(&mut buf, u64::from(r.offset));
            binary::append_uvarint(&mut buf, u64::from(r.length));
            w.write(&buf)?;
            crc.write(&buf);
        }

        w.write(&crc.sum32().to_le_bytes())
    })
}

struct Reader<'a> {
    body: &'a [u8],
    pos: usize,
    bad: bool,
}

impl Reader<'_> {
    fn uv(&mut self) -> u64 {
        let (v, n) = binary::uvarint(&self.body[self.pos..]);
        if n <= 0 {
            self.bad = true;
            return 0;
        }
        self.pos += n as usize;
        v
    }

    fn sv(&mut self) -> i64 {
        let (v, n) = binary::varint(&self.body[self.pos..]);
        if n <= 0 {
            self.bad = true;
            return 0;
        }
        self.pos += n as usize;
        v
    }
}

pub fn read_kv_file(f: &dyn Fs, path: &Path) -> Result<Vec<KvRecord>, Error> {
    let data = read_file(f, path)?;

    if data.len() < KV_FILE_MAGIC.len() + 4 || &data[..KV_FILE_MAGIC.len()] != KV_FILE_MAGIC {
        return Err(Error::CorruptFile);
    }

    let body = &data[..data.len() - 4];
    let tail = &data[data.len() - 4..];
    if crc32c::checksum(body) != u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]) {
        return Err(Error::CorruptFile);
    }

    let mut r = Reader {
        body,
        pos: KV_FILE_MAGIC.len(),
        bad: false,
    };

    let mut out = Vec::new();
    let mut n = r.uv();
    while n > 0 && !r.bad {
        let l = r.uv();
        if r.bad || ((body.len() - r.pos) as u64) < l {
            return Err(Error::CorruptFile);
        }

        let l = l as usize;
        let mut rec = KvRecord {
            key: body[r.pos..r.pos + l].to_vec(),
            ..KvRecord::default()
        };
        r.pos += l;
        rec.lsn = r.uv();
        rec.expires_at = r.sv();
        rec.segment = r.uv() as u32;
        rec.offset = r.uv() as u32;
        rec.length = r.uv() as u32;
        out.push(rec);
        n -= 1;
    }

    if r.bad || r.pos != body.len() {
        return Err(Error::CorruptFile);
    }

    Ok(out)
}
