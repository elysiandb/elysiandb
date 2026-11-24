mod common;

use common::{hex, read_jsonl, str_field, unhex};
use elysiandb::crypto::sha256;

const ROWS: usize = 9;
const REPEAT_LEN: usize = 1 << 20;

fn repeated() -> Vec<u8> {
    (0..REPEAT_LEN)
        .map(|i| ((i * 31 + 7) & 0xff) as u8)
        .collect()
}

#[test]
fn sha256_sums_match_go() {
    let rel = "security/sha256.jsonl";
    let mut checked = 0;
    for row in &read_jsonl(rel) {
        let input = match row.get("input_hex").and_then(|v| v.as_str()) {
            Some(h) => unhex(h),
            None => {
                assert_eq!(
                    str_field(row, "repeat"),
                    "1 MiB, byte i = (i*31+7)&0xff",
                    "{row:?}"
                );
                repeated()
            }
        };
        let want = str_field(row, "sum_hex");

        assert_eq!(hex(&sha256::sum256(&input)), want, "{row:?}");

        let mut d = sha256::Digest::new();
        for piece in input.chunks(7) {
            d.write(piece);
        }
        assert_eq!(hex(&d.sum()), want, "streamed {row:?}");

        let mut d = sha256::Digest::new();
        let (head, tail) = input.split_at(input.len() / 2);
        d.write(head);
        let partial = d.sum();
        d.write(tail);
        assert_eq!(hex(&d.sum()), want, "summed twice {row:?}");
        assert_eq!(partial, sha256::sum256(head), "partial {row:?}");

        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
