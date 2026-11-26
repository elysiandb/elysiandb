macro_rules! table {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/golden/hooks/xtext/norm/",
            $name,
            ".bin"
        ))
    };
}

pub const VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/golden/hooks/xtext/norm/Version.bin"
));

pub const MAX_TRANSFORM_CHUNK_SIZE: usize = 35 + super::composition::MAX_NON_STARTERS * 4;

pub(super) const FIRST_MULTI: u16 = 0x199A;
pub(super) const FIRST_CCC: u16 = 0x2DD5;
pub(super) const END_MULTI: u16 = 0x30A1;
pub(super) const FIRST_LEADING_CCC: u16 = 0x4AEF;
pub(super) const FIRST_CCC_ZERO_EXCEPT: u16 = 0x4BB9;
pub(super) const FIRST_STARTER_WITH_N_LEAD: u16 = 0x4BE0;

pub(super) static CCC: &[u8] = table!("ccc");
pub(super) static DECOMPS: &[u8] = table!("decomps");
pub(super) static NFC_INDEX: &[u8] = table!("nfcIndex");
pub(super) static NFC_SPARSE_OFFSET: &[u8] = table!("nfcSparseOffset");
pub(super) static NFC_SPARSE_VALUES: &[u8] = table!("nfcSparseValues");
pub(super) static NFC_VALUES: &[u8] = table!("nfcValues");
pub(super) static NFKC_INDEX: &[u8] = table!("nfkcIndex");
pub(super) static NFKC_SPARSE_OFFSET: &[u8] = table!("nfkcSparseOffset");
pub(super) static NFKC_SPARSE_VALUES: &[u8] = table!("nfkcSparseValues");
pub(super) static NFKC_VALUES: &[u8] = table!("nfkcValues");
pub(super) static RECOMP_MAP_PACKED: &[u8] = table!("recompMapPacked");

#[inline]
pub(crate) fn le_u16(t: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([t[2 * i], t[2 * i + 1]])
}

#[derive(Clone, Copy)]
pub(crate) struct ValueRange {
    pub(crate) value: u16,
    pub(crate) lo: u8,
    pub(crate) hi: u8,
}

#[inline]
pub(crate) fn value_range(t: &[u8], i: usize) -> ValueRange {
    ValueRange {
        value: u16::from_le_bytes([t[4 * i], t[4 * i + 1]]),
        lo: t[4 * i + 2],
        hi: t[4 * i + 3],
    }
}
