use gocompat::utf8::{self, Rune};

use super::composition::{Flusher, GRAPHEME_JOINER, ReorderBuffer};
use super::normalize::{Form, decompose_segment, decompose_to_last_boundary};
use super::transform::Error;

pub(super) const CGJ: &[u8] = GRAPHEME_JOINER.as_bytes();
pub(super) const SEG_SIZE: usize = super::composition::MAX_BYTE_BUFFER_SIZE;
pub(super) const FORMS: [Form; 4] = [Form::Nfc, Form::Nfd, Form::Nfkc, Form::Nfkd];

pub(super) fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub(super) fn rep(r: Rune, n: usize) -> Vec<u8> {
    let mut b = Vec::new();
    utf8::append_rune(&mut b, r);
    b.repeat(n)
}

pub(super) fn grave(n: usize) -> Vec<u8> {
    rep(0x0300, n)
}

pub(super) fn repeat(s: &[u8], n: usize) -> Vec<u8> {
    s.repeat(n)
}

fn ascii() -> Vec<u8> {
    b"There is nothing to change here! ".repeat(500)
}

type PositionTest = (Vec<u8>, isize, Vec<u8>);

type PositionFunc = for<'a, 'b> fn(&'b mut ReorderBuffer<'a>, Form, &'a [u8]) -> (isize, Vec<u8>);

fn run_pos_tests(name: &str, f: Form, func: PositionFunc, tests: &[PositionTest]) {
    let mut rb = ReorderBuffer::new(f, &[]);
    for (i, test) in tests.iter().enumerate() {
        rb.reset();
        rb.src = &test.0;
        rb.nsrc = test.0.len();
        let (pos, out) = func(&mut rb, f, &test.0);
        assert_eq!(pos, test.1, "{name}:{i}: position");
        assert_eq!(out, test.2, "{name}:{i}: buffer");
    }
}

fn flush(rb: &mut ReorderBuffer<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..rb.nrune {
        let start = usize::from(rb.rune[i].pos);
        let end = start + usize::from(rb.rune[i].size);
        out.extend_from_slice(&rb.byte[start..end]);
    }
    rb.reset();
    out
}

fn decompose_segment_f<'a>(rb: &mut ReorderBuffer<'a>, _f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    rb.init(Form::Nfd, s);
    rb.set_flusher(Vec::new(), Flusher::Append);
    let p = decompose_segment(rb, 0, true);
    (p, rb.out.clone())
}

#[test]
fn test_decompose_segment() {
    run_pos_tests(
        "TestDecomposeSegment",
        Form::Nfc,
        decompose_segment_f,
        &decompose_segment_tests(),
    );
}

fn first_boundary_f<'a>(_rb: &mut ReorderBuffer<'a>, f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    (f.first_boundary(s), Vec::new())
}

fn first_boundary_string_f<'a>(
    _rb: &mut ReorderBuffer<'a>,
    f: Form,
    s: &'a [u8],
) -> (isize, Vec<u8>) {
    (f.first_boundary_in_string(s), Vec::new())
}

#[test]
fn test_first_boundary() {
    run_pos_tests(
        "TestFirstBoundary",
        Form::Nfc,
        first_boundary_f,
        &first_boundary_tests(),
    );
    run_pos_tests(
        "TestFirstBoundaryInString",
        Form::Nfc,
        first_boundary_string_f,
        &first_boundary_tests(),
    );
}

#[test]
fn test_next_boundary() {
    for (input, at_eof, want) in next_boundary_tests() {
        assert_eq!(
            Form::Nfc.next_boundary(&input, at_eof),
            want,
            "NextBoundary({input:?}, {at_eof})"
        );
        assert_eq!(
            Form::Nfc.next_boundary_in_string(&input, at_eof),
            want,
            "NextBoundaryInString({input:?}, {at_eof})"
        );
    }
}

fn decompose_to_last<'a>(rb: &mut ReorderBuffer<'a>, _f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    rb.set_flusher(s.to_vec(), Flusher::Append);
    decompose_to_last_boundary(rb);
    let buf = flush(rb);
    (rb.out.len() as isize, buf)
}

#[test]
fn test_decompose_to_last_boundary() {
    run_pos_tests(
        "TestDecomposeToLastBoundary",
        Form::Nfkc,
        decompose_to_last,
        &decompose_to_last_tests(),
    );
}

fn last_boundary_f<'a>(_rb: &mut ReorderBuffer<'a>, f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    (f.last_boundary(s), Vec::new())
}

#[test]
fn test_last_boundary() {
    run_pos_tests(
        "TestLastBoundary",
        Form::Nfc,
        last_boundary_f,
        &last_boundary_tests(),
    );
}

type SpanTest = (Vec<u8>, bool, usize, Result<(), Error>);

fn run_span_tests(name: &str, f: Form, tests: &[SpanTest]) {
    for (i, tc) in tests.iter().enumerate() {
        assert_eq!(
            f.span(&tc.0, tc.1),
            (tc.2, tc.3),
            "Bytes/{name}/{i}={:?}/atEOF={}",
            tc.0,
            tc.1
        );
        assert_eq!(
            f.span_string(&tc.0, tc.1),
            (tc.2, tc.3),
            "String/{name}/{i}={:?}/atEOF={}",
            tc.0,
            tc.1
        );
    }
}

#[test]
fn test_span() {
    run_span_tests("NFD", Form::Nfd, &quick_span_tests());
    run_span_tests("NFD", Form::Nfd, &quick_span_nfd_tests());
    run_span_tests("NFC", Form::Nfc, &quick_span_tests());
    run_span_tests("NFC", Form::Nfc, &quick_span_nfc_tests());
}

fn is_normal_f<'a>(_rb: &mut ReorderBuffer<'a>, f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    (isize::from(f.is_normal(s)), Vec::new())
}

fn is_normal_string_f<'a>(_rb: &mut ReorderBuffer<'a>, f: Form, s: &'a [u8]) -> (isize, Vec<u8>) {
    (isize::from(f.is_normal_string(s)), Vec::new())
}

#[test]
fn test_is_normal() {
    run_pos_tests(
        "TestIsNormalNFD1",
        Form::Nfd,
        is_normal_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFD2",
        Form::Nfd,
        is_normal_f,
        &is_normal_nfd_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFC1",
        Form::Nfc,
        is_normal_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFC2",
        Form::Nfc,
        is_normal_f,
        &is_normal_nfc_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKD1",
        Form::Nfkd,
        is_normal_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKD2",
        Form::Nfkd,
        is_normal_f,
        &is_normal_nfd_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKD3",
        Form::Nfkd,
        is_normal_f,
        &is_normal_nfkx_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKC1",
        Form::Nfkc,
        is_normal_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKC2",
        Form::Nfkc,
        is_normal_f,
        &is_normal_nfc_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFKC3",
        Form::Nfkc,
        is_normal_f,
        &is_normal_nfkx_tests(),
    );
}

#[test]
fn test_is_normal_string() {
    run_pos_tests(
        "TestIsNormalNFD1",
        Form::Nfd,
        is_normal_string_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFD2",
        Form::Nfd,
        is_normal_string_f,
        &is_normal_nfd_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFC1",
        Form::Nfc,
        is_normal_string_f,
        &is_normal_tests(),
    );
    run_pos_tests(
        "TestIsNormalNFC2",
        Form::Nfc,
        is_normal_string_f,
        &is_normal_nfc_tests(),
    );
}

type AppendTest = (Vec<u8>, Vec<u8>, Vec<u8>);

pub(super) type AppendFunc = fn(Form, Vec<u8>, &[u8]) -> Vec<u8>;

const FSTR: [&str; 4] = ["NFC", "NFD", "NFKC", "NFKD"];

fn norm_tests() -> [Vec<AppendTest>; 4] {
    [
        append_tests_nfc(),
        Vec::new(),
        append_tests_nfkc(),
        append_tests_nfkd(),
    ]
}

pub(super) fn run_norm_tests(name: &str, func: AppendFunc) {
    let tests = norm_tests();
    for (k, f) in FORMS.iter().enumerate() {
        run_append_tests(name, *f, func, &tests[k]);
    }
}

fn run_append_tests(name: &str, f: Form, func: AppendFunc, tests: &[AppendTest]) {
    for (i, test) in tests.iter().enumerate() {
        let have = func(f, test.0.clone(), &test.1);
        assert_eq!(
            have.len(),
            test.2.len(),
            "{name}/{}/{i}/fn: length",
            FSTR[f as usize]
        );
        assert_eq!(have, test.2, "{name}/{}/{i}/fn", FSTR[f as usize]);
        for g in FORMS {
            if f == g {
                continue;
            }
            let joined = cat(&[&test.0, &test.1]);
            let want = g.string(&joined).into_owned();
            let have = func(g, g.append_string(Vec::new(), &test.0), &test.1);
            assert_eq!(
                have.len(),
                want.len(),
                "{name}/{}/{i}/{}: length",
                FSTR[f as usize],
                FSTR[g as usize]
            );
            assert_eq!(
                have, want,
                "{name}/{}/{i}/{}",
                FSTR[f as usize], FSTR[g as usize]
            );
        }
    }
}

#[test]
fn test_append() {
    run_norm_tests("Append", |f, out, s| f.append(out, s));
}

#[test]
fn test_append_string() {
    run_norm_tests("AppendString", |f, out, s| f.append_string(out, s));
}

#[test]
fn test_bytes() {
    run_norm_tests("Bytes", |f, out, s| {
        let buf = cat(&[&out, s]);
        f.bytes(&buf).into_owned()
    });
}

#[test]
fn test_string() {
    run_norm_tests("String", |f, out, s| {
        let outs = cat(&[&out, s]);
        f.string(&outs).into_owned()
    });
}

fn decompose_segment_tests() -> Vec<PositionTest> {
    vec![
        (b"\xc2".to_vec(), 0, b"".to_vec()),
        (b"\xc0".to_vec(), 1, b"\xc0".to_vec()),
        (b"\xc3\xa0\x80".to_vec(), 2, "a\u{300}".as_bytes().to_vec()),
        (b"a".to_vec(), 1, b"a".to_vec()),
        (b"ab".to_vec(), 1, b"a".to_vec()),
        (
            "a\u{300}".as_bytes().to_vec(),
            3,
            "a\u{300}".as_bytes().to_vec(),
        ),
        (
            "a\u{300}b".as_bytes().to_vec(),
            3,
            "a\u{300}".as_bytes().to_vec(),
        ),
        (
            "\u{c0}".as_bytes().to_vec(),
            2,
            "A\u{300}".as_bytes().to_vec(),
        ),
        (
            "\u{c0}b".as_bytes().to_vec(),
            2,
            "A\u{300}".as_bytes().to_vec(),
        ),
        (grave(31), 60, cat(&[&grave(30), CGJ])),
        (cat(&[b"a", &grave(31)]), 61, cat(&[b"a", &grave(30), CGJ])),
        (
            "\u{b47}\u{300}\u{b3e}".as_bytes().to_vec(),
            8,
            "\u{b47}\u{300}\u{b3e}".as_bytes().to_vec(),
        ),
        (
            "\u{1100}\u{300}\u{1161}".as_bytes().to_vec(),
            8,
            "\u{1100}\u{300}\u{1161}".as_bytes().to_vec(),
        ),
        (
            "\u{b47}\u{b3e}".as_bytes().to_vec(),
            6,
            "\u{b47}\u{b3e}".as_bytes().to_vec(),
        ),
        (
            "\u{1100}\u{1161}".as_bytes().to_vec(),
            6,
            "\u{1100}\u{1161}".as_bytes().to_vec(),
        ),
        (
            cat(&["\u{d4a}".as_bytes(), &repeat("\u{d3e}".as_bytes(), 31)]),
            90,
            cat(&["\u{d46}".as_bytes(), &repeat("\u{d3e}".as_bytes(), 30), CGJ]),
        ),
        (grave(30), 60, grave(30)),
        (
            cat(&[&grave(30), "\u{ff9e}".as_bytes()]),
            60,
            cat(&[&grave(30), CGJ]),
        ),
        (b"\xcc".to_vec(), 0, b"".to_vec()),
        (b"\xcc\x80\xcc".to_vec(), 2, "\u{300}".as_bytes().to_vec()),
    ]
}

fn first_boundary_tests() -> Vec<PositionTest> {
    vec![
        (b"".to_vec(), -1, b"".to_vec()),
        ("\u{300}".as_bytes().to_vec(), -1, b"".to_vec()),
        (b"\x80\x80".to_vec(), -1, b"".to_vec()),
        (b"\xff".to_vec(), 0, b"".to_vec()),
        (b"\xcc\x80\xff".to_vec(), 2, b"".to_vec()),
        (b"\xcc\x80\xc0\x80\x80".to_vec(), 2, b"".to_vec()),
        (b"a".to_vec(), 0, b"".to_vec()),
        ("\u{300}a".as_bytes().to_vec(), 2, b"".to_vec()),
        ("\u{1103}\u{1161}".as_bytes().to_vec(), 0, b"".to_vec()),
        (
            "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
            0,
            b"".to_vec(),
        ),
        (
            "\u{1161}\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
            3,
            b"".to_vec(),
        ),
        (
            "\u{1173}\u{11b7}\u{1103}\u{1161}".as_bytes().to_vec(),
            6,
            b"".to_vec(),
        ),
        (grave(29), -1, b"".to_vec()),
        (grave(30), 60, b"".to_vec()),
        (grave(31), 60, b"".to_vec()),
    ]
}

fn decompose_to_last_tests() -> Vec<PositionTest> {
    vec![
        (b"Hello!".to_vec(), 6, b"".to_vec()),
        ("\u{632}".as_bytes().to_vec(), 2, b"".to_vec()),
        ("a\u{301}\u{635}".as_bytes().to_vec(), 5, b"".to_vec()),
        (b"a".to_vec(), 0, b"a".to_vec()),
        ("a\u{301}a".as_bytes().to_vec(), 3, b"a".to_vec()),
        (
            "a\u{301}\u{3b9}".as_bytes().to_vec(),
            3,
            "\u{3b9}".as_bytes().to_vec(),
        ),
        (
            "a\u{327}".as_bytes().to_vec(),
            0,
            "a\u{327}".as_bytes().to_vec(),
        ),
        (b"\xff".to_vec(), 1, b"".to_vec()),
        (b"aa\xff".to_vec(), 3, b"".to_vec()),
        (b"\xc0\x80\x80".to_vec(), 3, b"".to_vec()),
        (b"\xcc\x80\x80".to_vec(), 3, b"".to_vec()),
        (b"a\xcc".to_vec(), 2, b"".to_vec()),
        (
            "\u{300}\u{301}".as_bytes().to_vec(),
            0,
            "\u{300}\u{301}".as_bytes().to_vec(),
        ),
        (
            "a\u{300}\u{301}".as_bytes().to_vec(),
            0,
            "a\u{300}\u{301}".as_bytes().to_vec(),
        ),
        (
            "a\u{301}\u{308}".as_bytes().to_vec(),
            0,
            "a\u{301}\u{308}".as_bytes().to_vec(),
        ),
        (
            "a\u{308}\u{301}".as_bytes().to_vec(),
            0,
            "a\u{308}\u{301}".as_bytes().to_vec(),
        ),
        (
            "aaaa\u{300}\u{301}".as_bytes().to_vec(),
            3,
            "a\u{300}\u{301}".as_bytes().to_vec(),
        ),
        (
            "\u{300}a\u{300}\u{301}".as_bytes().to_vec(),
            2,
            "a\u{300}\u{301}".as_bytes().to_vec(),
        ),
        (
            "\u{c0}".as_bytes().to_vec(),
            0,
            "A\u{300}".as_bytes().to_vec(),
        ),
        (
            "a\u{c0}".as_bytes().to_vec(),
            1,
            "A\u{300}".as_bytes().to_vec(),
        ),
        (
            "a\u{300}\u{e0}".as_bytes().to_vec(),
            3,
            "a\u{300}".as_bytes().to_vec(),
        ),
        (
            "a\u{300}\u{fdc0}".as_bytes().to_vec(),
            7,
            "\u{64a}".as_bytes().to_vec(),
        ),
        (
            cat(&["\u{fdc0}".as_bytes(), &grave(29)]),
            4,
            cat(&["\u{64a}".as_bytes(), &grave(29)]),
        ),
        (
            cat(&["\u{fdc0}".as_bytes(), &grave(30)]),
            4,
            cat(&["\u{64a}".as_bytes(), &grave(30)]),
        ),
        (cat(&["\u{fdc0}".as_bytes(), &grave(31)]), 5, grave(30)),
        (
            cat(&["\u{fdfa}".as_bytes(), &grave(14)]),
            31,
            cat(&["\u{645}".as_bytes(), &grave(14)]),
        ),
        (
            cat(&["\u{e0}".as_bytes(), &grave(29)]),
            0,
            cat(&[b"a", &grave(30)]),
        ),
        (cat(&["\u{e0}".as_bytes(), &grave(30)]), 2, grave(30)),
        (
            "a\u{1103}".as_bytes().to_vec(),
            1,
            "\u{1103}".as_bytes().to_vec(),
        ),
        (
            "a\u{110b}".as_bytes().to_vec(),
            1,
            "\u{110b}".as_bytes().to_vec(),
        ),
        (
            "a\u{110b}\u{1173}".as_bytes().to_vec(),
            1,
            "\u{110b}\u{1173}".as_bytes().to_vec(),
        ),
        (
            "a\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
            1,
            "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
        ),
        (
            "a\u{c73c}".as_bytes().to_vec(),
            1,
            "\u{110b}\u{1173}".as_bytes().to_vec(),
        ),
        (
            "\u{b2e4}\u{c74c}".as_bytes().to_vec(),
            3,
            "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
        ),
        (
            "\u{b2e4}".as_bytes().to_vec(),
            0,
            "\u{1103}\u{1161}".as_bytes().to_vec(),
        ),
        (
            "\u{1103}\u{1161}\u{110b}\u{1173}\u{11b7}"
                .as_bytes()
                .to_vec(),
            6,
            "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
        ),
        (
            "\u{110b}\u{1173}\u{11b7}\u{1103}\u{1161}"
                .as_bytes()
                .to_vec(),
            9,
            "\u{1103}\u{1161}".as_bytes().to_vec(),
        ),
        (
            "\u{b2e4}\u{c74c}\u{c74c}".as_bytes().to_vec(),
            6,
            "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
        ),
        (
            "\u{c74c}\u{b2e4}\u{b2e4}".as_bytes().to_vec(),
            6,
            "\u{1103}\u{1161}".as_bytes().to_vec(),
        ),
        (cat(&[b"a", &grave(30)]), 0, cat(&[b"a", &grave(30)])),
        (cat(&[b"a", &grave(31)]), 3, grave(30)),
        (
            "a\u{300}\u{11b7}".as_bytes().to_vec(),
            0,
            "a\u{300}\u{11b7}".as_bytes().to_vec(),
        ),
    ]
}

fn last_boundary_tests() -> Vec<PositionTest> {
    vec![
        (b"Hello!".to_vec(), 6, b"".to_vec()),
        ("\u{632}".as_bytes().to_vec(), 2, b"".to_vec()),
        (b"a".to_vec(), 0, b"".to_vec()),
        (b"\xff".to_vec(), 1, b"".to_vec()),
        (b"aa\xff".to_vec(), 3, b"".to_vec()),
        (b"a\xff\xcc\x80".to_vec(), 1, b"".to_vec()),
        (b"\xc0\x80\x80".to_vec(), 3, b"".to_vec()),
        (b"\xc0\x80\x80\xcc\x80".to_vec(), 3, b"".to_vec()),
        (b"\xcc".to_vec(), -1, b"".to_vec()),
        (b"\xe0\x80".to_vec(), -1, b"".to_vec()),
        (b"\xf0\x80\x80".to_vec(), -1, b"".to_vec()),
        (b"a\xcc".to_vec(), 0, b"".to_vec()),
        (b"\x80\xcc".to_vec(), 1, b"".to_vec()),
        (b"\xcc\xcc".to_vec(), 1, b"".to_vec()),
        ("a\u{300}\u{301}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("aaaa\u{300}\u{301}".as_bytes().to_vec(), 3, b"".to_vec()),
        (
            "\u{300}a\u{300}\u{301}".as_bytes().to_vec(),
            2,
            b"".to_vec(),
        ),
        ("\u{c2}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("a\u{c2}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{226}".as_bytes().to_vec(), 0, b"".to_vec()),
        (b"".to_vec(), -1, b"".to_vec()),
        ("\u{300}\u{301}".as_bytes().to_vec(), -1, b"".to_vec()),
        ("\u{300}".as_bytes().to_vec(), -1, b"".to_vec()),
        (b"\x80\x80".to_vec(), -1, b"".to_vec()),
        (b"\x80\x80\xcc\x81".to_vec(), -1, b"".to_vec()),
        ("\u{b2e4}\u{c74c}".as_bytes().to_vec(), 3, b"".to_vec()),
        ("\u{b2e4}".as_bytes().to_vec(), 0, b"".to_vec()),
        (
            "\u{1103}\u{1161}\u{110b}\u{1173}\u{11b7}"
                .as_bytes()
                .to_vec(),
            6,
            b"".to_vec(),
        ),
        (
            "\u{110b}\u{1173}\u{11b7}\u{1103}\u{1161}"
                .as_bytes()
                .to_vec(),
            9,
            b"".to_vec(),
        ),
        (grave(29), -1, b"".to_vec()),
        (grave(30), -1, b"".to_vec()),
        (grave(31), 2, b"".to_vec()),
        (grave(32), 4, b"".to_vec()),
        (cat(&[b"a", &grave(29)]), 0, b"".to_vec()),
        (cat(&[b"a", &grave(30)]), 0, b"".to_vec()),
        (cat(&[b"a", &grave(31)]), 3, b"".to_vec()),
        (cat(&[b"a", &grave(32)]), 5, b"".to_vec()),
    ]
}

fn is_normal_tests() -> Vec<PositionTest> {
    vec![
        (b"".to_vec(), 1, b"".to_vec()),
        (b"\xff".to_vec(), 1, b"".to_vec()),
        (b"a".to_vec(), 1, b"".to_vec()),
        (b"abc".to_vec(), 1, b"".to_vec()),
        ("\u{43e}b".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{300}\u{316}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("ab\u{300}\u{316}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("ab\u{300}\u{316}cd".as_bytes().to_vec(), 0, b"".to_vec()),
        ("\u{300}\u{316}cd".as_bytes().to_vec(), 0, b"".to_vec()),
    ]
}

fn is_normal_nfd_tests() -> Vec<PositionTest> {
    vec![
        ("\u{c0}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("abc\u{c0}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{316}\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{316}\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{316}\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{316}\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{43e}\u{308}b".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{ac19}\u{c740}".as_bytes().to_vec(), 0, b"".to_vec()),
    ]
}

fn is_normal_nfc_tests() -> Vec<PositionTest> {
    vec![
        ("\u{c0}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("abc\u{c0}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("a\u{300}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("a\u{300}cd".as_bytes().to_vec(), 0, b"".to_vec()),
        ("a\u{316}\u{300}".as_bytes().to_vec(), 0, b"".to_vec()),
        ("a\u{316}\u{300}cd".as_bytes().to_vec(), 0, b"".to_vec()),
        ("ab\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{316}\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("ab\u{316}\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{c0}\u{35d}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{300}".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{316}\u{300}cd".as_bytes().to_vec(), 1, b"".to_vec()),
        ("\u{ac19}\u{c740}".as_bytes().to_vec(), 1, b"".to_vec()),
    ]
}

fn is_normal_nfkx_tests() -> Vec<PositionTest> {
    vec![("\u{bc}".as_bytes().to_vec(), 0, b"".to_vec())]
}

fn quick_span_tests() -> Vec<SpanTest> {
    vec![
        (b"".to_vec(), true, 0, Ok(())),
        (b"a".to_vec(), true, 1, Ok(())),
        (b"abc".to_vec(), true, 3, Ok(())),
        ("\u{43e}b".as_bytes().to_vec(), true, 3, Ok(())),
        (b"\xcc".to_vec(), true, 1, Ok(())),
        (b"\xcc".to_vec(), false, 0, Err(Error::ShortSrc)),
        (b"a\xcc".to_vec(), true, 2, Ok(())),
        (b"a\xcc".to_vec(), false, 0, Err(Error::ShortSrc)),
        (
            "\u{300}\u{316}".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{300}\u{316}".as_bytes().to_vec(),
            false,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{300}\u{316}cd".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{300}\u{316}cd".as_bytes().to_vec(),
            false,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            cat(&[&rep(0x35d, 30), "\u{35b}".as_bytes()]),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            cat(&[b"a", &rep(0x35d, 30), "\u{35b}".as_bytes()]),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            cat(&["\u{19f}".as_bytes(), &rep(0x35d, 30), "\u{35b}".as_bytes()]),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            cat(&[b"aa", &rep(0x35d, 30), "\u{35b}".as_bytes()]),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            cat(&[&rep(0x35d, 30), CGJ, "\u{35b}".as_bytes()]),
            true,
            64,
            Ok(()),
        ),
        (
            cat(&[b"a", &rep(0x35d, 30), CGJ, "\u{35b}".as_bytes()]),
            true,
            65,
            Ok(()),
        ),
        (
            cat(&[
                "\u{19f}".as_bytes(),
                &rep(0x35d, 30),
                CGJ,
                "\u{35b}".as_bytes(),
            ]),
            true,
            66,
            Ok(()),
        ),
        (
            cat(&[b"aa", &rep(0x35d, 30), CGJ, "\u{35b}".as_bytes()]),
            true,
            66,
            Ok(()),
        ),
        (
            cat(&[b"a", &rep(0x35d, 30), CGJ, "\u{35b}".as_bytes()]),
            false,
            61,
            Err(Error::ShortSrc),
        ),
        (
            cat(&[
                "\u{19f}".as_bytes(),
                &rep(0x35d, 30),
                CGJ,
                "\u{35b}".as_bytes(),
            ]),
            false,
            62,
            Err(Error::ShortSrc),
        ),
        (
            cat(&[b"aa", &rep(0x35d, 30), CGJ, "\u{35b}".as_bytes()]),
            false,
            62,
            Err(Error::ShortSrc),
        ),
    ]
}

fn quick_span_nfd_tests() -> Vec<SpanTest> {
    vec![
        ("\u{c0}".as_bytes().to_vec(), true, 0, Err(Error::EndOfSpan)),
        (
            "abc\u{c0}".as_bytes().to_vec(),
            true,
            3,
            Err(Error::EndOfSpan),
        ),
        ("\u{300}".as_bytes().to_vec(), true, 2, Ok(())),
        ("ab\u{300}".as_bytes().to_vec(), true, 4, Ok(())),
        ("ab\u{300}cd".as_bytes().to_vec(), true, 6, Ok(())),
        ("\u{300}cd".as_bytes().to_vec(), true, 4, Ok(())),
        ("\u{316}\u{300}".as_bytes().to_vec(), true, 4, Ok(())),
        ("ab\u{316}\u{300}".as_bytes().to_vec(), true, 6, Ok(())),
        ("ab\u{316}\u{300}cd".as_bytes().to_vec(), true, 8, Ok(())),
        (
            "ab\u{316}\u{300}\u{c0}".as_bytes().to_vec(),
            true,
            6,
            Err(Error::EndOfSpan),
        ),
        ("\u{316}\u{300}cd".as_bytes().to_vec(), true, 6, Ok(())),
        ("\u{43e}\u{308}b".as_bytes().to_vec(), true, 5, Ok(())),
        (
            "ab\u{300}\u{316}".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{300}\u{316}cd".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{ac19}\u{c740}".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
    ]
}

fn quick_span_nfc_tests() -> Vec<SpanTest> {
    vec![
        ("\u{c0}".as_bytes().to_vec(), true, 2, Ok(())),
        ("abc\u{c0}".as_bytes().to_vec(), true, 5, Ok(())),
        (
            "ab\u{300}".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{300}cd".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{316}\u{300}".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{316}\u{300}cd".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        ("\u{c0}\u{35d}".as_bytes().to_vec(), true, 4, Ok(())),
        (
            "\u{300}cd".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{300}".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{316}\u{300}".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "\u{316}\u{300}cd".as_bytes().to_vec(),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{300}\u{316}".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        (
            "ab\u{300}\u{316}cd".as_bytes().to_vec(),
            true,
            1,
            Err(Error::EndOfSpan),
        ),
        ("\u{ac19}\u{c740}".as_bytes().to_vec(), true, 6, Ok(())),
        (
            "\u{ac19}\u{c740}".as_bytes().to_vec(),
            false,
            3,
            Err(Error::ShortSrc),
        ),
        (
            cat(&[&grave(30), "\u{ff9e}".as_bytes()]),
            true,
            0,
            Err(Error::EndOfSpan),
        ),
        (grave(30), true, 0, Err(Error::EndOfSpan)),
    ]
}

fn append_tests_nfc() -> Vec<AppendTest> {
    vec![
        (b"".to_vec(), ascii(), ascii()),
        (
            b"".to_vec(),
            TXT_ALL.as_bytes().to_vec(),
            TXT_ALL.as_bytes().to_vec(),
        ),
        (
            "\u{ff9e}".as_bytes().to_vec(),
            grave(30),
            cat(&["\u{ff9e}".as_bytes(), &grave(29), CGJ, &grave(1)]),
        ),
        (
            grave(30),
            "\u{ff9e}".as_bytes().to_vec(),
            cat(&[&grave(30), CGJ, "\u{ff9e}".as_bytes()]),
        ),
        (
            b"".to_vec(),
            "\u{305}\u{316}".as_bytes().to_vec(),
            "\u{316}\u{305}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            cat(&[b"a", &rep(0x305, 34), "\u{316}".as_bytes()]),
            cat(&[
                b"a",
                &rep(0x305, 30),
                CGJ,
                "\u{316}".as_bytes(),
                &rep(0x305, 4),
            ]),
        ),
        (
            b"".to_vec(),
            "a\u{327}\u{325}".as_bytes().to_vec(),
            "\u{1e01}\u{327}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            "\u{1161}\u{11a8}".as_bytes().to_vec(),
            "\u{1161}\u{11a8}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            "\u{b47}\u{300}\u{b3e}".as_bytes().to_vec(),
            "\u{b47}\u{300}\u{b3e}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            "\u{1100}\u{300}\u{1161}".as_bytes().to_vec(),
            "\u{1100}\u{300}\u{1161}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            "\u{b47}\u{b3e}".as_bytes().to_vec(),
            "\u{b4b}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            "\u{1100}\u{1161}".as_bytes().to_vec(),
            "\u{ac00}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            cat(&[
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 15),
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 15),
            ]),
            cat(&[
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 15),
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 15),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&["\u{d46}".as_bytes(), &repeat("\u{d3e}".as_bytes(), 31)]),
            cat(&[
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 29),
                CGJ,
                "\u{d3e}".as_bytes(),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&["\u{d4a}".as_bytes(), &repeat("\u{d3e}".as_bytes(), 30)]),
            cat(&[
                "\u{d4a}".as_bytes(),
                &repeat("\u{d3e}".as_bytes(), 29),
                CGJ,
                "\u{d3e}".as_bytes(),
            ]),
        ),
        (
            b"".to_vec(),
            b"\xeb\xcd\x84".to_vec(),
            b"\xeb\xcc\x88\xcc\x81".to_vec(),
        ),
        (
            b"".to_vec(),
            cat(&["\u{ac00}".as_bytes(), &repeat("\u{300}".as_bytes(), 30)]),
            cat(&[
                "\u{ac00}".as_bytes(),
                &repeat("\u{300}".as_bytes(), 29),
                "\u{34f}\u{300}".as_bytes(),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[b"\xeb", &repeat("\u{300}".as_bytes(), 31)]),
            cat(&[
                b"\xeb",
                &repeat("\u{300}".as_bytes(), 30),
                "\u{34f}\u{300}".as_bytes(),
            ]),
        ),
    ]
}

fn append_tests_nfkc() -> Vec<AppendTest> {
    vec![
        (b"".to_vec(), b"".to_vec(), b"".to_vec()),
        (b"a".to_vec(), b"".to_vec(), b"a".to_vec()),
        (b"".to_vec(), b"a".to_vec(), b"a".to_vec()),
        (b"".to_vec(), "A\u{307}\u{304}".as_bytes().to_vec(), "\u{1e0}".as_bytes().to_vec()),
        (b"".to_vec(), "a\u{300}b".as_bytes().to_vec(), "\u{e0}b".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{300}b".as_bytes().to_vec(), "\u{e0}b".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{300}\u{316}".as_bytes().to_vec(), "\u{e0}\u{316}".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{316}\u{300}".as_bytes().to_vec(), "\u{e0}\u{316}".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{300}a\u{300}".as_bytes().to_vec(), "\u{e0}\u{e0}".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{300}a\u{300}a\u{300}".as_bytes().to_vec(), "\u{e0}\u{e0}\u{e0}".as_bytes().to_vec()),
        (b"a".to_vec(), "\u{300}aaa\u{300}aaa\u{300}".as_bytes().to_vec(), "\u{e0}aa\u{e0}aa\u{e0}".as_bytes().to_vec()),
        ("a\u{300}".as_bytes().to_vec(), "\u{327}".as_bytes().to_vec(), "\u{e0}\u{327}".as_bytes().to_vec()),
        ("a\u{327}".as_bytes().to_vec(), "\u{300}".as_bytes().to_vec(), "\u{e0}\u{327}".as_bytes().to_vec()),
        ("a\u{316}".as_bytes().to_vec(), "\u{300}".as_bytes().to_vec(), "\u{e0}\u{316}".as_bytes().to_vec()),
        ("A\u{307}".as_bytes().to_vec(), "\u{304}".as_bytes().to_vec(), "\u{1e0}".as_bytes().to_vec()),
        (b"".to_vec(), "\u{110b}\u{1173}".as_bytes().to_vec(), "\u{c73c}".as_bytes().to_vec()),
        (b"".to_vec(), "\u{1103}\u{1161}".as_bytes().to_vec(), "\u{b2e4}".as_bytes().to_vec()),
        (b"".to_vec(), "\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(), "\u{c74c}".as_bytes().to_vec()),
        (b"".to_vec(), "\u{320e}".as_bytes().to_vec(), "(\u{ac00})".as_bytes().to_vec()),
        (b"".to_vec(), "(\u{1100}\u{1161})".as_bytes().to_vec(), "(\u{ac00})".as_bytes().to_vec()),
        ("\u{1103}".as_bytes().to_vec(), "\u{1161}".as_bytes().to_vec(), "\u{b2e4}".as_bytes().to_vec()),
        ("\u{110b}".as_bytes().to_vec(), "\u{1173}\u{11b7}".as_bytes().to_vec(), "\u{c74c}".as_bytes().to_vec()),
        ("\u{110b}\u{1173}".as_bytes().to_vec(), "\u{11b7}".as_bytes().to_vec(), "\u{c74c}".as_bytes().to_vec()),
        ("\u{c73c}".as_bytes().to_vec(), "\u{11b7}".as_bytes().to_vec(), "\u{c74c}".as_bytes().to_vec()),
        (b"a\xcc".to_vec(), b"\x80".to_vec(), "\u{e0}".as_bytes().to_vec()),
        (b"a\xcc".to_vec(), b"\x80b".to_vec(), "\u{e0}b".as_bytes().to_vec()),
        (b"a\xcc".to_vec(), b"\x80a\xcc\x80".to_vec(), "\u{e0}\u{e0}".as_bytes().to_vec()),
        (b"a\xcc".to_vec(), b"\x80\x80".to_vec(), b"\xc3\xa0\x80".to_vec()),
        (b"a\xcc".to_vec(), b"\x80\xcc".to_vec(), b"\xc3\xa0\xcc".to_vec()),
        (b"a\xcc\x96\xcc".to_vec(), b"\x80a\xcc\x96\xcc\x80".to_vec(), "\u{e0}\u{316}\u{e0}\u{316}".as_bytes().to_vec()),
        (b"".to_vec(), b"\xcc".to_vec(), b"\xcc".to_vec()),
        (b"a".to_vec(), b"\xcc".to_vec(), b"a\xcc".to_vec()),
        (b"a".to_vec(), b"b\xcc".to_vec(), b"ab\xcc".to_vec()),
        ("\u{226}".as_bytes().to_vec(), b"\xcc".to_vec(), b"\xc8\xa6\xcc".to_vec()),
        (b"".to_vec(), b"\x80".to_vec(), b"\x80".to_vec()),
        (b"".to_vec(), b"\x80\x80\x80".to_vec(), b"\x80\x80\x80".to_vec()),
        (b"".to_vec(), b"\xcc\x80\x80\x80".to_vec(), b"\xcc\x80\x80\x80".to_vec()),
        (b"".to_vec(), b"a\x80".to_vec(), b"a\x80".to_vec()),
        (b"".to_vec(), b"a\x80\x80\x80".to_vec(), b"a\x80\x80\x80".to_vec()),
        (b"".to_vec(), b"a\x80\x80\x80\x80\x80\x80".to_vec(), b"a\x80\x80\x80\x80\x80\x80".to_vec()),
        (b"a".to_vec(), b"\x80\x80\x80".to_vec(), b"a\x80\x80\x80".to_vec()),
        (b"".to_vec(), repeat(b"\x80", 33), repeat(b"\x80", 33)),
        (repeat(b"\x80", 33), b"".to_vec(), repeat(b"\x80", 33)),
        (repeat(b"\x80", 33), repeat(b"\x80", 33), repeat(b"\x80", 66)),
        (b"".to_vec(), grave(34), cat(&[&grave(30), CGJ, &grave(4)])),
        (b"".to_vec(), grave(36), cat(&[&grave(30), CGJ, &grave(6)])),
        (grave(29), grave(5), cat(&[&grave(30), CGJ, &grave(4)])),
        (grave(30), grave(4), cat(&[&grave(30), CGJ, &grave(4)])),
        (grave(30), grave(3), cat(&[&grave(30), CGJ, &grave(3)])),
        (cat(&[&grave(30), b"\xcc"]), b"\x80".to_vec(), cat(&[&grave(30), CGJ, &grave(1)])),
        (b"".to_vec(), cat(&["\u{fdfa}".as_bytes(), &grave(14)]), cat(&["\u{635}\u{644}\u{649} \u{627}\u{644}\u{644}\u{647} \u{639}\u{644}\u{64a}\u{647} \u{648}\u{633}\u{644}\u{645}".as_bytes(), &grave(14)])),
        (b"".to_vec(), cat(&["\u{fdfa}".as_bytes(), &grave(28), "\u{316}".as_bytes()]), cat(&["\u{635}\u{644}\u{649} \u{627}\u{644}\u{644}\u{647} \u{639}\u{644}\u{64a}\u{647} \u{648}\u{633}\u{644}\u{645}\u{316}".as_bytes(), &grave(28)])),
        ("\u{d5}".as_bytes().to_vec(), grave(30), cat(&["\u{d5}".as_bytes(), &grave(29), CGJ, &grave(1)])),
        ("\u{ff9e}".as_bytes().to_vec(), grave(30), cat(&["\u{3099}".as_bytes(), &grave(29), CGJ, &grave(1)])),
        (grave(30), "\u{ff9e}".as_bytes().to_vec(), cat(&[&grave(30), CGJ, "\u{3099}".as_bytes()])),
        (b"".to_vec(), rep(0x340, 31), cat(&[&rep(0x300, 30), CGJ, "\u{300}".as_bytes()])),
        (b"".to_vec(), rep(0xff9e, 31), cat(&[&rep(0x3099, 30), CGJ, "\u{3099}".as_bytes()])),
        (b"".to_vec(), cat(&["\u{644}\u{625}".as_bytes(), &rep(0x300, 31)]), cat(&["\u{644}\u{625}".as_bytes(), &rep(0x300, 29), CGJ, "\u{300}\u{300}".as_bytes()])),
        (b"".to_vec(), cat(&["\u{fef9}".as_bytes(), &rep(0x300, 31)]), cat(&["\u{644}\u{625}".as_bytes(), &rep(0x300, 29), CGJ, &rep(0x300, 2)])),
        (b"".to_vec(), cat(&["\u{fef9}".as_bytes(), &rep(0x300, 31)]), cat(&["\u{644}\u{625}".as_bytes(), &rep(0x300, 29), CGJ, &rep(0x300, 2)])),
        (b"".to_vec(), cat(&["\u{f7f}".as_bytes(), &rep(0xf71, 29), "\u{f81}".as_bytes()]), cat(&["\u{f7f}".as_bytes(), &rep(0xf71, 29), CGJ, "\u{f71}\u{f80}".as_bytes()])),
        (b"".to_vec(), cat(&["\u{f7f}".as_bytes(), &rep(0xf71, 28), "\u{f81}".as_bytes()]), cat(&["\u{f7f}".as_bytes(), &rep(0xf71, 29), "\u{f80}".as_bytes()])),
        (b"".to_vec(), cat(&["\u{f7f}".as_bytes(), &rep(0xf81, 16)]), cat(&["\u{f7f}".as_bytes(), &rep(0xf71, 15), &rep(0xf80, 15), CGJ, "\u{f71}\u{f80}".as_bytes()])),
        (b"\xc3\xa0\xe1".to_vec(), b"\x86".to_vec(), b"\xc3\xa0\xe1\x86".to_vec()),
        ("a\u{300}\u{11b7}".as_bytes().to_vec(), "\u{300}".as_bytes().to_vec(), "\u{e0}\u{11b7}\u{300}".as_bytes().to_vec()),
        ("a\u{300}\u{11b7}\u{300}".as_bytes().to_vec(), "\u{300}".as_bytes().to_vec(), "\u{e0}\u{11b7}\u{300}\u{300}".as_bytes().to_vec()),
        ("\u{300}".as_bytes().to_vec(), b"\xf8\x80\x80\x80\x80\xcc\x80".to_vec(), b"\xcc\x80\xf8\x80\x80\x80\x80\xcc\x80".to_vec()),
        ("\u{300}".as_bytes().to_vec(), b"\xfc\x80\x80\x80\x80\x80\xcc\x80".to_vec(), b"\xcc\x80\xfc\x80\x80\x80\x80\x80\xcc\x80".to_vec()),
        (b"\xf8\x80\x80\x80\x80\xcc\x80".to_vec(), "\u{300}".as_bytes().to_vec(), b"\xf8\x80\x80\x80\x80\xcc\x80\xcc\x80".to_vec()),
        (b"\xfc\x80\x80\x80\x80\x80\xcc\x80".to_vec(), "\u{300}".as_bytes().to_vec(), b"\xfc\x80\x80\x80\x80\x80\xcc\x80\xcc\x80".to_vec()),
        (b"\xf8\x80\x80\x80".to_vec(), b"\x80\xcc\x80\xcc\x80".to_vec(), b"\xf8\x80\x80\x80\x80\xcc\x80\xcc\x80".to_vec()),
        (b"".to_vec(), repeat("a\u{316}\u{300}".as_bytes(), 6), repeat("\u{e0}\u{316}".as_bytes(), 6)),
        (b"".to_vec(), repeat("a\u{300}\u{316}".as_bytes(), 31), repeat("\u{e0}\u{316}".as_bytes(), 31)),
        (b"".to_vec(), repeat("a\u{300}\u{316}".as_bytes(), 4000), repeat("\u{e0}\u{316}".as_bytes(), 4000)),
        (b"".to_vec(), repeat(b"\x80\x80", 4000), repeat(b"\x80\x80", 4000)),
        (b"".to_vec(), "A\u{307}\u{304}".as_bytes().to_vec(), "\u{1e0}".as_bytes().to_vec()),
    ]
}

fn append_tests_nfkd() -> Vec<AppendTest> {
    vec![
        (
            b"".to_vec(),
            cat(&[b"a", &grave(64)]),
            cat(&[b"a", &grave(30), CGJ, &grave(30), CGJ, &grave(4)]),
        ),
        (
            b"".to_vec(),
            cat(&[b"a", &grave(64), "\u{316}".as_bytes()]),
            cat(&[
                b"a",
                &grave(30),
                CGJ,
                &grave(30),
                CGJ,
                "\u{316}".as_bytes(),
                &grave(4),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[b"a", &grave(98), "\u{316}".as_bytes()]),
            cat(&[
                b"a",
                &grave(30),
                CGJ,
                &grave(30),
                CGJ,
                &grave(30),
                CGJ,
                "\u{316}".as_bytes(),
                &grave(8),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[b"a", &grave(59), "\u{340}".as_bytes()]),
            cat(&[b"a", &grave(30), CGJ, &grave(30)]),
        ),
        (
            b"".to_vec(),
            cat(&[
                b"a",
                &grave(33),
                "\u{340}".as_bytes(),
                &grave(30),
                "\u{320}".as_bytes(),
            ]),
            cat(&[
                b"a",
                &grave(30),
                CGJ,
                &grave(30),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(4),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[&rep(0x61, 128), &grave(32), "\u{320}".as_bytes()]),
            cat(&[
                &rep(0x61, 128),
                &grave(30),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(2),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[
                "\u{1100}\u{1161}".as_bytes(),
                &grave(30),
                "\u{320}".as_bytes(),
                &grave(2),
            ]),
            cat(&[
                "\u{1100}\u{1161}".as_bytes(),
                &grave(29),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(3),
            ]),
        ),
        (
            b"".to_vec(),
            "\u{ac00}".as_bytes().to_vec(),
            "\u{1100}\u{1161}".as_bytes().to_vec(),
        ),
        (
            b"".to_vec(),
            cat(&["\u{ac00}".as_bytes(), &grave(32), "\u{320}".as_bytes()]),
            cat(&[
                "\u{1100}\u{1161}".as_bytes(),
                &grave(29),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(3),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[
                "\u{ac00}\u{ac00}".as_bytes(),
                &grave(32),
                "\u{320}".as_bytes(),
            ]),
            cat(&[
                "\u{1100}\u{1161}\u{1100}\u{1161}".as_bytes(),
                &grave(29),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(3),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[
                &repeat("\u{ac00}".as_bytes(), 3),
                &grave(32),
                "\u{320}".as_bytes(),
            ]),
            cat(&[
                &repeat("\u{1100}\u{1161}".as_bytes(), 3),
                &grave(29),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(3),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[
                "\u{60a8}\u{60a8}".as_bytes(),
                &grave(34),
                "\u{320}".as_bytes(),
            ]),
            cat(&[
                "\u{60a8}\u{60a8}".as_bytes(),
                &grave(30),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(4),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&[
                "\u{300}\u{320}a".as_bytes(),
                &grave(34),
                "\u{320}".as_bytes(),
            ]),
            cat(&[
                "\u{320}\u{300}a".as_bytes(),
                &grave(30),
                CGJ,
                "\u{320}".as_bytes(),
                &grave(4),
            ]),
        ),
        (
            b"".to_vec(),
            cat(&["a\u{f7f}".as_bytes(), &rep(0xf71, 29), "\u{f81}".as_bytes()]),
            cat(&[
                "a\u{f7f}".as_bytes(),
                &rep(0xf71, 29),
                CGJ,
                "\u{f71}\u{f80}".as_bytes(),
            ]),
        ),
    ]
}

fn next_boundary_tests() -> Vec<(Vec<u8>, bool, isize)> {
    vec![
        (b"".to_vec(), true, 0),
        (b"".to_vec(), false, -1),
        ("\u{300}".as_bytes().to_vec(), true, 2),
        ("\u{300}".as_bytes().to_vec(), false, -1),
        (b"\x80\x80".to_vec(), true, 1),
        (b"\x80\x80".to_vec(), false, 1),
        (b"\xff".to_vec(), false, 1),
        (b"\xcc\x80\xff".to_vec(), false, 2),
        (b"\xcc\x80\xc0\x80\x80".to_vec(), false, 2),
        (b"\xc2\x80\x80".to_vec(), false, 2),
        (b"\xc2".to_vec(), false, -1),
        (b"\xc2".to_vec(), true, 1),
        (b"a\xcc\x80\xc2".to_vec(), false, -1),
        (b"a\xcc\x80\xc2".to_vec(), true, 3),
        (b"a".to_vec(), true, 1),
        (b"a".to_vec(), false, -1),
        (b"aa".to_vec(), false, 1),
        ("\u{300}".as_bytes().to_vec(), true, 2),
        ("\u{300}".as_bytes().to_vec(), false, -1),
        ("\u{300}a".as_bytes().to_vec(), false, 2),
        ("\u{1103}\u{1161}".as_bytes().to_vec(), true, 6),
        ("\u{1103}\u{1161}".as_bytes().to_vec(), false, -1),
        ("\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(), false, -1),
        (
            "\u{110b}\u{1173}\u{11b7}\u{110b}\u{1173}\u{11b7}"
                .as_bytes()
                .to_vec(),
            false,
            9,
        ),
        (
            "\u{1161}\u{110b}\u{1173}\u{11b7}".as_bytes().to_vec(),
            false,
            3,
        ),
        (
            "\u{1173}\u{11b7}\u{1103}\u{1161}".as_bytes().to_vec(),
            false,
            6,
        ),
        (grave(29), false, -1),
        (grave(30), false, 60),
        (grave(31), false, 60),
    ]
}

const TXT_ALL: &str = concat!(
    r#"Với các điều kiện sau: Ghi nhận công của tác giả. 
Nếu bạn sử dụng, chuyển đổi, hoặc xây dựng dự án từ 
nội dung được chia sẻ này, bạn phải áp dụng giấy phép này hoặc 
một giấy phép khác có các điều khoản tương tự như giấy phép này
cho dự án của bạn. Hiểu rằng: Miễn — Bất kỳ các điều kiện nào
trên đây cũng có thể được miễn bỏ nếu bạn được sự cho phép của
người sở hữu bản quyền. Phạm vi công chúng — Khi tác phẩm hoặc
bất kỳ chương nào của tác phẩm đã trong vùng dành cho công
chúng theo quy định của pháp luật thì tình trạng của nó không 
bị ảnh hưởng bởi giấy phép trong bất kỳ trường hợp nào."#,
    r#"При обязательном соблюдении следующих условий:
Attribution — Вы должны атрибутировать произведение (указывать
автора и источник) в порядке, предусмотренном автором или
лицензиаром (но только так, чтобы никоим образом не подразумевалось,
что они поддерживают вас или использование вами данного произведения).
Υπό τις ακόλουθες προϋποθέσεις:"#,
    r#"Αναφορά Δημιουργού — Θα πρέπει να κάνετε την αναφορά στο έργο με τον
τρόπο που έχει οριστεί από το δημιουργό ή το χορηγούντο την άδεια
(χωρίς όμως να εννοείται με οποιονδήποτε τρόπο ότι εγκρίνουν εσάς ή
τη χρήση του έργου από εσάς). Παρόμοια Διανομή — Εάν αλλοιώσετε,
τροποποιήσετε ή δημιουργήσετε περαιτέρω βασισμένοι στο έργο θα
μπορείτε να διανέμετε το έργο που θα προκύψει μόνο με την ίδια ή
παρόμοια άδεια."#,
    r#"بموجب الشروط التالية نسب المصنف — يجب عليك أن
تنسب العمل بالطريقة التي تحددها المؤلف أو المرخص (ولكن ليس بأي حال من
الأحوال أن توحي وتقترح بتحول أو استخدامك للعمل).
المشاركة على قدم المساواة — إذا كنت يعدل ، والتغيير ، أو الاستفادة
من هذا العمل ، قد ينتج عن توزيع العمل إلا في ظل تشابه او تطابق فى واحد
لهذا الترخيص."#,
    r#"בכפוף לתנאים הבאים: ייחוס — עליך לייחס את היצירה (לתת קרדיט) באופן
המצויין על-ידי היוצר או מעניק הרישיון (אך לא בשום אופן המרמז על כך
שהם תומכים בך או בשימוש שלך ביצירה). שיתוף זהה — אם תחליט/י לשנות,
לעבד או ליצור יצירה נגזרת בהסתמך על יצירה זו, תוכל/י להפיץ את יצירתך
החדשה רק תחת אותו הרישיון או רישיון דומה לרישיון זה."#,
    r#"ภายใต้เงื่อนไข ดังต่อไปนี้ : แสดงที่มา — คุณต้องแสดงที่
มาของงานดังกล่าว ตามรูปแบบที่ผู้สร้างสรรค์หรือผู้อนุญาตกำหนด (แต่
ไม่ใช่ในลักษณะที่ว่า พวกเขาสนับสนุนคุณหรือสนับสนุนการที่
คุณนำงานไปใช้) อนุญาตแบบเดียวกัน — หากคุณดัดแปลง เปลี่ยนรูป หรื
อต่อเติมงานนี้ คุณต้องใช้สัญญาอนุญาตแบบเดียวกันหรือแบบที่เหมื
อนกับสัญญาอนุญาตที่ใช้กับงานนี้เท่านั้น"#,
    r#"您可以自由： 复制、发行、展览、表演、放映、
广播或通过信息网络传播本作品 创作演绎作品
对本作品进行商业性使用 惟须遵守下列条件：
署名 — 您必须按照作者或者许可人指定的方式对作品进行署名。
相同方式共享 — 如果您改变、转换本作品或者以本作品为基础进行创作，
您只能采用与本协议相同的许可协议发布基于本作品的演绎作品。"#,
    r#"あなたの従うべき条件は以下の通りです。
表示 — あなたは原著作者のクレジットを表示しなければなりません。
継承 — もしあなたがこの作品を改変、変形または加工した場合、
あなたはその結果生じた作品をこの作品と同一の許諾条件の下でのみ
頒布することができます。"#,
    r#"다음과 같은 조건을 따라야 합니다: 저작자표시
(Attribution) — 저작자나 이용허락자가 정한 방법으로 저작물의
원저작자를 표시하여야 합니다(그러나 원저작자가 이용자나 이용자의
이용을 보증하거나 추천한다는 의미로 표시해서는 안됩니다). 
동일조건변경허락 — 이 저작물을 이용하여 만든 이차적 저작물에는 본
라이선스와 동일한 라이선스를 적용해야 합니다."#,
);
