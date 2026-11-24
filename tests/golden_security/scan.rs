use std::io::{self, Read};

use crate::common::{hex, read_jsonl, str_field, unhex};
use crate::{assert_manifest_entry, bool_of, int_of};
use elysiandb::gocompat::fmt;

const ROWS: usize = 10;

struct OneByteReader {
    data: Vec<u8>,
    pos: usize,
}

impl OneByteReader {
    fn new(data: Vec<u8>) -> OneByteReader {
        OneByteReader { data, pos: 0 }
    }

    fn rest(&self) -> &[u8] {
        &self.data[self.pos..]
    }
}

impl Read for OneByteReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.data.len() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

#[test]
fn scan_rows_match_go() {
    assert_manifest_entry("scan.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/scan.jsonl") {
        let stdin = unhex(str_field(row, "stdin_hex"));

        let mut r = OneByteReader::new(stdin.clone());
        let (token, result) = fmt::fscanln_word(&mut r);
        assert_eq!(hex(&token), str_field(row, "token_hex"), "{row:?}");
        assert_eq!(result.is_err(), bool_of(row, "scan_error"), "{row:?}");
        assert_eq!(hex(r.rest()), str_field(row, "remaining_hex"), "{row:?}");

        let mut r = OneByteReader::new(stdin);
        let (choice, result) = fmt::fscanln_int(&mut r);
        assert_eq!(choice, int_of(row, "int_choice"), "{row:?}");
        assert_eq!(result.is_err(), bool_of(row, "int_error"), "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
