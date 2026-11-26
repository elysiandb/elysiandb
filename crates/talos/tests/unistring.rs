use talos::unistring::{BOM, new_from_string};

#[test]
fn test_string_as_utf16() {
    const STR: &str = "más";
    let s = new_from_string(STR.as_bytes());
    let b = s.as_utf16().expect("unicode string");
    assert!(b.len() == 4 && b[0] == BOM, "{b:?}");
    assert_eq!(s.string(), STR, "{s:?}");
}
