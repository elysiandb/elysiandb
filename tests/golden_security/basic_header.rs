use crate::common::{read_jsonl, str_field, unhex};
use crate::{assert_manifest_entry, bool_of, flag};
use elysiandb::security::{BasicHeader, parse_basic_header};

const ROWS: usize = 12;

#[test]
fn basic_header_rows_match_go() {
    assert_manifest_entry("basic_header.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/basic_header.jsonl") {
        let header = unhex(str_field(row, "header_hex"));
        let parsed = parse_basic_header(&header);
        let accepted = bool_of(row, "accepted_shape");

        let expected = if flag(row, "error") {
            BasicHeader::Undecodable
        } else if row.get("decoded_hex").is_some() {
            BasicHeader::Decoded {
                payload: unhex(str_field(row, "decoded_hex")),
                credentials: accepted.then(|| {
                    (
                        unhex(str_field(row, "username_hex")),
                        unhex(str_field(row, "password_hex")),
                    )
                }),
            }
        } else {
            BasicHeader::NotBasic
        };

        assert_eq!(parsed, expected, "{row:?}");
        assert_eq!(parsed.credentials().is_some(), accepted, "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
