use gocompat::strconv;
use gocompat::unicode;
use gocompat::utf8::Rune;

use super::parser::{Error, ErrorCode, Parser};
use super::{decode_runes, push_rune, rune_to_char};

pub fn escape<S: AsRef<[u8]> + ?Sized>(input: &S) -> String {
    let mut b = String::new();
    for r in decode_runes(input.as_ref()) {
        escape_rune(&mut b, r, false);
    }
    b
}

const META: &str = "\\.+*?()|[]{}^$# ";

pub(crate) fn escape_rune(b: &mut String, r: Rune, force: bool) {
    if unicode::is_print(r) {
        if rune_to_char(r).is_some_and(|c| META.contains(c)) || force {
            b.push('\\');
        }
        push_rune(b, r);
        return;
    }
    match r {
        0x07 => b.push_str("\\a"),
        0x0C => b.push_str("\\f"),
        0x0A => b.push_str("\\n"),
        0x0D => b.push_str("\\r"),
        0x09 => b.push_str("\\t"),
        0x0B => b.push_str("\\v"),
        _ => {
            if r < 0x100 {
                b.push_str("\\x");
                let s = strconv::format_int(i64::from(r), 16);
                if s.len() == 1 {
                    b.push('0');
                }
                b.push_str(&s);
                return;
            }
            b.push_str("\\u");
            b.push_str(&strconv::format_int(i64::from(r), 16));
        }
    }
}

pub fn unescape(input: &str) -> Result<String, Error> {
    let idx = match input.find('\\') {
        Some(i) => i,
        None => return Ok(input.to_string()),
    };
    let mut buf = String::from(&input[..idx]);
    let mut p = Parser::new(0);
    p.set_pattern(&input.as_bytes()[idx + 1..]);
    loop {
        if p.right_most() {
            return Err(p.get_err(ErrorCode::IllegalEndEscape, Vec::new()));
        }
        let r = p.scan_char_escape()?;
        push_rune(&mut buf, r);
        if p.right_most() {
            return Ok(buf);
        }
        let mut r = p.move_right_get_char();
        while r != '\\' as Rune {
            push_rune(&mut buf, r);
            if p.right_most() {
                return Ok(buf);
            }
            r = p.move_right_get_char();
        }
    }
}
