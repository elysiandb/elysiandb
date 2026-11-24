use crate::common::{read_jsonl, str_field, unhex};
use crate::{assert_manifest_entry, bool_of, flag};
use elysiandb::gocompat::json::{self, Decode, Encode, Value};
use elysiandb::transport::http::adminui::LoginRequest;
use elysiandb::transport::http::api::acl::AclPayload;
use elysiandb::transport::http::api::security::{PasswordDto, UserDto, UserRoleDto};

const ROWS: usize = 75;
const BODIES: usize = 15;

fn decode<T: Decode + Encode + Default>(body: &[u8]) -> Result<(String, T), json::Error> {
    let mut v = T::default();
    json::unmarshal(body, &mut v)?;
    let encoded = json::marshal(&v).expect("marshal");
    Ok((String::from_utf8_lossy(&encoded).into_owned(), v))
}

fn check<T: Decode + Encode + Default>(row: &Value, body: &[u8]) -> Option<T> {
    match decode::<T>(body) {
        Err(_) => {
            assert!(flag(row, "error"), "{row:?}");
            None
        }
        Ok((encoded, v)) => {
            assert!(!flag(row, "error"), "{row:?}");
            assert_eq!(encoded, str_field(row, "decoded"), "{row:?}");
            Some(v)
        }
    }
}

#[test]
fn dto_decode_rows_match_go() {
    assert_manifest_entry("dto_decode.jsonl");
    let mut counts = std::collections::BTreeMap::new();
    for row in &read_jsonl("security/dto_decode.jsonl") {
        let body = unhex(str_field(row, "body_hex"));
        let kind = str_field(row, "type");
        match kind {
            "UserDto" => {
                check::<UserDto>(row, &body);
            }
            "aclPayload" => {
                check::<AclPayload>(row, &body);
            }
            "loginRequest" => {
                check::<LoginRequest>(row, &body);
            }
            "passwordDto" => {
                if let Some(dto) = check::<PasswordDto>(row, &body) {
                    assert_eq!(
                        dto.password.is_some(),
                        bool_of(row, "password_set"),
                        "{row:?}"
                    );
                }
            }
            "userRoleDto" => {
                check::<UserRoleDto>(row, &body);
            }
            other => panic!("unknown type {other}"),
        }
        *counts.entry(kind.to_string()).or_insert(0usize) += 1;
    }
    assert_eq!(counts.len(), 5);
    assert!(counts.values().all(|&n| n == BODIES), "{counts:?}");
    assert_eq!(counts.values().sum::<usize>(), ROWS);
}
