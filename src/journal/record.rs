use std::fmt;

use crate::gocompat::binary;
use crate::hash::crc32c;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kind(pub u8);

pub const KIND_DOC_PUT: Kind = Kind(1);
pub const KIND_DOC_DELETE: Kind = Kind(2);
pub const KIND_DOC_MOVE: Kind = Kind(3);
pub const KIND_ENTITY_RESET: Kind = Kind(4);
pub const KIND_ENTITY_CREATE: Kind = Kind(5);
pub const KIND_ENTITY_DROP: Kind = Kind(6);
pub const KIND_KV_PUT: Kind = Kind(7);
pub const KIND_KV_DELETE: Kind = Kind(8);
pub const KIND_KV_EXPIRE: Kind = Kind(9);
pub const KIND_KV_MOVE: Kind = Kind(10);
pub const KIND_KV_RESET: Kind = Kind(11);
pub const KIND_CLEAR_ALL: Kind = Kind(12);

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            KIND_DOC_PUT => "doc_put",
            KIND_DOC_DELETE => "doc_delete",
            KIND_DOC_MOVE => "doc_move",
            KIND_ENTITY_RESET => "entity_reset",
            KIND_ENTITY_CREATE => "entity_create",
            KIND_ENTITY_DROP => "entity_drop",
            KIND_KV_PUT => "kv_put",
            KIND_KV_DELETE => "kv_delete",
            KIND_KV_EXPIRE => "kv_expire",
            KIND_KV_MOVE => "kv_move",
            KIND_KV_RESET => "kv_reset",
            KIND_CLEAR_ALL => "clear_all",
            Kind(n) => return write!(f, "kind_{n}"),
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Location {
    pub segment: u32,
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Position {
    pub segment: u32,
    pub offset: u32,
}

impl Position {
    pub fn less(&self, o: &Position) -> bool {
        if self.segment != o.segment {
            return self.segment < o.segment;
        }

        self.offset < o.offset
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub kind: Kind,
    pub lsn: u64,
    pub r#gen: u64,
    pub expires_at: i64,
    pub entity: Vec<u8>,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub loc: Location,
}

pub(crate) const SEGMENT_MAGIC: &[u8; 8] = b"ELYSLOG1";
pub(crate) const SEGMENT_HEADER_SIZE: usize = 16;
pub(crate) const SEGMENT_VERSION: u32 = 1;
pub(crate) const BATCH_MAGIC: u32 = 0x3142_4c45;
pub(crate) const BATCH_HEADER_SIZE: usize = 16;
pub(crate) const MAX_RECORDS_IN_BATCH: usize = 1 << 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Closed,
    Corrupt,
    TornBatch,
    BadSegment,
    SegmentMismatch { header: u32, file: u32 },
    TooLarge(usize),
    ShortWrite { segment: u32, n: usize, len: usize },
    ShortRead(u32),
    StillActive(u32),
    UnwrittenAtClose,
}

pub const ERR_CLOSED: Error = Error::Closed;
pub const ERR_CORRUPT: Error = Error::Corrupt;
pub const ERR_TORN_BATCH: Error = Error::TornBatch;
pub const ERR_BAD_SEGMENT: Error = Error::BadSegment;

static BAD_SEGMENT: Error = Error::BadSegment;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Closed => f.write_str("journal: closed"),
            Error::Corrupt => f.write_str("journal: corrupt batch"),
            Error::TornBatch => f.write_str("journal: torn batch"),
            Error::BadSegment => f.write_str("journal: not a segment file"),
            Error::SegmentMismatch { header, file } => write!(
                f,
                "journal: not a segment file: header names segment {header}, file is {file}"
            ),
            Error::TooLarge(n) => write!(f, "journal: batch of {n} bytes is too large"),
            Error::ShortWrite { segment, n, len } => write!(
                f,
                "journal: short write on segment {segment} ({n} of {len} bytes)"
            ),
            Error::ShortRead(segment) => write!(f, "journal: short read on segment {segment}"),
            Error::StillActive(segment) => {
                write!(f, "journal: segment {segment} is still active")
            }
            Error::UnwrittenAtClose => f.write_str("journal: unwritten data at close"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::SegmentMismatch { .. } => Some(&BAD_SEGMENT),
            _ => None,
        }
    }
}

pub(crate) fn batch_size(recs: &[Record]) -> usize {
    let mut size = BATCH_HEADER_SIZE;
    for r in recs {
        size += record_size(r);
    }

    size
}

pub(crate) fn append_batch(
    dst: &mut Vec<u8>,
    recs: &mut [Record],
    first_lsn: u64,
    base: u32,
    segment: u32,
) {
    let start = dst.len();
    dst.resize(start + BATCH_HEADER_SIZE, 0);
    for (i, r) in recs.iter_mut().enumerate() {
        r.lsn = first_lsn.wrapping_add(i as u64);
        dst.push(r.kind.0);
        dst.extend_from_slice(&r.lsn.to_le_bytes());
        binary::append_uvarint(dst, r.r#gen);
        binary::append_varint(dst, r.expires_at);
        binary::append_uvarint(dst, r.entity.len() as u64);
        dst.extend_from_slice(&r.entity);
        binary::append_uvarint(dst, r.key.len() as u64);
        dst.extend_from_slice(&r.key);
        binary::append_uvarint(dst, r.value.len() as u64);
        r.loc = Location {
            segment,
            offset: base.wrapping_add((dst.len() - start) as u32),
            length: r.value.len() as u32,
        };
        dst.extend_from_slice(&r.value);
    }

    let payload_len = dst.len() - start - BATCH_HEADER_SIZE;
    let crc = crc32c::checksum(&dst[start + BATCH_HEADER_SIZE..]);
    dst[start..start + 4].copy_from_slice(&BATCH_MAGIC.to_le_bytes());
    dst[start + 4..start + 8].copy_from_slice(&(payload_len as u32).to_le_bytes());
    dst[start + 8..start + 12].copy_from_slice(&crc.to_le_bytes());
    dst[start + 12..start + 16].copy_from_slice(&(recs.len() as u32).to_le_bytes());
}

pub(crate) fn record_size(r: &Record) -> usize {
    1 + 8
        + 3 * binary::MAX_VARINT_LEN64
        + r.entity.len()
        + r.key.len()
        + r.value.len()
        + binary::MAX_VARINT_LEN64
}

pub(crate) fn segment_header(id: u32) -> [u8; SEGMENT_HEADER_SIZE] {
    let mut h = [0u8; SEGMENT_HEADER_SIZE];
    h[..8].copy_from_slice(SEGMENT_MAGIC);
    h[8..12].copy_from_slice(&id.to_le_bytes());
    h[12..16].copy_from_slice(&SEGMENT_VERSION.to_le_bytes());

    h
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub(crate) fn check_segment_header(h: &[u8], id: u32) -> Result<(), Error> {
    if h.len() < SEGMENT_HEADER_SIZE || &h[..8] != SEGMENT_MAGIC {
        return Err(Error::BadSegment);
    }

    let got = le32(&h[8..]);
    if got != id {
        return Err(Error::SegmentMismatch {
            header: got,
            file: id,
        });
    }

    Ok(())
}

pub(crate) fn decode_batch(
    data: &[u8],
    segment: u32,
    base: u32,
) -> Result<(Vec<Record>, usize), Error> {
    if data.len() < BATCH_HEADER_SIZE {
        return Err(Error::TornBatch);
    }

    if le32(&data[0..]) != BATCH_MAGIC {
        return Err(Error::Corrupt);
    }

    let payload_len = le32(&data[4..]) as usize;
    if BATCH_HEADER_SIZE + payload_len > data.len() {
        return Err(Error::TornBatch);
    }

    let payload = &data[BATCH_HEADER_SIZE..BATCH_HEADER_SIZE + payload_len];
    if crc32c::checksum(payload) != le32(&data[8..]) {
        return Err(Error::Corrupt);
    }

    let count = le32(&data[12..]) as usize;
    if count > MAX_RECORDS_IN_BATCH {
        return Err(Error::Corrupt);
    }

    let mut recs = Vec::with_capacity(count.min(payload_len / 14 + 1));
    let mut pos = 0;
    for _ in 0..count {
        let (mut r, n) = decode_record(&payload[pos..])?;

        let value_start = base.wrapping_add((BATCH_HEADER_SIZE + pos + n - r.value.len()) as u32);
        r.loc = Location {
            segment,
            offset: value_start,
            length: r.value.len() as u32,
        };
        recs.push(r);
        pos += n;
    }

    if pos != payload.len() {
        return Err(Error::Corrupt);
    }

    Ok((recs, BATCH_HEADER_SIZE + payload_len))
}

fn decode_record(b: &[u8]) -> Result<(Record, usize), Error> {
    let mut r = Record::default();
    if b.len() < 9 {
        return Err(Error::Corrupt);
    }

    r.kind = Kind(b[0]);
    r.lsn = u64::from_le_bytes([b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8]]);
    let mut pos = 9;

    let (generation, n) = binary::uvarint(&b[pos..]);
    if n <= 0 {
        return Err(Error::Corrupt);
    }
    r.r#gen = generation;
    pos += n as usize;

    let (exp, n) = binary::varint(&b[pos..]);
    if n <= 0 {
        return Err(Error::Corrupt);
    }
    r.expires_at = exp;
    pos += n as usize;

    let (entity, n) = read_bytes(&b[pos..]).ok_or(Error::Corrupt)?;
    r.entity = entity.to_vec();
    pos += n;

    let (key, n) = read_bytes(&b[pos..]).ok_or(Error::Corrupt)?;
    r.key = key.to_vec();
    pos += n;

    let (value, n) = read_bytes(&b[pos..]).ok_or(Error::Corrupt)?;
    r.value = value.to_vec();
    pos += n;

    Ok((r, pos))
}

fn read_bytes(b: &[u8]) -> Option<(&[u8], usize)> {
    let (l, n) = binary::uvarint(b);
    if n <= 0 || ((b.len() - n as usize) as u64) < l {
        return None;
    }

    let n = n as usize;
    let l = l as usize;
    Some((&b[n..n + l], n + l))
}
