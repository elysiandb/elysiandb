use goja::deps::regexp2::{RIGHT_TO_LEFT, must_compile};

#[test]
fn test_right_to_left_basic() {
    let re = must_compile(r"foo\d+", RIGHT_TO_LEFT);
    let s = "0123456789foo4567890foo1foo  0987";
    let m = re
        .find_string_match(s)
        .expect("Unexpected err")
        .expect("a match");
    assert_eq!(m.string(), "foo1", "Match 0 failed");
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err")
        .expect("a second match");
    assert_eq!(m.string(), "foo4567890", "Match 1 failed");
}

#[test]
fn test_right_to_left_start_at() {
    let re = must_compile(r"\d", RIGHT_TO_LEFT);
    let m = re
        .find_string_match_starting_at("0123", -1)
        .expect("Unexpected err")
        .expect("Expected match");
    assert_eq!(m.string(), "3", "Find failed");
}

#[test]
fn test_right_to_left_replace() {
    let re = must_compile(r"\d", RIGHT_TO_LEFT);
    let s = "0123456789foo4567890foo         ";
    let out = re.replace(s, "#", -1, 7).expect("Unexpected err");
    assert_eq!(out, "0123456789foo#######foo         ", "Replace failed");
}
