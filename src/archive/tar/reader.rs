use super::common::{
    Header, PAX_ATIME, PAX_CTIME, PAX_GID, PAX_GNAME, PAX_GNU_SPARSE_MAJOR, PAX_GNU_SPARSE_MAP,
    PAX_GNU_SPARSE_MINOR, PAX_GNU_SPARSE_NAME, PAX_GNU_SPARSE_NUM_BLOCKS, PAX_GNU_SPARSE_NUM_BYTES,
    PAX_GNU_SPARSE_OFFSET, PAX_GNU_SPARSE_REAL_SIZE, PAX_GNU_SPARSE_SIZE, PAX_LINKPATH, PAX_MTIME,
    PAX_PATH, PAX_SCHILY_XATTR, PAX_SIZE, PAX_UID, PAX_UNAME, Records, SparseEntry, TYPE_DIR,
    TYPE_GNU_LONG_LINK, TYPE_GNU_LONG_NAME, TYPE_GNU_SPARSE, TYPE_REG, TYPE_REG_A,
    TYPE_X_GLOBAL_HEADER, TYPE_X_HEADER, invert_sparse_entries, is_header_only_type, records_get,
    validate_sparse_entries,
};
use super::format::{
    BLOCK_SIZE, Block, FORMAT_GNU, FORMAT_PAX, FORMAT_STAR, FORMAT_UNKNOWN, FORMAT_USTAR,
    FORMAT_V7, MAX_SPECIAL_FILE_SIZE, ZERO_BLOCK, block_padding, get_format, gnu,
    sparse_is_extended, sparse_length, sparse_max_entries, sparse_offset, star, ustar, v7,
};
use super::strconv::{Parser, is_ascii, parse_pax_record, parse_pax_time, parse_string};
use super::{ERR_FIELD_TOO_LONG, ERR_HEADER, ERR_MISS_DATA, ERR_UNREF_DATA};
use crate::archive::Fail;
use gocompat::strconv::parse_int;
use gocompat::time::Time;
use std::io::{self, Read};

fn err_header() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, ERR_HEADER.to_string())
}

fn err_field_too_long() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, ERR_FIELD_TOO_LONG.to_string())
}

fn err_miss_data() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, ERR_MISS_DATA.to_string())
}

fn err_unref_data() -> Fail {
    Fail::Error(io::ErrorKind::InvalidData, ERR_UNREF_DATA.to_string())
}

struct RegFile {
    nb: i64,
}

struct SparseFile {
    reg: RegFile,
    sp: Vec<SparseEntry>,
    pos: i64,
}

enum FileReader {
    Reg(RegFile),
    Sparse(SparseFile),
}

fn raw_read<R: Read + ?Sized>(r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
    loop {
        match r.read(b) {
            Ok(0) if !b.is_empty() => return (0, Some(Fail::Eof)),
            Ok(n) => return (n, None),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return (0, Some(Fail::from_io(&e))),
        }
    }
}

impl RegFile {
    fn read<R: Read + ?Sized>(&mut self, r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
        let len = (b.len() as i64).min(self.nb).max(0) as usize;
        let b = &mut b[..len];
        let (mut n, mut err) = (0, None);
        if !b.is_empty() {
            (n, err) = raw_read(r, b);
            self.nb -= n as i64;
        }
        match err {
            Some(Fail::Eof) if self.nb > 0 => (n, Some(Fail::unexpected_eof())),
            None if self.nb == 0 => (n, Some(Fail::Eof)),
            other => (n, other),
        }
    }

    fn physical_remaining(&self) -> i64 {
        self.nb
    }
}

fn try_read_full_with<F>(b: &mut [u8], mut read: F) -> (usize, Option<Fail>)
where
    F: FnMut(&mut [u8]) -> (usize, Option<Fail>),
{
    let mut n = 0;
    let mut err = None;
    while b.len() > n && err.is_none() {
        let (nn, e) = read(&mut b[n..]);
        n += nn;
        err = e;
    }
    if b.len() == n && err == Some(Fail::Eof) {
        err = None;
    }
    (n, err)
}

impl SparseFile {
    fn logical_remaining(&self) -> i64 {
        self.sp[self.sp.len() - 1].end_offset() - self.pos
    }

    fn physical_remaining(&self) -> i64 {
        self.reg.physical_remaining()
    }

    fn read<R: Read + ?Sized>(&mut self, r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
        let finished = b.len() as i64 >= self.logical_remaining();
        let total = if finished {
            self.logical_remaining().max(0) as usize
        } else {
            b.len()
        };

        let mut done = 0usize;
        let end_pos = self.pos + total as i64;
        let mut err: Option<Fail> = None;
        while end_pos > self.pos && err.is_none() {
            let nf;
            let (hole_start, hole_end) = (self.sp[0].offset, self.sp[0].end_offset());
            let rest = &mut b[done..total];
            if self.pos < hole_start {
                let len = (rest.len() as i64).min(hole_start - self.pos) as usize;
                let reg = &mut self.reg;
                let (n, e) = try_read_full_with(&mut rest[..len], |buf| reg.read(r, buf));
                nf = n;
                err = e;
            } else {
                let len = (rest.len() as i64).min(hole_end - self.pos) as usize;
                rest[..len].fill(0);
                nf = len;
            }
            done += nf;
            self.pos += nf as i64;
            if self.pos >= hole_end && self.sp.len() > 1 {
                self.sp.remove(0);
            }
        }

        let n = done;
        match err {
            Some(Fail::Eof) => (n, Some(err_miss_data())),
            Some(e) => (n, Some(e)),
            None if self.logical_remaining() == 0 && self.physical_remaining() > 0 => {
                (n, Some(err_unref_data()))
            }
            None if finished => (n, Some(Fail::Eof)),
            None => (n, None),
        }
    }
}

impl FileReader {
    fn read<R: Read + ?Sized>(&mut self, r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
        match self {
            FileReader::Reg(f) => f.read(r, b),
            FileReader::Sparse(f) => f.read(r, b),
        }
    }

    fn physical_remaining(&self) -> i64 {
        match self {
            FileReader::Reg(f) => f.physical_remaining(),
            FileReader::Sparse(f) => f.physical_remaining(),
        }
    }
}

pub struct Reader<R: Read> {
    r: R,
    pad: i64,
    curr: FileReader,
    blk: Block,
    err: Option<Fail>,
}

fn try_read_full<R: Read + ?Sized>(r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
    try_read_full_with(b, |buf| raw_read(r, buf))
}

fn must_read_full<R: Read + ?Sized>(r: &mut R, b: &mut [u8]) -> (usize, Option<Fail>) {
    let (n, err) = try_read_full(r, b);
    (n, err.map(Fail::no_eof))
}

fn read_full<R: Read + ?Sized>(r: &mut R, b: &mut [u8]) -> Option<Fail> {
    let (n, err) = try_read_full(r, b);
    match err {
        Some(Fail::Eof) if n > 0 => Some(Fail::unexpected_eof()),
        other => other,
    }
}

fn discard<R: Read + ?Sized>(r: &mut R, n: i64) -> Option<Fail> {
    let mut buf = [0u8; 8192];
    let mut left = n;
    while left > 0 {
        let len = (buf.len() as i64).min(left) as usize;
        match raw_read(r, &mut buf[..len]) {
            (m, None) => left -= m as i64,
            (_, Some(Fail::Eof)) => return Some(Fail::unexpected_eof()),
            (_, Some(err)) => return Some(err),
        }
    }
    None
}

pub(super) fn merge_pax(hdr: &mut Header, pax_hdrs: Option<Records>) -> Result<(), Fail> {
    let Some(pax_hdrs) = pax_hdrs else {
        return Ok(());
    };
    for (k, v) in &pax_hdrs {
        if v.is_empty() {
            continue;
        }
        let ok = match k.as_slice() {
            PAX_PATH => {
                hdr.name = v.clone();
                true
            }
            PAX_LINKPATH => {
                hdr.linkname = v.clone();
                true
            }
            PAX_UNAME => {
                hdr.uname = v.clone();
                true
            }
            PAX_GNAME => {
                hdr.gname = v.clone();
                true
            }
            PAX_UID => {
                let r = parse_int(v, 10, 64);
                hdr.uid = r.as_ref().map_or_else(|e| e.value, |x| *x);
                r.is_ok()
            }
            PAX_GID => {
                let r = parse_int(v, 10, 64);
                hdr.gid = r.as_ref().map_or_else(|e| e.value, |x| *x);
                r.is_ok()
            }
            PAX_ATIME => match parse_pax_time(v) {
                Ok(t) => {
                    hdr.access_time = t;
                    true
                }
                Err(()) => false,
            },
            PAX_MTIME => match parse_pax_time(v) {
                Ok(t) => {
                    hdr.mod_time = t;
                    true
                }
                Err(()) => false,
            },
            PAX_CTIME => match parse_pax_time(v) {
                Ok(t) => {
                    hdr.change_time = t;
                    true
                }
                Err(()) => false,
            },
            PAX_SIZE => {
                let r = parse_int(v, 10, 64);
                hdr.size = r.as_ref().map_or_else(|e| e.value, |x| *x);
                r.is_ok()
            }
            _ => {
                if let Some(name) = k.strip_prefix(PAX_SCHILY_XATTR) {
                    hdr.xattrs.insert(name.to_vec(), v.clone());
                }
                true
            }
        };
        if !ok {
            return Err(err_header());
        }
    }
    hdr.pax_records = pax_hdrs;
    Ok(())
}

pub(super) fn parse_pax(buf: &[u8]) -> Result<Records, Fail> {
    let mut sbuf = buf;
    let mut sparse_map: Vec<Vec<u8>> = Vec::new();

    let mut pax_hdrs = Records::new();
    while !sbuf.is_empty() {
        let (key, value, residual) = parse_pax_record(sbuf).map_err(|()| err_header())?;
        sbuf = residual;

        if key == PAX_GNU_SPARSE_OFFSET || key == PAX_GNU_SPARSE_NUM_BYTES {
            if (sparse_map.len().is_multiple_of(2) && key != PAX_GNU_SPARSE_OFFSET)
                || (sparse_map.len() % 2 == 1 && key != PAX_GNU_SPARSE_NUM_BYTES)
                || value.contains(&b',')
            {
                return Err(err_header());
            }
            sparse_map.push(value);
        } else {
            pax_hdrs.insert(key, value);
        }
    }
    if !sparse_map.is_empty() {
        pax_hdrs.insert(PAX_GNU_SPARSE_MAP.to_vec(), sparse_map.join(&b","[..]));
    }
    Ok(pax_hdrs)
}

fn read_gnu_sparse_map_0x1(pax_hdrs: &Records) -> Result<Vec<SparseEntry>, Fail> {
    let num_entries = match parse_int(records_get(pax_hdrs, PAX_GNU_SPARSE_NUM_BLOCKS), 10, 0) {
        Ok(n) if n >= 0 && n.checked_mul(2).is_some() => n,
        _ => return Err(err_header()),
    };

    let joined = records_get(pax_hdrs, PAX_GNU_SPARSE_MAP);
    let mut sparse_map: Vec<&[u8]> = joined.split(|&c| c == b',').collect();
    if sparse_map.len() == 1 && sparse_map[0].is_empty() {
        sparse_map.clear();
    }
    if sparse_map.len() as i64 != 2 * num_entries {
        return Err(err_header());
    }

    let mut spd = Vec::with_capacity(num_entries as usize);
    let mut rest = sparse_map.as_slice();
    while rest.len() >= 2 {
        let offset = parse_int(rest[0], 10, 64);
        let length = parse_int(rest[1], 10, 64);
        match (offset, length) {
            (Ok(offset), Ok(length)) => spd.push(SparseEntry { offset, length }),
            _ => return Err(err_header()),
        }
        rest = &rest[2..];
    }
    Ok(spd)
}

impl<R: Read> Reader<R> {
    pub fn new(r: R) -> Reader<R> {
        Reader {
            r,
            pad: 0,
            curr: FileReader::Reg(RegFile { nb: 0 }),
            blk: ZERO_BLOCK,
            err: None,
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.r
    }

    pub fn get_mut(&mut self) -> &mut R {
        &mut self.r
    }

    pub fn into_inner(self) -> R {
        self.r
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Header>> {
        if let Some(err) = &self.err {
            return match err {
                Fail::Eof => Ok(None),
                err => Err(err.to_io()),
            };
        }
        match self.next_header() {
            Ok(hdr) => Ok(Some(hdr)),
            Err(err) => {
                self.err = Some(err.clone());
                match err {
                    Fail::Eof => Ok(None),
                    err => Err(err.to_io()),
                }
            }
        }
    }

    fn read_go(&mut self, b: &mut [u8]) -> (usize, Option<Fail>) {
        if let Some(err) = &self.err {
            return (0, Some(err.clone()));
        }
        let (n, err) = self.curr.read(&mut self.r, b);
        if let Some(e) = &err
            && *e != Fail::Eof
        {
            self.err = Some(e.clone());
        }
        (n, err)
    }

    fn read_special_file(&mut self) -> Result<Vec<u8>, Fail> {
        let mut buf = Vec::new();
        let limit = MAX_SPECIAL_FILE_SIZE + 1;
        let mut chunk = [0u8; 512];
        let mut result = Ok(());
        while buf.len() < limit {
            let want = chunk.len().min(limit - buf.len());
            let (n, err) = self.read_go(&mut chunk[..want]);
            buf.extend_from_slice(&chunk[..n]);
            match err {
                None => {}
                Some(Fail::Eof) => break,
                Some(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        if buf.len() > MAX_SPECIAL_FILE_SIZE {
            return Err(err_field_too_long());
        }
        result.map(|()| buf)
    }

    fn next_header(&mut self) -> Result<Header, Fail> {
        let mut pax_hdrs: Option<Records> = None;
        let mut gnu_long_name: Vec<u8> = Vec::new();
        let mut gnu_long_link: Vec<u8> = Vec::new();

        let mut format = FORMAT_USTAR | FORMAT_PAX | FORMAT_GNU;
        loop {
            if let Some(err) = discard(&mut self.r, self.curr.physical_remaining()) {
                return Err(err);
            }
            let pad = self.pad as usize;
            let mut padding = [0u8; BLOCK_SIZE];
            if let (_, Some(err)) = try_read_full(&mut self.r, &mut padding[..pad]) {
                return Err(err);
            }
            self.pad = 0;

            let mut hdr = self.read_header()?;
            self.handle_regular_file(&hdr)?;
            format.may_only_be(hdr.format);

            match hdr.typeflag {
                TYPE_X_HEADER | TYPE_X_GLOBAL_HEADER => {
                    format.may_only_be(FORMAT_PAX);
                    let buf = self.read_special_file()?;
                    let records = parse_pax(&buf)?;
                    pax_hdrs = Some(records);
                    if hdr.typeflag == TYPE_X_GLOBAL_HEADER {
                        let _ = merge_pax(&mut hdr, pax_hdrs);
                        return Ok(Header {
                            name: hdr.name,
                            typeflag: hdr.typeflag,
                            xattrs: hdr.xattrs,
                            pax_records: hdr.pax_records,
                            format,
                            ..Header::default()
                        });
                    }
                    continue;
                }
                TYPE_GNU_LONG_NAME | TYPE_GNU_LONG_LINK => {
                    format.may_only_be(FORMAT_GNU);
                    let realname = self.read_special_file()?;

                    if hdr.typeflag == TYPE_GNU_LONG_NAME {
                        gnu_long_name = parse_string(&realname);
                    } else {
                        gnu_long_link = parse_string(&realname);
                    }
                    continue;
                }
                _ => {
                    merge_pax(&mut hdr, pax_hdrs)?;
                    if !gnu_long_name.is_empty() {
                        hdr.name = gnu_long_name;
                    }
                    if !gnu_long_link.is_empty() {
                        hdr.linkname = gnu_long_link;
                    }
                    if hdr.typeflag == TYPE_REG_A {
                        if hdr.name.ends_with(b"/") {
                            hdr.typeflag = TYPE_DIR;
                        } else {
                            hdr.typeflag = TYPE_REG;
                        }
                    }

                    self.handle_regular_file(&hdr)?;

                    self.handle_sparse_file(&mut hdr)?;

                    if format.has(FORMAT_USTAR) && format.has(FORMAT_PAX) {
                        format.may_only_be(FORMAT_USTAR);
                    }
                    hdr.format = format;
                    return Ok(hdr);
                }
            }
        }
    }

    fn handle_regular_file(&mut self, hdr: &Header) -> Result<(), Fail> {
        let mut nb = hdr.size;
        if is_header_only_type(hdr.typeflag) {
            nb = 0;
        }
        if nb < 0 {
            return Err(err_header());
        }

        self.pad = block_padding(nb);
        self.curr = FileReader::Reg(RegFile { nb });
        Ok(())
    }

    fn handle_sparse_file(&mut self, hdr: &mut Header) -> Result<(), Fail> {
        let spd = if hdr.typeflag == TYPE_GNU_SPARSE {
            self.read_old_gnu_sparse_map(hdr)?
        } else {
            self.read_gnu_sparse_pax_headers(hdr)?
        };

        if let Some(spd) = spd {
            if is_header_only_type(hdr.typeflag) || !validate_sparse_entries(&spd, hdr.size) {
                return Err(err_header());
            }
            let sph = invert_sparse_entries(&spd, hdr.size);
            let reg = match std::mem::replace(&mut self.curr, FileReader::Reg(RegFile { nb: 0 })) {
                FileReader::Reg(reg) => reg,
                FileReader::Sparse(sparse) => sparse.reg,
            };
            self.curr = FileReader::Sparse(SparseFile {
                reg,
                sp: sph,
                pos: 0,
            });
        }
        Ok(())
    }

    fn read_gnu_sparse_pax_headers(
        &mut self,
        hdr: &mut Header,
    ) -> Result<Option<Vec<SparseEntry>>, Fail> {
        let is_1x0;
        let major = records_get(&hdr.pax_records, PAX_GNU_SPARSE_MAJOR).to_vec();
        let minor = records_get(&hdr.pax_records, PAX_GNU_SPARSE_MINOR).to_vec();
        if major == b"0" && (minor == b"0" || minor == b"1") {
            is_1x0 = false;
        } else if major == b"1" && minor == b"0" {
            is_1x0 = true;
        } else if !major.is_empty() || !minor.is_empty() {
            return Ok(None);
        } else if !records_get(&hdr.pax_records, PAX_GNU_SPARSE_MAP).is_empty() {
            is_1x0 = false;
        } else {
            return Ok(None);
        }
        hdr.format.may_only_be(FORMAT_PAX);

        let name = records_get(&hdr.pax_records, PAX_GNU_SPARSE_NAME);
        if !name.is_empty() {
            hdr.name = name.to_vec();
        }
        let mut size = records_get(&hdr.pax_records, PAX_GNU_SPARSE_SIZE);
        if size.is_empty() {
            size = records_get(&hdr.pax_records, PAX_GNU_SPARSE_REAL_SIZE);
        }
        if !size.is_empty() {
            hdr.size = parse_int(size, 10, 64).map_err(|_| err_header())?;
        }

        if is_1x0 {
            return self.read_gnu_sparse_map_1x0().map(Some);
        }
        read_gnu_sparse_map_0x1(&hdr.pax_records).map(Some)
    }

    fn read_gnu_sparse_map_1x0(&mut self) -> Result<Vec<SparseEntry>, Fail> {
        let mut cnt_newline: i64 = 0;
        let mut buf: Vec<u8> = Vec::new();
        let mut pos = 0usize;
        let mut blk = ZERO_BLOCK;

        let mut feed_tokens = |this: &mut Self, buf: &mut Vec<u8>, cnt: &mut i64, n: i64| {
            while *cnt < n {
                let (_, err) = try_read_full_with(&mut blk[..], |b| this.curr.read(&mut this.r, b));
                if let Some(err) = err {
                    return Err(err.no_eof());
                }
                buf.extend_from_slice(&blk);
                for &c in blk.iter() {
                    if c == b'\n' {
                        *cnt += 1;
                    }
                }
            }
            Ok(())
        };

        let next_token = |buf: &[u8], pos: &mut usize, cnt: &mut i64| -> Vec<u8> {
            *cnt -= 1;
            let start = *pos;
            match buf[start..].iter().position(|&c| c == b'\n') {
                Some(i) => {
                    *pos = start + i + 1;
                    buf[start..start + i].to_vec()
                }
                None => {
                    *pos = buf.len();
                    buf[start..].to_vec()
                }
            }
        };

        feed_tokens(self, &mut buf, &mut cnt_newline, 1)?;
        let num_entries = match parse_int(&next_token(&buf, &mut pos, &mut cnt_newline), 10, 0) {
            Ok(n) if n >= 0 && n.checked_mul(2).is_some() => n,
            _ => return Err(err_header()),
        };

        feed_tokens(self, &mut buf, &mut cnt_newline, 2 * num_entries)?;
        let mut spd = Vec::with_capacity(num_entries.min(1 << 16) as usize);
        for _ in 0..num_entries {
            let offset = parse_int(&next_token(&buf, &mut pos, &mut cnt_newline), 10, 64);
            let length = parse_int(&next_token(&buf, &mut pos, &mut cnt_newline), 10, 64);
            match (offset, length) {
                (Ok(offset), Ok(length)) => spd.push(SparseEntry { offset, length }),
                _ => return Err(err_header()),
            }
        }
        Ok(spd)
    }

    fn read_old_gnu_sparse_map(
        &mut self,
        hdr: &mut Header,
    ) -> Result<Option<Vec<SparseEntry>>, Fail> {
        if get_format(&self.blk) != FORMAT_GNU {
            return Err(err_header());
        }
        hdr.format.may_only_be(FORMAT_GNU);

        let mut p = Parser::default();
        hdr.size = p.parse_numeric(&self.blk[gnu::REAL_SIZE]);
        if p.err {
            return Err(err_header());
        }
        let mut blk = self.blk;
        let mut range = gnu::SPARSE;
        let mut spd = Vec::new();
        loop {
            let s = &blk[range.clone()];
            for i in 0..sparse_max_entries(s) {
                if sparse_offset(s, i)[0] == 0x00 {
                    break;
                }
                let offset = p.parse_numeric(sparse_offset(s, i));
                let length = p.parse_numeric(sparse_length(s, i));
                if p.err {
                    return Err(err_header());
                }
                spd.push(SparseEntry { offset, length });
            }

            if sparse_is_extended(s) > 0 {
                if let (_, Some(err)) = must_read_full(&mut self.r, &mut blk) {
                    return Err(err);
                }
                range = 0..BLOCK_SIZE;
                continue;
            }
            return Ok(Some(spd));
        }
    }

    fn read_header(&mut self) -> Result<Header, Fail> {
        if let Some(err) = read_full(&mut self.r, &mut self.blk) {
            return Err(err);
        }
        if self.blk == ZERO_BLOCK {
            if let Some(err) = read_full(&mut self.r, &mut self.blk) {
                return Err(err);
            }
            if self.blk == ZERO_BLOCK {
                return Err(Fail::Eof);
            }
            return Err(err_header());
        }

        let format = get_format(&self.blk);
        if format == FORMAT_UNKNOWN {
            return Err(err_header());
        }

        let mut p = Parser::default();
        let mut hdr = Header::default();

        let blk = &self.blk;
        hdr.typeflag = blk[v7::TYPE_FLAG];
        hdr.name = parse_string(&blk[v7::NAME]);
        hdr.linkname = parse_string(&blk[v7::LINK_NAME]);
        hdr.size = p.parse_numeric(&blk[v7::SIZE]);
        hdr.mode = p.parse_numeric(&blk[v7::MODE]);
        hdr.uid = p.parse_numeric(&blk[v7::UID]);
        hdr.gid = p.parse_numeric(&blk[v7::GID]);
        hdr.mod_time = Time::from_unix(p.parse_numeric(&blk[v7::MOD_TIME]), 0);

        if format > FORMAT_V7 {
            hdr.uname = parse_string(&blk[ustar::USER_NAME]);
            hdr.gname = parse_string(&blk[ustar::GROUP_NAME]);
            hdr.devmajor = p.parse_numeric(&blk[ustar::DEV_MAJOR]);
            hdr.devminor = p.parse_numeric(&blk[ustar::DEV_MINOR]);

            let mut prefix: Vec<u8> = Vec::new();
            if format.has(FORMAT_USTAR | FORMAT_PAX) {
                hdr.format = format;
                prefix = parse_string(&blk[ustar::PREFIX]);

                if blk.iter().any(|&c| c >= 0x80) {
                    hdr.format = FORMAT_UNKNOWN;
                }
                let nul = |b: &[u8]| b[b.len() - 1] == 0;
                if !(nul(&blk[v7::SIZE])
                    && nul(&blk[v7::MODE])
                    && nul(&blk[v7::UID])
                    && nul(&blk[v7::GID])
                    && nul(&blk[v7::MOD_TIME])
                    && nul(&blk[ustar::DEV_MAJOR])
                    && nul(&blk[ustar::DEV_MINOR]))
                {
                    hdr.format = FORMAT_UNKNOWN;
                }
            } else if format.has(FORMAT_STAR) {
                prefix = parse_string(&blk[star::PREFIX]);
                hdr.access_time = Time::from_unix(p.parse_numeric(&blk[star::ACCESS_TIME]), 0);
                hdr.change_time = Time::from_unix(p.parse_numeric(&blk[star::CHANGE_TIME]), 0);
            } else if format.has(FORMAT_GNU) {
                hdr.format = format;
                let mut p2 = Parser::default();
                let b = &blk[gnu::ACCESS_TIME];
                if b[0] != 0 {
                    hdr.access_time = Time::from_unix(p2.parse_numeric(b), 0);
                }
                let b = &blk[gnu::CHANGE_TIME];
                if b[0] != 0 {
                    hdr.change_time = Time::from_unix(p2.parse_numeric(b), 0);
                }

                if p2.err {
                    hdr.access_time = Time::default();
                    hdr.change_time = Time::default();
                    let s = parse_string(&blk[ustar::PREFIX]);
                    if is_ascii(&s) {
                        prefix = s;
                    }
                    hdr.format = FORMAT_UNKNOWN;
                }
            }
            if !prefix.is_empty() {
                let mut name = prefix;
                name.push(b'/');
                name.extend_from_slice(&hdr.name);
                hdr.name = name;
            }
        }
        if p.err {
            return Err(err_header());
        }
        Ok(hdr)
    }
}

impl<R: Read> Read for Reader<R> {
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
