use gocompat::unicode;
use gocompat::utf8::Rune;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Primary = 0,
    Secondary = 1,
    Tertiary = 2,
    Quaternary = 3,
    Identity = 4,
}

pub const NUM_LEVELS: usize = 5;

pub(super) const DEFAULT_SECONDARY: i32 = 0x20;
pub(super) const DEFAULT_TERTIARY: i32 = 0x2;
pub const MAX_TERTIARY: u8 = 0x1F;
pub const MAX_QUATERNARY: i32 = 0x1FFFFF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Elem(pub u32);

const MAX_CE: u32 = 0xAFFFFFFF;
pub const PRIVATE_USE: Elem = Elem(MIN_CONTRACT);
const MIN_CONTRACT: u32 = 0xC0000000;
const MAX_CONTRACT: u32 = 0xDFFFFFFF;
const MAX_EXPAND: u32 = 0xEFFFFFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CeType {
    Normal,
    ContractionIndex,
    ExpansionIndex,
    Decompose,
}

const CE_TYPE_MASK: u32 = 0xC0000000;
const CE_TYPE_MASK_EXT: u32 = 0xE0000000;
const CE_IGNORE_MASK: u32 = 0xF00FFFFF;
const CE_TYPE1: u32 = 0x40000000;
const CE_TYPE2: u32 = 0x00000000;
const CE_TYPE3_OR_4: u32 = 0x80000000;
const CE_TYPE4: u32 = 0xA0000000;
const CE_TYPE_Q: u32 = 0xC0000000;
pub const IGNORE: Elem = Elem(CE_TYPE4);
const FIRST_NON_PRIMARY: u32 = 0x80000000;
const LAST_SPECIAL_PRIMARY: u32 = 0xA0000000;
const HAS_TERTIARY_MASK: u32 = 0x40000000;
const PRIMARY_VALUE_MASK: u32 = 0x3FFFFE00;
const MAX_PRIMARY_BITS: u32 = 21;
const COMPACT_PRIMARY_BITS: u32 = 16;
const MAX_SECONDARY_BITS: u32 = 12;
const MAX_TERTIARY_BITS: u32 = 8;
const MAX_CCC_BITS: u32 = 8;
const MAX_SECONDARY_COMPACT_BITS: u32 = 8;
const MAX_SECONDARY_DIFF_BITS: u32 = 4;
const MAX_TERTIARY_COMPACT_BITS: u32 = 5;
const PRIMARY_SHIFT: u32 = 9;
const COMPACT_SECONDARY_SHIFT: u32 = 5;
const MIN_COMPACT_SECONDARY: i32 = DEFAULT_SECONDARY - 4;

pub(super) fn make_implicit_ce(primary: i32) -> Elem {
    Elem(CE_TYPE1 | ((primary as u32) << PRIMARY_SHIFT) | DEFAULT_SECONDARY as u32)
}

fn gox(v: i32) -> String {
    if v < 0 {
        format!("-{:x}", -i64::from(v))
    } else {
        format!("{v:x}")
    }
}

pub fn make_elem(primary: i32, secondary: i32, tertiary: i32, ccc: u8) -> Result<Elem, String> {
    let w = primary;
    if !(0..1 << MAX_PRIMARY_BITS).contains(&w) {
        return Err(format!(
            "makeCE: primary weight out of bounds: {} >= {:x}",
            gox(w),
            1 << MAX_PRIMARY_BITS
        ));
    }
    let w = secondary;
    if !(0..1 << MAX_SECONDARY_BITS).contains(&w) {
        return Err(format!(
            "makeCE: secondary weight out of bounds: {} >= {:x}",
            gox(w),
            1 << MAX_SECONDARY_BITS
        ));
    }
    let w = tertiary;
    if !(0..1 << MAX_TERTIARY_BITS).contains(&w) {
        return Err(format!(
            "makeCE: tertiary weight out of bounds: {} >= {:x}",
            gox(w),
            1 << MAX_TERTIARY_BITS
        ));
    }
    let mut ce: u32;
    if primary != 0 {
        if ccc != 0 {
            if primary >= 1 << COMPACT_PRIMARY_BITS {
                return Err(format!(
                    "makeCE: primary weight with non-zero CCC out of bounds: {:x} >= {:x}",
                    primary,
                    1 << COMPACT_PRIMARY_BITS
                ));
            }
            if secondary != DEFAULT_SECONDARY {
                return Err(format!(
                    "makeCE: cannot combine non-default secondary value ({}) with non-zero CCC ({ccc:x})",
                    gox(secondary)
                ));
            }
            ce = (tertiary as u32) << (COMPACT_PRIMARY_BITS + MAX_CCC_BITS);
            ce |= u32::from(ccc) << COMPACT_PRIMARY_BITS;
            ce |= primary as u32;
            ce |= CE_TYPE3_OR_4;
        } else if tertiary == DEFAULT_TERTIARY {
            if secondary >= 1 << MAX_SECONDARY_COMPACT_BITS {
                return Err(format!(
                    "makeCE: secondary weight with non-zero primary out of bounds: {:x} >= {:x}",
                    secondary,
                    1 << MAX_SECONDARY_COMPACT_BITS
                ));
            }
            ce = ((primary << (MAX_SECONDARY_COMPACT_BITS + 1)) + secondary) as u32;
            ce |= CE_TYPE1;
        } else {
            let d = secondary - DEFAULT_SECONDARY + MAX_SECONDARY_DIFF_BITS as i32;
            if !(0..1 << MAX_SECONDARY_DIFF_BITS).contains(&d) {
                return Err(format!(
                    "makeCE: secondary weight diff out of bounds: {} < 0 || {} > {:x}",
                    gox(d),
                    gox(d),
                    1 << MAX_SECONDARY_DIFF_BITS
                ));
            }
            if tertiary >= 1 << MAX_TERTIARY_COMPACT_BITS {
                return Err(format!(
                    "makeCE: tertiary weight with non-zero primary out of bounds: {:x} > {:x}",
                    tertiary,
                    1 << MAX_TERTIARY_COMPACT_BITS
                ));
            }
            ce = ((primary << MAX_SECONDARY_DIFF_BITS) + d) as u32;
            ce = (ce << MAX_TERTIARY_COMPACT_BITS) + tertiary as u32;
        }
    } else {
        ce = ((secondary << MAX_TERTIARY_BITS) + tertiary) as u32;
        ce = ce.wrapping_add(u32::from(ccc) << (MAX_SECONDARY_BITS + MAX_TERTIARY_BITS));
        ce |= CE_TYPE4;
    }
    Ok(Elem(ce))
}

pub fn make_quaternary(v: i32) -> Elem {
    Elem(CE_TYPE_Q | ((v as u32) << PRIMARY_SHIFT))
}

impl Elem {
    pub(super) fn ctype(self) -> CeType {
        let ce = self.0;
        if ce <= MAX_CE {
            return CeType::Normal;
        }
        if ce <= MAX_CONTRACT {
            CeType::ContractionIndex
        } else if ce <= MAX_EXPAND {
            CeType::ExpansionIndex
        } else {
            CeType::Decompose
        }
    }

    pub fn mask(self, _l: Level) -> u32 {
        0
    }

    pub fn ccc(self) -> u8 {
        let ce = self.0;
        if ce & CE_TYPE3_OR_4 != 0 {
            if ce & CE_TYPE4 == CE_TYPE3_OR_4 {
                return (ce >> 16) as u8;
            }
            return (ce >> 20) as u8;
        }
        0
    }

    pub fn primary(self) -> i32 {
        let ce = self.0;
        if ce >= FIRST_NON_PRIMARY {
            if ce > LAST_SPECIAL_PRIMARY {
                return 0;
            }
            return i32::from(ce as u16);
        }
        ((ce & PRIMARY_VALUE_MASK) >> PRIMARY_SHIFT) as i32
    }

    pub fn secondary(self) -> i32 {
        let ce = self.0;
        match ce & CE_TYPE_MASK {
            CE_TYPE1 => i32::from(ce as u8),
            CE_TYPE2 => MIN_COMPACT_SECONDARY + ((ce >> COMPACT_SECONDARY_SHIFT) & 0xF) as i32,
            CE_TYPE3_OR_4 => {
                if ce < CE_TYPE4 {
                    return DEFAULT_SECONDARY;
                }
                ((ce >> 8) & 0xFFF) as i32
            }
            _ => 0,
        }
    }

    pub fn tertiary(self) -> u8 {
        let ce = self.0;
        if ce & HAS_TERTIARY_MASK == 0 {
            if ce & CE_TYPE3_OR_4 == 0 {
                return (ce & 0x1F) as u8;
            }
            if ce & CE_TYPE4 == CE_TYPE4 {
                return ce as u8;
            }
            return ((ce >> 24) as u8) & 0x1F;
        } else if ce & CE_TYPE_MASK == CE_TYPE1 {
            return DEFAULT_TERTIARY as u8;
        }
        0
    }

    pub(super) fn update_tertiary(self, t: u8) -> Elem {
        let mut ce = self.0;
        if ce & CE_TYPE_MASK == CE_TYPE1 {
            let mut nce = ce & PRIMARY_VALUE_MASK;
            nce |= u32::from((ce as u8).wrapping_sub(MIN_COMPACT_SECONDARY as u8))
                << COMPACT_SECONDARY_SHIFT;
            ce = nce;
        } else if ce & CE_TYPE_MASK_EXT == CE_TYPE3_OR_4 {
            ce &= !(u32::from(MAX_TERTIARY) << 24);
            return Elem(ce | (u32::from(t) << 24));
        } else {
            ce &= !u32::from(MAX_TERTIARY);
        }
        Elem(ce | u32::from(t))
    }

    pub fn quaternary(self) -> i32 {
        let ce = self.0;
        if ce & CE_TYPE_MASK == CE_TYPE_Q {
            return ((ce & PRIMARY_VALUE_MASK) >> PRIMARY_SHIFT) as i32;
        } else if ce & CE_IGNORE_MASK == IGNORE.0 {
            return 0;
        }
        MAX_QUATERNARY
    }

    pub fn weight(self, l: Level) -> i32 {
        match l {
            Level::Primary => self.primary(),
            Level::Secondary => self.secondary(),
            Level::Tertiary => i32::from(self.tertiary()),
            Level::Quaternary => self.quaternary(),
            Level::Identity => 0,
        }
    }
}

const MAX_N_BITS: u32 = 4;
const MAX_TRIE_INDEX_BITS: u32 = 12;
const MAX_CONTRACT_OFFSET_BITS: u32 = 13;

pub(super) fn split_contract_index(ce: Elem) -> (usize, usize, usize) {
    let mut ce = ce.0;
    let n = (ce & ((1 << MAX_N_BITS) - 1)) as usize;
    ce >>= MAX_N_BITS;
    let index = (ce & ((1 << MAX_TRIE_INDEX_BITS) - 1)) as usize;
    ce >>= MAX_TRIE_INDEX_BITS;
    let offset = (ce & ((1 << MAX_CONTRACT_OFFSET_BITS) - 1)) as usize;
    (index, n, offset)
}

pub(super) fn split_expand_index(ce: Elem) -> usize {
    usize::from(ce.0 as u16)
}

pub(super) fn split_decompose(ce: Elem) -> (u8, u8) {
    (ce.0 as u8, (ce.0 >> 8) as u8)
}

const MIN_UNIFIED: Rune = 0x4E00;
const MAX_UNIFIED: Rune = 0x9FFF;
const MIN_COMPATIBILITY: Rune = 0xF900;
const MAX_COMPATIBILITY: Rune = 0xFAFF;
const COMMON_UNIFIED_OFFSET: i32 = 0x10000;
const RARE_UNIFIED_OFFSET: i32 = 0x20000;
const OTHER_OFFSET: i32 = 0x50000;

pub(super) fn implicit_primary(r: Rune) -> i32 {
    if unicode::is(&unicode::IDEOGRAPHIC, r) {
        if (MIN_UNIFIED..=MAX_UNIFIED).contains(&r) {
            return r + COMMON_UNIFIED_OFFSET;
        }
        if (MIN_COMPATIBILITY..=MAX_COMPATIBILITY).contains(&r) {
            return r + COMMON_UNIFIED_OFFSET;
        }
        return r + RARE_UNIFIED_OFFSET;
    }
    r + OTHER_OFFSET
}
