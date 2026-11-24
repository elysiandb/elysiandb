mod regexp_common;

use gocompat::utf8;
use goja::deps::regexp::syntax::{self, Flags};
use goja::deps::regexp::{
    self, MAX_BACKTRACK_VECTOR, Regexp, compile, compile_one_pass, min_input_len, must_compile,
    quote_meta,
};
use regexp_common::find_tests;

const GOOD_RE: &[&[u8]] = &[
    b"",
    b".",
    b"^.$",
    b"a",
    b"a*",
    b"a+",
    b"a?",
    b"a|b",
    b"a*|b*",
    b"(a*|b)(c*|d)",
    b"[a-z]",
    br"[a-abc-c\-\]\[]",
    b"[a-z]+",
    b"[abc]",
    b"[^1234]",
    br"[^\n]",
    br"\!\\",
];

struct StringError {
    re: Vec<u8>,
    err: &'static str,
    expr: Vec<u8>,
}

fn bad_re() -> Vec<StringError> {
    let big = br"\pL".repeat(27000);
    vec![
        StringError {
            re: b"*".to_vec(),
            err: "missing argument to repetition operator",
            expr: b"*".to_vec(),
        },
        StringError {
            re: b"+".to_vec(),
            err: "missing argument to repetition operator",
            expr: b"+".to_vec(),
        },
        StringError {
            re: b"?".to_vec(),
            err: "missing argument to repetition operator",
            expr: b"?".to_vec(),
        },
        StringError {
            re: b"(abc".to_vec(),
            err: "missing closing )",
            expr: b"(abc".to_vec(),
        },
        StringError {
            re: b"abc)".to_vec(),
            err: "unexpected )",
            expr: b"abc)".to_vec(),
        },
        StringError {
            re: b"x[a-z".to_vec(),
            err: "missing closing ]",
            expr: b"[a-z".to_vec(),
        },
        StringError {
            re: b"[z-a]".to_vec(),
            err: "invalid character class range",
            expr: b"z-a".to_vec(),
        },
        StringError {
            re: br"abc\".to_vec(),
            err: "trailing backslash at end of expression",
            expr: b"".to_vec(),
        },
        StringError {
            re: b"a**".to_vec(),
            err: "invalid nested repetition operator",
            expr: b"**".to_vec(),
        },
        StringError {
            re: b"a*+".to_vec(),
            err: "invalid nested repetition operator",
            expr: b"*+".to_vec(),
        },
        StringError {
            re: br"\x".to_vec(),
            err: "invalid escape sequence",
            expr: br"\x".to_vec(),
        },
        StringError {
            re: big.clone(),
            err: "expression too large",
            expr: big,
        },
    ]
}

fn compile_test(expr: &[u8], error: Option<(&str, &[u8])>) -> Option<Regexp> {
    let res = compile(expr);
    match (error, res) {
        (None, Ok(re)) => Some(re),
        (None, Err(err)) => panic!(
            "compiling {:?}; unexpected error: {err}",
            String::from_utf8_lossy(expr)
        ),
        (Some(_), Ok(_)) => panic!(
            "compiling {:?}; missing error",
            String::from_utf8_lossy(expr)
        ),
        (Some((reason, fragment)), Err(err)) => {
            assert_eq!(
                err.to_string(),
                reason,
                "compiling {:?}; wrong error",
                String::from_utf8_lossy(expr)
            );
            assert_eq!(
                err.expr,
                fragment,
                "compiling {:?}; wrong expression",
                String::from_utf8_lossy(expr)
            );
            None
        }
    }
}

#[test]
fn test_good_compile() {
    for re in GOOD_RE {
        compile_test(re, None);
    }
}

#[test]
fn test_bad_compile() {
    for tc in bad_re() {
        compile_test(&tc.re, Some((tc.err, &tc.expr)));
    }
}

#[test]
fn test_match() {
    for test in find_tests() {
        let re = compile_test(test.pat, None).expect("compiled");
        let want = test.matches.as_ref().is_some_and(|m| !m.is_empty());
        assert_eq!(
            re.match_string(test.text),
            want,
            "MatchString failure on {:?}",
            test.pat
        );
        assert_eq!(
            re.r#match(test.text),
            want,
            "Match failure on {:?}",
            test.pat
        );
    }
}

#[test]
fn test_match_function() {
    for test in find_tests() {
        let (m, err) = match regexp::match_string(test.pat, test.text) {
            Ok(m) => (m, false),
            Err(_) => (false, true),
        };
        if !err {
            continue;
        }
        let want = test.matches.as_ref().is_some_and(|m| !m.is_empty());
        assert_eq!(m, want, "Match failure on {:?}", test.pat);
    }
}

#[test]
fn test_copy_match() {
    for test in find_tests() {
        let re = compile_test(test.pat, None).expect("compiled");
        let m1 = re.match_string(test.text);
        let m2 = re.copy().match_string(test.text);
        assert_eq!(m1, m2, "Copied Regexp match failure on {:?}", test.pat);
    }
}

struct ReplaceTest {
    pattern: &'static [u8],
    replacement: &'static [u8],
    input: &'static [u8],
    output: &'static [u8],
}

const fn rt(
    pattern: &'static [u8],
    replacement: &'static [u8],
    input: &'static [u8],
    output: &'static [u8],
) -> ReplaceTest {
    ReplaceTest {
        pattern,
        replacement,
        input,
        output,
    }
}

const REPLACE_TESTS: &[ReplaceTest] = &[
    rt(b"", b"", b"", b""),
    rt(b"", b"x", b"", b"x"),
    rt(b"", b"", b"abc", b"abc"),
    rt(b"", b"x", b"abc", b"xaxbxcx"),
    rt(b"b", b"", b"", b""),
    rt(b"b", b"x", b"", b""),
    rt(b"b", b"", b"abc", b"ac"),
    rt(b"b", b"x", b"abc", b"axc"),
    rt(b"y", b"", b"", b""),
    rt(b"y", b"x", b"", b""),
    rt(b"y", b"", b"abc", b"abc"),
    rt(b"y", b"x", b"abc", b"abc"),
    rt(
        b"[a-c]*",
        b"x",
        "\u{65e5}".as_bytes(),
        "x\u{65e5}x".as_bytes(),
    ),
    rt(
        "[^\u{65e5}]".as_bytes(),
        b"x",
        "abc\u{65e5}def".as_bytes(),
        "xxx\u{65e5}xxx".as_bytes(),
    ),
    rt(b"^[a-c]*", b"x", b"abcdabc", b"xdabc"),
    rt(b"[a-c]*$", b"x", b"abcdabc", b"abcdx"),
    rt(b"^[a-c]*$", b"x", b"abcdabc", b"abcdabc"),
    rt(b"^[a-c]*", b"x", b"abc", b"x"),
    rt(b"[a-c]*$", b"x", b"abc", b"x"),
    rt(b"^[a-c]*$", b"x", b"abc", b"x"),
    rt(b"^[a-c]*", b"x", b"dabce", b"xdabce"),
    rt(b"[a-c]*$", b"x", b"dabce", b"dabcex"),
    rt(b"^[a-c]*$", b"x", b"dabce", b"dabce"),
    rt(b"^[a-c]*", b"x", b"", b"x"),
    rt(b"[a-c]*$", b"x", b"", b"x"),
    rt(b"^[a-c]*$", b"x", b"", b"x"),
    rt(b"^[a-c]+", b"x", b"abcdabc", b"xdabc"),
    rt(b"[a-c]+$", b"x", b"abcdabc", b"abcdx"),
    rt(b"^[a-c]+$", b"x", b"abcdabc", b"abcdabc"),
    rt(b"^[a-c]+", b"x", b"abc", b"x"),
    rt(b"[a-c]+$", b"x", b"abc", b"x"),
    rt(b"^[a-c]+$", b"x", b"abc", b"x"),
    rt(b"^[a-c]+", b"x", b"dabce", b"dabce"),
    rt(b"[a-c]+$", b"x", b"dabce", b"dabce"),
    rt(b"^[a-c]+$", b"x", b"dabce", b"dabce"),
    rt(b"^[a-c]+", b"x", b"", b""),
    rt(b"[a-c]+$", b"x", b"", b""),
    rt(b"^[a-c]+$", b"x", b"", b""),
    rt(b"abc", b"def", b"abcdefg", b"defdefg"),
    rt(b"bc", b"BC", b"abcbcdcdedef", b"aBCBCdcdedef"),
    rt(b"abc", b"", b"abcdabc", b"d"),
    rt(b"x", b"xXx", b"xxxXxxx", b"xXxxXxxXxXxXxxXxxXx"),
    rt(b"abc", b"d", b"", b""),
    rt(b"abc", b"d", b"abc", b"d"),
    rt(b".+", b"x", b"abc", b"x"),
    rt(b"[a-c]*", b"x", b"def", b"xdxexfx"),
    rt(b"[a-c]+", b"x", b"abcbcdcdedef", b"xdxdedef"),
    rt(b"[a-c]*", b"x", b"abcbcdcdedef", b"xdxdxexdxexfx"),
    rt(b"a+", b"($0)", b"banana", b"b(a)n(a)n(a)"),
    rt(b"a+", b"(${0})", b"banana", b"b(a)n(a)n(a)"),
    rt(b"a+", b"(${0})$0", b"banana", b"b(a)an(a)an(a)a"),
    rt(b"a+", b"(${0})$0", b"banana", b"b(a)an(a)an(a)a"),
    rt(
        b"hello, (.+)",
        b"goodbye, ${1}",
        b"hello, world",
        b"goodbye, world",
    ),
    rt(
        b"hello, (.+)",
        b"goodbye, $1x",
        b"hello, world",
        b"goodbye, ",
    ),
    rt(
        b"hello, (.+)",
        b"goodbye, ${1}x",
        b"hello, world",
        b"goodbye, worldx",
    ),
    rt(
        b"hello, (.+)",
        b"<$0><$1><$2><$3>",
        b"hello, world",
        b"<hello, world><world><><>",
    ),
    rt(
        b"hello, (?P<noun>.+)",
        b"goodbye, $noun!",
        b"hello, world",
        b"goodbye, world!",
    ),
    rt(
        b"hello, (?P<noun>.+)",
        b"goodbye, ${noun}",
        b"hello, world",
        b"goodbye, world",
    ),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$x$x$x", b"hi", b"hihihi"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$x$x$x", b"bye", b"byebyebye"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$xyz", b"hi", b""),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"${x}yz", b"hi", b"hiyz"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"hello $$x", b"hi", b"hello $x"),
    rt(b"a+", b"${oops", b"aaa", b"${oops"),
    rt(b"a+", b"$$", b"aaa", b"$"),
    rt(b"a+", b"$", b"aaa", b"$"),
    rt(b"(x)?", b"$1", b"123", b"123"),
    rt(b"abc", b"$1", b"123", b"123"),
    rt(b"(a)(b){0}(c)", b".$1|$3.", b"xacxacx", b"x.a|c.x.a|c.x"),
    rt(b"(a)(((b))){0}c", b".$1.", b"xacxacx", b"x.a.x.a.x"),
    rt(
        b"((a(b){0}){3}){5}(h)",
        b"y caramb$2",
        b"say aaaaaaaaaaaaaaaah",
        b"say ay caramba",
    ),
    rt(
        b"((a(b){0}){3}){5}h",
        b"y caramb$2",
        b"say aaaaaaaaaaaaaaaah",
        b"say ay caramba",
    ),
];

const REPLACE_LITERAL_TESTS: &[ReplaceTest] = &[
    rt(b"a+", b"($0)", b"banana", b"b($0)n($0)n($0)"),
    rt(b"a+", b"(${0})", b"banana", b"b(${0})n(${0})n(${0})"),
    rt(
        b"a+",
        b"(${0})$0",
        b"banana",
        b"b(${0})$0n(${0})$0n(${0})$0",
    ),
    rt(
        b"a+",
        b"(${0})$0",
        b"banana",
        b"b(${0})$0n(${0})$0n(${0})$0",
    ),
    rt(
        b"hello, (.+)",
        b"goodbye, ${1}",
        b"hello, world",
        b"goodbye, ${1}",
    ),
    rt(
        b"hello, (?P<noun>.+)",
        b"goodbye, $noun!",
        b"hello, world",
        b"goodbye, $noun!",
    ),
    rt(
        b"hello, (?P<noun>.+)",
        b"goodbye, ${noun}",
        b"hello, world",
        b"goodbye, ${noun}",
    ),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$x$x$x", b"hi", b"$x$x$x"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$x$x$x", b"bye", b"$x$x$x"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"$xyz", b"hi", b"$xyz"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"${x}yz", b"hi", b"${x}yz"),
    rt(b"(?P<x>hi)|(?P<x>bye)", b"hello $$x", b"hi", b"hello $$x"),
    rt(b"a+", b"${oops", b"aaa", b"${oops"),
    rt(b"a+", b"$$", b"aaa", b"$$"),
    rt(b"a+", b"$", b"aaa", b"$"),
];

fn wrap(s: &[u8]) -> Vec<u8> {
    let mut v = b"x".to_vec();
    v.extend_from_slice(s);
    v.push(b'y');
    v
}

struct ReplaceFuncTest {
    pattern: &'static [u8],
    replacement: fn(&[u8]) -> Vec<u8>,
    input: &'static [u8],
    output: &'static [u8],
}

const REPLACE_FUNC_TESTS: &[ReplaceFuncTest] = &[
    ReplaceFuncTest {
        pattern: b"[a-c]",
        replacement: wrap,
        input: b"defabcdef",
        output: b"defxayxbyxcydef",
    },
    ReplaceFuncTest {
        pattern: b"[a-c]+",
        replacement: wrap,
        input: b"defabcdef",
        output: b"defxabcydef",
    },
    ReplaceFuncTest {
        pattern: b"[a-c]*",
        replacement: wrap,
        input: b"defabcdef",
        output: b"xydxyexyfxabcydxyexyfxy",
    },
];

#[test]
fn test_replace_all() {
    for tc in REPLACE_TESTS {
        let re = compile(tc.pattern)
            .unwrap_or_else(|e| panic!("Unexpected error compiling {:?}: {e}", tc.pattern));
        let actual = re.replace_all_string(tc.input, tc.replacement);
        assert_eq!(
            actual, tc.output,
            "{:?}.ReplaceAllString({:?},{:?})",
            tc.pattern, tc.input, tc.replacement
        );
        let actual = re.replace_all(tc.input, tc.replacement);
        assert_eq!(
            actual, tc.output,
            "{:?}.ReplaceAll({:?},{:?})",
            tc.pattern, tc.input, tc.replacement
        );
    }
}

#[test]
fn test_replace_all_literal() {
    for tc in REPLACE_TESTS {
        if tc.replacement.contains(&b'$') {
            continue;
        }
        let re = compile(tc.pattern).expect("compile");
        assert_eq!(
            re.replace_all_literal_string(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
        assert_eq!(
            re.replace_all_literal(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
    }
    for tc in REPLACE_LITERAL_TESTS {
        let re = compile(tc.pattern).expect("compile");
        assert_eq!(
            re.replace_all_literal_string(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
        assert_eq!(
            re.replace_all_literal(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
    }
}

#[test]
fn test_replace_all_func() {
    for tc in REPLACE_FUNC_TESTS {
        let re = compile(tc.pattern).expect("compile");
        assert_eq!(
            re.replace_all_string_func(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
        assert_eq!(
            re.replace_all_func(tc.input, tc.replacement),
            tc.output,
            "{:?}",
            tc.pattern
        );
    }
}

struct MetaTest {
    pattern: &'static [u8],
    output: &'static [u8],
    literal: &'static [u8],
    is_literal: bool,
}

const fn mt(
    pattern: &'static [u8],
    output: &'static [u8],
    literal: &'static [u8],
    is_literal: bool,
) -> MetaTest {
    MetaTest {
        pattern,
        output,
        literal,
        is_literal,
    }
}

const META_TESTS: &[MetaTest] = &[
    mt(b"", b"", b"", true),
    mt(b"foo", b"foo", b"foo", true),
    mt(
        "日本語+".as_bytes(),
        "日本語\\+".as_bytes(),
        "日本語".as_bytes(),
        false,
    ),
    mt(br"foo\.\$", br"foo\\\.\\\$", b"foo.$", true),
    mt(br"foo.\$", br"foo\.\\\$", b"foo", false),
    mt(
        br"!@#$%^&*()_+-=[{]}\|,<.>/?~",
        br"!@#\$%\^&\*\(\)_\+-=\[\{\]\}\\\|,<\.>/\?~",
        b"!@#",
        false,
    ),
];

const LITERAL_PREFIX_TESTS: &[MetaTest] = &[
    mt(b"^0^0$", b"", b"0", false),
    mt(b"^0^", b"", b"", false),
    mt(b"^0$", b"", b"0", true),
    mt(b"$0^", b"", b"", false),
    mt(b"$0$", b"", b"", false),
    mt(b"^^0$$", b"", b"", false),
    mt(b"^$^$", b"", b"", false),
    mt(b"$$0^^", b"", b"", false),
    mt(br"a\x{fffd}b", b"", b"a", false),
    mt(br"\x{fffd}b", b"", b"", false),
    mt("\u{fffd}".as_bytes(), b"", b"", false),
];

#[test]
fn test_quote_meta() {
    for tc in META_TESTS {
        let quoted = quote_meta(tc.pattern);
        assert_eq!(quoted, tc.output, "QuoteMeta({:?})", tc.pattern);
        if !tc.pattern.is_empty() {
            let re = compile(&quoted).unwrap_or_else(|e| {
                panic!(
                    "Unexpected error compiling QuoteMeta({:?}): {e}",
                    tc.pattern
                )
            });
            let mut src = b"abc".to_vec();
            src.extend_from_slice(tc.pattern);
            src.extend_from_slice(b"def");
            let replaced = re.replace_all_string(&src, b"xyz");
            assert_eq!(
                replaced, b"abcxyzdef",
                "QuoteMeta({:?}).Replace",
                tc.pattern
            );
        }
    }
}

#[test]
fn test_literal_prefix() {
    for tc in META_TESTS.iter().chain(LITERAL_PREFIX_TESTS) {
        let re = must_compile(tc.pattern);
        let (s, complete) = re.literal_prefix();
        assert_eq!(complete, tc.is_literal, "LiteralPrefix({:?})", tc.pattern);
        assert_eq!(s, tc.literal, "LiteralPrefix({:?})", tc.pattern);
    }
}

struct SubexpCase {
    input: &'static [u8],
    num: usize,
    names: Option<Vec<&'static [u8]>>,
    indices: Vec<(&'static [u8], isize)>,
}

fn empty_subexp_indices() -> Vec<(&'static [u8], isize)> {
    vec![(b"", -1), (b"missing", -1)]
}

fn subexp_cases() -> Vec<SubexpCase> {
    let unnamed = |n: usize| Some(vec![&b""[..]; n]);
    vec![
        SubexpCase {
            input: b"",
            num: 0,
            names: None,
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b".*",
            num: 0,
            names: None,
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"abba",
            num: 0,
            names: None,
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"ab(b)a",
            num: 1,
            names: unnamed(2),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"ab(.*)a",
            num: 1,
            names: unnamed(2),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"(.*)ab(.*)a",
            num: 2,
            names: unnamed(3),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"(.*)(ab)(.*)a",
            num: 3,
            names: unnamed(4),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"(.*)((a)b)(.*)a",
            num: 4,
            names: unnamed(5),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: br"(.*)(\(ab)(.*)a",
            num: 3,
            names: unnamed(4),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: br"(.*)(\(a\)b)(.*)a",
            num: 3,
            names: unnamed(4),
            indices: empty_subexp_indices(),
        },
        SubexpCase {
            input: b"(?P<foo>.*)(?P<bar>(a)b)(?P<foo>.*)a",
            num: 4,
            names: Some(vec![b"", b"foo", b"bar", b"", b"foo"]),
            indices: vec![(b"", -1), (b"missing", -1), (b"foo", 1), (b"bar", 2)],
        },
    ]
}

#[test]
fn test_subexp() {
    for c in subexp_cases() {
        let re = must_compile(c.input);
        let n = re.num_subexp();
        assert_eq!(n, c.num, "{:?}: NumSubexp", c.input);
        let names = re.subexp_names();
        assert_eq!(names.len(), 1 + n, "{:?}: len(SubexpNames)", c.input);
        if let Some(want) = &c.names {
            for i in 0..1 + n {
                assert_eq!(names[i], want[i], "{:?}: SubexpNames[{i}]", c.input);
            }
        }
        for (name, index) in &c.indices {
            assert_eq!(
                re.subexp_index(name),
                *index,
                "{:?}: SubexpIndex({:?})",
                c.input,
                name
            );
        }
    }
}

struct SplitTest {
    s: &'static [u8],
    r: &'static [u8],
    n: isize,
    out: Option<&'static [&'static [u8]]>,
}

const fn st(
    s: &'static [u8],
    r: &'static [u8],
    n: isize,
    out: Option<&'static [&'static [u8]]>,
) -> SplitTest {
    SplitTest { s, r, n, out }
}

const SPLIT_TESTS: &[SplitTest] = &[
    st(b"foo:and:bar", b":", -1, Some(&[b"foo", b"and", b"bar"])),
    st(b"foo:and:bar", b":", 1, Some(&[b"foo:and:bar"])),
    st(b"foo:and:bar", b":", 2, Some(&[b"foo", b"and:bar"])),
    st(b"foo:and:bar", b"foo", -1, Some(&[b"", b":and:bar"])),
    st(b"foo:and:bar", b"bar", -1, Some(&[b"foo:and:", b""])),
    st(b"foo:and:bar", b"baz", -1, Some(&[b"foo:and:bar"])),
    st(b"baabaab", b"a", -1, Some(&[b"b", b"", b"b", b"", b"b"])),
    st(b"baabaab", b"a*", -1, Some(&[b"b", b"b", b"b"])),
    st(b"baabaab", b"ba*", -1, Some(&[b"", b"", b"", b""])),
    st(b"foobar", b"f*b*", -1, Some(&[b"", b"o", b"o", b"a", b"r"])),
    st(b"foobar", b"f+.*b+", -1, Some(&[b"", b"ar"])),
    st(b"foobooboar", b"o{2}", -1, Some(&[b"f", b"b", b"boar"])),
    st(b"a,b,c,d,e,f", b",", 3, Some(&[b"a", b"b", b"c,d,e,f"])),
    st(b"a,b,c,d,e,f", b",", 0, None),
    st(b",", b",", -1, Some(&[b"", b""])),
    st(b",,,", b",", -1, Some(&[b"", b"", b"", b""])),
    st(b"", b",", -1, Some(&[b""])),
    st(b"", b".*", -1, Some(&[b""])),
    st(b"", b".+", -1, Some(&[b""])),
    st(b"", b"", -1, Some(&[])),
    st(
        b"foobar",
        b"",
        -1,
        Some(&[b"f", b"o", b"o", b"b", b"a", b"r"]),
    ),
    st(
        b"abaabaccadaaae",
        b"a*",
        5,
        Some(&[b"", b"b", b"b", b"c", b"cadaaae"]),
    ),
    st(b":x:y:z:", b":", -1, Some(&[b"", b"x", b"y", b"z", b""])),
];

fn index(s: &[u8], sep: &[u8]) -> Option<usize> {
    if sep.is_empty() {
        return Some(0);
    }
    s.windows(sep.len()).position(|w| w == sep)
}

fn count(s: &[u8], sep: &[u8]) -> usize {
    let mut n = 0;
    let mut s = s;
    while let Some(i) = index(s, sep) {
        n += 1;
        s = &s[i + sep.len()..];
    }
    n
}

fn split_n<'a>(s: &'a [u8], sep: &[u8], n: isize) -> Option<Vec<&'a [u8]>> {
    if n == 0 {
        return None;
    }
    if sep.is_empty() {
        let l = utf8::rune_count(s) as isize;
        let n = if n < 0 || n > l { l } else { n } as usize;
        let mut a = Vec::with_capacity(n);
        let mut s = s;
        for _ in 0..n.saturating_sub(1) {
            let size = utf8::decode_rune(s).1;
            a.push(&s[..size]);
            s = &s[size..];
        }
        if n > 0 {
            a.push(s);
        }
        return Some(a);
    }
    let mut n = if n < 0 { count(s, sep) as isize + 1 } else { n };
    if n > s.len() as isize + 1 {
        n = s.len() as isize + 1;
    }
    let mut a = Vec::new();
    let mut s = s;
    let limit = n as usize - 1;
    while a.len() < limit {
        let Some(m) = index(s, sep) else { break };
        a.push(&s[..m]);
        s = &s[m + sep.len()..];
    }
    a.push(s);
    Some(a)
}

#[test]
fn test_split() {
    for (i, test) in SPLIT_TESTS.iter().enumerate() {
        let re =
            compile(test.r).unwrap_or_else(|e| panic!("#{i}: {:?}: compile error: {e}", test.r));
        let split = re.split(test.s, test.n);
        let want = test.out.map(|o| o.to_vec());
        assert_eq!(split, want, "#{i}: {:?}", test.r);
        if quote_meta(test.r) == test.r {
            let strsplit = split_n(test.s, test.r, test.n);
            assert_eq!(
                split, strsplit,
                "#{i}: Split({:?}, {:?}, {}): regexp vs strings mismatch",
                test.s, test.r, test.n
            );
        }
    }
}

#[test]
fn test_parse_and_compile() {
    let expr = b"a$";
    let s = b"a\nb";
    let cases: [(Flags, bool); 2] = [
        (syntax::PERL | syntax::ONE_LINE, false),
        (syntax::PERL & !syntax::ONE_LINE, true),
    ];
    for (i, (flags, exp_match)) in cases.iter().enumerate() {
        let parsed = syntax::parse(expr, *flags).unwrap_or_else(|e| panic!("{i}: parse: {e}"));
        let re = compile(&parsed.string()).unwrap_or_else(|e| panic!("{i}: compile: {e}"));
        assert_eq!(re.match_string(s), *exp_match, "{i}");
    }
}

#[test]
fn test_one_pass_cutoff() {
    let re = syntax::parse(b"^x{1,1000}y{1,1000}$", syntax::PERL).expect("parse");
    let p = syntax::compile(&re.simplify()).expect("compile");
    assert!(
        compile_one_pass(&p).is_none(),
        "makeOnePass succeeded; wanted nil"
    );
}

#[test]
fn test_switch_backtrack() {
    let re = must_compile(b"a|b");
    let long = vec![0u8; MAX_BACKTRACK_VECTOR + 1];
    re.r#match(&long);
    re.r#match(&long[..1]);
}

#[test]
fn test_deep_equal() {
    let re1 = must_compile(b"a.*b.*c.*d");
    let re2 = must_compile(b"a.*b.*c.*d");
    assert!(re1 == re2, "DeepEqual(re1, re2) = false, want true");
    re1.match_string(b"abcdefghijklmn");
    assert!(re1 == re2, "DeepEqual(re1, re2) = false, want true");
    re2.match_string(b"abcdefghijklmn");
    assert!(re1 == re2, "DeepEqual(re1, re2) = false, want true");
    re2.match_string(&b"abcdefghijklmn".repeat(100));
    assert!(re1 == re2, "DeepEqual(re1, re2) = false, want true");
}

const MIN_INPUT_LEN_TESTS: &[(&[u8], isize)] = &[
    (b"", 0),
    (b"a", 1),
    (b"aa", 2),
    (b"(aa)a", 3),
    (b"(?:aa)a", 3),
    (b"a?a", 1),
    (b"(aaa)|(aa)", 2),
    (b"(aa)+a", 3),
    (b"(aa)*a", 1),
    (b"(aa){3,5}", 6),
    (b"[a-z]", 1),
    ("日".as_bytes(), 3),
];

#[test]
fn test_min_input_len() {
    for (expr, min) in MIN_INPUT_LEN_TESTS {
        let re = syntax::parse(expr, syntax::PERL).expect("parse");
        let m = min_input_len(&re);
        assert_eq!(
            m, *min,
            "regexp {:?} has minInputLen {m}, should be {min}",
            expr
        );
    }
}

#[test]
fn test_unmarshal_text() {
    let mut unmarshaled = must_compile(b"");
    for good in GOOD_RE {
        let re = compile_test(good, None).expect("compiled");
        let marshaled = re.marshal_text();
        unmarshaled
            .unmarshal_text(&marshaled)
            .unwrap_or_else(|e| panic!("regexp {:?} failed to unmarshal: {e}", good));
        assert_eq!(
            unmarshaled.string(),
            *good,
            "UnmarshalText returned unexpected value"
        );
        let buf = vec![0u8; 4];
        let marshal_append = re.append_text(buf);
        unmarshaled
            .unmarshal_text(&marshal_append[4..])
            .unwrap_or_else(|e| panic!("regexp {:?} failed to unmarshal: {e}", good));
        assert_eq!(
            unmarshaled.string(),
            *good,
            "UnmarshalText returned unexpected value"
        );
    }
    let mut re = must_compile(b"");
    assert!(re.unmarshal_text(br"\").is_err(), "unexpected success");
}
