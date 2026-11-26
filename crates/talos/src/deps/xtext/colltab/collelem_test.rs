use super::collelem::{
    CeType, DEFAULT_SECONDARY, DEFAULT_TERTIARY, Elem, implicit_primary, make_elem,
    split_contract_index, split_decompose, split_expand_index,
};

const MAX_N_BITS: i32 = 4;
const MAX_TRIE_INDEX_BITS: i32 = 12;
const MAX_CONTRACT_OFFSET_BITS: i32 = 13;
const MAX_EXPAND_INDEX_BITS: i32 = 16;

pub(super) fn make_ce(weights: &[i32]) -> Elem {
    make_elem(weights[0], weights[1], weights[2], weights[3] as u8).unwrap_or_default()
}

fn make_contract_index(index: i32, n: i32, offset: i32) -> Elem {
    let mut ce: u32 = 0xC0000000;
    ce = ce.wrapping_add((offset << (MAX_N_BITS + MAX_TRIE_INDEX_BITS)) as u32);
    ce = ce.wrapping_add((index << MAX_N_BITS) as u32);
    ce = ce.wrapping_add(n as u32);
    Elem(ce)
}

fn make_expand_index(index: i32) -> Elem {
    Elem(0xE0000000u32.wrapping_add(index as u32))
}

fn make_decompose(t1: i32, t2: i32) -> Elem {
    Elem((((t2 << 8) + t1) as u32).wrapping_add(0xF0000000))
}

fn normal_ce(inout: &mut [i32]) -> (Elem, CeType) {
    let ce = make_ce(inout);
    inout[0] = ce.primary();
    inout[1] = ce.secondary();
    inout[2] = i32::from(ce.tertiary());
    inout[3] = i32::from(ce.ccc());
    (ce, CeType::Normal)
}

fn expand_ce(inout: &mut [i32]) -> (Elem, CeType) {
    let ce = make_expand_index(inout[0]);
    inout[0] = split_expand_index(ce) as i32;
    (ce, CeType::ExpansionIndex)
}

fn contract_ce(inout: &mut [i32]) -> (Elem, CeType) {
    let ce = make_contract_index(inout[0], inout[1], inout[2]);
    let (i, n, o) = split_contract_index(ce);
    inout[0] = i as i32;
    inout[1] = n as i32;
    inout[2] = o as i32;
    (ce, CeType::ContractionIndex)
}

fn decomp_ce(inout: &mut [i32]) -> (Elem, CeType) {
    let ce = make_decompose(inout[0], inout[1]);
    let (t1, t2) = split_decompose(ce);
    inout[0] = i32::from(t1);
    inout[1] = i32::from(t2);
    (ce, CeType::Decompose)
}

type CeFunc = fn(&mut [i32]) -> (Elem, CeType);

#[test]
fn test_col_elem() {
    let ce_tests: Vec<(CeFunc, Vec<i32>)> = vec![
        (normal_ce, vec![0, 0, 0, 0]),
        (normal_ce, vec![0, 30, 3, 0]),
        (normal_ce, vec![0, 30, 3, 0xFF]),
        (normal_ce, vec![100, DEFAULT_SECONDARY, DEFAULT_TERTIARY, 0]),
        (
            normal_ce,
            vec![100, DEFAULT_SECONDARY, DEFAULT_TERTIARY, 0xFF],
        ),
        (normal_ce, vec![100, DEFAULT_SECONDARY, 3, 0]),
        (normal_ce, vec![0x123, DEFAULT_SECONDARY, 8, 0xFF]),
        (contract_ce, vec![0, 0, 0]),
        (contract_ce, vec![1, 1, 1]),
        (contract_ce, vec![1, (1 << MAX_N_BITS) - 1, 1]),
        (contract_ce, vec![(1 << MAX_TRIE_INDEX_BITS) - 1, 1, 1]),
        (contract_ce, vec![1, 1, (1 << MAX_CONTRACT_OFFSET_BITS) - 1]),
        (expand_ce, vec![0]),
        (expand_ce, vec![5]),
        (expand_ce, vec![(1 << MAX_EXPAND_INDEX_BITS) - 1]),
        (decomp_ce, vec![0, 0]),
        (decomp_ce, vec![1, 1]),
        (decomp_ce, vec![0x1F, 0x1F]),
    ];
    for (i, (f, arg)) in ce_tests.iter().enumerate() {
        let mut inout = arg.clone();
        let (ce, typ) = f(&mut inout);
        assert_eq!(ce.ctype(), typ, "{i}: type (ColElem: {:X})", ce.0);
        for (j, a) in arg.iter().enumerate() {
            assert_eq!(inout[j], *a, "{i}: argument {j} (ColElem: {:X})", ce.0);
        }
    }
}

#[test]
fn test_implicit() {
    let implicit_tests: [(i32, i32); 15] = [
        (0x33FF, 0x533FF),
        (0x3400, 0x23400),
        (0x4DC0, 0x54DC0),
        (0x4DFF, 0x54DFF),
        (0x4E00, 0x14E00),
        (0x9FCB, 0x19FCB),
        (0xA000, 0x5A000),
        (0xF8FF, 0x5F8FF),
        (0xF900, 0x1F900),
        (0xFA23, 0x1FA23),
        (0xFAD9, 0x1FAD9),
        (0xFB00, 0x5FB00),
        (0x20000, 0x40000),
        (0x2B81C, 0x4B81C),
        (0x10FFFF, 0x15FFFF),
    ];
    for (r, p) in implicit_tests {
        assert_eq!(implicit_primary(r), p, "U+{r:04X}");
    }
}

#[test]
fn test_update_tertiary() {
    let tests: [(u32, u32, u8); 5] = [
        (0x4000FE20, 0x0000FE8A, 0x0A),
        (0x4000FE21, 0x0000FEAA, 0x0A),
        (0x0000FE8B, 0x0000FE83, 0x03),
        (0x82FF0188, 0x9BFF0188, 0x1B),
        (0xAFF0CC02, 0xAFF0CC1B, 0x1B),
    ];
    for (i, (input, out, t)) in tests.iter().enumerate() {
        assert_eq!(Elem(*input).update_tertiary(*t), Elem(*out), "{i}");
    }
}
