use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    EmptyInput,
    IntegerOverflow,
    NoDigitsFound,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ParseError::EmptyInput => "empty input",
            ParseError::IntegerOverflow => "integer overflow",
            ParseError::NoDigitsFound => "no digits found",
        })
    }
}

impl std::error::Error for ParseError {}

pub fn first_word_bytes(b: &[u8]) -> (&[u8], &[u8]) {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b'\n' || b[end - 1] == b'\r') {
        end -= 1;
    }
    let b = &b[..end];

    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let start = i;

    while i < b.len() && b[i] != b' ' && b[i] != b'\t' {
        i += 1;
    }
    let cmd = &b[start..i];

    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }

    (cmd, &b[i..])
}

pub fn eq_ascii(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn parse_decimal_bytes(b: &[u8]) -> Result<i64, ParseError> {
    if b.is_empty() {
        return Err(ParseError::EmptyInput);
    }

    let mut n: i64 = 0;
    let mut read = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            break;
        }

        let d = i64::from(c - b'0');

        if n > i64::MAX / 10 || (n == i64::MAX / 10 && d > i64::MAX % 10) {
            return Err(ParseError::IntegerOverflow);
        }

        n = n * 10 + d;
        read += 1;
    }

    if read == 0 {
        return Err(ParseError::NoDigitsFound);
    }

    Ok(n)
}

pub fn join_byte_slices<S: AsRef<[u8]>>(slices: &[S], sep: &[u8]) -> Vec<u8> {
    if slices.is_empty() {
        return Vec::new();
    }

    let mut total_len = 0;
    for s in slices {
        total_len += s.as_ref().len();
    }
    total_len += sep.len() * (slices.len() - 1);

    let mut result = Vec::with_capacity(total_len);
    for (i, s) in slices.iter().enumerate() {
        if i > 0 {
            result.extend_from_slice(sep);
        }
        result.extend_from_slice(s.as_ref());
    }

    result
}
