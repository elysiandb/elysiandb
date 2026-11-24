use crate::deps::xtext::colltab::le_u16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Info(pub(super) u16);

pub(super) const CASED_MASK: u16 = 0x0003;
pub(super) const FULL_CASED_MASK: u16 = 0x0007;
pub(super) const IGNORABLE_MASK: u16 = 0x0006;
pub(super) const IGNORABLE_VALUE: u16 = 0x0004;
pub(super) const EXCEPTION_BIT: u16 = 1 << 3;
pub(super) const EXCEPTION_SHIFT: u16 = 4;
pub(super) const XOR_INDEX_BIT: u16 = 1 << 6;
pub(super) const XOR_SHIFT: u16 = 8;
pub(super) const HAS_MAPPING_MASK: u16 = 0xff80 | EXCEPTION_BIT;

pub(super) const C_UNCASED: Info = Info(0);
pub(super) const C_TITLE: Info = Info(1);
pub(super) const C_LOWER: Info = Info(2);
pub(super) const C_UPPER: Info = Info(3);
pub(super) const C_IGNORABLE_CASED: Info = Info(5);
pub(super) const C_XOR_CASE: Info = Info(6);

#[cfg(test)]
pub(super) const CCC_BREAK: Info = Info(0);
#[cfg(test)]
pub(super) const CCC_ZERO: Info = Info(1 << 4);
#[cfg(test)]
pub(super) const CCC_ABOVE: Info = Info(2 << 4);
#[cfg(test)]
pub(super) const CCC_OTHER: Info = Info(3 << 4);
#[cfg(test)]
pub(super) const CCC_MASK: u16 = CCC_BREAK.0 | CCC_ZERO.0 | CCC_ABOVE.0 | CCC_OTHER.0;

pub(super) const LENGTH_MASK: u8 = 0x07;
pub(super) const LENGTH_BITS: u8 = 3;
pub(super) const NO_CHANGE: u8 = 0;

impl Info {
    pub(super) fn is_cased(self) -> bool {
        self.0 & CASED_MASK != 0
    }

    pub(super) fn is_case_ignorable(self) -> bool {
        self.0 & IGNORABLE_MASK == IGNORABLE_VALUE
    }

    pub(super) fn is_not_cased_and_not_case_ignorable(self) -> bool {
        self.0 & FULL_CASED_MASK == 0
    }
}

pub(super) struct SparseBlocks {
    pub(super) values: &'static [u8],
    pub(super) offsets: &'static [u8],
}

impl SparseBlocks {
    pub(super) fn lookup(&self, n: u32, b: u8) -> u16 {
        let mut lo = le_u16(self.offsets, n as usize);
        let mut hi = le_u16(self.offsets, n as usize + 1);
        while lo < hi {
            let m = lo + (hi - lo) / 2;
            let i = 4 * usize::from(m);
            let value = u16::from_le_bytes([self.values[i], self.values[i + 1]]);
            let (rlo, rhi) = (self.values[i + 2], self.values[i + 3]);
            if rlo <= b && b <= rhi {
                return value;
            }
            if b < rlo {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        0
    }
}
