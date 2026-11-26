use super::tables::{
    LOCALES, MAIN_CONTRACT_ELEM, MAIN_CT_ENTRIES, MAIN_EXPAND_ELEM, MAIN_LOOKUP, MAIN_VALUES,
    VAR_TOP,
};
use crate::deps::xtext::colltab::{ContractTrieSet, Table, Trie, le_u32};

const BLOCK_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TableIndex {
    pub(super) lookup_offset: u32,
    pub(super) values_offset: u32,
}

pub(super) fn locale(i: usize) -> TableIndex {
    TableIndex {
        lookup_offset: le_u32(LOCALES, 2 * i),
        values_offset: le_u32(LOCALES, 2 * i + 1),
    }
}

pub(super) fn get_table(t: TableIndex) -> Table<'static> {
    Table {
        index: Trie {
            index0: &MAIN_LOOKUP[2 * BLOCK_SIZE * t.lookup_offset as usize..],
            values0: &MAIN_VALUES[4 * BLOCK_SIZE * t.values_offset as usize..],
            index: MAIN_LOOKUP,
            values: MAIN_VALUES,
        },
        expand_elem: MAIN_EXPAND_ELEM,
        contract_tries: ContractTrieSet(MAIN_CT_ENTRIES),
        contract_elem: MAIN_CONTRACT_ELEM,
        max_contract_len: 18,
        variable_top: VAR_TOP,
    }
}
