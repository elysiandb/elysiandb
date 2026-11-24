use super::iter::Iter;
use super::normalize::Form;
use super::normalize_test::{CGJ, SEG_SIZE, cat, grave, rep, run_norm_tests};

fn do_iter_norm_string(f: Form, s: &[u8]) -> Vec<u8> {
    let mut acc = Vec::new();
    let mut i = Iter::new();
    i.init_string(f, s);
    while !i.done() {
        acc.extend_from_slice(i.next());
    }
    acc
}

fn do_iter_norm(f: Form, s: &[u8]) -> Vec<u8> {
    let mut acc = Vec::new();
    let mut i = Iter::new();
    i.init(f, s);
    while !i.done() {
        acc.extend_from_slice(i.next());
    }
    acc
}

#[test]
fn test_iter_next() {
    run_norm_tests("IterNext", |f, out, s| {
        do_iter_norm_string(f, &cat(&[&out, s]))
    });
    run_norm_tests("IterNext", |f, out, s| do_iter_norm(f, &cat(&[&out, s])));
}

type SegmentTest = (Vec<u8>, Vec<Vec<u8>>);

fn split(s: &[u8]) -> Vec<Vec<u8>> {
    s.iter().map(|&b| vec![b]).collect()
}

fn segs(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|p| p.to_vec()).collect()
}

fn segment_tests() -> Vec<SegmentTest> {
    vec![
        (
            "\u{1e0a}\u{323}a".as_bytes().to_vec(),
            segs(&["\u{44}\u{323}\u{307}".as_bytes(), b"a", b""]),
        ),
        (
            rep(0x61, SEG_SIZE),
            [split(&rep(0x61, SEG_SIZE)), vec![Vec::new()]].concat(),
        ),
        (
            rep(0x61, SEG_SIZE + 2),
            [split(&rep(0x61, SEG_SIZE + 2)), vec![Vec::new()]].concat(),
        ),
        (
            cat(&[&rep(0x61, SEG_SIZE), "\u{300}aa".as_bytes()]),
            [
                split(&rep(0x61, SEG_SIZE - 1)),
                segs(&["a\u{300}".as_bytes(), b"a", b"a", b""]),
            ]
            .concat(),
        ),
        (
            cat(&[b"a", &grave(29), "\u{f73}".as_bytes()]),
            vec![cat(&[b"a", &grave(29)]), cat(&[CGJ, "\u{f73}".as_bytes()])],
        ),
        (
            "a\u{f73}".as_bytes().to_vec(),
            segs(&["a\u{f73}".as_bytes()]),
        ),
        (
            cat(&[b"a", &grave(30), "\u{ff9e}".as_bytes()]),
            vec![cat(&[b"a", &grave(30)]), cat(&[CGJ, "\u{ff9e}".as_bytes()])],
        ),
        (
            "a\u{ff9e}".as_bytes().to_vec(),
            segs(&["a\u{ff9e}".as_bytes()]),
        ),
    ]
}

fn fdfa_segments() -> Vec<Vec<u8>> {
    segs(&[
        "\u{635}".as_bytes(),
        "\u{644}".as_bytes(),
        "\u{649}".as_bytes(),
        b" ",
        "\u{627}".as_bytes(),
        "\u{644}".as_bytes(),
        "\u{644}".as_bytes(),
        "\u{647}".as_bytes(),
        b" ",
        "\u{639}".as_bytes(),
        "\u{644}".as_bytes(),
        "\u{64a}".as_bytes(),
        "\u{647}".as_bytes(),
        b" ",
        "\u{648}".as_bytes(),
        "\u{633}".as_bytes(),
        "\u{644}".as_bytes(),
    ])
}

fn segment_tests_k() -> Vec<SegmentTest> {
    vec![
        (
            "\u{3332}".as_bytes().to_vec(),
            segs(&[
                "\u{30d5}".as_bytes(),
                "\u{30a1}".as_bytes(),
                "\u{30e9}".as_bytes(),
                "\u{30c3}".as_bytes(),
                "\u{30c8}\u{3099}".as_bytes(),
                b"",
            ]),
        ),
        (
            "\u{3332}\u{93c}".as_bytes().to_vec(),
            segs(&[
                "\u{30d5}".as_bytes(),
                "\u{30a1}".as_bytes(),
                "\u{30e9}".as_bytes(),
                "\u{30c3}".as_bytes(),
                "\u{30c8}\u{93c}\u{3099}".as_bytes(),
                b"",
            ]),
        ),
        (
            "\u{320e}".as_bytes().to_vec(),
            segs(&[b"\x28", "\u{ac00}".as_bytes(), b"\x29"]),
        ),
        (
            "\u{fdfa}".as_bytes().to_vec(),
            [fdfa_segments(), segs(&["\u{645}".as_bytes(), b""])].concat(),
        ),
        (
            cat(&["\u{fdfa}".as_bytes(), &grave(30)]),
            [
                fdfa_segments(),
                vec![cat(&["\u{645}".as_bytes(), &grave(30)]), Vec::new()],
            ]
            .concat(),
        ),
        (
            cat(&["\u{fdfa}".as_bytes(), &grave(64)]),
            [
                fdfa_segments(),
                vec![
                    cat(&["\u{645}".as_bytes(), &grave(30)]),
                    cat(&[CGJ, &grave(30)]),
                    cat(&[CGJ, &grave(4)]),
                    Vec::new(),
                ],
            ]
            .concat(),
        ),
        (
            "\u{ac00}".as_bytes().to_vec(),
            segs(&["\u{1100}\u{1161}".as_bytes(), b""]),
        ),
        (
            "\u{ac01}".as_bytes().to_vec(),
            segs(&["\u{1100}\u{1161}\u{11a8}".as_bytes(), b""]),
        ),
        (
            "\u{1100}\u{1161}".as_bytes().to_vec(),
            segs(&["\u{1100}\u{1161}".as_bytes(), b""]),
        ),
    ]
}

#[test]
fn test_iter_segmentation() {
    segment_test("SegmentTestD", Form::Nfd, &segment_tests());
    segment_test("SegmentTestC", Form::Nfc, &segment_tests());
    segment_test("SegmentTestKD", Form::Nfkd, &segment_tests_k());
    segment_test("SegmentTestKC", Form::Nfkc, &segment_tests_k());
}

fn segment_test(name: &str, f: Form, tests: &[SegmentTest]) {
    let mut iter = Iter::new();
    for (i, tt) in tests.iter().enumerate() {
        iter.init_string(f, &tt.0);
        for (j, seg) in tt.1.iter().enumerate() {
            if seg.is_empty() {
                if !iter.done() {
                    let res = iter.next().to_vec();
                    panic!("{name}:{i}:{j}: expected Done()==true, found segment {res:?}");
                }
                continue;
            }
            assert!(!iter.done(), "{name}:{i}:{j}: Done()==true, want false");
            let seg = f.string(seg).into_owned();
            assert_eq!(iter.next(), seg.as_slice(), "{name}:{i}:{j}: segment");
        }
    }
}
