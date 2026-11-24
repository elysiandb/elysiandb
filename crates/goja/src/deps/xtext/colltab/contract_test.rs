use super::contract::ContractTrieSet;

struct LookupTest {
    lookup: &'static [(&'static str, usize, usize)],
    n: usize,
    tries: &'static [[u8; 4]],
}

static LOOKUP_TESTS: [LookupTest; 5] = [
    LookupTest {
        lookup: &[
            ("abc", 1, 3),
            ("a", 0, 0),
            ("b", 0, 0),
            ("c", 0, 0),
            ("d", 0, 0),
        ],
        n: 1,
        tries: &[[b'a', 0, 1, 0xFF], [b'b', 0, 1, 0xFF], [b'c', b'c', 0, 1]],
    },
    LookupTest {
        lookup: &[
            ("abc", 1, 3),
            ("abd", 2, 3),
            ("abe", 3, 3),
            ("a", 0, 0),
            ("ab", 0, 0),
            ("d", 0, 0),
            ("f", 0, 0),
        ],
        n: 1,
        tries: &[[b'a', 0, 1, 0xFF], [b'b', 0, 1, 0xFF], [b'c', b'e', 0, 1]],
    },
    LookupTest {
        lookup: &[
            ("abc", 1, 3),
            ("ab", 2, 2),
            ("a", 3, 1),
            ("abcd", 1, 3),
            ("abe", 2, 2),
        ],
        n: 1,
        tries: &[[b'a', 0, 1, 3], [b'b', 0, 1, 2], [b'c', b'c', 0, 1]],
    },
    LookupTest {
        lookup: &[
            ("abc", 1, 3),
            ("abd", 2, 3),
            ("ab", 3, 2),
            ("ac", 4, 2),
            ("a", 5, 1),
            ("b", 6, 1),
            ("ba", 6, 1),
        ],
        n: 2,
        tries: &[
            [b'b', b'b', 0, 6],
            [b'a', 0, 2, 5],
            [b'c', b'c', 0, 4],
            [b'b', 0, 1, 3],
            [b'c', b'd', 0, 1],
        ],
    },
    LookupTest {
        lookup: &[
            ("bcde", 2, 4),
            ("bc", 7, 2),
            ("ab", 6, 2),
            ("bcd", 5, 3),
            ("abcd", 1, 4),
            ("abc", 4, 3),
            ("bcdf", 3, 4),
        ],
        n: 2,
        tries: &[
            [b'b', 3, 1, 0xFF],
            [b'a', 0, 1, 0xFF],
            [b'b', 0, 1, 6],
            [b'c', 0, 1, 4],
            [b'd', b'd', 0, 1],
            [b'c', 0, 1, 7],
            [b'd', 0, 1, 5],
            [b'e', b'f', 0, 2],
        ],
    },
];

fn lookup(c: &ContractTrieSet<'_>, nnode: usize, s: &[u8]) -> (usize, usize) {
    let mut scan = c.scanner(0, nnode, s);
    scan.scan(0);
    scan.result()
}

#[test]
fn test_lookup_contraction() {
    for (i, tt) in LOOKUP_TESTS.iter().enumerate() {
        let bytes: Vec<u8> = tt.tries.concat();
        let cts = ContractTrieSet(&bytes);
        for (j, lu) in tt.lookup.iter().enumerate() {
            let str = lu.0;
            for s in [str.to_string(), format!("{str}X")] {
                let (offset, n) = lookup(&cts, tt.n, s.as_bytes());
                assert_eq!(offset, lu.1, "{i}:{j}: offset of {s:?}");
                assert_eq!(n, lu.2, "{i}:{j}: bytes consumed of {s:?}");
            }
        }
    }
}
