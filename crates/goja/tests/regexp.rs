use goja::deps::regexp;
use goja::parser::{RE2_DOT, WHITESPACE_CHARS, transform_reg_exp};

fn test_err(input: &str, expect: &str) {
    let err = transform_reg_exp(input.as_bytes(), false, false).expect_err(input);
    assert!(!err.is_incompatible(), "{input:?}");
    assert_eq!(err.message(), expect, "{input:?}");
}

fn test_incompat(input: &str, expect_err: &str) {
    let err = transform_reg_exp(input.as_bytes(), false, false).expect_err(input);
    assert!(err.is_incompatible(), "{input:?}");
    assert_eq!(err.message(), expect_err, "{input:?}");
}

fn test_ok(input: &str, expect: &str) {
    let result = transform_reg_exp(input.as_bytes(), false, false)
        .unwrap_or_else(|e| panic!("{input:?}: {e}"));
    assert_eq!(String::from_utf8_lossy(&result), expect, "{input:?}");
    if let Err(e) = regexp::compile(&result) {
        panic!("{input:?}: {e}");
    }
}

#[test]
fn test_reg_exp() {
    test_err("[", "Unterminated character class");
    test_err("(", "Unterminated group");
    test_err("\\(?=)", "Unmatched ')'");
    test_err(")", "Unmatched ')'");
    test_err("0:(?)", "Invalid group");
    test_err("(?)", "Invalid group");
    test_err("(?U)", "Invalid group");
    test_err("(?)|(?i)", "Invalid group");
    test_err("(?P<w>)(?P<w>)(?P<D>)", "Invalid group");
    test_incompat("<%([\\s\\S]+?)%>", "S in class");
    test_incompat("(?<=y)x", "re2: Invalid (?<) <lookbehind>");
    test_incompat("(?!test)", "re2: Invalid (?!) <lookahead>");
    test_incompat("\\1", "re2: Invalid \\1 <backreference>");
    test_incompat("\\8", "re2: Invalid \\8 <backreference>");
    test_ok("", "");
    test_ok("abc", "abc");
    test_ok("\\abc", "abc");
    test_ok("\\a\\b\\c", "a\\bc");
    test_ok("\\x", "x");
    test_ok("\\c", "c");
    test_ok("\\cA", "\\x01");
    test_ok("\\cz", "\\x1a");
    test_ok("\\ca", "\\x01");
    test_ok("\\cj", "\\x0a");
    test_ok("\\ck", "\\x0b");
    test_ok("\\+", "\\+");
    test_ok("[\\b]", "[\\x08]");
    test_ok("\\u0z01\\x\\undefined", "u0z01xundefined");
    test_ok(
        "\\\\|'|\\r|\\n|\\t|\\u2028|\\u2029",
        "\\\\|'|\\r|\\n|\\t|\\x{2028}|\\x{2029}",
    );
    test_ok("]", "]");
    test_ok("}", "}");
    test_ok("%", "%");
    test_ok("(%)", "(%)");
    test_ok("(?:[%\\s])", &["(?:[%", WHITESPACE_CHARS, "])"].concat());
    test_ok("[[]", "[[]");
    test_ok("\\101", "\\x41");
    test_ok("\\51", "\\x29");
    test_ok("\\051", "\\x29");
    test_ok("\\175", "\\x7d");
    test_ok("\\0", "\\0");
    test_ok("\\04", "\\x04");
    test_ok("(.)^", &["(", RE2_DOT, ")^"].concat());
    test_ok("\\$", "\\$");
    test_ok("[G-b]", "[G-b]");
    test_ok("[G-b\\0]", "[G-b\\0]");
    test_ok("\\k", "k");
    test_ok("\\x20", "\\x20");
    test_ok("😊", "😊");
    test_ok("^.*", &["^", RE2_DOT, "*"].concat());
    test_ok("(\\n)", "(\\n)");
    test_ok("(a(bc))", "(a(bc))");
    test_ok("[]", "[^\u{0}-🿿]");
    test_ok("[^]", "[\u{0}-🿿]");
    test_ok("\\s+", &["[", WHITESPACE_CHARS, "]+"].concat());
    test_ok("\\S+", &["[^", WHITESPACE_CHARS, "]+"].concat());
}

#[test]
fn test_transform_reg_exp() {
    let pattern = transform_reg_exp(b"\\s+abc\\s+", false, false).expect("pattern");
    assert_eq!(
        String::from_utf8_lossy(&pattern),
        ["[", WHITESPACE_CHARS, "]+abc[", WHITESPACE_CHARS, "]+"].concat()
    );
    assert!(regexp::must_compile(&pattern).match_string(b"\t abc def"));
    let pattern = transform_reg_exp(b"\\u{1d306}", false, true).expect("pattern");
    assert_eq!(String::from_utf8_lossy(&pattern), "\\x{1d306}");
    let pattern = transform_reg_exp(b"\\u1234", false, false).expect("pattern");
    assert_eq!(String::from_utf8_lossy(&pattern), "\\x{1234}");
}
