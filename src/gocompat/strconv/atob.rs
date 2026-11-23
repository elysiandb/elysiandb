use super::atoi::{ErrorKind, NumError, Target};

pub fn parse_bool(s: &[u8]) -> Result<bool, NumError<bool>> {
    match s {
        b"1" | b"t" | b"T" | b"true" | b"TRUE" | b"True" => Ok(true),
        b"0" | b"f" | b"F" | b"false" | b"FALSE" | b"False" => Ok(false),
        _ => Err(NumError::new(ErrorKind::Syntax, Target::Boolean, s, false)),
    }
}

pub fn format_bool(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

pub fn append_bool(dst: &mut Vec<u8>, b: bool) {
    dst.extend_from_slice(format_bool(b).as_bytes());
}
