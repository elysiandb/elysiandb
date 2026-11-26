use talos::deps::regexp2::{IGNORE_CASE, NONE, must_compile};

#[test]
fn test_replace_basic() {
    let re = must_compile("test", 0);
    let s = re
        .replace("this is a test", "unit", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "this is a unit");
}

#[test]
fn test_replace_named_group() {
    let re = must_compile(r"[^ ]+\s(?<time>)", 0);
    let s = re
        .replace("08/10/99 16:00", "${time}", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "16:00");
}

#[test]
fn test_replace_ignore_case_upper() {
    let re = must_compile("dog", IGNORE_CASE);
    let s = re
        .replace("my dog has fleas", "CAT", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "my CAT has fleas");
}

#[test]
fn test_replace_ignore_case_lower() {
    let re = must_compile("olang", IGNORE_CASE);
    let s = re
        .replace("GoLAnG", "olang", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "Golang");
}

#[test]
fn test_replace_number_group() {
    let re = must_compile(r"D\.(.+)", NONE);
    let s = re
        .replace("D.Bau", "David $1", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "David Bau");
}

#[test]
fn test_replace_limit_count() {
    let re = must_compile("a", NONE);
    let s = re.replace("aaaaa", "b", 0, 2).expect("Unexpected err");
    assert_eq!(s, "bbaaa");
}

#[test]
fn test_replace_limit_count_slice() {
    let re = must_compile("a", NONE);
    let s = re.replace("aaaaa", "b", 3, 2).expect("Unexpected err");
    assert_eq!(s, "aaabb");
}

#[test]
fn test_replace_begin_before_after_end() {
    let re = must_compile("a", NONE);
    let s = re
        .replace("a test a blah and a", "stuff", -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "stuff test stuff blstuffh stuffnd stuff");
}

#[test]
fn test_replace_bad_syntax() {
    let re = must_compile("a", NONE);
    assert!(
        re.replace("this is a test", "$5000000000", -1, -1).is_err(),
        "Expected err"
    );
}

#[test]
fn test_replace_func_basic() {
    let re = must_compile("test", NONE);
    let s = re
        .replace_func("this is a test", |_| "unit".to_string(), -1, -1)
        .expect("Unexpected err");
    assert_eq!(s, "this is a unit");
}

#[test]
fn test_replace_func_multiple() {
    let re = must_compile("test", NONE);
    let mut count = 0;
    let s = re
        .replace_func(
            "This test is another test for stuff",
            |_| {
                count += 1;
                count.to_string()
            },
            -1,
            -1,
        )
        .expect("Unexpected err");
    assert_eq!(s, "This 1 is another 2 for stuff");
}

#[test]
fn test_replace_func_groups() {
    let re = must_compile("test(?<sub>ing)?", NONE);
    let mut count: i64 = 0;
    let s = re
        .replace_func(
            "This testing is another test testingly junk",
            |m| {
                count += 1;
                if m.group_by_name("sub").expect("group sub").length > 0 {
                    return (-count).to_string();
                }
                count.to_string()
            },
            -1,
            -1,
        )
        .expect("Unexpected err");
    assert_eq!(s, "This -1 is another 2 -3ly junk");
}

#[test]
fn test_replace_ref_nums_dollar_ambiguous() {
    let re = must_compile("(123)hello(789)", NONE);
    let res = re
        .replace("123hello789", "$1456$2", -1, -1)
        .expect("replace");
    assert_eq!(res, "$1456789", "Wrong result");
}

#[test]
fn test_replace_nested_groups() {
    let re = must_compile(r"(\p{Sc}\s?)?(\d+\.?((?<=\.)\d+)?)(?(1)|\s?\p{Sc})?", NONE);
    let res = re
        .replace(
            "$17.43  \u{20ac}2 16.33  \u{a3}0.98  0.43   \u{a3}43   12\u{20ac}  17",
            "$2",
            -1,
            -1,
        )
        .expect("replace");
    assert_eq!(
        res, "17.43  2 16.33  0.98  0.43   43   12  17",
        "Wrong result"
    );
}
