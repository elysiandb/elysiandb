use super::common::{
    Header, Records, TYPE_DIR, TYPE_GNU_LONG_LINK, TYPE_GNU_LONG_NAME, TYPE_REG, TYPE_REG_A,
    TYPE_X_GLOBAL_HEADER, TYPE_X_HEADER, is_header_only_type, path_join, path_split,
};
use super::format::{
    Block, FORMAT_GNU, FORMAT_PAX, FORMAT_UNKNOWN, FORMAT_USTAR, Format, MAX_SPECIAL_FILE_SIZE,
    NAME_SIZE, PREFIX_SIZE, ZERO_BLOCK, block_padding, gnu, set_format, ustar, v7,
};
use super::strconv::{Formatter, format_pax_record, is_ascii, to_ascii};
use super::{ERR_FIELD_TOO_LONG, ERR_HEADER, ERR_WRITE_AFTER_CLOSE, ERR_WRITE_TOO_LONG, tar_error};
use crate::archive::Fail;
use crate::archive::flate::write_once;
use gocompat::time::{Duration, Time};
use std::io::{self, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
enum WriterState {
    Failed(Fail),
    Closed,
}

impl WriterState {
    fn to_io(&self) -> io::Error {
        match self {
            WriterState::Failed(err) => err.to_io(),
            WriterState::Closed => tar_error(io::ErrorKind::Other, ERR_WRITE_AFTER_CLOSE),
        }
    }
}

pub struct Writer<W: Write> {
    w: W,
    pad: i64,
    curr: i64,
    hdr: Header,
    blk: Block,
    err: Option<WriterState>,
}

pub(crate) fn split_ustar_path(name: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut length = name.len();
    if length <= NAME_SIZE || !is_ascii(name) {
        return None;
    } else if length > PREFIX_SIZE + 1 {
        length = PREFIX_SIZE + 1;
    } else if name[length - 1] == b'/' {
        length -= 1;
    }

    let i = name[..length].iter().rposition(|&c| c == b'/');
    let i = match i {
        Some(i) => i as i64,
        None => -1,
    };
    let nlen = name.len() as i64 - i - 1;
    let plen = i;
    if i <= 0 || nlen > NAME_SIZE as i64 || nlen == 0 || plen > PREFIX_SIZE as i64 {
        return None;
    }
    let i = i as usize;
    Some((&name[..i], &name[i + 1..]))
}

fn missed(nb: i64) -> io::Error {
    tar_error(
        io::ErrorKind::InvalidInput,
        &format!("missed writing {nb} bytes of the tar entry"),
    )
}

impl<W: Write> Writer<W> {
    pub fn new(w: W) -> Writer<W> {
        Writer {
            w,
            pad: 0,
            curr: 0,
            hdr: Header::default(),
            blk: ZERO_BLOCK,
            err: None,
        }
    }

    pub fn get_ref(&self) -> &W {
        &self.w
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.w
    }

    pub fn into_inner(self) -> W {
        self.w
    }

    fn fail(&mut self, e: &io::Error) -> io::Error {
        let fail = Fail::from_io(e);
        let out = fail.to_io();
        self.err = Some(WriterState::Failed(fail));
        out
    }

    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        let nb = self.curr;
        if nb > 0 {
            return Err(missed(nb));
        }
        let pad = self.pad as usize;
        if let Err(e) = write_once(&mut self.w, &ZERO_BLOCK[..pad]) {
            return Err(self.fail(&e));
        }
        self.pad = 0;
        Ok(())
    }

    pub fn write_header(&mut self, hdr: &Header) -> io::Result<()> {
        self.flush()?;
        self.hdr = hdr.clone();

        if self.hdr.typeflag == TYPE_REG_A {
            if self.hdr.name.ends_with(b"/") {
                self.hdr.typeflag = TYPE_DIR;
            } else {
                self.hdr.typeflag = TYPE_REG;
            }
        }

        if self.hdr.format == FORMAT_UNKNOWN {
            self.hdr.mod_time = self.hdr.mod_time.round(Duration::SECOND);
            self.hdr.access_time = Time::default();
            self.hdr.change_time = Time::default();
        }

        let (allowed, pax_hdrs, err) = self.hdr.allowed_formats();
        let mut hdr = std::mem::take(&mut self.hdr);
        let result = if allowed.has(FORMAT_USTAR) {
            self.write_ustar_header(&mut hdr)
        } else if allowed.has(FORMAT_PAX) {
            self.write_pax_header(&mut hdr, &pax_hdrs)
        } else if allowed.has(FORMAT_GNU) {
            self.write_gnu_header(&mut hdr)
        } else {
            self.hdr = hdr;
            let err = err.unwrap_or_else(|| super::common::HeaderError(Vec::new()));
            return Err(io::Error::new(io::ErrorKind::InvalidInput, err));
        };
        self.hdr = hdr;
        if let Err(e) = &result {
            let fail = Fail::from_io(e);
            self.err = Some(WriterState::Failed(fail));
        }
        result
    }

    fn write_ustar_header(&mut self, hdr: &mut Header) -> io::Result<()> {
        let mut name_prefix: Vec<u8> = Vec::new();
        if let Some((prefix, suffix)) = split_ustar_path(&hdr.name) {
            let (prefix, suffix) = (prefix.to_vec(), suffix.to_vec());
            name_prefix = prefix;
            hdr.name = suffix;
        }

        let mut f = Formatter::default();
        self.template_v7_plus(hdr, &mut f, false, false);
        f.format_string(&mut self.blk[ustar::PREFIX], &name_prefix);
        set_format(&mut self.blk, FORMAT_USTAR);
        if f.err {
            return Err(tar_error(io::ErrorKind::InvalidInput, ERR_FIELD_TOO_LONG));
        }
        let blk = self.blk;
        self.write_raw_header(&blk, hdr.size, hdr.typeflag)
    }

    fn write_pax_header(&mut self, hdr: &mut Header, pax_hdrs: &Records) -> io::Result<()> {
        let real_name = hdr.name.clone();

        let is_global = hdr.typeflag == TYPE_X_GLOBAL_HEADER;
        if !pax_hdrs.is_empty() || is_global {
            let mut buf = Vec::new();
            for (k, v) in pax_hdrs {
                match format_pax_record(k, v) {
                    Ok(rec) => buf.extend_from_slice(&rec),
                    Err(()) => return Err(tar_error(io::ErrorKind::InvalidInput, ERR_HEADER)),
                }
            }

            let name;
            let flag;
            if is_global {
                name = if real_name.is_empty() {
                    b"GlobalHead.0.0".to_vec()
                } else {
                    real_name.clone()
                };
                flag = TYPE_X_GLOBAL_HEADER;
            } else {
                let (dir, file) = path_split(&real_name);
                name = path_join(&[dir, b"PaxHeaders.0", file]);
                flag = TYPE_X_HEADER;
            }
            if buf.len() > MAX_SPECIAL_FILE_SIZE {
                return Err(tar_error(io::ErrorKind::InvalidInput, ERR_FIELD_TOO_LONG));
            }
            self.write_raw_file(&name, &buf, flag, FORMAT_PAX)?;
            if is_global {
                return Ok(());
            }
        }

        let mut f = Formatter::default();
        self.template_v7_plus(hdr, &mut f, true, false);
        set_format(&mut self.blk, FORMAT_PAX);
        let blk = self.blk;
        self.write_raw_header(&blk, hdr.size, hdr.typeflag)
    }

    fn write_gnu_header(&mut self, hdr: &mut Header) -> io::Result<()> {
        const LONG_NAME: &[u8] = b"././@LongLink";
        if hdr.name.len() > NAME_SIZE {
            let mut data = hdr.name.clone();
            data.push(0);
            self.write_raw_file(LONG_NAME, &data, TYPE_GNU_LONG_NAME, FORMAT_GNU)?;
        }
        if hdr.linkname.len() > NAME_SIZE {
            let mut data = hdr.linkname.clone();
            data.push(0);
            self.write_raw_file(LONG_NAME, &data, TYPE_GNU_LONG_LINK, FORMAT_GNU)?;
        }

        let mut f = Formatter::default();
        self.template_v7_plus(hdr, &mut f, false, true);
        if !hdr.access_time.is_zero() {
            f.format_numeric(&mut self.blk[gnu::ACCESS_TIME], hdr.access_time.unix());
        }
        if !hdr.change_time.is_zero() {
            f.format_numeric(&mut self.blk[gnu::CHANGE_TIME], hdr.change_time.unix());
        }
        set_format(&mut self.blk, FORMAT_GNU);
        let blk = self.blk;
        self.write_raw_header(&blk, hdr.size, hdr.typeflag)
    }

    fn template_v7_plus(&mut self, hdr: &Header, f: &mut Formatter, ascii: bool, numeric: bool) {
        self.blk = ZERO_BLOCK;

        let mod_time = if hdr.mod_time.is_zero() {
            Time::from_unix(0, 0)
        } else {
            hdr.mod_time.clone()
        };

        let blk = &mut self.blk;
        let fmt_str = |f: &mut Formatter, b: &mut [u8], s: &[u8]| {
            if ascii {
                f.format_string(b, &to_ascii(s));
            } else {
                f.format_string(b, s);
            }
        };
        let fmt_num = |f: &mut Formatter, b: &mut [u8], x: i64| {
            if numeric {
                f.format_numeric(b, x);
            } else {
                f.format_octal(b, x);
            }
        };

        blk[v7::TYPE_FLAG] = hdr.typeflag;
        fmt_str(f, &mut blk[v7::NAME], &hdr.name);
        fmt_str(f, &mut blk[v7::LINK_NAME], &hdr.linkname);
        fmt_num(f, &mut blk[v7::MODE], hdr.mode);
        fmt_num(f, &mut blk[v7::UID], hdr.uid);
        fmt_num(f, &mut blk[v7::GID], hdr.gid);
        fmt_num(f, &mut blk[v7::SIZE], hdr.size);
        fmt_num(f, &mut blk[v7::MOD_TIME], mod_time.unix());

        fmt_str(f, &mut blk[ustar::USER_NAME], &hdr.uname);
        fmt_str(f, &mut blk[ustar::GROUP_NAME], &hdr.gname);
        fmt_num(f, &mut blk[ustar::DEV_MAJOR], hdr.devmajor);
        fmt_num(f, &mut blk[ustar::DEV_MINOR], hdr.devminor);
    }

    fn write_raw_file(
        &mut self,
        name: &[u8],
        data: &[u8],
        flag: u8,
        format: Format,
    ) -> io::Result<()> {
        self.blk = ZERO_BLOCK;

        let mut name = to_ascii(name);
        if name.len() > NAME_SIZE {
            name.truncate(NAME_SIZE);
        }
        while name.last() == Some(&b'/') {
            name.pop();
        }

        let mut f = Formatter::default();
        self.blk[v7::TYPE_FLAG] = flag;
        f.format_string(&mut self.blk[v7::NAME], &name);
        f.format_octal(&mut self.blk[v7::MODE], 0);
        f.format_octal(&mut self.blk[v7::UID], 0);
        f.format_octal(&mut self.blk[v7::GID], 0);
        f.format_octal(&mut self.blk[v7::SIZE], data.len() as i64);
        f.format_octal(&mut self.blk[v7::MOD_TIME], 0);
        set_format(&mut self.blk, format);
        if f.err {
            return Err(tar_error(io::ErrorKind::InvalidInput, ERR_FIELD_TOO_LONG));
        }

        let blk = self.blk;
        self.write_raw_header(&blk, data.len() as i64, flag)?;
        self.write_entry(data).map(|_| ())
    }

    fn write_raw_header(&mut self, blk: &Block, size: i64, flag: u8) -> io::Result<()> {
        self.flush()?;
        write_once(&mut self.w, blk)?;
        let size = if is_header_only_type(flag) { 0 } else { size };
        self.curr = size;
        self.pad = block_padding(size);
        Ok(())
    }

    fn write_entry(&mut self, b: &[u8]) -> io::Result<usize> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        let overwrite = b.len() as i64 > self.curr;
        let b = if overwrite {
            &b[..self.curr as usize]
        } else {
            b
        };
        if !b.is_empty() {
            if let Err(e) = write_once(&mut self.w, b) {
                return Err(self.fail(&e));
            }
            self.curr -= b.len() as i64;
        }
        if overwrite && b.is_empty() {
            return Err(tar_error(io::ErrorKind::InvalidInput, ERR_WRITE_TOO_LONG));
        }
        Ok(b.len())
    }

    pub fn close(&mut self) -> io::Result<()> {
        match &self.err {
            Some(WriterState::Closed) => return Ok(()),
            Some(err) => return Err(err.to_io()),
            None => {}
        }

        let mut result = self.flush();
        for _ in 0..2 {
            if result.is_err() {
                break;
            }
            result = write_once(&mut self.w, &ZERO_BLOCK);
        }

        self.err = Some(WriterState::Closed);
        result
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_entry(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
