use talos::deps::regexp::syntax::{self, PERL};

const COMPILE_TESTS: &[(&[u8], &str)] = &[
    (b"a", "  0\tfail\n  1*\trune1 \"a\" -> 2\n  2\tmatch\n"),
    (
        b"[A-M][n-z]",
        "  0\tfail\n  1*\trune \"AM\" -> 2\n  2\trune \"nz\" -> 3\n  3\tmatch\n",
    ),
    (b"", "  0\tfail\n  1*\tnop -> 2\n  2\tmatch\n"),
    (
        b"a?",
        "  0\tfail\n  1\trune1 \"a\" -> 3\n  2*\talt -> 1, 3\n  3\tmatch\n",
    ),
    (
        b"a??",
        "  0\tfail\n  1\trune1 \"a\" -> 3\n  2*\talt -> 3, 1\n  3\tmatch\n",
    ),
    (
        b"a+",
        "  0\tfail\n  1*\trune1 \"a\" -> 2\n  2\talt -> 1, 3\n  3\tmatch\n",
    ),
    (
        b"a+?",
        "  0\tfail\n  1*\trune1 \"a\" -> 2\n  2\talt -> 3, 1\n  3\tmatch\n",
    ),
    (
        b"a*",
        "  0\tfail\n  1\trune1 \"a\" -> 2\n  2*\talt -> 1, 3\n  3\tmatch\n",
    ),
    (
        b"a*?",
        "  0\tfail\n  1\trune1 \"a\" -> 2\n  2*\talt -> 3, 1\n  3\tmatch\n",
    ),
    (
        b"a+b+",
        "  0\tfail\n  1*\trune1 \"a\" -> 2\n  2\talt -> 1, 3\n  3\trune1 \"b\" -> 4\n  4\talt -> 3, 5\n  5\tmatch\n",
    ),
    (
        b"(a+)(b+)",
        "  0\tfail\n  1*\tcap 2 -> 2\n  2\trune1 \"a\" -> 3\n  3\talt -> 2, 4\n  4\tcap 3 -> 5\n  5\tcap 4 -> 6\n  6\trune1 \"b\" -> 7\n  7\talt -> 6, 8\n  8\tcap 5 -> 9\n  9\tmatch\n",
    ),
    (
        b"a+|b+",
        "  0\tfail\n  1\trune1 \"a\" -> 2\n  2\talt -> 1, 6\n  3\trune1 \"b\" -> 4\n  4\talt -> 3, 6\n  5*\talt -> 1, 3\n  6\tmatch\n",
    ),
    (
        b"A[Aa]",
        "  0\tfail\n  1*\trune1 \"A\" -> 2\n  2\trune \"A\"/i -> 3\n  3\tmatch\n",
    ),
    (
        b"(?:(?:^).)",
        "  0\tfail\n  1*\tempty 4 -> 2\n  2\tanynotnl -> 3\n  3\tmatch\n",
    ),
    (
        b"(?:|a)+",
        "  0\tfail\n  1\tnop -> 4\n  2\trune1 \"a\" -> 4\n  3*\talt -> 1, 2\n  4\talt -> 3, 5\n  5\tmatch\n",
    ),
    (
        b"(?:|a)*",
        "  0\tfail\n  1\tnop -> 4\n  2\trune1 \"a\" -> 4\n  3\talt -> 1, 2\n  4\talt -> 3, 6\n  5*\talt -> 3, 6\n  6\tmatch\n",
    ),
];

#[test]
fn test_compile() {
    for (regexp, want) in COMPILE_TESTS {
        let re = syntax::parse(regexp, PERL).expect("parse");
        let p = syntax::compile(&re).expect("compile");
        let s = p.to_string();
        assert_eq!(s, *want, "compiled {:?}", String::from_utf8_lossy(regexp));
    }
}
