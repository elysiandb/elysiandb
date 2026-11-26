use talos::deps::regexp2::{IGNORE_CASE, must_compile};

#[test]
fn test_ignore_case_simple() {
    let r = must_compile("aaamatch thisbbb", IGNORE_CASE);
    let m = r
        .find_string_match("AaAMatch thisBBb")
        .expect("unexpected error")
        .expect("no match when one was expected");
    assert_eq!(m.string(), "AaAMatch thisBBb", "group 0");
}

#[test]
fn test_ignore_case_inline() {
    let r = must_compile("aaa(?i:match this)bbb", 0);
    let m = r
        .find_string_match("aaaMaTcH ThIsbbb")
        .expect("unexpected error")
        .expect("no match when one was expected");
    assert_eq!(m.string(), "aaaMaTcH ThIsbbb", "group 0");
}

#[test]
fn test_ignore_case_revert() {
    let r = must_compile("aaa(?-i:match this)bbb", IGNORE_CASE);
    let m = r
        .find_string_match("AaAMatch thisBBb")
        .expect("unexpected error");
    assert!(m.is_none(), "had a match but expected no match");
}
