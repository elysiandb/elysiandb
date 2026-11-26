use super::tables::{
    NFC_INDEX, NFC_SPARSE_OFFSET, NFC_SPARSE_VALUES, NFC_VALUES, NFKC_INDEX, NFKC_SPARSE_OFFSET,
    NFKC_SPARSE_VALUES, NFKC_VALUES, le_u16, value_range,
};

pub(super) struct SparseBlocks {
    values: &'static [u8],
    offset: &'static [u8],
}

impl SparseBlocks {
    fn lookup(&self, n: u32, b: u8) -> u16 {
        let offset = le_u16(self.offset, n as usize);
        let header = value_range(self.values, usize::from(offset));
        let mut lo = offset.wrapping_add(1);
        let mut hi = lo.wrapping_add(u16::from(header.lo));
        while lo < hi {
            let m = lo + (hi - lo) / 2;
            let r = value_range(self.values, usize::from(m));
            if r.lo <= b && b <= r.hi {
                return r
                    .value
                    .wrapping_add(u16::from(b.wrapping_sub(r.lo)).wrapping_mul(header.value));
            }
            if b < r.lo {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        0
    }
}

pub(super) struct NormTrie {
    index: &'static [u8],
    wide_index: bool,
    values: &'static [u8],
    dense_blocks: u32,
    sparse: SparseBlocks,
}

pub(super) static NFC_DATA: NormTrie = NormTrie {
    index: NFC_INDEX,
    wide_index: false,
    values: NFC_VALUES,
    dense_blocks: 46,
    sparse: SparseBlocks {
        values: NFC_SPARSE_VALUES,
        offset: NFC_SPARSE_OFFSET,
    },
};

pub(super) static NFKC_DATA: NormTrie = NormTrie {
    index: NFKC_INDEX,
    wide_index: true,
    values: NFKC_VALUES,
    dense_blocks: 95,
    sparse: SparseBlocks {
        values: NFKC_SPARSE_VALUES,
        offset: NFKC_SPARSE_OFFSET,
    },
};

impl NormTrie {
    #[inline]
    fn index_at(&self, i: usize) -> u32 {
        if self.wide_index {
            u32::from(le_u16(self.index, i))
        } else {
            u32::from(self.index[i])
        }
    }

    #[inline]
    fn lookup_value(&self, n: u32, b: u8) -> u16 {
        if n < self.dense_blocks {
            le_u16(self.values, ((n << 6) + u32::from(b)) as usize)
        } else {
            self.sparse.lookup(n - self.dense_blocks, b)
        }
    }

    pub(super) fn lookup(&self, s: &[u8]) -> (u16, usize) {
        let c0 = s[0];
        if c0 < 0x80 {
            return (le_u16(self.values, usize::from(c0)), 1);
        }
        if c0 < 0xC2 {
            return (0, 1);
        }
        if c0 < 0xE0 {
            if s.len() < 2 {
                return (0, 0);
            }
            let i = self.index_at(usize::from(c0));
            let c1 = s[1];
            if !(0x80..0xC0).contains(&c1) {
                return (0, 1);
            }
            return (self.lookup_value(i, c1), 2);
        }
        if c0 < 0xF0 {
            if s.len() < 3 {
                return (0, 0);
            }
            let mut i = self.index_at(usize::from(c0));
            let c1 = s[1];
            if !(0x80..0xC0).contains(&c1) {
                return (0, 1);
            }
            let o = (i << 6) + u32::from(c1);
            i = self.index_at(o as usize);
            let c2 = s[2];
            if !(0x80..0xC0).contains(&c2) {
                return (0, 2);
            }
            return (self.lookup_value(i, c2), 3);
        }
        if c0 < 0xF8 {
            if s.len() < 4 {
                return (0, 0);
            }
            let mut i = self.index_at(usize::from(c0));
            let c1 = s[1];
            if !(0x80..0xC0).contains(&c1) {
                return (0, 1);
            }
            let mut o = (i << 6) + u32::from(c1);
            i = self.index_at(o as usize);
            let c2 = s[2];
            if !(0x80..0xC0).contains(&c2) {
                return (0, 2);
            }
            o = (i << 6) + u32::from(c2);
            i = self.index_at(o as usize);
            let c3 = s[3];
            if !(0x80..0xC0).contains(&c3) {
                return (0, 3);
            }
            return (self.lookup_value(i, c3), 4);
        }
        (0, 1)
    }
}
