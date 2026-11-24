use goja::Value;
use goja::string::{JsString, StringBuilder, ascii_string, new_string_value, string_from_utf16};
use goja::string_imported::ImportedString;

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
    let mut buf = vec![0xFEFF];
    buf.extend_from_slice(&u);
    JsString::Unicode(goja::string_unicode::UnicodeString::from_raw(buf))
}

fn same(a: &JsString, b: &JsString) -> bool {
    a.same_as(&Value::String(b.clone()))
}

#[test]
fn test_value_string_builder() {
    let mut sb = StringBuilder::new();
    let s = new_string_value("a\u{10000}b".as_bytes());
    sb.write_substring(&s, 0, 1);
    let res = sb.string();
    assert!(matches!(&res, JsString::Ascii(a) if a.as_bytes() == b"a"));

    let mut sb = StringBuilder::new();
    let s = new_string_value(b"ab");
    sb.write_substring(&s, 0, 1);
    let res = sb.string();
    assert!(matches!(&res, JsString::Ascii(a) if a.as_bytes() == b"a"));

    let mut sb = StringBuilder::new();
    let s = new_string_value("a\u{10000}b".as_bytes());
    sb.write_substring(&s, 1, 3);
    assert!(same(&sb.string(), &from_runes(&[0x10000])));

    let mut sb = StringBuilder::new();
    let s = new_string_value("a\u{10000}b".as_bytes());
    sb.write_substring(&s, 0, 2);
    assert!(same(&sb.string(), &from_runes(&[0x61, 0xD800])));

    let mut sb = StringBuilder::new();
    let s = new_string_value("a\u{10000}b".as_bytes());
    sb.write_substring(&s, 2, 4);
    assert!(same(&sb.string(), &from_runes(&[0xDC00, 0x62])));

    let mut sb = StringBuilder::new();
    sb.write_string(&new_string_value("юникод".as_bytes()));
    sb.write_substring(&ascii_string(" ascii"), 0, 6);
    assert!(same(
        &sb.string(),
        &new_string_value("юникод ascii".as_bytes())
    ));

    let mut sb = StringBuilder::new();
    sb.write_string(&ascii_string("ascii"));
    sb.write_string(&JsString::Imported(ImportedString::new(
        b" imported_ascii1234567890",
    )));
    let s = sb.string();
    assert!(matches!(&s, JsString::Ascii(a) if a.as_bytes() == b"ascii imported_ascii1234567890"));

    let mut sb = StringBuilder::new();
    sb.write_string(&ascii_string("ascii"));
    sb.write_string(&JsString::Imported(ImportedString::new(
        " imported_юникод".as_bytes(),
    )));
    let s = sb.string();
    assert!(matches!(&s, JsString::Unicode(_)));
    assert!(same(
        &s,
        &new_string_value("ascii imported_юникод".as_bytes())
    ));
    assert!(same(
        &string_from_utf16(&"ascii imported_юникод".encode_utf16().collect::<Vec<_>>()),
        &s
    ));
}
