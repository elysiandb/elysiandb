use super::trieval::SparseBlocks;
use crate::deps::xtext::colltab::le_u16;

macro_rules! table {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/golden/hooks/xtext/cases/",
            $name,
            ".bin"
        ))
    };
}

pub const UNICODE_VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/golden/hooks/xtext/cases/UnicodeVersion.bin"
));

pub(super) static XOR_DATA: &[u8] = table!("xorData");
pub(super) static EXCEPTIONS: &[u8] = table!("exceptions");
static CASE_VALUES: &[u8] = table!("caseValues");
static CASE_INDEX: &[u8] = table!("caseIndex");
static SPARSE_OFFSETS: &[u8] = table!("sparseOffsets");
static SPARSE_VALUES: &[u8] = table!("sparseValues");

static SPARSE: SparseBlocks = SparseBlocks {
    values: SPARSE_VALUES,
    offsets: SPARSE_OFFSETS,
};

fn lookup_value(n: u32, b: u8) -> u16 {
    if n < 22 {
        le_u16(CASE_VALUES, ((n << 6) + u32::from(b)) as usize)
    } else {
        SPARSE.lookup(n - 22, b)
    }
}

fn index_at(i: usize) -> u32 {
    u32::from(le_u16(CASE_INDEX, i))
}

pub(super) fn lookup(s: &[u8]) -> (u16, usize) {
    let c0 = s[0];
    if c0 < 0x80 {
        return (le_u16(CASE_VALUES, usize::from(c0)), 1);
    }
    if c0 < 0xC2 {
        return (0, 1);
    }
    if c0 < 0xE0 {
        if s.len() < 2 {
            return (0, 0);
        }
        let i = index_at(usize::from(c0));
        let c1 = s[1];
        if !(0x80..0xC0).contains(&c1) {
            return (0, 1);
        }
        return (lookup_value(i, c1), 2);
    }
    if c0 < 0xF0 {
        if s.len() < 3 {
            return (0, 0);
        }
        let mut i = index_at(usize::from(c0));
        let c1 = s[1];
        if !(0x80..0xC0).contains(&c1) {
            return (0, 1);
        }
        let o = (i << 6) + u32::from(c1);
        i = index_at(o as usize);
        let c2 = s[2];
        if !(0x80..0xC0).contains(&c2) {
            return (0, 2);
        }
        return (lookup_value(i, c2), 3);
    }
    if c0 < 0xF8 {
        if s.len() < 4 {
            return (0, 0);
        }
        let mut i = index_at(usize::from(c0));
        let c1 = s[1];
        if !(0x80..0xC0).contains(&c1) {
            return (0, 1);
        }
        let mut o = (i << 6) + u32::from(c1);
        i = index_at(o as usize);
        let c2 = s[2];
        if !(0x80..0xC0).contains(&c2) {
            return (0, 2);
        }
        o = (i << 6) + u32::from(c2);
        i = index_at(o as usize);
        let c3 = s[3];
        if !(0x80..0xC0).contains(&c3) {
            return (0, 3);
        }
        return (lookup_value(i, c3), 4);
    }
    (0, 1)
}
