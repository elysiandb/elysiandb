use elysiandb::hash::xxh64;

use super::common;
use super::support::{array, dec_u64, hex_bytes, uint};

#[test]
fn xxh64_matches_go() {
    let doc = common::read_json("persistence/primitives/xxh64.json");

    let cases = array(&doc, "cases");
    let mut checked = 0;
    for row in cases {
        let input = hex_bytes(row, "hex");
        let sum = xxh64::sum64(&input);
        assert_eq!(sum, dec_u64(row, "sum"), "{row:?}");
        assert_eq!(sum & 63, uint(row, "shard64"), "{row:?}");
        assert_eq!(sum & 7, uint(row, "shard8"), "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, cases.len());
    assert_eq!(checked, 307);

    assert_eq!(xxh64::sum64(b""), 0xef46_db37_51d8_e999);
    assert_eq!(xxh64::sum64(b"a"), 0xd24e_c4f1_a98c_6e5b);
    assert_eq!(xxh64::sum64(b"abc"), 0x44bc_2cf5_ad77_0999);
    assert_eq!(xxh64::sum64(b"session:1"), 0xa914_1db0_2cae_47f9);
}
