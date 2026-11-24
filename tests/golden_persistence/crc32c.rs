use elysiandb::hash::crc32c;

use super::common;
use super::support::{array, field, formula_bytes, string, uint};

const FORMULA: &str = "(i*31+7)&0xff";

#[test]
fn crc32c_matches_go() {
    let doc = common::read_json("persistence/primitives/crc32c.json");

    let cases = array(&doc, "cases");
    let mut checked = 0;
    for row in cases {
        let len = uint(row, "len") as usize;
        let input = if row.get("formula").is_some() {
            assert_eq!(string(row, "formula"), FORMULA);
            formula_bytes(len, 0)
        } else {
            string(row, "input").as_bytes().to_vec()
        };
        assert_eq!(input.len(), len);
        let want = uint(row, "crc") as u32;
        assert_eq!(crc32c::checksum(&input), want, "{row:?}");
        assert_eq!(crc32c::update(0, &input), want, "{row:?}");
        let mut d = crc32c::new();
        for b in &input {
            d.write(std::slice::from_ref(b));
        }
        assert_eq!(d.sum32(), want, "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, cases.len());
    assert_eq!(checked, 71);
    assert_eq!(crc32c::checksum(b""), 0);
    assert_eq!(crc32c::checksum(b"123456789"), 0xe306_9283);

    let inc = field(&doc, "incremental");
    assert_eq!(string(inc, "formula"), FORMULA);
    let data = formula_bytes(uint(inc, "len") as usize, 0);
    let mut d = crc32c::new();
    let slices = array(inc, "slices");
    for s in slices {
        let from = s.index(0).and_then(|v| v.as_u64()).expect("slice start") as usize;
        let to = s.index(1).and_then(|v| v.as_u64()).expect("slice end") as usize;
        d.write(&data[from..to]);
    }
    assert_eq!(slices.len(), 3);
    assert_eq!(d.sum32(), uint(inc, "crc") as u32);
    assert_eq!(
        crc32c::update(crc32c::checksum(&data[..1000]), &data[1000..]),
        uint(inc, "update") as u32
    );
}
