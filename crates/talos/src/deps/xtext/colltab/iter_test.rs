use super::collelem::DEFAULT_SECONDARY;
use super::collelem_test::make_ce;
use super::contract::ContractTrieSet;
use super::iter::Iter;
use super::table::Table;
use super::trie::Trie;

static EMPTY: Table<'static> = Table {
    index: Trie {
        index0: &[],
        values0: &[],
        index: &[],
        values: &[],
    },
    expand_elem: &[],
    contract_tries: ContractTrieSet(&[]),
    contract_elem: &[],
    max_contract_len: 0,
    variable_top: 0,
};

#[test]
fn test_do_norm() {
    const DIV: i32 = -1;
    let tests: [(&[i32], &[i32]); 6] = [
        (&[4, DIV, 3], &[3, 4]),
        (&[4, DIV, 3, 3, 3], &[3, 3, 3, 4]),
        (&[0, 4, DIV, 3], &[0, 3, 4]),
        (&[0, 0, 4, 5, DIV, 3, 3], &[0, 0, 3, 3, 4, 5]),
        (&[0, 0, 1, 4, 5, DIV, 3, 3], &[0, 0, 1, 3, 3, 4, 5]),
        (&[0, 0, 1, 4, 5, DIV, 4, 4], &[0, 0, 1, 4, 4, 4, 5]),
    ];
    for (j, (input, out)) in tests.iter().enumerate() {
        let mut i = Iter::new(&EMPTY, Vec::new());
        let mut w = 0;
        let mut p = 0;
        for (k, &cc) in input.iter().enumerate() {
            if cc == DIV {
                w = 100;
                p = k;
                continue;
            }
            i.elems.push(make_ce(&[w, DEFAULT_SECONDARY, 2, cc]));
        }
        let ccc = i.elems[p].ccc();
        i.do_norm(p, ccc);
        assert_eq!(i.elems.len(), out.len(), "{j}: length");
        let prev_ccc = 0u8;
        for (k, ce) in i.elems.iter().enumerate() {
            assert_eq!(i32::from(ce.ccc()), out[k], "{j}:{k}: unexpected CCC");
            if k > 0 && ce.ccc() == prev_ccc {
                assert!(
                    i.elems[k - 1].primary() <= ce.primary(),
                    "{j}:{k}: normalization crossed across CCC boundary."
                );
            }
        }
    }
}
