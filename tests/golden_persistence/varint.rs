use elysiandb::gocompat::binary;

use super::common;
use super::support::{array, dec_i64, dec_u64, field, hex_bytes, int, string};

#[test]
fn varints_match_go() {
    let doc = common::read_json("persistence/primitives/varint.json");

    let uvarints = array(&doc, "uvarint");
    let mut checked = 0;
    for row in uvarints {
        let mut out = Vec::new();
        binary::append_uvarint(&mut out, dec_u64(row, "value"));
        assert_eq!(common::hex(&out), string(row, "hex"), "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, uvarints.len());
    assert_eq!(checked, 13);

    let varints = array(&doc, "varint");
    let mut checked = 0;
    for row in varints {
        let mut out = Vec::new();
        binary::append_varint(&mut out, dec_i64(row, "value"));
        assert_eq!(common::hex(&out), string(row, "hex"), "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, varints.len());
    assert_eq!(checked, 11);

    let decodes = array(&doc, "decode");
    let mut checked = 0;
    for row in decodes {
        let input = hex_bytes(row, "hex");
        let n = int(row, "n");
        if field(row, "signed").as_bool().expect("signed is a boolean") {
            let (v, m) = binary::varint(&input);
            assert_eq!((v, m as i64), (dec_i64(row, "value"), n), "{row:?}");
        } else {
            let (v, m) = binary::uvarint(&input);
            assert_eq!((v, m as i64), (dec_u64(row, "value"), n), "{row:?}");
        }
        checked += 1;
    }
    assert_eq!(checked, decodes.len());
    assert_eq!(checked, 35);
}
