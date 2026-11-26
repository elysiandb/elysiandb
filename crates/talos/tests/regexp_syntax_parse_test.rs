mod regexp_common;

use gocompat::unicode;
use gocompat::utf8::Rune;
use regexp_common::dump;
use std::sync::Arc;
use talos::deps::regexp::syntax::{
    self, FOLD_CASE, Flags, LITERAL, MATCH_NL, MAX_FOLD, MIN_FOLD, Op, PERL, PERL_X, POSIX, Regexp,
    UNICODE_GROUPS, append_range,
};

fn s(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

fn parse_tests() -> Vec<(Vec<u8>, String)> {
    let fixed: &[(&str, &str)] = &[
        ("a", "lit{a}"),
        ("a.", "cat{lit{a}dot{}}"),
        ("a.b", "cat{lit{a}dot{}lit{b}}"),
        ("ab", "str{ab}"),
        ("a.b.c", "cat{lit{a}dot{}lit{b}dot{}lit{c}}"),
        ("abc", "str{abc}"),
        ("a|^", "alt{lit{a}bol{}}"),
        ("a|b", "cc{0x61-0x62}"),
        ("(a)", "cap{lit{a}}"),
        ("(a)|b", "alt{cap{lit{a}}lit{b}}"),
        ("a*", "star{lit{a}}"),
        ("a+", "plus{lit{a}}"),
        ("a?", "que{lit{a}}"),
        ("a{2}", "rep{2,2 lit{a}}"),
        ("a{2,3}", "rep{2,3 lit{a}}"),
        ("a{2,}", "rep{2,-1 lit{a}}"),
        ("a*?", "nstar{lit{a}}"),
        ("a+?", "nplus{lit{a}}"),
        ("a??", "nque{lit{a}}"),
        ("a{2}?", "nrep{2,2 lit{a}}"),
        ("a{2,3}?", "nrep{2,3 lit{a}}"),
        ("a{2,}?", "nrep{2,-1 lit{a}}"),
        ("x{1001", "str{x{1001}"),
        ("x{9876543210", "str{x{9876543210}"),
        ("x{9876543210,", "str{x{9876543210,}"),
        ("x{2,1", "str{x{2,1}"),
        ("x{1,9876543210", "str{x{1,9876543210}"),
        ("", "emp{}"),
        ("|", "emp{}"),
        ("|x|", "alt{emp{}lit{x}emp{}}"),
        (".", "dot{}"),
        ("^", "bol{}"),
        ("$", "eol{}"),
        (r"\|", "lit{|}"),
        (r"\(", "lit{(}"),
        (r"\)", "lit{)}"),
        (r"\*", "lit{*}"),
        (r"\+", "lit{+}"),
        (r"\?", "lit{?}"),
        ("{", "lit{{}"),
        ("}", "lit{}}"),
        (r"\.", "lit{.}"),
        (r"\^", "lit{^}"),
        (r"\$", "lit{$}"),
        (r"\\", r"lit{\}"),
        ("[ace]", "cc{0x61 0x63 0x65}"),
        ("[abc]", "cc{0x61-0x63}"),
        ("[a-z]", "cc{0x61-0x7a}"),
        ("[a]", "lit{a}"),
        (r"\-", "lit{-}"),
        ("-", "lit{-}"),
        (r"\_", "lit{_}"),
        ("abc", "str{abc}"),
        ("abc|def", "alt{str{abc}str{def}}"),
        ("abc|def|ghi", "alt{str{abc}str{def}str{ghi}}"),
        ("[[:lower:]]", "cc{0x61-0x7a}"),
        ("[a-z]", "cc{0x61-0x7a}"),
        ("[^[:lower:]]", "cc{0x0-0x60 0x7b-0x10ffff}"),
        ("[[:^lower:]]", "cc{0x0-0x60 0x7b-0x10ffff}"),
        ("(?i)[[:lower:]]", "cc{0x41-0x5a 0x61-0x7a 0x17f 0x212a}"),
        ("(?i)[a-z]", "cc{0x41-0x5a 0x61-0x7a 0x17f 0x212a}"),
        (
            "(?i)[^[:lower:]]",
            "cc{0x0-0x40 0x5b-0x60 0x7b-0x17e 0x180-0x2129 0x212b-0x10ffff}",
        ),
        (
            "(?i)[[:^lower:]]",
            "cc{0x0-0x40 0x5b-0x60 0x7b-0x17e 0x180-0x2129 0x212b-0x10ffff}",
        ),
        (r"\d", "cc{0x30-0x39}"),
        (r"\D", "cc{0x0-0x2f 0x3a-0x10ffff}"),
        (r"\s", "cc{0x9-0xa 0xc-0xd 0x20}"),
        (r"\S", "cc{0x0-0x8 0xb 0xe-0x1f 0x21-0x10ffff}"),
        (r"\w", "cc{0x30-0x39 0x41-0x5a 0x5f 0x61-0x7a}"),
        (r"\W", "cc{0x0-0x2f 0x3a-0x40 0x5b-0x5e 0x60 0x7b-0x10ffff}"),
        (
            r"(?i)\w",
            "cc{0x30-0x39 0x41-0x5a 0x5f 0x61-0x7a 0x17f 0x212a}",
        ),
        (
            r"(?i)\W",
            "cc{0x0-0x2f 0x3a-0x40 0x5b-0x5e 0x60 0x7b-0x17e 0x180-0x2129 0x212b-0x10ffff}",
        ),
        (r"[^\\]", "cc{0x0-0x5b 0x5d-0x10ffff}"),
        (r"\p{Braille}", "cc{0x2800-0x28ff}"),
        (r"\P{Braille}", "cc{0x0-0x27ff 0x2900-0x10ffff}"),
        (r"\p{^Braille}", "cc{0x0-0x27ff 0x2900-0x10ffff}"),
        (r"\P{^Braille}", "cc{0x2800-0x28ff}"),
        (
            r"\pZ",
            "cc{0x20 0xa0 0x1680 0x2000-0x200a 0x2028-0x2029 0x202f 0x205f 0x3000}",
        ),
        (r"[\p{Braille}]", "cc{0x2800-0x28ff}"),
        (r"[\P{Braille}]", "cc{0x0-0x27ff 0x2900-0x10ffff}"),
        (r"[\p{^Braille}]", "cc{0x0-0x27ff 0x2900-0x10ffff}"),
        (r"[\P{^Braille}]", "cc{0x2800-0x28ff}"),
        (
            r"[\pZ]",
            "cc{0x20 0xa0 0x1680 0x2000-0x200a 0x2028-0x2029 0x202f 0x205f 0x3000}",
        ),
    ];
    let mut tests: Vec<(Vec<u8>, String)> = fixed
        .iter()
        .map(|(a, b)| (a.as_bytes().to_vec(), b.to_string()))
        .collect();
    tests.push((br"\p{Lu}".to_vec(), mk_char_class(unicode::is_upper)));
    tests.push((br"[\p{Lu}]".to_vec(), mk_char_class(unicode::is_upper)));
    tests.push((br"(?i)[\p{Lu}]".to_vec(), mk_char_class(is_upper_fold)));
    let more: &[(&str, &str)] = &[
        (r"\p{Any}", "dot{}"),
        (r"\p{^Any}", "cc{}"),
        (r"[\012-\234]\141", "cat{cc{0xa-0x9c}lit{a}}"),
        (r"[\x{41}-\x7a]\x61", "cat{cc{0x41-0x7a}lit{a}}"),
        ("a{,2}", "str{a{,2}}"),
        (r"\.\^\$\\", r"str{.^$\}"),
        ("[a-zABC]", "cc{0x41-0x43 0x61-0x7a}"),
        ("[^a]", "cc{0x0-0x60 0x62-0x10ffff}"),
        ("[α-ε☺]", "cc{0x3b1-0x3b5 0x263a}"),
        ("a*{", "cat{star{lit{a}}lit{{}}"),
        ("(?:ab)*", "star{str{ab}}"),
        ("(ab)*", "star{cap{str{ab}}}"),
        ("ab|cd", "alt{str{ab}str{cd}}"),
        ("a(b|c)d", "cat{lit{a}cap{cc{0x62-0x63}}lit{d}}"),
        ("(?:a)", "lit{a}"),
        ("(?:ab)(?:cd)", "str{abcd}"),
        (
            "(?:a+b+)(?:c+d+)",
            "cat{plus{lit{a}}plus{lit{b}}plus{lit{c}}plus{lit{d}}}",
        ),
        (
            "(?:a+|b+)|(?:c+|d+)",
            "alt{plus{lit{a}}plus{lit{b}}plus{lit{c}}plus{lit{d}}}",
        ),
        ("(?:a|b)|(?:c|d)", "cc{0x61-0x64}"),
        ("a|.", "dot{}"),
        (".|a", "dot{}"),
        (
            "(?:[abc]|A|Z|hello|world)",
            "alt{cc{0x41 0x5a 0x61-0x63}str{hello}str{world}}",
        ),
        ("(?:[abc]|A|Z)", "cc{0x41 0x5a 0x61-0x63}"),
        (r"\Q+|*?{[\E", "str{+|*?{[}"),
        (r"\Q+\E+", "plus{lit{+}}"),
        (r"\Qab\E+", "cat{lit{a}plus{lit{b}}}"),
        (r"\Q\\E", r"lit{\}"),
        (r"\Q\\\E", r"str{\\}"),
        ("(?m)^", "bol{}"),
        ("(?m)$", "eol{}"),
        ("(?-m)^", "bot{}"),
        ("(?-m)$", "eot{}"),
        (r"(?m)\A", "bot{}"),
        (r"(?m)\z", r"eot{\z}"),
        (r"(?-m)\A", "bot{}"),
        (r"(?-m)\z", r"eot{\z}"),
        ("(?P<name>a)", "cap{name:lit{a}}"),
        ("(?<name>a)", "cap{name:lit{a}}"),
        ("[Aa]", "litfold{A}"),
        (r"[\x{100}\x{101}]", "litfold{Ā}"),
        ("[Δδ]", "litfold{Δ}"),
        ("abcde", "str{abcde}"),
        ("[Aa][Bb]cd", "cat{strfold{AB}str{cd}}"),
        (
            "abc|abd|aef|bcx|bcy",
            "alt{cat{lit{a}alt{cat{lit{b}cc{0x63-0x64}}str{ef}}}cat{str{bc}cc{0x78-0x79}}}",
        ),
        (
            "ax+y|ax+z|ay+w",
            "cat{lit{a}alt{cat{plus{lit{x}}lit{y}}cat{plus{lit{x}}lit{z}}cat{plus{lit{y}}lit{w}}}}",
        ),
        ("(?:.)", "dot{}"),
        ("(?:x|(?:xa))", "cat{lit{x}alt{emp{}lit{a}}}"),
        ("(?:.|(?:.a))", "cat{dot{}alt{emp{}lit{a}}}"),
        ("(?:A(?:A|a))", "cat{lit{A}litfold{A}}"),
        ("(?:A|a)", "litfold{A}"),
        ("A|(?:A|a)", "litfold{A}"),
        ("(?s).", "dot{}"),
        ("(?-s).", "dnl{}"),
        ("(?:(?:^).)", "cat{bol{}dot{}}"),
        ("(?-s)(?:(?:^).)", "cat{bol{}dnl{}}"),
        (r"[\s\S]a", "cat{cc{0x0-0x10ffff}lit{a}}"),
        ("abc|abd", "cat{str{ab}cc{0x63-0x64}}"),
        ("a(?:b)c|abd", "cat{str{ab}cc{0x63-0x64}}"),
        (
            "abc|abd|aef|bcx|bcy",
            "alt{cat{lit{a}alt{cat{lit{b}cc{0x63-0x64}}str{ef}}}cat{str{bc}cc{0x78-0x79}}}",
        ),
        ("abc|x|abd", "alt{str{abc}lit{x}str{abd}}"),
        ("(?i)abc|ABD", "cat{strfold{AB}cc{0x43-0x44 0x63-0x64}}"),
        ("[ab]c|[ab]d", "cat{cc{0x61-0x62}cc{0x63-0x64}}"),
        (".c|.d", "cat{dot{}cc{0x63-0x64}}"),
        (
            "x{2}|x{2}[0-9]",
            "cat{rep{2,2 lit{x}}alt{emp{}cc{0x30-0x39}}}",
        ),
        (
            "x{2}y|x{2}[0-9]y",
            "cat{rep{2,2 lit{x}}alt{lit{y}cat{cc{0x30-0x39}lit{y}}}}",
        ),
        (
            "a.*?c|a.*?b",
            "cat{lit{a}alt{cat{nstar{dot{}}lit{c}}cat{nstar{dot{}}lit{b}}}}",
        ),
        ("((((((((((x{2}){2}){2}){2}){2}){2}){2}){2}){2}))", ""),
        ("((((((((((x{1}){2}){2}){2}){2}){2}){2}){2}){2}){2})", ""),
    ];
    tests.extend(
        more.iter()
            .map(|(a, b)| (a.as_bytes().to_vec(), b.to_string())),
    );
    let mut deep = b"(".repeat(999);
    deep.extend_from_slice(&b")".repeat(999));
    tests.push((deep, String::new()));
    let mut deep = b"(?:".repeat(999);
    deep.extend_from_slice(&b")*".repeat(999));
    tests.push((deep, String::new()));
    let mut flat = b"(".to_vec();
    flat.extend_from_slice(&b"|".repeat(12345));
    flat.push(b')');
    tests.push((flat, String::new()));
    tests
}

const TEST_FLAGS: Flags = MATCH_NL | PERL_X | UNICODE_GROUPS;

#[test]
fn test_parse_simple() {
    test_parse_dump(&parse_tests(), TEST_FLAGS);
}

const FOLDCASE_TESTS: &[(&str, &str)] = &[
    ("AbCdE", "strfold{ABCDE}"),
    ("[Aa]", "litfold{A}"),
    ("a", "litfold{A}"),
    ("A[F-g]", "cat{litfold{A}cc{0x41-0x7a 0x17f 0x212a}}"),
    ("[[:upper:]]", "cc{0x41-0x5a 0x61-0x7a 0x17f 0x212a}"),
    ("[[:lower:]]", "cc{0x41-0x5a 0x61-0x7a 0x17f 0x212a}"),
];

fn owned(tests: &[(&str, &str)]) -> Vec<(Vec<u8>, String)> {
    tests
        .iter()
        .map(|(a, b)| (a.as_bytes().to_vec(), b.to_string()))
        .collect()
}

#[test]
fn test_parse_fold_case() {
    test_parse_dump(&owned(FOLDCASE_TESTS), FOLD_CASE);
}

const LITERAL_TESTS: &[(&str, &str)] = &[("(|)^$.[*+?]{5,10},\\", "str{(|)^$.[*+?]{5,10},\\}")];

#[test]
fn test_parse_literal() {
    test_parse_dump(&owned(LITERAL_TESTS), LITERAL);
}

const MATCHNL_TESTS: &[(&str, &str)] = &[
    (".", "dot{}"),
    ("\n", "lit{\n}"),
    ("[^a]", "cc{0x0-0x60 0x62-0x10ffff}"),
    ("[a\\n]", "cc{0xa 0x61}"),
];

#[test]
fn test_parse_match_nl() {
    test_parse_dump(&owned(MATCHNL_TESTS), MATCH_NL);
}

const NOMATCHNL_TESTS: &[(&str, &str)] = &[
    (".", "dnl{}"),
    ("\n", "lit{\n}"),
    ("[^a]", "cc{0x0-0x9 0xb-0x60 0x62-0x10ffff}"),
    ("[a\\n]", "cc{0xa 0x61}"),
];

#[test]
fn test_parse_no_match_nl() {
    test_parse_dump(&owned(NOMATCHNL_TESTS), 0);
}

fn test_parse_dump(tests: &[(Vec<u8>, String)], flags: Flags) {
    for (regexp, want) in tests {
        let re =
            syntax::parse(regexp, flags).unwrap_or_else(|e| panic!("Parse({}): {e}", s(regexp)));
        if want.is_empty() {
            continue;
        }
        let d = s(&dump(&re));
        assert_eq!(&d, want, "Parse({}).Dump()", s(regexp));
    }
}

fn mk_char_class(f: fn(Rune) -> bool) -> String {
    let mut re = Regexp::new(Op::CharClass);
    let mut runes: Vec<Rune> = Vec::new();
    let mut lo: Rune = -1;
    for i in 0..=unicode::MAX_RUNE {
        if f(i) {
            if lo < 0 {
                lo = i;
            }
        } else if lo >= 0 {
            runes.push(lo);
            runes.push(i - 1);
            lo = -1;
        }
    }
    if lo >= 0 {
        runes.push(lo);
        runes.push(unicode::MAX_RUNE);
    }
    re.rune = Arc::from(runes);
    s(&dump(&re))
}

fn is_upper_fold(r: Rune) -> bool {
    if unicode::is_upper(r) {
        return true;
    }
    let mut c = unicode::simple_fold(r);
    while c != r {
        if unicode::is_upper(c) {
            return true;
        }
        c = unicode::simple_fold(c);
    }
    false
}

#[test]
fn test_fold_constants() {
    let mut last: Rune = -1;
    for i in 0..=unicode::MAX_RUNE {
        if unicode::simple_fold(i) == i {
            continue;
        }
        if last == -1 {
            assert_eq!(MIN_FOLD, i, "minFold");
        }
        last = i;
    }
    assert_eq!(MAX_FOLD, last, "maxFold");
}

#[test]
fn test_append_range_collapse() {
    let mut r: Vec<Rune> = Vec::new();
    for i in 'A' as Rune..='Z' as Rune {
        append_range(&mut r, i, i);
        append_range(
            &mut r,
            i + 'a' as Rune - 'A' as Rune,
            i + 'a' as Rune - 'A' as Rune,
        );
    }
    assert_eq!(
        r,
        vec!['A' as Rune, 'Z' as Rune, 'a' as Rune, 'z' as Rune],
        "appendRange interlaced A-Z a-z"
    );
}

fn invalid_regexps() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = [
        &b"("[..],
        b")",
        b"(a",
        b"a)",
        b"(a))",
        b"(a|b|",
        b"a|b|)",
        b"(a|b|))",
        b"(a|b",
        b"a|b)",
        b"(a|b))",
        b"[a-z",
        b"([a-z)",
        b"[a-z)",
        b"([a-z]))",
        b"x{1001}",
        b"x{9876543210}",
        b"x{2,1}",
        b"x{1,9876543210}",
        b"\xff",
        b"[\xff]",
        b"[\\\xff]",
        b"\\\xff",
        b"(?P<name>a",
        b"(?P<name>",
        b"(?P<name",
        b"(?P<x y>a)",
        b"(?P<>a)",
        b"(?<name>a",
        b"(?<name>",
        b"(?<name",
        b"(?<x y>a)",
        b"(?<>a)",
        b"[a-Z]",
        b"(?i)[a-Z]",
        br"\Q\E*",
        b"a{100000}",
        b"a{100000,}",
        b"((((((((((x{2}){2}){2}){2}){2}){2}){2}){2}){2}){2})",
    ]
    .iter()
    .map(|p| p.to_vec())
    .collect();
    let mut deep = b"(".repeat(1000);
    deep.extend_from_slice(&b")".repeat(1000));
    v.push(deep);
    let mut deep = b"(?:".repeat(1000);
    deep.extend_from_slice(&b")*".repeat(1000));
    v.push(deep);
    let mut long = b"(".to_vec();
    long.extend_from_slice(&b"(xx?)".repeat(1000));
    long.extend_from_slice(b"){1000}");
    v.push(long);
    v.push(b"(xx?){1000}".repeat(1000));
    v.push(br"\pL".repeat(27000));
    v
}

const ONLY_PERL: &[&[u8]] = &[
    b"[a-b-c]",
    br"\Qabc\E",
    br"\Q*+?{[\E",
    br"\Q\\E",
    br"\Q\\\E",
    br"\Q\\\\E",
    br"\Q\\\\\E",
    b"(?:a)",
    b"(?P<name>a)",
];

const ONLY_POSIX: &[&[u8]] = &[b"a++", b"a**", b"a?*", b"a+*", b"a{1}*", b".{1}{2}.{3}"];

#[test]
fn test_parse_invalid_regexps() {
    for regexp in invalid_regexps() {
        if let Ok(re) = syntax::parse(&regexp, PERL) {
            panic!(
                "Parse({}, Perl) = {}, should have failed",
                s(&regexp[..regexp.len().min(40)]),
                s(&dump(&re))
            );
        }
        if let Ok(re) = syntax::parse(&regexp, POSIX) {
            panic!(
                "Parse({}, POSIX) = {}, should have failed",
                s(&regexp[..regexp.len().min(40)]),
                s(&dump(&re))
            );
        }
    }
    for regexp in ONLY_PERL {
        if let Err(e) = syntax::parse(regexp, PERL) {
            panic!("Parse({}, Perl): {e}", s(regexp));
        }
        if let Ok(re) = syntax::parse(regexp, POSIX) {
            panic!(
                "Parse({}, POSIX) = {}, should have failed",
                s(regexp),
                s(&dump(&re))
            );
        }
    }
    for regexp in ONLY_POSIX {
        if let Ok(re) = syntax::parse(regexp, PERL) {
            panic!(
                "Parse({}, Perl) = {}, should have failed",
                s(regexp),
                s(&dump(&re))
            );
        }
        if let Err(e) = syntax::parse(regexp, POSIX) {
            panic!("Parse({}, POSIX): {e}", s(regexp));
        }
    }
}

#[test]
fn test_to_string_equivalent_parse() {
    for (regexp, want) in parse_tests() {
        let re = syntax::parse(&regexp, TEST_FLAGS)
            .unwrap_or_else(|e| panic!("Parse({}): {e}", s(&regexp)));
        if want.is_empty() {
            continue;
        }
        let d = s(&dump(&re));
        assert_eq!(d, want, "Parse({}).Dump()", s(&regexp));
        let st = re.string();
        if st != regexp {
            let nre = syntax::parse(&st, TEST_FLAGS)
                .unwrap_or_else(|e| panic!("Parse({}.String() = {}): {e}", s(&regexp), s(&st)));
            let nd = s(&dump(&nre));
            assert_eq!(d, nd, "Parse({}) -> {}", s(&regexp), s(&st));
            let ns = nre.string();
            assert_eq!(st, ns, "Parse({}) -> {} -> {}", s(&regexp), s(&st), s(&ns));
        }
    }
}

const STRING_TESTS: &[(&str, &str)] = &[
    ("x(?i:ab*c|d?e)1", "x(?i:AB*C|D?E)1"),
    ("x(?i:ab*cd?e)1", "x(?i:AB*CD?E)1"),
    ("0(?i:ab*c|d?e)1", "(?i:0(?:AB*C|D?E)1)"),
    ("0(?i:ab*cd?e)1", "(?i:0AB*CD?E1)"),
    ("x(?i:ab*c|d?e)", "x(?i:AB*C|D?E)"),
    ("x(?i:ab*cd?e)", "x(?i:AB*CD?E)"),
    ("0(?i:ab*c|d?e)", "(?i:0(?:AB*C|D?E))"),
    ("0(?i:ab*cd?e)", "(?i:0AB*CD?E)"),
    ("(?i:ab*c|d?e)1", "(?i:(?:AB*C|D?E)1)"),
    ("(?i:ab*cd?e)1", "(?i:AB*CD?E1)"),
    ("(?i:ab)[123](?i:cd)", "(?i:AB[1-3]CD)"),
    ("(?i:ab*c|d?e)", "(?i:AB*C|D?E)"),
    ("[Aa][Bb]", "(?i:AB)"),
    ("[Aa][Bb]*[Cc]", "(?i:AB*C)"),
    ("A(?:[Bb][Cc]|[Dd])[Zz]", "A(?i:(?:BC|D)Z)"),
    ("[Aa](?:[Bb][Cc]|[Dd])Z", "(?i:A(?:BC|D))Z"),
];

#[test]
fn test_string() {
    for (re, out) in STRING_TESTS {
        let parsed =
            syntax::parse(re.as_bytes(), PERL).unwrap_or_else(|e| panic!("Parse({re}): {e}"));
        assert_eq!(s(&parsed.string()), *out, "Parse({re}).String()");
    }
}
