use std::sync::atomic::Ordering;

use goja::deps::regexp2::syntax::any_class;
use goja::deps::regexp2::{
    DEBUG, DEFAULT_UNMARSHAL_OPTIONS, ECMASCRIPT, Error, IGNORE_CASE, MULTILINE, NONE, Regexp,
    SINGLELINE, UNICODE, escape, must_compile, unescape,
};

fn compile<P: AsRef<[u8]> + ?Sized>(p: &P, opts: i32) -> Result<Regexp, Error> {
    Regexp::compile(p, opts)
}

fn matches<I: AsRef<[u8]> + ?Sized>(re: &Regexp, input: &I) -> bool {
    re.match_string(input).expect("no error without a timeout")
}

#[test]
fn test_set_prefix() {
    let r = must_compile(r"^\s*-TEST", 0);
    let fc = r
        .code()
        .fc_prefix
        .as_ref()
        .expect("Expected prefix set [-\\s] but was nil");
    assert_eq!(fc.prefix_set.string(), "[-\\s]");
}

#[test]
fn test_set_in_code() {
    let r = must_compile(r"(?<body>\s*(?<name>.+))", 0);
    let _ = r.code().dump();
    assert_eq!(r.code().sets.len(), 1);
    assert_eq!(r.code().sets[0].string(), "[\\s]");
}

#[test]
fn test_regexp_basic() {
    let r = compile("test(?<named>ing)?", 0).expect("unexpected compile err");
    let m = r
        .find_string_match("this is a testing stuff")
        .expect("unexpected match err");
    assert!(m.is_some(), "Nil match, expected success");
}

#[test]
fn test_capture_basic() {
    let r = must_compile(r".*\B(SUCCESS)\B.*", 0);
    let m = r
        .find_string_match("adfadsfSUCCESSadsfadsf")
        .expect("Unexpected match error")
        .expect("Should have matched");
    assert_eq!(m.string(), "adfadsfSUCCESSadsfadsf");
    assert_eq!(m.index, 0);
    assert_eq!(m.length, 22);
    assert_eq!(m.captures.len(), 1);
    assert_eq!(m.string(), m.captures[0].string());
    assert_eq!(m.captures[0].index, 0);
    assert_eq!(m.captures[0].length, 22);
    let g = m.groups();
    assert_eq!(g.len(), 2);
    assert_eq!(m.string(), g[0].string());
    assert_eq!(g[0].captures.len(), 1);
    assert_eq!(m.captures[0].string(), g[0].captures[0].string());
    assert_eq!(g[1].index, 7);
    assert_eq!(g[1].length, 7);
    assert_eq!(g[1].string(), "SUCCESS");
}

#[test]
fn test_escape_unescape_basic() {
    let s1 = "#$^*+(){}<>\\|. ";
    let s2 = escape(s1);
    let s3 = unescape(&s2).expect("Unexpected error during unescape");
    assert_eq!(s2, r"\#\$\^\*\+\(\)\{\}<>\\\|\.\ ");
    assert_eq!(s3, s1);
}

struct GroupData {
    p: &'static str,
    s: &'static str,
    name: &'static [&'static str],
    num: &'static [isize],
    strs: &'static [&'static str],
}

#[test]
fn test_groups_basic() {
    let data = [
        GroupData {
            p: r"(?<first_name>\S+)\s(?<last_name>\S+)",
            s: "Ryan Byington",
            name: &["0", "first_name", "last_name"],
            num: &[0, 1, 2],
            strs: &["Ryan Byington", "Ryan", "Byington"],
        },
        GroupData {
            p: r"((?<One>abc)\d+)?(?<Two>xyz)(.*)",
            s: "abc208923xyzanqnakl",
            name: &["0", "1", "2", "One", "Two"],
            num: &[0, 1, 2, 3, 4],
            strs: &["abc208923xyzanqnakl", "abc208923", "anqnakl", "abc", "xyz"],
        },
        GroupData {
            p: r"((?<256>abc)\d+)?(?<16>xyz)(.*)",
            s: "0272saasdabc8978xyz][]12_+-",
            name: &["0", "1", "2", "16", "256"],
            num: &[0, 1, 2, 16, 256],
            strs: &["abc8978xyz][]12_+-", "abc8978", "][]12_+-", "xyz", "abc"],
        },
        GroupData {
            p: r"((?<4>abc)(?<digits>\d+))?(?<2>xyz)(?<everything_else>.*)",
            s: "0272saasdabc8978xyz][]12_+-",
            name: &["0", "1", "2", "digits", "4", "everything_else"],
            num: &[0, 1, 2, 3, 4, 5],
            strs: &[
                "abc8978xyz][]12_+-",
                "abc8978",
                "xyz",
                "8978",
                "abc",
                "][]12_+-",
            ],
        },
        GroupData {
            p: r"(?<first_name>\S+)\s(?<first_name>\S+)",
            s: "Ryan Byington",
            name: &["0", "first_name"],
            num: &[0, 1],
            strs: &["Ryan Byington", "Byington"],
        },
        GroupData {
            p: r"(?<15>\S+)\s(?<15>\S+)",
            s: "Ryan Byington",
            name: &["0", "15"],
            num: &[0, 15],
            strs: &["Ryan Byington", "Byington"],
        },
        GroupData {
            p: r"(?'first_name'\S+)\s(?'last_name'\S+)",
            s: "Ryan Byington",
            name: &["0", "first_name", "last_name"],
            num: &[0, 1, 2],
            strs: &["Ryan Byington", "Ryan", "Byington"],
        },
        GroupData {
            p: r"((?'One'abc)\d+)?(?'Two'xyz)(.*)",
            s: "abc208923xyzanqnakl",
            name: &["0", "1", "2", "One", "Two"],
            num: &[0, 1, 2, 3, 4],
            strs: &["abc208923xyzanqnakl", "abc208923", "anqnakl", "abc", "xyz"],
        },
        GroupData {
            p: r"((?'256'abc)\d+)?(?'16'xyz)(.*)",
            s: "0272saasdabc8978xyz][]12_+-",
            name: &["0", "1", "2", "16", "256"],
            num: &[0, 1, 2, 16, 256],
            strs: &["abc8978xyz][]12_+-", "abc8978", "][]12_+-", "xyz", "abc"],
        },
        GroupData {
            p: r"((?'4'abc)(?'digits'\d+))?(?'2'xyz)(?'everything_else'.*)",
            s: "0272saasdabc8978xyz][]12_+-",
            name: &["0", "1", "2", "digits", "4", "everything_else"],
            num: &[0, 1, 2, 3, 4, 5],
            strs: &[
                "abc8978xyz][]12_+-",
                "abc8978",
                "xyz",
                "8978",
                "abc",
                "][]12_+-",
            ],
        },
        GroupData {
            p: r"(?'first_name'\S+)\s(?'first_name'\S+)",
            s: "Ryan Byington",
            name: &["0", "first_name"],
            num: &[0, 1],
            strs: &["Ryan Byington", "Byington"],
        },
        GroupData {
            p: r"(?'15'\S+)\s(?'15'\S+)",
            s: "Ryan Byington",
            name: &["0", "15"],
            num: &[0, 15],
            strs: &["Ryan Byington", "Byington"],
        },
    ];
    let validate_group_names_numbers = |re: &Regexp, v: &GroupData| {
        assert_eq!(
            v.name.len(),
            v.num.len(),
            "Invalid data, group name count and number count must match"
        );
        let group_names = re.get_group_names();
        assert_eq!(
            group_names,
            v.name,
            "group names, pattern {}\n{}",
            v.p,
            re.code().dump()
        );
        let group_nums = re.get_group_numbers();
        assert_eq!(group_nums, v.num, "group numbers, pattern {}", v.p);
        for i in 0..group_nums.len() {
            assert_eq!(
                group_nums[i],
                re.group_number_from_name(&group_names[i]),
                "group num from name"
            );
            assert_eq!(
                group_names[i],
                re.group_name_from_number(group_nums[i]),
                "group name from num"
            );
        }
    };
    for v in &data {
        let re = must_compile(v.p, 0);
        validate_group_names_numbers(&re, v);
        let m = re
            .find_string_match(v.s)
            .expect("Unexpected error in match")
            .expect("Match is nil");
        assert_eq!(v.strs.len(), m.group_count(), "GroupCount()");
        let g = m.groups();
        assert_eq!(v.strs.len(), g.len(), "len(m.Groups())");
        for i in 0..v.name.len() {
            let grp1 = m.group_by_name(v.name[i]).expect("group by name");
            let grp2 = m.group_by_number(v.num[i]).expect("group by number");
            assert!(
                std::ptr::eq(grp1, grp2),
                "Expected GroupByName and GroupByNumber to return same result for {}, {}",
                v.name[i],
                v.num[i]
            );
            assert_eq!(v.strs[i], grp1.string(), "Value[{i}]");
        }
        validate_group_names_numbers(&re, v);
    }
}

#[test]
fn test_err_group_name() {
    let cases = [
        ("foo(?<0>bar)", "capture number cannot be zero"),
        ("foo(?'0'bar)", "capture number cannot be zero"),
        (
            "foo(?<1bar>)",
            "invalid group name: group names must begin with a word character and have a matching terminator",
        ),
        (
            "foo(?'1bar')",
            "invalid group name: group names must begin with a word character and have a matching terminator",
        ),
        (
            "foo(?<bar)",
            "invalid group name: group names must begin with a word character and have a matching terminator",
        ),
        (
            "foo(?'bar)",
            "invalid group name: group names must begin with a word character and have a matching terminator",
        ),
    ];
    for (pattern, want) in cases {
        let err = compile(pattern, 0).expect_err("expected error during compile");
        assert_eq!(err.to_string(), want, "invalid error text for {pattern}");
    }
}

#[test]
fn test_constant_uneffected() {
    let re = must_compile(r"(\s|\*)test\s", 0);
    assert_eq!(re.code().sets.len(), 2);
    assert_eq!(re.code().sets[0].string(), "[\\*\\s]");
    assert_eq!(re.code().sets[1].string(), "[\\s]");
}

#[test]
fn test_alternation_const_and_escape() {
    let re = must_compile(r"\:|\s", 0);
    assert_eq!(re.code().sets.len(), 1);
    assert_eq!(re.code().sets[0].string(), "[:\\s]");
}

#[test]
fn test_starting_chars_optional_negate() {
    let re = must_compile(
        r"(^(\S{2} )?\S{2}(\d+|/) *\S{3}\S{3} ?\d{2,4}[A-Z] ?\d{2}[A-Z]{3}|(\S{2} )?\d{2,4})",
        0,
    );
    assert!(re.code().fc_prefix.is_none(), "FcPrefix wanted nil");
}

#[test]
fn test_parse_negative_digit() {
    let re = must_compile(r"\D", 0);
    assert_eq!(re.code().sets.len(), 1);
    assert_eq!(re.code().sets[0].string(), "[\\P{Nd}]");
}

#[test]
fn test_run_negative_digit() {
    let re = must_compile(r"\D", 0);
    assert!(matches(&re, "this is a test"), "Expected match");
}

#[test]
fn test_cancelling_classes() {
    let re = must_compile(r"[\w\W\s]", 0);
    assert_eq!(re.code().sets.len(), 1);
    assert_eq!(re.code().sets[0].string(), any_class().string());
}

#[test]
fn test_concat_loop_capture_set() {
    let re = must_compile("(A|B)*CD", 0);
    assert_eq!(re.code().sets.len(), 1);
    assert_eq!(re.code().sets[0].string(), "[AB]");
}

#[test]
fn test_firstchars_ignore_case() {
    let re = must_compile("((?i)AB(?-i)C|D)E", 0);
    let fc = re
        .code()
        .fc_prefix
        .as_ref()
        .expect("wanted prefix, got nil");
    assert_eq!(fc.prefix_set.string(), "[ad]");
}

#[test]
fn test_repeating_group() {
    let re = must_compile("(data?)+", 0);
    let m = re
        .find_string_match("datadat")
        .expect("Unexpected err")
        .expect("Expected match");
    let g = m.group_by_number(1).expect("Expected group");
    assert_eq!(g.captures.len(), 2);
    assert_eq!(
        g.captures[1].string(),
        g.capture.string(),
        "expected last capture of the group to be embedded"
    );
    assert_eq!(g.captures[0].string(), "data");
    assert_eq!(g.captures[1].string(), "dat");
}

#[test]
fn test_find_next_match_basic() {
    let re = must_compile("(T|E)(?=h|E|S|$)", 0);
    let m = re
        .find_string_match("This is a TEST")
        .expect("Unexpected err 0")
        .expect("Expected match 0");
    assert_eq!(m.index, 0);
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err 1")
        .expect("Expected match 1");
    assert_eq!(m.index, 10);
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err 2")
        .expect("Expected match 2");
    assert_eq!(m.index, 11);
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err 3")
        .expect("Expected match 3");
    assert_eq!(m.index, 13);
}

#[test]
fn test_unicode_supplementary_char_set_match() {
    let re = must_compile("[\u{2070e}-\u{20779}]", 0);
    assert!(!matches(&re, "\u{2070}"), "Unexpected match");
    assert!(matches(&re, "\u{20731}"), "Expected match");
}

#[test]
fn test_unicode_supplementary_char_in_range() {
    let re = must_compile(".", 0);
    assert!(matches(&re, "\u{2070}"), "Expected match");
    assert!(matches(&re, "\u{20731}"), "Expected match");
}

#[test]
fn test_unicode_script_sets() {
    let re = must_compile(r"\p{Katakana}+", 0);
    assert!(matches(&re, "\u{30a0}\u{30ff}"), "Expected match");
}

#[test]
fn test_hexadecimal_curly_braces() {
    let cases: [(&str, String); 8] = [
        (r"\x20", " ".to_string()),
        (r"\x{C4}", "\u{c4}".to_string()),
        (r"\x{0C5}", "\u{c5}".to_string()),
        (r"\x{00C6}", "\u{c6}".to_string()),
        (r"\x{1FF}", "\u{1ff}".to_string()),
        (r"\x{02FF}", "\u{2ff}".to_string()),
        (r"\x{1392}", "\u{1392}".to_string()),
        (r"\x{0010ffff}", "\u{10ffff}".to_string()),
    ];
    for (pattern, input) in &cases {
        let re = must_compile(*pattern, 0);
        assert!(matches(&re, input), "Expected match for {pattern}");
    }
    for bad in [
        r"\x2R",
        r"\x0",
        r"\x",
        r"\x{",
        r"\x{2",
        r"\x{2R",
        r"\x{2R}",
        r"\x{}",
        r"\x{10000",
        r"\x{1234",
        r"\x{123456789}",
    ] {
        assert!(compile(bad, 0).is_err(), "Expected error for {bad}");
    }
}

#[test]
fn test_empty_char_class() {
    assert!(
        compile("[]", 0).is_err(),
        "Empty char class isn't valid outside of ECMAScript mode"
    );
}

#[test]
fn test_ecma_empty_char_class() {
    let re = must_compile("[]", ECMASCRIPT);
    assert!(!matches(&re, "a"), "Expected no match");
}

#[test]
fn test_dot() {
    let re = must_compile(".", 0);
    assert!(matches(&re, "\r"), "Expected match");
}

#[test]
fn test_ecma_dot() {
    let re = must_compile(".", ECMASCRIPT);
    assert!(!matches(&re, "\r"), "Expected no match");
}

#[test]
fn test_decimal_lookahead() {
    let re = must_compile(r"\1(A)", 0);
    let m = re.find_string_match("AA").expect("no error");
    assert!(m.is_none(), "Expected no match");
}

#[test]
fn test_ecma_decimal_lookahead() {
    let re = must_compile(r"\1(A)", ECMASCRIPT);
    let m = re
        .find_string_match("AA")
        .expect("no error")
        .expect("a match");
    assert_eq!(m.group_count(), 2);
    assert_eq!(m.group_by_number(0).expect("group 0").string(), "A");
    assert_eq!(m.group_by_number(1).expect("group 1").string(), "A");
}

#[test]
fn test_ecma_octal() {
    let re = must_compile(r"\100", ECMASCRIPT);
    assert!(matches(&re, "@"), "Expected match");
    assert!(!matches(&re, "x"), "Expected no match");
    let re = must_compile(r"\377", ECMASCRIPT);
    assert!(matches(&re, "\u{ff}"), "Expected match");
    let re = must_compile(r"\400", ECMASCRIPT);
    assert!(matches(&re, " 0"), "Expected match");
}

#[test]
fn test_ecma_invalid_escape() {
    let re = must_compile(r"\x0", ECMASCRIPT);
    assert!(matches(&re, "x0"), "Expected match");
    let re = must_compile(r"\x0z", ECMASCRIPT);
    assert!(matches(&re, "x0z"), "Expected match");
}

#[test]
fn test_ecma_named_group() {
    let re = must_compile(r"\k", ECMASCRIPT);
    assert!(matches(&re, "k"), "Expected match");
    let re = must_compile(r"\k'test'", ECMASCRIPT);
    assert!(matches(&re, "k'test'"), "Expected match");
    let re = must_compile(r"\k<test>", ECMASCRIPT);
    assert!(matches(&re, "k<test>"), "Expected match");
    assert!(
        compile(r"(?<title>\w+), yes \k'title'", ECMASCRIPT).is_err(),
        "Expected error"
    );
    let re = must_compile(r"(?<title>\w+), yes \k<title>", ECMASCRIPT);
    assert!(matches(&re, "sir, yes sir"), "Expected match");
    let re = must_compile(r"\k<title>, yes (?<title>\w+)", ECMASCRIPT);
    assert!(matches(&re, ", yes sir"), "Expected match");
    assert!(
        compile(r"\k<(?<name>)>", ECMASCRIPT).is_err(),
        "Expected error"
    );
    must_compile(r"\k<(<name>)>", ECMASCRIPT);
    assert!(compile(r"\k<(<name>)>", 0).is_err(), "Expected error");
    let re = must_compile(r"\'|\<?", 0);
    assert!(matches(&re, "'"), "Expected match");
    assert!(matches(&re, "<"), "Expected match");
}

#[test]
fn test_ecma_invalid_escape_char_class() {
    let re = must_compile(r"[\x0]", ECMASCRIPT);
    assert!(matches(&re, "x"), "Expected match");
    assert!(matches(&re, "0"), "Expected match");
    assert!(!matches(&re, "z"), "Expected no match");
}

#[test]
fn test_ecma_script_x_curly_brace_escape() {
    let re = must_compile(r"\x{20}", ECMASCRIPT);
    assert!(!matches(&re, " "), "Expected no match");
    assert!(matches(&re, "xxxxxxxxxxxxxxxxxxxx"), "Expected match");
}

#[test]
fn test_ecma_script_unicode_range() {
    let r = compile(r"([\u{001a}-\u{ffff}]+)", ECMASCRIPT | UNICODE).expect("compile");
    let m = r.find_string_match("qqqq").expect("match");
    assert!(m.is_some(), "Expected non-nil, got nil");
}

#[test]
fn test_negate_range() {
    let re = must_compile(r"[\D]", 0);
    assert!(matches(&re, "A"), "Expected match");
}

#[test]
fn test_ecma_negate_range() {
    let re = must_compile(r"[\D]", ECMASCRIPT);
    assert!(matches(&re, "A"), "Expected match");
}

#[test]
fn test_dollar() {
    let re = must_compile("ac$", 0);
    assert!(matches(&re, "ac\n"), "Expected match");
}

#[test]
fn test_ecma_dollar() {
    let re = must_compile("ac$", ECMASCRIPT);
    assert!(!matches(&re, "ac\n"), "Expected no match");
}

#[test]
fn test_three_byte_unicode_input_only() {
    let re = must_compile("\u{9ad8}", 0);
    assert!(matches(&re, "\u{1f4cd}Test\u{9ad8}"), "Expected match");
}

#[test]
fn test_multibyte_unicode_match_partial_pattern() {
    let re = must_compile("\u{731f}\u{306a}", 0);
    assert!(
        !matches(&re, "\u{306a}\u{3042}\u{1f37a}\u{306a}"),
        "Expected no match"
    );
}

#[test]
fn test_multibyte_unicode_match() {
    let re = must_compile("\u{731f}\u{306a}", 0);
    assert!(
        matches(&re, "\u{306a}\u{3042}\u{1f37a}\u{731f}\u{306a}"),
        "Expected match"
    );
}

#[test]
fn test_alternation_named_options_errors() {
    let data = [
        "(?(?e))", "(?(?a)", "(?(?", "(?(", "?(a:b)", "?(a)", "?(a|b)", "?((a)", "?((a)a",
        "?((a)a|", "?((a)a|b", "(?(?i))", "(?(?I))", "(?(?m))", "(?(?M))", "(?(?s))", "(?(?S))",
        "(?(?x))", "(?(?X))", "(?(?n))", "(?(?N))", " (?(?n))",
    ];
    for p in data {
        let err = compile(p, 0).expect_err("Expected error, got nil");
        assert!(
            matches!(err, Error::Syntax(_)),
            "Wanted parse error, got '{err}'"
        );
    }
}

#[test]
fn test_alternation_named_options_success() {
    let data = [
        ("(?(cat)|dog)", "cat", true, ""),
        ("(?(cat)|dog)", "catdog", true, ""),
        ("(?(cat)dog1|dog2)", "catdog1", false, ""),
        ("(?(cat)dog1|dog2)", "catdog2", true, "dog2"),
        ("(?(cat)dog1|dog2)", "catdog1dog2", true, "dog2"),
        ("(?(dog2))", "dog2", true, ""),
        ("(?(cat)|dog)", "oof", false, ""),
        ("(?(a:b))", "a", true, ""),
        ("(?(a:))", "a", true, ""),
    ];
    for (pattern, input, expect_success, match_val) in data {
        let re = must_compile(pattern, 0);
        let m = re
            .find_string_match(input)
            .expect("Unexpected error during match");
        assert_eq!(
            expect_success,
            m.is_some(),
            "Success mismatch for {pattern}"
        );
        if let Some(m) = m {
            assert_eq!(match_val, m.string(), "Match val mismatch for {pattern}");
        }
    }
}

#[test]
fn test_alternation_construct_matches() {
    let re = must_compile("(?(A)A123|C789)", 0);
    let m = re
        .find_string_match("A123 B456 C789")
        .expect("Unexpected err")
        .expect("Expected match, got nil");
    assert_eq!(m.string(), "A123");
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err in second match")
        .expect("Expected second match, got nil");
    assert_eq!(m.string(), "C789");
    let m = re
        .find_next_match(&m)
        .expect("Unexpected err in third match");
    assert!(m.is_none(), "Did not expect third match");
}

#[test]
fn test_start_at_end() {
    let re = must_compile("(?:)", 0);
    let m = re.find_string_match_starting_at("t", 1).expect("no error");
    assert!(m.is_some(), "Expected match");
}

#[test]
fn test_parser_fuzz_crashes() {
    let crashes: [&[u8]; 7] = [
        b"(?'-",
        b"(\\c0)",
        b"(\\00(?())",
        b"[\\p{0}",
        b"(\x00?.*.()?(()?)?)*.x\xcb?&(\\s\x80)",
        b"\\p{0}",
        b"[0-[\\p{0}",
    ];
    for c in crashes {
        let _ = compile(c, 0);
    }
}

#[test]
fn test_parser_fuzz_hangs() {
    let hangs: [&[u8]; 6] = [
        b"\r{865720113}z\xd5{\r{861o",
        b"\r{915355}\r{9153}",
        b"\r{525005}",
        b"\x01{19765625}",
        b"(\r{068828256})",
        b"\r{677525005}",
    ];
    for c in hangs {
        let _ = compile(c, 0);
    }
}

#[test]
fn test_control_bracket_fail() {
    let re = must_compile(r"(cat)(\c[*)(dog)", 0);
    assert!(!matches(&re, "asdlkcat\u{ff}dogiwod"), "expected no match");
}

#[test]
fn test_control_bracket_groups() {
    let re = must_compile(r"(cat)(\c[*)(dog)", 0);
    assert_eq!(re.capsize(), 4, "Capsize wrong");
    let m = re
        .find_string_match("asdlkcat\u{1b}dogiwod")
        .expect("no error")
        .expect("expected match");
    let want = ["cat\u{1b}dog", "cat", "\u{1b}", "dog"];
    for (i, g) in m.groups().iter().enumerate() {
        assert_eq!(want[i], g.string(), "Bad group num {i}");
    }
}

#[test]
fn test_bad_group_construct() {
    for b in [
        "(?>-", "(?<", "(?<=", "(?<!", "(?>", "(?)", "(?<)", "(?')", "(?<-",
    ] {
        assert!(
            compile(b, 0).is_err(),
            "Wanted error, but got no error for pattern: {b}"
        );
    }
}

#[test]
fn test_empty_capture_large_repeat() {
    let r = must_compile("(?:){40}", 0);
    let m = r
        .find_string_match("1")
        .expect("Unexpected error")
        .expect("a match");
    assert_eq!(m.index, 0, "First Match Index");
    assert_eq!(m.length, 0, "First Match Length");
    let m = r
        .find_next_match(&m)
        .expect("no error")
        .expect("a second match");
    assert_eq!(m.index, 1, "Second Match Index");
    assert_eq!(m.length, 0, "Second Match Length");
    let m = r.find_next_match(&m).expect("no error");
    assert!(m.is_none(), "Expected 2 matches, got more");
}

#[test]
fn test_fuzz_bytes_no_compile() {
    let cases: [&[u8]; 4] = [
        &[0x28, 0x28, 0x29, 0x5c, 0x37, 0x28, 0x3f, 0x28, 0x29, 0x29],
        &[0x28, 0x5c, 0x32, 0x28, 0x3f, 0x28, 0x30, 0x29, 0x29],
        &[
            0x28, 0x3f, 0x28, 0x29, 0x29, 0x5c, 0x31, 0x30, 0x28, 0x3f, 0x28, 0x30, 0x29,
        ],
        &[
            0x28, 0x29, 0x28, 0x28, 0x29, 0x5c, 0x37, 0x28, 0x3f, 0x28, 0x29, 0x29,
        ],
    ];
    for r in cases {
        assert!(
            compile(r, MULTILINE | ECMASCRIPT | DEBUG).is_err(),
            "should fail compile, but didn't"
        );
    }
}

#[test]
fn test_fuzz_bytes_match() {
    let cases: [(&[u8], &[u8]); 2] = [
        (
            &[0x30, 0xbf, 0x30, 0x2a, 0x30, 0x30],
            &[0xf0, 0xb0, 0x80, 0x91, 0xf7],
        ),
        (
            &[0x30, 0xaf, 0xf3, 0x30, 0x2a],
            &[0xf3, 0x80, 0x80, 0x87, 0x80, 0x89],
        ),
    ];
    for (r, s) in cases {
        let re = compile(r, 0).expect("should compile, but didn't");
        let _ = re.match_string(s);
    }
}

#[test]
fn test_concat_accidental_pattern_charge() {
    let r = compile(r"(?<=1234\.\*56).*(?=890)", 0).expect("compile");
    let m = r.find_string_match("1234.*567890").expect("match");
    assert!(m.is_some(), "Expected non-nil, got nil");
}

#[test]
fn test_good_reverse_order_message() {
    let err = compile("[h-c]", ECMASCRIPT).expect_err("expected error");
    assert_eq!(err.to_string(), "[h-c] range in reverse order");
}

#[test]
fn test_parse_short_slash_p() {
    let re = must_compile(r"[!\pL\pN]{1,}", 0);
    let m = re
        .find_string_match("this23! is a! test 1a 2b")
        .expect("Unexpected error")
        .expect("a match");
    assert_eq!(m.string(), "this23!", "Expected match");
}

#[test]
fn test_parse_short_slash_negate_p() {
    let re = must_compile(r"\PNa", 0);
    let m = re
        .find_string_match("this is a test 1a 2b")
        .expect("Unexpected error")
        .expect("a match");
    assert_eq!(m.string(), " a", "Expected match");
}

#[test]
fn test_parse_short_slash_p_end() {
    let re = must_compile(r"\pN", 0);
    let m = re
        .find_string_match("this is a test 1a 2b")
        .expect("Unexpected error")
        .expect("a match");
    assert_eq!(m.string(), "1", "Expected match");
}

#[test]
fn test_marshal() {
    let re = must_compile(".*", 0);
    assert_eq!(re.marshal_text(), b".*", "Expected \".*\"");
}

#[test]
fn test_un_marshal() {
    DEFAULT_UNMARSHAL_OPTIONS.store(IGNORE_CASE, Ordering::SeqCst);
    let mut re = must_compile("", NONE);
    re.unmarshal_text(b"^[abc]").expect("Unexpected error");
    assert_eq!(re.options(), IGNORE_CASE, "Expected options ignore case");
    assert_eq!(re.string(), b"^[abc]");
    assert!(matches(&re, "A"), "Expected match");
}

#[test]
fn test_regexp_ecma_script_with_singleline() {
    let re = must_compile(".", SINGLELINE);
    assert!(matches(&re, "\n"), "Expected match");
    let re = must_compile(".", ECMASCRIPT | SINGLELINE);
    assert!(matches(&re, "\n"), "Expected match");
}
