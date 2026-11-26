use crate::huffman_tokens::{HuffTest, WRITE_BLOCK_TESTS};
use crate::{Shared, testdata};
use elysiandb::archive::flate::HuffmanBitWriter;

const HUFF_INPUTS: [&str; 7] = [
    "huffman-null-max",
    "huffman-pi",
    "huffman-rand-1k",
    "huffman-rand-limit",
    "huffman-rand-max",
    "huffman-shifts",
    "huffman-zero",
];

#[test]
fn block_huff() {
    let mut checked = 0;
    for name in HUFF_INPUTS {
        let all = testdata(&format!("{name}.in"));
        let want = testdata(&format!("{name}.golden"));

        let buf = Shared::default();
        let mut bw = HuffmanBitWriter::new(buf.clone());
        bw.write_block_huff(false, &all);
        bw.flush();
        assert!(buf.bytes() == want, "{name}.in != {name}.golden");

        let again = Shared::default();
        bw.reset(again.clone());
        bw.write_block_huff(false, &all);
        bw.flush();
        assert!(
            again.bytes() == want,
            "after reset {name}.in != {name}.golden"
        );

        test_writer_eof(TestKind::Huff, &[], Some(&all), name);
        checked += 1;
    }
    assert_eq!(checked, HUFF_INPUTS.len());
}

#[derive(Clone, Copy)]
enum TestKind {
    Huff,
    Dyn,
}

fn write_to_type(bw: &mut HuffmanBitWriter<Shared>, tokens: &[u32], input: Option<&[u8]>) {
    bw.write_block_dynamic(tokens, false, input);
    assert!(bw.error().is_none(), "{:?}", bw.error());

    bw.flush();
    assert!(bw.error().is_none(), "{:?}", bw.error());
}

fn test_writer_eof(kind: TestKind, tokens: &[u32], input: Option<&[u8]>, name: &str) {
    let buf = Shared::default();
    let mut bw = HuffmanBitWriter::new(buf.clone());
    match kind {
        TestKind::Dyn => bw.write_block_dynamic(tokens, true, input),
        TestKind::Huff => bw.write_block_huff(true, input.unwrap_or(&[])),
    }
    assert!(bw.error().is_none(), "{:?}", bw.error());

    bw.flush();
    assert!(bw.error().is_none(), "{:?}", bw.error());
    let b = buf.bytes();
    assert!(!b.is_empty(), "no output received");
    assert_eq!(b[0] & 1, 1, "block not marked with EOF for input {name}");
}

fn test_block(test: &HuffTest) {
    if !test.input.is_empty() {
        let input = testdata(test.input);
        let want = testdata(test.want);
        let buf = Shared::default();
        let mut bw = HuffmanBitWriter::new(buf.clone());
        write_to_type(&mut bw, test.tokens, Some(&input));
        assert!(
            buf.bytes() == want,
            "write_block_dynamic did not yield the expected result for {}",
            test.want
        );

        let again = Shared::default();
        bw.reset(again.clone());
        write_to_type(&mut bw, test.tokens, Some(&input));
        bw.flush();
        assert!(
            again.bytes() == want,
            "reset: write_block_dynamic did not yield the expected result for {}",
            test.want
        );
        test_writer_eof(TestKind::Dyn, test.tokens, Some(&input), test.input);
    }
    let want_ni = testdata(test.want_no_input);
    let buf = Shared::default();
    let mut bw = HuffmanBitWriter::new(buf.clone());
    write_to_type(&mut bw, test.tokens, None);

    let got = buf.bytes();
    assert!(
        got == want_ni,
        "write_block_dynamic did not yield the expected result for {}",
        test.want_no_input
    );
    assert_eq!(got[0] & 1, 0, "got unexpected EOF");

    let again = Shared::default();
    bw.reset(again.clone());
    write_to_type(&mut bw, test.tokens, None);
    bw.flush();
    assert!(
        again.bytes() == want_ni,
        "reset: write_block_dynamic did not yield the expected result for {}",
        test.want_no_input
    );
    test_writer_eof(TestKind::Dyn, test.tokens, None, test.want_no_input);
}

#[test]
fn write_block_dynamic() {
    assert_eq!(WRITE_BLOCK_TESTS.len(), 7);
    for test in WRITE_BLOCK_TESTS {
        test_block(test);
    }
}
