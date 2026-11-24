use crate::deflate_once;
use elysiandb::archive::flate;
use std::io::{self, Read};

#[test]
fn reset() {
    let ss = [
        "lorem ipsum izzle fo rizzle",
        "the quick brown fox jumped over",
    ];

    let deflated: Vec<Vec<u8>> = ss.iter().map(|s| deflate_once(s.as_bytes())).collect();

    let mut inflated = [Vec::new(), Vec::new()];

    let mut f = flate::Reader::new(deflated[0].as_slice());
    io::copy(&mut f, &mut inflated[0]).unwrap();
    f.reset(deflated[1].as_slice());
    io::copy(&mut f, &mut inflated[1]).unwrap();
    f.close().unwrap();

    for (i, s) in ss.iter().enumerate() {
        assert_eq!(String::from_utf8_lossy(&inflated[i]), *s, "inflated[{i}]");
    }
}

#[test]
fn reader_truncated() {
    let vectors: [(&[u8], &[u8]); 10] = [
        (b"\x00", b""),
        (b"\x00\x0c", b""),
        (b"\x00\x0c\x00", b""),
        (b"\x00\x0c\x00\xf3\xff", b""),
        (b"\x00\x0c\x00\xf3\xffhello", b"hello"),
        (b"\x00\x0c\x00\xf3\xffhello, world", b"hello, world"),
        (b"\x02", b""),
        (b"\xf2H\xcd", b"He"),
        (b"\xf2H\xcd\x990a\xc2\x84\t", b"Hel\x90\x90\x90\x90\x90"),
        (b"\xf2H\xcd\x990a\xc2\x84\t\x00", b"Hel\x90\x90\x90\x90\x90"),
    ];

    for (i, (input, output)) in vectors.iter().enumerate() {
        let mut zr = flate::new_reader(*input);
        let mut b = Vec::new();
        let err = zr.read_to_end(&mut b).expect_err("truncated input");
        assert_eq!(
            err.kind(),
            io::ErrorKind::UnexpectedEof,
            "test {i}, error mismatch: {err}"
        );
        assert_eq!(b, *output, "test {i}, output mismatch");
    }
}
