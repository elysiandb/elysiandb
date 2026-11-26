use talos::builtin_regexp::{
    convert_regexp_to_unicode, convert_regexp_to_utf16, escape_invalid_utf16,
};
use talos::string::{JsString, ascii_string, new_string_value, string_from_utf16};

fn s(b: Vec<u8>) -> String {
    String::from_utf8(b).expect("utf-8")
}

fn from_runes(r: &[u32]) -> JsString {
    let mut u = Vec::new();
    for &c in r {
        if c > 0xFFFF {
            let c = c - 0x10000;
            u.push(0xD800 + (c >> 10) as u16);
            u.push(0xDC00 + (c & 0x3FF) as u16);
        } else {
            u.push(c as u16);
        }
    }
    string_from_utf16(&u)
}

#[test]
fn test_convert_regexp_to_unicode() {
    for (input, want) in [
        (r"test\uD800\u0C00passed", r"test\uD800\u0C00passed"),
        (r"test\uD800\uDC00passed", "test\u{10000}passed"),
        (r"test\u0023passed", r"test\u0023passed"),
        (r"test\u0passed", r"test\u0passed"),
        (r"test\uD800passed", r"test\uD800passed"),
        (r"test\uD800", r"test\uD800"),
        (r"test\uD80", r"test\uD80"),
        (r"\\uD800\uDC00passed", r"\\uD800\uDC00passed"),
        ("testpassed", "testpassed"),
    ] {
        assert_eq!(
            s(convert_regexp_to_unicode(input.as_bytes())),
            want,
            "{input}"
        );
    }
}

#[test]
fn test_convert_regexp_to_utf16() {
    assert_eq!(
        s(convert_regexp_to_utf16("\u{10000}".as_bytes())),
        r"\ud800\udc00"
    );
    assert_eq!(
        s(convert_regexp_to_utf16("\\\u{10000}".as_bytes())),
        r"\\\ud800\udc00"
    );
}

#[test]
fn test_escape_invalid_utf16() {
    assert_eq!(s(escape_invalid_utf16(&ascii_string("test"))), "test");
    assert_eq!(
        s(escape_invalid_utf16(&new_string_value(
            "test\u{10000}".as_bytes()
        ))),
        "test\u{10000}"
    );
    assert_eq!(
        s(escape_invalid_utf16(&from_runes(&[0x74, 0xD800]))),
        "t\\ud800"
    );
    assert_eq!(
        s(escape_invalid_utf16(&from_runes(&[0x74, 0xD800, 0x70]))),
        "t\\ud800p"
    );
    assert_eq!(
        s(escape_invalid_utf16(&from_runes(&[0xD800, 0x70]))),
        "\\ud800p"
    );
    assert_eq!(
        s(escape_invalid_utf16(&from_runes(&[
            0x74, 0x5C, 0xD800, 0x70
        ]))),
        r"t\\\ud800p"
    );
}
