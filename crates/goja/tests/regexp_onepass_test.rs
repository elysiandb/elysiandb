use gocompat::utf8::Rune;
use goja::deps::regexp::syntax;
use goja::deps::regexp::{MERGE_FAILED, compile, compile_one_pass, merge_rune_sets};

struct RuneMergeTest {
    left: &'static [Rune],
    right: &'static [Rune],
    merged: &'static [Rune],
    next: &'static [u32],
    left_pc: u32,
    right_pc: u32,
}

const fn rm(
    left: &'static [Rune],
    right: &'static [Rune],
    merged: &'static [Rune],
    next: &'static [u32],
    left_pc: u32,
    right_pc: u32,
) -> RuneMergeTest {
    RuneMergeTest {
        left,
        right,
        merged,
        next,
        left_pc,
        right_pc,
    }
}

const RUNE_MERGE_TESTS: &[RuneMergeTest] = &[
    rm(&[69, 69], &[], &[69, 69], &[1], 1, 2),
    rm(&[69, 69], &[69, 69], &[], &[MERGE_FAILED], 1, 1),
    rm(&[69, 69], &[69, 69], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 69], &[71, 71], &[69, 69, 71, 71], &[1, 2], 1, 2),
    rm(&[71, 71], &[69, 69], &[69, 69, 71, 71], &[2, 1], 1, 2),
    rm(
        &[60, 60, 71, 71, 101, 101],
        &[69, 69, 88, 88],
        &[60, 60, 69, 69, 71, 71, 88, 88, 101, 101],
        &[1, 2, 1, 2, 1],
        1,
        2,
    ),
    rm(&[69, 74], &[71, 71], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74], &[68, 75], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74], &[74, 75], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74], &[65, 69], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74], &[71, 74], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74], &[65, 71], &[], &[MERGE_FAILED], 1, 2),
    rm(&[69, 74, 60, 65], &[66, 67], &[], &[MERGE_FAILED], 1, 2),
];

#[test]
fn test_merge_rune_set() {
    for (ix, test) in RUNE_MERGE_TESTS.iter().enumerate() {
        let (merged, next) = merge_rune_sets(test.left, test.right, test.left_pc, test.right_pc);
        assert_eq!(merged, test.merged, "mergeRuneSet :{ix} merged");
        assert_eq!(next, test.next, "mergeRuneSet :{ix} next");
    }
}

fn one_pass_tests() -> Vec<(Vec<u8>, bool)> {
    let mut long = b"^l".to_vec();
    long.extend_from_slice(&b"o".repeat(2 << 8));
    long.extend_from_slice(b"ng$");
    let mut tests: Vec<(Vec<u8>, bool)> = [
        (&br"^(?:a|(?:a*))$"[..], false),
        (br"^(?:(a)|(?:a*))$", false),
        (br"^(?:(?:(?:.(?:$))?))$", true),
        (br"^abcd$", true),
        (br"^abcd", true),
        (br"^(?:(?:a{0,})*?)$", false),
        (br"^(?:(?:a+)*)$", true),
        (br"^(?:(?:a|(?:aa)))$", true),
        (br"^(?:[^\s\S])$", true),
        (br"^(?:(?:a{3,4}){0,})$", false),
        (br"^(?:(?:(?:a*)+))$", true),
        (br"^[a-c]+$", true),
        (br"^[a-c]*$", true),
        (br"^(?:a*)$", true),
        (br"^(?:(?:aa)|a)$", true),
        (br"^[a-c]*", false),
        (br"^...$", true),
        (br"^...", true),
        (br"^(?:a|(?:aa))$", true),
        (br"^a((b))c$", true),
        (br"^a.[l-nA-Cg-j]?e$", true),
        (br"^a((b))$", true),
        (br"^a(?:(b)|(c))c$", true),
        (br"^a(?:(b*)|(c))c$", false),
        (br"^a(?:b|c)$", true),
        (br"^a(?:b?|c)$", true),
        (br"^a(?:b?|c?)$", false),
        (br"^a(?:b?|c+)$", true),
        (br"^a(?:b+|(bc))d$", false),
        (br"^a(?:bc)+$", true),
        (br"^a(?:[bcd])+$", true),
        (br"^a((?:[bcd])+)$", true),
        (br"^a(:?b|c)*d$", true),
        (br"^.bc(d|e)*$", true),
        (br"^(?:(?:aa)|.)$", false),
        (br"^(?:(?:a{1,2}){1,2})$", false),
    ]
    .iter()
    .map(|(p, b)| (p.to_vec(), *b))
    .collect();
    tests.push((long, true));
    tests
}

#[test]
fn test_compile_one_pass() {
    for (re, want) in one_pass_tests() {
        let parsed = syntax::parse(&re, syntax::PERL).unwrap_or_else(|e| {
            panic!(
                "Parse({:?}) got err:{e}, want success",
                String::from_utf8_lossy(&re)
            )
        });
        let simple = parsed.simplify();
        let p = syntax::compile(&simple).expect("compile");
        let is_one_pass = compile_one_pass(&p).is_some();
        assert_eq!(
            is_one_pass,
            want,
            "CompileOnePass({:?})",
            String::from_utf8_lossy(&re)
        );
    }
}

const ONE_PASS_TESTS1: &[(&[u8], &[u8])] = &[(br"^a(/b+(#c+)*)*$", b"a/b#c")];

#[test]
fn test_run_one_pass() {
    for (re, m) in ONE_PASS_TESTS1 {
        let re = compile(re).unwrap_or_else(|e| panic!("Compile: got err: {e}"));
        assert!(re.onepass().is_some(), "Compile: got nil, want one-pass");
        assert!(re.match_string(m), "onepass did not match");
    }
}
