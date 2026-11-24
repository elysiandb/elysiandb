use elysiandb::transport::tcp::parsing;

#[test]
fn test_first_word_bytes_basic() {
    let (cmd, rest) = parsing::first_word_bytes(b"PING data");
    if cmd != b"PING" {
        panic!("bad cmd");
    }
    if rest != b"data" {
        panic!("bad rest");
    }
}

#[test]
fn test_first_word_bytes_trim() {
    let (cmd, rest) = parsing::first_word_bytes(b"  GET key\r\n");
    if cmd != b"GET" {
        panic!("bad cmd");
    }
    if rest != b"key" {
        panic!("bad rest");
    }
}

#[test]
fn test_first_word_bytes_only_cmd() {
    let (cmd, rest) = parsing::first_word_bytes(b"RESET\n");
    if cmd != b"RESET" {
        panic!("bad cmd");
    }
    if !rest.is_empty() {
        panic!("rest not empty");
    }
}

#[test]
fn test_first_word_bytes_empty() {
    let (cmd, rest) = parsing::first_word_bytes(b"");
    if !cmd.is_empty() || !rest.is_empty() {
        panic!("expected empty");
    }
}

#[test]
fn test_eq_ascii_true() {
    if !parsing::eq_ascii(b"ping", b"PING") {
        panic!("should match");
    }
}

#[test]
fn test_eq_ascii_false_length() {
    if parsing::eq_ascii(b"PING", b"PIN") {
        panic!("should fail");
    }
}

#[test]
fn test_eq_ascii_false_value() {
    if parsing::eq_ascii(b"PING", b"PONG") {
        panic!("should fail");
    }
}

#[test]
fn test_parse_decimal_bytes_valid() {
    match parsing::parse_decimal_bytes(b"12345") {
        Ok(12345) => {}
        _ => panic!("bad parse"),
    }
}

#[test]
fn test_parse_decimal_bytes_stops() {
    match parsing::parse_decimal_bytes(b"123abc") {
        Ok(123) => {}
        _ => panic!("bad parse"),
    }
}

#[test]
fn test_parse_decimal_bytes_empty() {
    if parsing::parse_decimal_bytes(b"").is_ok() {
        panic!("should error");
    }
}

#[test]
fn test_parse_decimal_bytes_no_digits() {
    if parsing::parse_decimal_bytes(b"abc").is_ok() {
        panic!("should error");
    }
}

#[test]
fn test_parse_decimal_bytes_overflow() {
    let max = b"92233720368547758070";
    if parsing::parse_decimal_bytes(max).is_ok() {
        panic!("should overflow");
    }
}

#[test]
fn test_join_byte_slices_basic() {
    let out = parsing::join_byte_slices(&[b"a", b"b", b"c"], b",");
    if out != b"a,b,c" {
        panic!("bad join");
    }
}

#[test]
fn test_join_byte_slices_empty() {
    let none: &[&[u8]] = &[];
    let out = parsing::join_byte_slices(none, b",");
    if !out.is_empty() {
        panic!("should be empty");
    }
}

#[test]
fn test_join_byte_slices_single() {
    let out = parsing::join_byte_slices(&[b"x"], b",");
    if out != b"x" {
        panic!("bad single");
    }
}
