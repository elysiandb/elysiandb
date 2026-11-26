use gocompat::utf8;

use super::{Caser, handle_final_sigma, lower, upper};
use crate::deps::xtext::language::UND;
use crate::deps::xtext::norm::{self, transform::Error};

struct MappingCase {
    src: &'static str,
    lower: &'static str,
    upper: &'static str,
    handle_sigma: bool,
}

#[test]
fn test_case_mappings() {
    let cases = [
        MappingCase {
            src: "abc aBc ABC abC \u{130}s\u{131}I \u{395}\u{3a3}\u{386}\u{3a3}",
            lower: "abc abc abc abc i\u{307}s\u{131}i \u{3b5}\u{3c3}\u{3ac}\u{3c3}",
            upper: "ABC ABC ABC ABC \u{130}SII \u{395}\u{3a3}\u{386}\u{3a3}",
            handle_sigma: false,
        },
        MappingCase {
            src: "abc aBc ABC abC \u{130}s\u{131}I \u{395}\u{3a3}\u{386}\u{3a3} \u{3a3} _\u{3a3} -\u{3a3}",
            lower: "abc abc abc abc i\u{307}s\u{131}i \u{3b5}\u{3c3}\u{3ac}\u{3c2} \u{3c3} _\u{3c3} -\u{3c3}",
            upper: "ABC ABC ABC ABC \u{130}SII \u{395}\u{3a3}\u{386}\u{3a3} \u{3a3} _\u{3a3} -\u{3a3}",
            handle_sigma: true,
        },
        MappingCase {
            src: "\u{1c5}A",
            lower: "\u{1c6}a",
            upper: "\u{1c4}A",
            handle_sigma: true,
        },
    ];
    for (i, tt) in cases.iter().enumerate() {
        let src: Vec<&str> = tt.src.split(' ').collect();
        let opts = [handle_final_sigma(tt.handle_sigma)];
        for (name, c, gold) in [
            ("Upper", upper(UND, &opts), tt.upper),
            ("Lower", lower(UND, &opts), tt.lower),
        ] {
            for (j, want) in gold.split(' ').enumerate() {
                assert_eq!(
                    c.string(src[j].as_bytes()).as_ref(),
                    want.as_bytes(),
                    "{i}:und: {name}.String({:?})",
                    src[j]
                );
            }
        }
    }
}

fn test_handover(c: Caser, src: &[u8]) {
    let want = c.string(src).into_owned();
    let mut p_src = 0;
    while p_src < src.len() && p_src < want.len() && want[p_src] == src[p_src] {
        p_src += 1;
    }
    for i in 0..p_src {
        let mut dst = vec![0u8; 4 * src.len()];
        c.reset();
        let (n_span, _) = c.span(&src[..i], false);
        dst[..n_span].copy_from_slice(&src[..n_span]);
        let (n_transform, _, _) = c.transform(&mut dst[n_span..], &src[n_span..], true);
        assert_eq!(
            &dst[..n_span + n_transform],
            want.as_slice(),
            "interleave/{i}: full string {src:?}"
        );
    }
}

const MIN_BUF_SIZE: usize = norm::MAX_SEGMENT_SIZE;

struct BufferCase {
    desc: &'static str,
    src: &'static str,
    want: &'static str,
    first_err: Result<(), Error>,
    dst_size: usize,
    src_size: usize,
    t: Caser,
}

#[test]
fn test_short_buffers_and_overflow() {
    let tests = [
        BufferCase {
            desc: "und/upper/short dst",
            src: "abcdefg",
            want: "ABCDEFG",
            first_err: Err(Error::ShortDst),
            dst_size: 3,
            src_size: MIN_BUF_SIZE,
            t: upper(UND, &[]),
        },
        BufferCase {
            desc: "und/upper/short src",
            src: "123\u{e9}56",
            want: "123\u{c9}56",
            first_err: Err(Error::ShortSrc),
            dst_size: 4,
            src_size: 4,
            t: upper(UND, &[]),
        },
        BufferCase {
            desc: "und/upper/no error on short",
            src: "12",
            want: "12",
            first_err: Ok(()),
            dst_size: 1,
            src_size: 1,
            t: upper(UND, &[]),
        },
        BufferCase {
            desc: "und/lower/short dst",
            src: "ABCDEFG",
            want: "abcdefg",
            first_err: Err(Error::ShortDst),
            dst_size: 3,
            src_size: MIN_BUF_SIZE,
            t: lower(UND, &[]),
        },
        BufferCase {
            desc: "und/lower/short src",
            src: "123\u{c9}56",
            want: "123\u{e9}56",
            first_err: Err(Error::ShortSrc),
            dst_size: 4,
            src_size: 4,
            t: lower(UND, &[]),
        },
        BufferCase {
            desc: "und/lower/no error on short",
            src: "12",
            want: "12",
            first_err: Ok(()),
            dst_size: 1,
            src_size: 1,
            t: lower(UND, &[]),
        },
        BufferCase {
            desc: "und/lower/simple (no final sigma)",
            src: "\u{39f}\u{3a3} \u{39f}\u{3a3}\u{3a3}",
            want: "\u{3bf}\u{3c3} \u{3bf}\u{3c3}\u{3c3}",
            first_err: Ok(()),
            dst_size: MIN_BUF_SIZE,
            src_size: MIN_BUF_SIZE,
            t: lower(UND, &[handle_final_sigma(false)]),
        },
    ];
    for (i, tt) in tests.iter().enumerate() {
        let src = tt.src.as_bytes();
        let mut buf = vec![0u8; tt.dst_size];
        let mut got = Vec::new();
        let mut p = 0;
        while p < src.len() {
            let q = (p + tt.src_size).min(src.len());
            let (n_dst, n_src, err) = tt.t.transform(&mut buf, &src[p..q], q == src.len());
            got.extend_from_slice(&buf[..n_dst]);
            if p == 0 && err != tt.first_err {
                panic!(
                    "{i}:{}: error was {err:?}; want {:?}",
                    tt.desc, tt.first_err
                );
            }
            p += n_src;
        }
        assert_eq!(got, tt.want.as_bytes(), "{i}:{}", tt.desc);
        test_handover(tt.t, src);
    }
}

struct SpanCase {
    desc: &'static str,
    src: &'static [u8],
    want: &'static [u8],
    at_eof: bool,
    err: Result<(), Error>,
    t: Caser,
}

#[test]
fn test_span() {
    let tests = [
        SpanCase {
            desc: "und/upper/basic",
            src: b"abcdefg",
            want: b"",
            at_eof: true,
            err: Err(Error::EndOfSpan),
            t: upper(UND, &[]),
        },
        SpanCase {
            desc: "und/upper/short src",
            src: b"123\xc3",
            want: b"123",
            at_eof: false,
            err: Err(Error::ShortSrc),
            t: upper(UND, &[]),
        },
        SpanCase {
            desc: "und/upper/no error on short",
            src: b"12",
            want: b"12",
            at_eof: false,
            err: Ok(()),
            t: upper(UND, &[]),
        },
        SpanCase {
            desc: "und/lower/basic",
            src: b"ABCDEFG",
            want: b"",
            at_eof: true,
            err: Err(Error::EndOfSpan),
            t: lower(UND, &[]),
        },
        SpanCase {
            desc: "und/lower/short src num",
            src: b"123\xc3",
            want: b"123",
            at_eof: false,
            err: Err(Error::ShortSrc),
            t: lower(UND, &[]),
        },
        SpanCase {
            desc: "und/lower/short src greek",
            src: b"\xce\xb1\xce\xb2\xce\xb3\xc3",
            want: "\u{3b1}\u{3b2}\u{3b3}".as_bytes(),
            at_eof: false,
            err: Err(Error::ShortSrc),
            t: lower(UND, &[]),
        },
        SpanCase {
            desc: "und/lower/no error on short",
            src: b"12",
            want: b"12",
            at_eof: false,
            err: Ok(()),
            t: lower(UND, &[]),
        },
        SpanCase {
            desc: "und/lower/simple (no final sigma)",
            src: "\u{3bf}\u{3c2} \u{3bf}\u{3c3}\u{3c3}".as_bytes(),
            want: "\u{3bf}\u{3c3} \u{3bf}\u{3c3}\u{3c3}".as_bytes(),
            at_eof: true,
            err: Ok(()),
            t: lower(UND, &[handle_final_sigma(false)]),
        },
        SpanCase {
            desc: "und/lower/final sigma: no error",
            src: "\u{3bf}\u{3a3}".as_bytes(),
            want: "\u{3bf}".as_bytes(),
            at_eof: false,
            err: Err(Error::EndOfSpan),
            t: lower(UND, &[]),
        },
    ];
    for tt in &tests {
        let mut p = 0;
        while p < tt.want.len() {
            tt.t.reset();
            let (n, err) = tt.t.span(&tt.src[..p], false);
            assert!(
                err.is_ok() || err == Err(Error::ShortSrc),
                "{}: early failure:Span({:?}): {err:?} ({n} < {})",
                tt.desc,
                &tt.src[..p],
                tt.want.len()
            );
            let (r, _) = utf8::decode_rune(&tt.src[p..]);
            p += utf8::rune_len(r).unwrap_or(0);
        }
        tt.t.reset();
        let (n, err) = tt.t.span(tt.src, tt.at_eof);
        assert_eq!(
            (n, err),
            (tt.want.len(), tt.err),
            "{}: Span({:?}, {})",
            tt.desc,
            tt.src,
            tt.at_eof
        );
        test_handover(tt.t, tt.src);
    }
}
