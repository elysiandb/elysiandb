use crate::assert_manifest_entry;
use crate::common::{read_jsonl, str_field, unhex};
use elysiandb::crypto::bcrypt::{self, Error};
use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::Value;

const ROWS: usize = 88;
const ZERO_FIRST: usize = 8;
const ZERO_LATER: usize = 8;

fn golden_key() -> Vec<u8> {
    b"ab".repeat(512)
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut d = sha256::Digest::new();
    for p in parts {
        d.write(p);
    }
    d.sum()
}

fn bool_field(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

pub(crate) fn is_go_shape(hash: &[u8]) -> bool {
    const ALPHABET: &[u8] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    hash.len() == 60
        && hash.starts_with(b"$2a$10$")
        && hash[7..].iter().all(|c| ALPHABET.contains(c))
}

#[test]
fn bcrypt_rows_verify_and_regenerate() {
    assert_manifest_entry("bcrypt.jsonl");
    let key = golden_key();
    let mut checked = 0;
    let mut zero_first = 0;
    let mut zero_later = 0;
    let mut long = 0;
    for row in &read_jsonl("security/bcrypt.jsonl") {
        assert_eq!(str_field(row, "key"), "ab*512", "{row:?}");
        let password = unhex(str_field(row, "password_hex"));
        let sum = unhex(str_field(row, "digest_hex"));
        assert_eq!(sum, digest(&[&password, &key]), "{row:?}");
        let hash = str_field(row, "hash").as_bytes();
        assert!(is_go_shape(hash), "{row:?}");

        assert!(bool_field(row, "verifies"), "{row:?}");
        assert_eq!(
            bcrypt::compare_hash_and_password(hash, &sum),
            Ok(()),
            "{row:?}"
        );

        let negative = unhex(str_field(row, "negative_digest_hex"));
        assert_eq!(negative, digest(&[&password, &key, b"x"]), "{row:?}");
        assert!(!bool_field(row, "verifies_negative"), "{row:?}");
        assert_eq!(
            bcrypt::compare_hash_and_password(hash, &negative),
            Err(Error::MismatchedHashAndPassword),
            "{row:?}"
        );

        let generated = bcrypt::generate_from_password(&sum, bcrypt::DEFAULT_COST).unwrap();
        assert!(is_go_shape(&generated), "{generated:?}");
        assert_ne!(generated, hash, "{row:?}");
        assert_eq!(bcrypt::compare_hash_and_password(&generated, &sum), Ok(()));
        assert_eq!(bcrypt::cost(&generated), Ok(10));

        if sum[0] == 0 {
            zero_first += 1;
        } else if sum.contains(&0) {
            zero_later += 1;
        }
        if password.len() == 100 {
            long += 1;
        }
        checked += 1;
    }
    assert_eq!(checked, ROWS);
    assert!(zero_first >= ZERO_FIRST, "{zero_first}");
    assert!(zero_later >= ZERO_LATER, "{zero_later}");
    assert_eq!(long, 1);
}

#[test]
fn specification_vectors_verify() {
    let key = b"0123456789abcdef".repeat(64);
    let rows: [(&str, &str, &str, Option<usize>); 3] = [
        (
            "correct-horse",
            "e7c2c0d3bd3167cb9df80c7068096cd83d0704ea1643381224c59428ff697b58",
            "$2a$10$l9J725JSBBLCQO4IJVE0Tu3m3s596qYZNIo/JWoEIhanC7a89m2Wa",
            None,
        ),
        (
            "pw-63",
            "a1ad9f2b5400d6d01c9fd4d26a222a6b1586f5668ff1ff9186124929ec80121a",
            "$2a$10$pJ1NmBNlPpukMaS9o1xByOcur2WuzqlSsvZgQvrYxojbMXsFcwwiG",
            Some(5),
        ),
        (
            "pw-207",
            "004e42e5d9dd31fba878e8b16c6b246e37dfe11eac0ec5c66380b27901d70979",
            "$2a$10$Dq4ytV4va0Opbh.tYfSLx.XmNbalTJ/y1jcXe3lBkax11flcGGFp6",
            Some(0),
        ),
    ];
    for (password, digest_hex, hash, zero_at) in rows {
        let sum = digest(&[password.as_bytes(), &key]);
        assert_eq!(sum.to_vec(), unhex(digest_hex), "{password}");
        assert_eq!(
            bcrypt::compare_hash_and_password(hash.as_bytes(), &sum),
            Ok(()),
            "{password}"
        );

        let mut flipped = sum;
        flipped[31] ^= 0x01;
        assert_eq!(
            bcrypt::compare_hash_and_password(hash.as_bytes(), &flipped),
            Err(Error::MismatchedHashAndPassword),
            "{password}"
        );

        if let Some(zero) = zero_at {
            assert_eq!(sum.iter().position(|&b| b == 0), Some(zero), "{password}");
            let mut later = sum;
            later[zero + 1] ^= 0x80;
            assert_eq!(
                bcrypt::compare_hash_and_password(hash.as_bytes(), &later),
                Err(Error::MismatchedHashAndPassword),
                "{password}"
            );
        }
    }
}
