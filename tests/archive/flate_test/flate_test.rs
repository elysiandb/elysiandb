use crate::inflate_all;
use crate::streams::STREAMS;
use elysiandb::archive::flate::{self, CorruptInputError, HuffmanDecoder, decode_symbol};
use std::io::{self, Read};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn issue_5915() {
    let bits = [
        4, 0, 0, 6, 4, 3, 2, 3, 3, 4, 4, 5, 0, 0, 0, 0, 5, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 8, 6, 0, 11, 0, 8, 0, 6,
        6, 10, 8,
    ];
    let mut h = HuffmanDecoder::default();
    assert!(
        !h.init(&bits),
        "given sequence of bits is bad, and should not succeed"
    );
}

#[test]
fn issue_5962() {
    let bits = [
        4, 0, 0, 6, 4, 3, 2, 3, 3, 4, 4, 5, 0, 0, 0, 0, 5, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11,
    ];
    let mut h = HuffmanDecoder::default();
    assert!(
        !h.init(&bits),
        "given sequence of bits is bad, and should not succeed"
    );
}

#[test]
fn issue_6255() {
    let bits1 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 11];
    let bits2 = [11, 13];
    let mut h = HuffmanDecoder::default();
    assert!(
        h.init(&bits1),
        "given sequence of bits is good and should succeed"
    );
    assert!(
        !h.init(&bits2),
        "given sequence of bits is bad and should not succeed"
    );
}

#[test]
fn invalid_encoding() {
    let mut h = HuffmanDecoder::default();
    assert!(h.init(&[1]), "failed to initialize Huffman decoder");

    let err = decode_symbol(&[0xff], &h).expect_err("should have rejected invalid bit sequence");
    assert_eq!(err.to_string(), "corrupt compressed data at byte 1");
    let corrupt = err
        .get_ref()
        .and_then(|e| e.downcast_ref::<CorruptInputError>())
        .copied();
    assert_eq!(corrupt, Some(CorruptInputError(1)));
}

#[test]
fn invalid_bits() {
    let oversubscribed = [1, 2, 3, 4, 4, 5];
    let incomplete = [1, 2, 4, 4];
    let mut h = HuffmanDecoder::default();
    assert!(
        !h.init(&oversubscribed),
        "should reject oversubscribed bit-length set"
    );
    assert!(
        !h.init(&incomplete),
        "should reject incomplete bit-length set"
    );
}

#[test]
fn streams() {
    assert_eq!(STREAMS.len(), 28);
    for (i, (desc, stream, want)) in STREAMS.iter().enumerate() {
        let data = unhex(stream);
        let result = inflate_all(&data);
        if *want == "fail" {
            assert!(
                result.is_err(),
                "#{i} ({desc}): got nil error, want non-nil"
            );
        } else {
            let data = result.unwrap_or_else(|e| panic!("#{i} ({desc}): {e}"));
            assert_eq!(hex(&data), *want, "#{i} ({desc})");
        }
    }
}

#[test]
fn truncated_streams() {
    let data = b"\x00\x0c\x00\xf3\xffhello, world\x01\x00\x00\xff\xff";

    for i in 0..data.len() - 1 {
        let mut r = flate::new_reader(&data[..i]);
        let err = io::copy(&mut r, &mut io::sink()).expect_err("truncated stream");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "{i}");
        assert_eq!(err.to_string(), "unexpected end of data", "{i}");
    }
}

#[test]
fn errors_are_sticky_and_follow_the_pending_data() {
    let data = b"\x00\x0c\x00\xf3\xffhello, world\x03";
    let mut r = flate::new_reader(&data[..]);
    let mut out = [0u8; 64];
    let n = r.read(&mut out).unwrap();
    assert_eq!(&out[..n], b"hello, world");
    for _ in 0..3 {
        let err = r.read(&mut out).unwrap_err();
        assert_eq!(err.to_string(), "unexpected end of data");
    }
    let err = r.close().unwrap_err();
    assert_eq!(err.to_string(), "unexpected end of data");
}
