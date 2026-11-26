use talos::deps::regexp2::{MULTILINE, RE2, Regexp, compile, must_compile};

fn matches<I: AsRef<[u8]> + ?Sized>(re: &Regexp, input: &I) -> bool {
    re.match_string(input).expect("no error without a timeout")
}

#[test]
fn test_re2_compat_capture() {
    let r = must_compile("re(?P<a>2)", RE2);
    let m = r
        .find_string_match("blahre2blah")
        .expect("Unexpected error")
        .expect("Expected match");
    let g = m.group_by_name("a").expect("group a");
    assert_eq!(g.string(), "2");
}

#[test]
fn test_re2_compat_capture_invalid() {
    for inp in [
        "(?P<name>a",
        "(?P<name>",
        "(?P<name",
        "(?P<x y>a)",
        "(?P<>a)",
    ] {
        let r = compile(inp, RE2);
        assert!(r.is_err(), "Expected failure to parse {inp}");
    }
}

#[test]
fn test_re2_named_ascii() {
    let table: [(&str, &[u8], &[u8]); 14] = [
        ("alnum", b"1", b"!"),
        ("alpha", b"g", b"0"),
        ("blank", b" ", b"_"),
        ("ascii", b"*", b"\x8f"),
        ("cntrl", b"\t", b"A"),
        ("graph", b"!", b" "),
        ("lower", b"a", b"A"),
        ("print", b" ", b"\r"),
        ("punct", b"@", b"A"),
        ("space", b" ", b"A"),
        ("digit", b"1", b"A"),
        ("upper", b"A", b"a"),
        ("word", b"_", b"-"),
        ("xdigit", b"A", b"G"),
    ];
    for (nm, pos, neg) in table {
        let r = must_compile(&format!("[[:{nm}:]]"), RE2);
        assert!(matches(&r, pos), "{nm}: Expected match");
        assert!(!matches(&r, neg), "{nm}: Expected no match");
        let r = must_compile(&format!("[[:^{nm}:]]"), RE2);
        assert!(matches(&r, neg), "{nm} negate: Expected match");
        assert!(!matches(&r, pos), "{nm} negate: Expected no match");
    }
}

#[test]
fn test_re2_named_ascii_concat() {
    let r = must_compile("[[:digit:]a]", RE2);
    assert!(!matches(&r, "b"), "Expected no match");
    assert!(matches(&r, "a"), "Expected match");
    assert!(!matches(&r, "["), "Expected no match");
    assert!(matches(&r, "5"), "Expected match");
}

#[test]
fn test_re2_dollar_singleline() {
    let r = must_compile("^ac$\\n", RE2);
    assert!(!matches(&r, "ac"), "Expected no match");
    assert!(!matches(&r, "ac\n"), "Expected no match");
}

#[test]
fn test_re2_dollar_multiline() {
    let r = must_compile("^ac$\\n", RE2 | MULTILINE);
    assert!(!matches(&r, "ac"), "Expected no match");
    assert!(matches(&r, "ac\n"), "Expected match");
}

#[test]
fn test_re2_extended_zero() {
    let not_zero = "\u{7c0}";
    let r = must_compile(r"^\d$", RE2);
    assert!(!matches(&r, not_zero), "Expected no match");
    let r = must_compile(r"^\D$", RE2);
    assert!(matches(&r, not_zero), "Expected match");
}

#[test]
fn test_regular_extended_zero() {
    let not_zero = "\u{7c0}";
    let r = must_compile(r"^\d$", 0);
    assert!(matches(&r, not_zero), "Expected match");
    let r = must_compile(r"^\D$", 0);
    assert!(!matches(&r, not_zero), "Expected no match");
}

#[test]
fn test_re2_word() {
    let r = must_compile(r"\w", RE2);
    assert!(!matches(&r, "\u{e5}"), "Expected no match");
    let r = must_compile(r"\W", RE2);
    assert!(matches(&r, "\u{e5}"), "Expected match");
}

#[test]
fn test_regular_word() {
    let r = must_compile(r"\w", 0);
    assert!(matches(&r, "\u{e5}"), "Expected match");
    let r = must_compile(r"\W", 0);
    assert!(!matches(&r, "\u{e5}"), "Expected no match");
}

#[test]
fn test_re2_space() {
    let r = must_compile(r"\s", RE2);
    assert!(!matches(&r, "\x0b"), "Expected no match");
    let r = must_compile(r"\S", RE2);
    assert!(matches(&r, "\x0b"), "Expected match");
}

#[test]
fn test_regular_space() {
    let r = must_compile(r"\s", 0);
    assert!(matches(&r, "\x0b"), "Expected match");
    let r = must_compile(r"\S", 0);
    assert!(!matches(&r, "\x0b"), "Expected no match");
}

#[test]
fn test_escape_literal_defaults() {
    assert!(compile(r"a\_test", 0).is_err(), "Expected compile fail");
    let r = must_compile(r"a\_test", RE2);
    assert!(matches(&r, "a_test"), "Expected match");
    assert!(!matches(&r, "a\\_test"), "Expected no match");
}
