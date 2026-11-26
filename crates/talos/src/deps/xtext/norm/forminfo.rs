use std::collections::HashMap;
use std::sync::OnceLock;

use gocompat::utf8::Rune;

use super::iter::IterFn;
use super::normalize::Form;
use super::tables::{
    CCC, DECOMPS, END_MULTI, FIRST_CCC, FIRST_CCC_ZERO_EXCEPT, FIRST_LEADING_CCC, FIRST_MULTI,
    FIRST_STARTER_WITH_N_LEAD, RECOMP_MAP_PACKED,
};
use super::trie::{NFC_DATA, NFKC_DATA};

const QC_INFO_MASK: u8 = 0x3F;
const HEADER_LEN_MASK: u8 = 0x3F;
const HEADER_FLAGS_MASK: u8 = 0xC0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    pub(super) pos: u8,
    pub(super) size: u8,
    pub(super) ccc: u8,
    pub(super) tccc: u8,
    pub(super) n_lead: u8,
    pub(super) flags: u8,
    pub(super) index: u16,
}

pub(super) type LookupFunc = fn(&[u8], usize) -> Properties;

pub(super) struct FormInfo {
    pub(super) composing: bool,
    pub(super) info: LookupFunc,
    pub(super) next_main: IterFn,
}

pub(super) static FORM_TABLE: [FormInfo; 4] = [
    FormInfo {
        composing: true,
        info: lookup_info_nfc,
        next_main: IterFn::Composed,
    },
    FormInfo {
        composing: false,
        info: lookup_info_nfc,
        next_main: IterFn::Decomposed,
    },
    FormInfo {
        composing: true,
        info: lookup_info_nfkc,
        next_main: IterFn::Composed,
    },
    FormInfo {
        composing: false,
        info: lookup_info_nfkc,
        next_main: IterFn::Decomposed,
    },
];

impl FormInfo {
    #[inline]
    pub(super) fn info(&self, src: &[u8], i: usize) -> Properties {
        (self.info)(src, i)
    }
}

impl Properties {
    pub fn boundary_before(&self) -> bool {
        self.ccc == 0 && !self.combines_backward()
    }

    pub fn boundary_after(&self) -> bool {
        self.is_inert()
    }

    pub(super) fn is_yes_c(&self) -> bool {
        self.flags & 0x10 == 0
    }

    pub(super) fn is_yes_d(&self) -> bool {
        self.flags & 0x4 == 0
    }

    #[cfg(test)]
    pub(super) fn combines_forward(&self) -> bool {
        self.flags & 0x20 != 0
    }

    pub(super) fn combines_backward(&self) -> bool {
        self.flags & 0x8 != 0
    }

    pub(super) fn has_decomposition(&self) -> bool {
        self.flags & 0x4 != 0
    }

    pub(super) fn is_inert(&self) -> bool {
        self.flags & QC_INFO_MASK == 0 && self.ccc == 0
    }

    pub(super) fn multi_segment(&self) -> bool {
        self.index >= FIRST_MULTI && self.index < END_MULTI
    }

    pub(super) fn n_leading_non_starters(&self) -> u8 {
        self.n_lead
    }

    pub(super) fn n_trailing_non_starters(&self) -> u8 {
        self.flags & 0x03
    }

    pub fn decomposition(&self) -> Option<&'static [u8]> {
        if self.index == 0 {
            return None;
        }
        let mut i = usize::from(self.index);
        let n = usize::from(DECOMPS[i] & HEADER_LEN_MASK);
        i += 1;
        Some(&DECOMPS[i..i + n])
    }

    pub fn size(&self) -> usize {
        usize::from(self.size)
    }

    pub fn ccc(&self) -> u8 {
        if self.index >= FIRST_CCC_ZERO_EXCEPT {
            return 0;
        }
        CCC[usize::from(self.ccc)]
    }

    pub fn lead_ccc(&self) -> u8 {
        CCC[usize::from(self.ccc)]
    }

    pub fn trail_ccc(&self) -> u8 {
        CCC[usize::from(self.tccc)]
    }
}

fn build_recomp_map() -> HashMap<u32, Rune> {
    let mut m = HashMap::with_capacity(RECOMP_MAP_PACKED.len() / 8);
    let mut i = 0;
    while i < RECOMP_MAP_PACKED.len() {
        let b = &RECOMP_MAP_PACKED[i..i + 8];
        let key = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let val = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        m.insert(key, val as Rune);
        i += 8;
    }
    m
}

static RECOMP_MAP: OnceLock<HashMap<u32, Rune>> = OnceLock::new();

pub(super) fn recomp_map() -> &'static HashMap<u32, Rune> {
    RECOMP_MAP.get_or_init(build_recomp_map)
}

pub(super) fn combine(map: &HashMap<u32, Rune>, a: Rune, b: Rune) -> Rune {
    let key = (u32::from(a as u16) << 16).wrapping_add(u32::from(b as u16));
    map.get(&key).copied().unwrap_or(0)
}

pub(super) fn lookup_info_nfc(b: &[u8], i: usize) -> Properties {
    let (v, sz) = NFC_DATA.lookup(&b[i..]);
    comp_info(v, sz)
}

pub(super) fn lookup_info_nfkc(b: &[u8], i: usize) -> Properties {
    let (v, sz) = NFKC_DATA.lookup(&b[i..]);
    comp_info(v, sz)
}

impl Form {
    pub fn properties(&self, s: &[u8]) -> Properties {
        if *self == Form::Nfc || *self == Form::Nfd {
            let (v, sz) = NFC_DATA.lookup(s);
            return comp_info(v, sz);
        }
        let (v, sz) = NFKC_DATA.lookup(s);
        comp_info(v, sz)
    }

    pub fn properties_string(&self, s: &[u8]) -> Properties {
        self.properties(s)
    }
}

pub(super) fn comp_info(v: u16, sz: usize) -> Properties {
    if v == 0 {
        return Properties {
            size: sz as u8,
            ..Properties::default()
        };
    } else if v >= 0x8000 {
        let mut p = Properties {
            size: sz as u8,
            ccc: v as u8,
            tccc: v as u8,
            flags: (v >> 8) as u8,
            ..Properties::default()
        };
        if p.ccc > 0 || p.combines_backward() {
            p.n_lead = p.flags & 0x3;
        }
        return p;
    }
    let mut v = v;
    let h = DECOMPS[usize::from(v)];
    let f = ((h & HEADER_FLAGS_MASK) >> 2) | 0x4;
    let mut p = Properties {
        size: sz as u8,
        flags: f,
        index: v,
        ..Properties::default()
    };
    if v >= FIRST_CCC {
        v += u16::from(h & HEADER_LEN_MASK) + 1;
        let c = DECOMPS[usize::from(v)];
        p.tccc = c >> 2;
        p.flags |= c & 0x3;
        if v >= FIRST_LEADING_CCC {
            p.n_lead = c & 0x3;
            if v >= FIRST_STARTER_WITH_N_LEAD {
                p.flags &= 0x03;
                p.index = 0;
                return p;
            }
            p.ccc = DECOMPS[usize::from(v) + 1];
        }
    }
    p
}
