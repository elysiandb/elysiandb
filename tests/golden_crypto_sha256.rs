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

#[test]
fn sha256_hardware_and_software_paths_agree() {
    let mut r =
        elysiandb::gocompat::rand::Rand::new(elysiandb::gocompat::rand::new_source(20261125));
    let mut data = vec![0u8; 4096 + 129];
    for b in data.iter_mut() {
        *b = r.intn(256) as u8;
    }

    for len in (0..=1100).chain([2047, 2048, 2049, 4096, 4096 + 129]) {
        let input = &data[..len];
        let want = sha256::sum256_software(input);
        assert_eq!(sha256::sum256(input), want, "length {len}");

        let mut d = sha256::Digest::new();
        let mut rest = input;
        let mut step = 1;
        while !rest.is_empty() {
            let n = step.min(rest.len());
            d.write(&rest[..n]);
            rest = &rest[n..];
            step = step * 3 % 97 + 1;
        }
        assert_eq!(d.sum(), want, "streamed length {len}");
    }

    println!("hardware path: {}", sha256::hardware());
}
