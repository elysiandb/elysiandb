use super::strconv::{Formatter, Parser};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Format(pub u32);

pub const FORMAT_UNKNOWN: Format = Format(0);
pub(super) const FORMAT_V7: Format = Format(1);
pub const FORMAT_USTAR: Format = Format(2);
pub const FORMAT_PAX: Format = Format(4);
pub const FORMAT_GNU: Format = Format(8);
pub(super) const FORMAT_STAR: Format = Format(16);
const FORMAT_MAX: Format = Format(32);

impl Format {
    pub fn has(self, f2: Format) -> bool {
        self.0 & f2.0 != 0
    }

    pub(super) fn may_be(&mut self, f2: Format) {
        self.0 |= f2.0;
    }

    pub(super) fn may_only_be(&mut self, f2: Format) {
        self.0 &= f2.0;
    }

    pub(super) fn must_not_be(&mut self, f2: Format) {
        self.0 &= !f2.0;
    }
}

impl std::ops::BitOr for Format {
    type Output = Format;

    fn bitor(self, rhs: Format) -> Format {
        Format(self.0 | rhs.0)
    }
}

fn format_name(f: Format) -> &'static str {
    match f {
        FORMAT_V7 => "V7",
        FORMAT_USTAR => "USTAR",
        FORMAT_PAX => "PAX",
        FORMAT_GNU => "GNU",
        FORMAT_STAR => "STAR",
        _ => "",
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ss = Vec::new();
        let mut f2 = Format(1);
        while f2 < FORMAT_MAX {
            if self.has(f2) {
                ss.push(format_name(f2));
            }
            f2 = Format(f2.0 << 1);
        }
        match ss.len() {
            0 => f.write_str("<unknown>"),
            1 => f.write_str(ss[0]),
            _ => write!(f, "({})", ss.join(" | ")),
        }
    }
}

pub(super) const MAGIC_GNU: &[u8] = b"ustar ";
pub(super) const VERSION_GNU: &[u8] = b" \x00";
pub(super) const MAGIC_USTAR: &[u8] = b"ustar\x00";
pub(super) const VERSION_USTAR: &[u8] = b"00";
pub(super) const TRAILER_STAR: &[u8] = b"tar\x00";

pub(super) const BLOCK_SIZE: usize = 512;
pub(super) const NAME_SIZE: usize = 100;
pub(super) const PREFIX_SIZE: usize = 155;

pub(super) const MAX_SPECIAL_FILE_SIZE: usize = 1 << 20;

pub(super) fn block_padding(offset: i64) -> i64 {
    offset.wrapping_neg() & (BLOCK_SIZE as i64 - 1)
}

pub(super) type Block = [u8; BLOCK_SIZE];

pub(super) const ZERO_BLOCK: Block = [0; BLOCK_SIZE];

pub(super) mod v7 {
    use std::ops::Range;
    pub const NAME: Range<usize> = 0..100;
    pub const MODE: Range<usize> = 100..108;
    pub const UID: Range<usize> = 108..116;
    pub const GID: Range<usize> = 116..124;
    pub const SIZE: Range<usize> = 124..136;
    pub const MOD_TIME: Range<usize> = 136..148;
    pub const CHKSUM: Range<usize> = 148..156;
    pub const TYPE_FLAG: usize = 156;
    pub const LINK_NAME: Range<usize> = 157..257;
}

pub(super) mod ustar {
    use std::ops::Range;
    pub const MAGIC: Range<usize> = 257..263;
    pub const VERSION: Range<usize> = 263..265;
    pub const USER_NAME: Range<usize> = 265..297;
    pub const GROUP_NAME: Range<usize> = 297..329;
    pub const DEV_MAJOR: Range<usize> = 329..337;
    pub const DEV_MINOR: Range<usize> = 337..345;
    pub const PREFIX: Range<usize> = 345..500;
}

pub(super) mod gnu {
    use std::ops::Range;
    pub const MAGIC: Range<usize> = 257..263;
    pub const VERSION: Range<usize> = 263..265;
    pub const ACCESS_TIME: Range<usize> = 345..357;
    pub const CHANGE_TIME: Range<usize> = 357..369;
    pub const SPARSE: Range<usize> = 386..483;
    pub const REAL_SIZE: Range<usize> = 483..495;
}

pub(super) mod star {
    use std::ops::Range;
    pub const MAGIC: Range<usize> = 257..263;
    pub const VERSION: Range<usize> = 263..265;
    pub const PREFIX: Range<usize> = 345..476;
    pub const ACCESS_TIME: Range<usize> = 476..488;
    pub const CHANGE_TIME: Range<usize> = 488..500;
    pub const TRAILER: Range<usize> = 508..512;
}

pub(super) fn sparse_max_entries(s: &[u8]) -> usize {
    s.len() / 24
}

pub(super) fn sparse_offset(s: &[u8], i: usize) -> &[u8] {
    &s[i * 24..i * 24 + 12]
}

pub(super) fn sparse_length(s: &[u8], i: usize) -> &[u8] {
    &s[i * 24 + 12..i * 24 + 24]
}

pub(super) fn sparse_is_extended(s: &[u8]) -> u8 {
    s[24 * sparse_max_entries(s)]
}

pub(super) fn compute_checksum(b: &Block) -> (i64, i64) {
    let mut unsigned: i64 = 0;
    let mut signed: i64 = 0;
    for (i, &c) in b.iter().enumerate() {
        let c = if (148..156).contains(&i) { b' ' } else { c };
        unsigned += i64::from(c);
        signed += i64::from(c as i8);
    }
    (unsigned, signed)
}

pub(super) fn get_format(b: &Block) -> Format {
    let mut p = Parser::default();
    let value = p.parse_octal(&b[v7::CHKSUM]);
    let (chksum1, chksum2) = compute_checksum(b);
    if p.err || (value != chksum1 && value != chksum2) {
        return FORMAT_UNKNOWN;
    }

    let magic = &b[ustar::MAGIC];
    let version = &b[ustar::VERSION];
    let trailer = &b[star::TRAILER];
    if magic == MAGIC_USTAR && trailer == TRAILER_STAR {
        FORMAT_STAR
    } else if magic == MAGIC_USTAR {
        FORMAT_USTAR | FORMAT_PAX
    } else if magic == MAGIC_GNU && version == VERSION_GNU {
        FORMAT_GNU
    } else {
        FORMAT_V7
    }
}

pub(super) fn set_format(b: &mut Block, format: Format) {
    if format.has(FORMAT_V7) {
    } else if format.has(FORMAT_GNU) {
        b[gnu::MAGIC].copy_from_slice(MAGIC_GNU);
        b[gnu::VERSION].copy_from_slice(VERSION_GNU);
    } else if format.has(FORMAT_STAR) {
        b[star::MAGIC].copy_from_slice(MAGIC_USTAR);
        b[star::VERSION].copy_from_slice(VERSION_USTAR);
        b[star::TRAILER].copy_from_slice(TRAILER_STAR);
    } else if format.has(FORMAT_USTAR | FORMAT_PAX) {
        b[ustar::MAGIC].copy_from_slice(MAGIC_USTAR);
        b[ustar::VERSION].copy_from_slice(VERSION_USTAR);
    } else {
        panic!("invalid tar format");
    }

    let mut f = Formatter::default();
    let (chksum, _) = compute_checksum(b);
    f.format_octal(&mut b[148..155], chksum);
    b[155] = b' ';
}
