use gocompat::utf8;

use super::trie::Trie;

const BLOCK_SIZE: usize = 64;

const TX: u8 = 0x80;
const T2: u8 = 0xC0;
const T3: u8 = 0xE0;
const T4: u8 = 0xF0;
const T5: u8 = 0xF8;
const T6: u8 = 0xFC;

const TEST_RUNES: [i32; 17] = [
    0x01, 0x0C, 0x7F, 0x80, 0x100, 0x7FF, 0x800, 0x999, 0xFFFF, 0x10000, 0x10101, 0x10FFFF, 0x200,
    0x201, 0x202, 0x210, 0x215,
];

fn test_values() -> Vec<u8> {
    let mut v = [0u32; 832];
    for (i, x) in [
        (0x000c, 0x01),
        (0x007f, 0x02),
        (0x00c0, 0x03),
        (0x0100, 0x04),
        (0x0140, 0x0c),
        (0x0141, 0x0d),
        (0x0142, 0x0e),
        (0x0150, 0x0f),
        (0x0155, 0x10),
        (0x01bf, 0x05),
        (0x01c0, 0x06),
        (0x0219, 0x07),
        (0x027f, 0x08),
        (0x0280, 0x09),
        (0x02c1, 0x0a),
        (0x033f, 0x0b),
    ] {
        v[i] = x;
    }
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn test_lookup() -> Vec<u8> {
    let mut v = [0u16; 640];
    for (i, x) in [
        (0x0e0, 0x05),
        (0x0e6, 0x06),
        (0x13f, 0x07),
        (0x140, 0x08),
        (0x144, 0x09),
        (0x190, 0x03),
        (0x1ff, 0x0a),
        (0x20f, 0x05),
        (0x242, 0x01),
        (0x244, 0x02),
        (0x248, 0x03),
        (0x25f, 0x04),
        (0x260, 0x01),
        (0x26f, 0x02),
        (0x270, 0x04),
        (0x274, 0x06),
    ] {
        v[i] = x;
    }
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[test]
fn test_lookup_trie() {
    let values = test_values();
    let lookup = test_lookup();
    let test_trie = Trie {
        index0: &lookup[2 * 6 * BLOCK_SIZE..],
        values0: &values,
        index: &lookup,
        values: &values,
    };
    for (i, &r) in TEST_RUNES.iter().enumerate() {
        let mut b = Vec::new();
        utf8::append_rune(&mut b, r);
        let (v, sz) = test_trie.lookup(&b);
        assert_eq!(v.0 as usize, i, "lookup(U+{r:04X}): value");
        assert_eq!(sz, b.len(), "lookup(U+{r:04X}): size");
    }
    let tests: [(usize, &[u8]); 16] = [
        (1, &[0x80]),
        (1, &[0xFF]),
        (1, &[T2, TX - 1]),
        (1, &[T2, T2]),
        (2, &[T3, TX, TX - 1]),
        (2, &[T3, TX, T2]),
        (1, &[T3, TX - 1, TX]),
        (3, &[T4, TX, TX, TX - 1]),
        (3, &[T4, TX, TX, T2]),
        (1, &[T4, T2, TX, TX - 1]),
        (2, &[T4, TX, T2, TX - 1]),
        (0, &[T2]),
        (0, &[T3, TX]),
        (0, &[T4, TX, TX]),
        (1, &[T5, TX, TX, TX, TX]),
        (1, &[T6, TX, TX, TX, TX, TX]),
    ];
    for (i, (size, bytes)) in tests.iter().enumerate() {
        let (v, sz) = test_trie.lookup(bytes);
        assert_eq!(v.0, 0, "lookup of illegal rune, case {i}: value");
        assert_eq!(sz, *size, "lookup of illegal rune, case {i}: size");
    }
}
