use super::format::{FORMAT_GNU, FORMAT_PAX, FORMAT_UNKNOWN, FORMAT_USTAR, Format, gnu, ustar, v7};
use super::strconv::{
    fits_in_base256, fits_in_octal, format_pax_time, has_nul, is_ascii, valid_pax_record,
};
use super::writer::split_ustar_path;
use gocompat::json::quote_text;
use gocompat::strconv::format_int;
use gocompat::time::{RFC3339, Time};
use std::collections::BTreeMap;
use std::fmt;

pub const TYPE_REG: u8 = b'0';
pub const TYPE_REG_A: u8 = b'\x00';
pub const TYPE_LINK: u8 = b'1';
pub const TYPE_SYMLINK: u8 = b'2';
pub const TYPE_CHAR: u8 = b'3';
pub const TYPE_BLOCK: u8 = b'4';
pub const TYPE_DIR: u8 = b'5';
pub const TYPE_FIFO: u8 = b'6';
pub const TYPE_CONT: u8 = b'7';
pub const TYPE_X_HEADER: u8 = b'x';
pub const TYPE_X_GLOBAL_HEADER: u8 = b'g';
pub const TYPE_GNU_SPARSE: u8 = b'S';
pub const TYPE_GNU_LONG_NAME: u8 = b'L';
pub const TYPE_GNU_LONG_LINK: u8 = b'K';

pub(super) const PAX_NONE: &[u8] = b"";
pub(super) const PAX_PATH: &[u8] = b"path";
pub(super) const PAX_LINKPATH: &[u8] = b"linkpath";
pub(super) const PAX_SIZE: &[u8] = b"size";
pub(super) const PAX_UID: &[u8] = b"uid";
pub(super) const PAX_GID: &[u8] = b"gid";
pub(super) const PAX_UNAME: &[u8] = b"uname";
pub(super) const PAX_GNAME: &[u8] = b"gname";
pub(super) const PAX_MTIME: &[u8] = b"mtime";
pub(super) const PAX_ATIME: &[u8] = b"atime";
pub(super) const PAX_CTIME: &[u8] = b"ctime";

pub(super) const PAX_SCHILY_XATTR: &[u8] = b"SCHILY.xattr.";

pub(super) const PAX_GNU_SPARSE: &[u8] = b"GNU.sparse.";
pub(super) const PAX_GNU_SPARSE_NUM_BLOCKS: &[u8] = b"GNU.sparse.numblocks";
pub(super) const PAX_GNU_SPARSE_OFFSET: &[u8] = b"GNU.sparse.offset";
pub(super) const PAX_GNU_SPARSE_NUM_BYTES: &[u8] = b"GNU.sparse.numbytes";
pub(super) const PAX_GNU_SPARSE_MAP: &[u8] = b"GNU.sparse.map";
pub(super) const PAX_GNU_SPARSE_NAME: &[u8] = b"GNU.sparse.name";
pub(super) const PAX_GNU_SPARSE_MAJOR: &[u8] = b"GNU.sparse.major";
pub(super) const PAX_GNU_SPARSE_MINOR: &[u8] = b"GNU.sparse.minor";
pub(super) const PAX_GNU_SPARSE_SIZE: &[u8] = b"GNU.sparse.size";
pub(super) const PAX_GNU_SPARSE_REAL_SIZE: &[u8] = b"GNU.sparse.realsize";

fn is_basic_key(k: &[u8]) -> bool {
    matches!(
        k,
        b"path"
            | b"linkpath"
            | b"size"
            | b"uid"
            | b"gid"
            | b"uname"
            | b"gname"
            | b"mtime"
            | b"atime"
            | b"ctime"
    )
}

pub type Records = BTreeMap<Vec<u8>, Vec<u8>>;

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub typeflag: u8,
    pub name: Vec<u8>,
    pub linkname: Vec<u8>,
    pub size: i64,
    pub mode: i64,
    pub uid: i64,
    pub gid: i64,
    pub uname: Vec<u8>,
    pub gname: Vec<u8>,
    pub mod_time: Time,
    pub access_time: Time,
    pub change_time: Time,
    pub devmajor: i64,
    pub devminor: i64,
    pub xattrs: Records,
    pub pax_records: Records,
    pub format: Format,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderError(pub Vec<String>);

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const PREFIX: &str = "cannot encode the tar header";
        let ss: Vec<&str> = self
            .0
            .iter()
            .filter(|s| !s.is_empty())
            .map(String::as_str)
            .collect();
        if ss.is_empty() {
            return f.write_str(PREFIX);
        }
        write!(f, "{PREFIX}: {}", ss.join("; and "))
    }
}

impl std::error::Error for HeaderError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SparseEntry {
    pub(super) offset: i64,
    pub(super) length: i64,
}

impl SparseEntry {
    pub(super) fn end_offset(&self) -> i64 {
        self.offset.wrapping_add(self.length)
    }
}

pub(super) fn validate_sparse_entries(sp: &[SparseEntry], size: i64) -> bool {
    if size < 0 {
        return false;
    }
    let mut pre = SparseEntry::default();
    for &cur in sp {
        if cur.offset < 0
            || cur.length < 0
            || cur.offset > i64::MAX - cur.length
            || cur.end_offset() > size
            || pre.end_offset() > cur.offset
        {
            return false;
        }
        pre = cur;
    }
    true
}

pub(super) fn invert_sparse_entries(src: &[SparseEntry], size: i64) -> Vec<SparseEntry> {
    let mut dst = Vec::with_capacity(src.len() + 1);
    let mut pre = SparseEntry::default();
    for cur in src {
        if cur.length == 0 {
            continue;
        }
        pre.length = cur.offset - pre.offset;
        if pre.length > 0 {
            dst.push(pre);
        }
        pre.offset = cur.end_offset();
    }
    pre.length = size - pre.offset;
    dst.push(pre);
    dst
}

pub fn is_header_only_type(flag: u8) -> bool {
    matches!(
        flag,
        TYPE_LINK | TYPE_SYMLINK | TYPE_CHAR | TYPE_BLOCK | TYPE_DIR | TYPE_FIFO
    )
}

fn quoted(s: &[u8]) -> String {
    quote_text(s)
}

impl Header {
    fn is_global_shape(&self) -> bool {
        self.linkname.is_empty()
            && self.size == 0
            && self.mode == 0
            && self.uid == 0
            && self.gid == 0
            && self.uname.is_empty()
            && self.gname.is_empty()
            && self.mod_time.is_zero()
            && self.access_time.is_zero()
            && self.change_time.is_zero()
            && self.devmajor == 0
            && self.devminor == 0
    }

    pub(super) fn allowed_formats(&self) -> (Format, Records, Option<HeaderError>) {
        let mut format = FORMAT_USTAR | FORMAT_PAX | FORMAT_GNU;
        let mut pax_hdrs = Records::new();

        let mut why_no_ustar = String::new();
        let mut why_no_pax = String::new();
        let mut why_no_gnu = String::new();
        let mut prefer_pax = false;

        let mut verify_string = |format: &mut Format,
                                 pax_hdrs: &mut Records,
                                 s: &[u8],
                                 size: usize,
                                 name: &str,
                                 pax_key: &[u8]| {
            let too_long = s.len() > size;
            let allow_long_gnu = pax_key == PAX_PATH || pax_key == PAX_LINKPATH;
            if has_nul(s) || (too_long && !allow_long_gnu) {
                why_no_gnu = format!("GNU cannot encode {name}={}", quoted(s));
                format.must_not_be(FORMAT_GNU);
            }
            if !is_ascii(s) || too_long {
                let can_split_ustar = pax_key == PAX_PATH;
                if !can_split_ustar || split_ustar_path(s).is_none() {
                    why_no_ustar = format!("USTAR cannot encode {name}={}", quoted(s));
                    format.must_not_be(FORMAT_USTAR);
                }
                if pax_key == PAX_NONE {
                    why_no_pax = format!("PAX cannot encode {name}={}", quoted(s));
                    format.must_not_be(FORMAT_PAX);
                } else {
                    pax_hdrs.insert(pax_key.to_vec(), s.to_vec());
                }
            }
            if let Some(v) = self.pax_records.get(pax_key)
                && v.as_slice() == s
            {
                pax_hdrs.insert(pax_key.to_vec(), v.clone());
            }
        };
        verify_string(
            &mut format,
            &mut pax_hdrs,
            &self.name,
            v7::NAME.len(),
            "Name",
            PAX_PATH,
        );
        verify_string(
            &mut format,
            &mut pax_hdrs,
            &self.linkname,
            v7::LINK_NAME.len(),
            "Linkname",
            PAX_LINKPATH,
        );
        verify_string(
            &mut format,
            &mut pax_hdrs,
            &self.uname,
            ustar::USER_NAME.len(),
            "Uname",
            PAX_UNAME,
        );
        verify_string(
            &mut format,
            &mut pax_hdrs,
            &self.gname,
            ustar::GROUP_NAME.len(),
            "Gname",
            PAX_GNAME,
        );

        let mut verify_numeric = |format: &mut Format,
                                  pax_hdrs: &mut Records,
                                  n: i64,
                                  size: usize,
                                  name: &str,
                                  pax_key: &[u8]| {
            if !fits_in_base256(size, n) {
                why_no_gnu = format!("GNU cannot encode {name}={n}");
                format.must_not_be(FORMAT_GNU);
            }
            if !fits_in_octal(size, n) {
                why_no_ustar = format!("USTAR cannot encode {name}={n}");
                format.must_not_be(FORMAT_USTAR);
                if pax_key == PAX_NONE {
                    why_no_pax = format!("PAX cannot encode {name}={n}");
                    format.must_not_be(FORMAT_PAX);
                } else {
                    pax_hdrs.insert(pax_key.to_vec(), format_int(n, 10).into_bytes());
                }
            }
            if let Some(v) = self.pax_records.get(pax_key)
                && v.as_slice() == format_int(n, 10).as_bytes()
            {
                pax_hdrs.insert(pax_key.to_vec(), v.clone());
            }
        };
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.mode,
            v7::MODE.len(),
            "Mode",
            PAX_NONE,
        );
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.uid,
            v7::UID.len(),
            "Uid",
            PAX_UID,
        );
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.gid,
            v7::GID.len(),
            "Gid",
            PAX_GID,
        );
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.size,
            v7::SIZE.len(),
            "Size",
            PAX_SIZE,
        );
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.devmajor,
            ustar::DEV_MAJOR.len(),
            "Devmajor",
            PAX_NONE,
        );
        verify_numeric(
            &mut format,
            &mut pax_hdrs,
            self.devminor,
            ustar::DEV_MINOR.len(),
            "Devminor",
            PAX_NONE,
        );

        let mut verify_time = |format: &mut Format,
                               pax_hdrs: &mut Records,
                               ts: &Time,
                               size: usize,
                               name: &str,
                               pax_key: &[u8]| {
            if ts.is_zero() {
                return;
            }
            let shown = ts.format(RFC3339);
            if !fits_in_base256(size, ts.unix()) {
                why_no_gnu = format!("GNU cannot encode {name}={shown}");
                format.must_not_be(FORMAT_GNU);
            }
            let is_mtime = pax_key == PAX_MTIME;
            let fits_octal = fits_in_octal(size, ts.unix());
            if !(is_mtime && fits_octal) {
                why_no_ustar = format!("USTAR cannot encode {name}={shown}");
                format.must_not_be(FORMAT_USTAR);
            }
            let needs_nano = ts.nanosecond() != 0;
            if !is_mtime || !fits_octal || needs_nano {
                prefer_pax = true;
                if pax_key == PAX_NONE {
                    why_no_pax = format!("PAX cannot encode {name}={shown}");
                    format.must_not_be(FORMAT_PAX);
                } else {
                    pax_hdrs.insert(pax_key.to_vec(), format_pax_time(ts));
                }
            }
            if let Some(v) = self.pax_records.get(pax_key)
                && *v == format_pax_time(ts)
            {
                pax_hdrs.insert(pax_key.to_vec(), v.clone());
            }
        };
        verify_time(
            &mut format,
            &mut pax_hdrs,
            &self.mod_time,
            v7::MOD_TIME.len(),
            "ModTime",
            PAX_MTIME,
        );
        verify_time(
            &mut format,
            &mut pax_hdrs,
            &self.access_time,
            gnu::ACCESS_TIME.len(),
            "AccessTime",
            PAX_ATIME,
        );
        verify_time(
            &mut format,
            &mut pax_hdrs,
            &self.change_time,
            gnu::CHANGE_TIME.len(),
            "ChangeTime",
            PAX_CTIME,
        );

        let mut why_only_pax = String::new();
        let why_only_gnu = String::new();
        match self.typeflag {
            TYPE_REG | TYPE_CHAR | TYPE_BLOCK | TYPE_FIFO | TYPE_GNU_SPARSE => {
                if self.name.ends_with(b"/") {
                    return (
                        FORMAT_UNKNOWN,
                        Records::new(),
                        Some(HeaderError(vec![
                            "filename may not have trailing slash".to_string(),
                        ])),
                    );
                }
            }
            TYPE_X_HEADER | TYPE_GNU_LONG_NAME | TYPE_GNU_LONG_LINK => {
                return (
                    FORMAT_UNKNOWN,
                    Records::new(),
                    Some(HeaderError(vec![
                        "cannot write an extended header, a long name or a long link entry directly"
                            .to_string(),
                    ])),
                );
            }
            TYPE_X_GLOBAL_HEADER => {
                if !self.is_global_shape() {
                    return (
                        FORMAT_UNKNOWN,
                        Records::new(),
                        Some(HeaderError(vec![
                            "only the PAX records may be set on a global header".to_string(),
                        ])),
                    );
                }
                why_only_pax = "only PAX supports global headers".to_string();
                format.may_only_be(FORMAT_PAX);
            }
            _ => {}
        }
        if !is_header_only_type(self.typeflag) && self.size < 0 {
            return (
                FORMAT_UNKNOWN,
                Records::new(),
                Some(HeaderError(vec![
                    "negative size on header-only type".to_string(),
                ])),
            );
        }

        if !self.xattrs.is_empty() {
            for (k, v) in &self.xattrs {
                let mut key = PAX_SCHILY_XATTR.to_vec();
                key.extend_from_slice(k);
                pax_hdrs.insert(key, v.clone());
            }
            why_only_pax = "only PAX supports extended attributes".to_string();
            format.may_only_be(FORMAT_PAX);
        }
        if !self.pax_records.is_empty() {
            for (k, v) in &self.pax_records {
                if pax_hdrs.contains_key(k) {
                    continue;
                }
                if self.typeflag == TYPE_X_GLOBAL_HEADER
                    || (!is_basic_key(k) && !k.starts_with(PAX_GNU_SPARSE))
                {
                    pax_hdrs.insert(k.clone(), v.clone());
                }
            }
            why_only_pax = "only PAX supports PAX records".to_string();
            format.may_only_be(FORMAT_PAX);
        }
        for (k, v) in &pax_hdrs {
            if !valid_pax_record(k, v) {
                let mut rec = k.clone();
                rec.extend_from_slice(b" = ");
                rec.extend_from_slice(v);
                return (
                    FORMAT_UNKNOWN,
                    Records::new(),
                    Some(HeaderError(vec![format!(
                        "invalid PAX record: {}",
                        quoted(&rec)
                    )])),
                );
            }
        }

        if self.format != FORMAT_UNKNOWN {
            let mut want_format = self.format;
            if want_format.has(FORMAT_PAX) && !prefer_pax {
                want_format.may_be(FORMAT_USTAR);
            }
            format.may_only_be(want_format);
        }
        let mut err = None;
        if format == FORMAT_UNKNOWN {
            err = Some(match self.format {
                FORMAT_USTAR => HeaderError(vec![
                    "the requested format is USTAR".to_string(),
                    why_no_ustar,
                    why_only_pax,
                    why_only_gnu,
                ]),
                FORMAT_PAX => HeaderError(vec![
                    "the requested format is PAX".to_string(),
                    why_no_pax,
                    why_only_gnu,
                ]),
                FORMAT_GNU => HeaderError(vec![
                    "the requested format is GNU".to_string(),
                    why_no_gnu,
                    why_only_pax,
                ]),
                _ => HeaderError(vec![
                    why_no_ustar,
                    why_no_pax,
                    why_no_gnu,
                    why_only_pax,
                    why_only_gnu,
                ]),
            });
        }
        (format, pax_hdrs, err)
    }
}

pub(super) fn path_split(p: &[u8]) -> (&[u8], &[u8]) {
    match p.iter().rposition(|&c| c == b'/') {
        Some(i) => (&p[..i + 1], &p[i + 1..]),
        None => (&p[..0], p),
    }
}

pub(super) fn path_join(elems: &[&[u8]]) -> Vec<u8> {
    let mut size = 0;
    for e in elems {
        size += e.len();
    }
    if size == 0 {
        return Vec::new();
    }
    let mut buf = Vec::with_capacity(size + elems.len());
    for e in elems {
        if !buf.is_empty() || !e.is_empty() {
            if !buf.is_empty() {
                buf.push(b'/');
            }
            buf.extend_from_slice(e);
        }
    }
    path_clean(&buf)
}

pub(super) fn path_clean(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }

    let rooted = path[0] == b'/';
    let n = path.len();

    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }

    while r < n {
        if path[r] == b'/' || (path[r] == b'.' && (r + 1 == n || path[r + 1] == b'/')) {
            r += 1;
        } else if path[r] == b'.' && path[r + 1] == b'.' && (r + 2 == n || path[r + 2] == b'/') {
            r += 2;
            if out.len() > dotdot {
                let mut w = out.len() - 1;
                while w > dotdot && out[w] != b'/' {
                    w -= 1;
                }
                out.truncate(w);
            } else if !rooted {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                out.push(b'/');
            }
            while r < n && path[r] != b'/' {
                out.push(path[r]);
                r += 1;
            }
        }
    }

    if out.is_empty() {
        return b".".to_vec();
    }
    out
}

pub(super) fn records_get<'a>(records: &'a Records, key: &[u8]) -> &'a [u8] {
    records.get(key).map_or(&[], Vec::as_slice)
}
