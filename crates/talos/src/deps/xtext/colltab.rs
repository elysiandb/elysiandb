mod collelem;
#[cfg(test)]
mod collelem_test;
mod contract;
#[cfg(test)]
mod contract_test;
mod iter;
#[cfg(test)]
mod iter_test;
mod table;
mod trie;
#[cfg(test)]
mod trie_test;
mod weighter;

pub use collelem::{
    Elem, IGNORE, Level, MAX_QUATERNARY, MAX_TERTIARY, NUM_LEVELS, PRIVATE_USE, make_elem,
    make_quaternary,
};
pub use contract::ContractTrieSet;
pub use iter::Iter;
pub use table::Table;
pub use trie::Trie;
pub use weighter::Weighter;

use super::language::Tag;

pub fn match_lang(_t: Tag, _tags: &[Tag]) -> usize {
    0
}

#[inline]
pub(crate) fn le_u16(t: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([t[2 * i], t[2 * i + 1]])
}

#[inline]
pub(crate) fn le_u32(t: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([t[4 * i], t[4 * i + 1], t[4 * i + 2], t[4 * i + 3]])
}
