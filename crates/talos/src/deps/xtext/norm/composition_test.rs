use gocompat::utf8::{self, Rune};

use super::composition::{Flusher, InsertErr, MAX_BYTE_BUFFER_SIZE, ReorderBuffer};
use super::normalize::Form;

type TestCase = (&'static [Rune], &'static [Rune]);

fn runes_to_bytes(rs: &[Rune]) -> Vec<u8> {
    let mut b = Vec::new();
    for &r in rs {
        utf8::append_rune(&mut b, r);
    }
    b
}

fn run_tests(name: &str, fm: Form, tests: &[TestCase]) {
    let mut rb = ReorderBuffer::new(fm, &[]);
    rb.init(fm, &[]);
    for (i, test) in tests.iter().enumerate() {
        rb.set_flusher(Vec::new(), Flusher::Append);
        for (j, &rune) in test.0.iter().enumerate() {
            let b = runes_to_bytes(&[rune]);
            let info = rb.f.info(&b, 0);
            if j == 0 {
                rb.ss.first(info);
            } else {
                rb.ss.next(info);
            }
            assert_eq!(
                rb.insert_flush(&b, 0, info),
                InsertErr::Success,
                "{name}:{i}: insert failed for rune {j}"
            );
        }
        rb.do_flush();
        let was = rb.out.clone();
        let want = runes_to_bytes(test.1);
        assert_eq!(was.len(), want.len(), "{name}:{i}: length");
        assert_eq!(was, want, "{name}:{i}");
    }
}

#[test]
fn test_flush() {
    const HELLO: &[u8] = b"Hello ";
    const WORLD: &[u8] = b"world!";
    let mut buf = [0u8; MAX_BYTE_BUFFER_SIZE];
    buf[..HELLO.len()].copy_from_slice(HELLO);
    let mut p = HELLO.len();
    let mut rb = ReorderBuffer::new(Form::Nfc, WORLD);
    let i = rb.flush_copy(&mut buf[p..]);
    assert_eq!(i, 0, "wrote bytes on flush of empty buffer");
    for i in 0..WORLD.len() {
        let info = rb.f.info(rb.src, i);
        rb.insert_flush(rb.src, i, info);
        let n = rb.flush_copy(&mut buf[p..]);
        p += n;
    }
    assert_eq!(&buf[..p], b"Hello world!");
    assert_eq!(rb.nrune, 0, "non-null size of info buffer");
    assert_eq!(rb.nbyte, 0, "non-null size of byte buffer");
}

#[test]
fn test_insert() {
    run_tests("TestInsert", Form::Nfd, &insert_tests());
}

#[test]
fn test_decomposition() {
    run_tests("TestDecompositionNFD", Form::Nfd, &decomposition_nfd_test());
    run_tests(
        "TestDecompositionNFKD",
        Form::Nfkd,
        &decomposition_nfkd_test(),
    );
}

#[test]
fn test_composition() {
    run_tests("TestComposition", Form::Nfc, &composition_test());
}

fn insert_tests() -> Vec<TestCase> {
    vec![
        (&[0x61], &[0x61]),
        (&[0x300], &[0x300]),
        (&[0x300, 0x316], &[0x316, 0x300]),
        (&[0x316, 0x300], &[0x316, 0x300]),
        (&[0x41, 0x316, 0x300], &[0x41, 0x316, 0x300]),
        (&[0x41, 0x300, 0x316], &[0x41, 0x316, 0x300]),
        (&[0x300, 0x316, 0x41], &[0x316, 0x300, 0x41]),
        (&[0x41, 0x300, 0x40, 0x316], &[0x41, 0x300, 0x40, 0x316]),
    ]
}

fn decomposition_nfd_test() -> Vec<TestCase> {
    vec![
        (&[0xC0], &[0x41, 0x300]),
        (&[0xAC00], &[0x1100, 0x1161]),
        (&[0x01C4], &[0x01C4]),
        (&[0x320E], &[0x320E]),
        (
            &[0xc74c, 0x1ebb, 0xacfc],
            &[0x110B, 0x1173, 0x11B7, 0x65, 0x309, 0x1100, 0x116A],
        ),
    ]
}

fn decomposition_nfkd_test() -> Vec<TestCase> {
    vec![
        (&[0xC0], &[0x41, 0x300]),
        (&[0xAC00], &[0x1100, 0x1161]),
        (&[0x01C4], &[0x44, 0x5A, 0x030C]),
        (&[0x320E], &[0x28, 0x1100, 0x1161, 0x29]),
    ]
}

fn composition_test() -> Vec<TestCase> {
    vec![
        (&[0x41, 0x300], &[0xC0]),
        (&[0x41, 0x316], &[0x41, 0x316]),
        (&[0x41, 0x300, 0x35D], &[0xC0, 0x35D]),
        (&[0x41, 0x316, 0x300], &[0xC0, 0x316]),
        (&[0x41, 0x316, 0x40, 0x300], &[0x41, 0x316, 0x40, 0x300]),
        (&[0x1100, 0x1161], &[0xAC00]),
        (&[0x28, 0x1100, 0x1161, 0x29], &[0x28, 0xAC00, 0x29]),
    ]
}
