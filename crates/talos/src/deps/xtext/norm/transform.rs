use std::borrow::Cow;
use std::fmt;

use super::composition::copy_into;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    ShortDst,
    ShortSrc,
    EndOfSpan,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ShortDst => "transform: short destination buffer",
            Error::ShortSrc => "transform: short source buffer",
            Error::EndOfSpan => "transform: input and output are not identical",
        })
    }
}

impl std::error::Error for Error {}

pub trait Transformer {
    fn transform(
        &mut self,
        dst: &mut [u8],
        src: &[u8],
        at_eof: bool,
    ) -> (usize, usize, Result<(), Error>);

    fn reset(&mut self);
}

pub trait SpanningTransformer: Transformer {
    fn span(&mut self, src: &[u8], at_eof: bool) -> (usize, Result<(), Error>);
}

const INITIAL_BUF_SIZE: usize = 128;

fn grow(b: &[u8], n: usize) -> Vec<u8> {
    let mut m = b.len();
    if m <= 32 {
        m = 64;
    } else if m <= 256 {
        m *= 2;
    } else {
        m += m >> 1;
    }
    let mut buf = vec![0; m];
    buf[..n].copy_from_slice(&b[..n]);
    buf
}

pub fn string<'a, T: Transformer + ?Sized>(
    t: &mut T,
    s: &'a [u8],
) -> (Cow<'a, [u8]>, usize, Result<(), Error>) {
    t.reset();
    if s.is_empty() {
        let (_, _, err) = t.transform(&mut [], &[], true);
        if err.is_ok() {
            return (Cow::Borrowed(s), 0, Ok(()));
        }
    }
    let mut dst: Vec<u8> = vec![0; INITIAL_BUF_SIZE];
    let mut src: Vec<u8> = vec![0; INITIAL_BUF_SIZE];
    let mut n_dst;
    let mut n_src;
    let mut err;
    let mut p_dst = 0;
    let mut p_src = 0;
    let mut p_prefix = 0;
    loop {
        let n = copy_into(&mut src, &s[p_src..]);
        (n_dst, n_src, err) = t.transform(&mut dst, &src[..n], p_src + n == s.len());
        p_dst += n_dst;
        p_src += n_src;
        if dst[..n_dst] != src[..n_src] {
            break;
        }
        p_prefix = p_src;
        if err == Err(Error::ShortDst) {
            break;
        } else if err == Err(Error::ShortSrc) {
            if n_src == 0 {
                break;
            }
        } else if err.is_err() || p_prefix == s.len() {
            return (Cow::Borrowed(&s[..p_prefix]), p_prefix, err);
        }
    }
    if p_prefix != 0 {
        if p_dst > dst.len() {
            let mut new_dst = vec![0; s.len() + n_dst - n_src];
            new_dst[p_prefix..p_dst].copy_from_slice(&dst[..n_dst]);
            dst = new_dst;
        } else {
            dst.copy_within(..n_dst, p_prefix);
        }
        dst[..p_prefix].copy_from_slice(&s[..p_prefix]);
    }
    if (err.is_ok() && p_src == s.len()) || matches!(err, Err(Error::EndOfSpan)) {
        dst.truncate(p_dst);
        return (Cow::Owned(dst), p_src, err);
    }
    loop {
        let n = copy_into(&mut src, &s[p_src..]);
        let at_eof = p_src + n == s.len();
        let (n_dst, n_src, err) = t.transform(&mut dst[p_dst..], &src[..n], at_eof);
        p_dst += n_dst;
        p_src += n_src;
        if err == Err(Error::ShortDst) {
            if n_dst == 0 {
                dst = grow(&dst, p_dst);
            }
        } else if err == Err(Error::ShortSrc) {
            if at_eof {
                dst.truncate(p_dst);
                return (Cow::Owned(dst), p_src, err);
            }
            if n_src == 0 {
                src = grow(&src, 0);
            }
        } else if err.is_err() || p_src == s.len() {
            dst.truncate(p_dst);
            return (Cow::Owned(dst), p_src, err);
        }
    }
}

pub fn bytes<T: Transformer + ?Sized>(t: &mut T, b: &[u8]) -> (Vec<u8>, usize, Result<(), Error>) {
    do_append(t, 0, vec![0; b.len()], b)
}

fn do_append<T: Transformer + ?Sized>(
    t: &mut T,
    mut p_dst: usize,
    mut dst: Vec<u8>,
    src: &[u8],
) -> (Vec<u8>, usize, Result<(), Error>) {
    t.reset();
    let mut p_src = 0;
    loop {
        let (n_dst, n_src, err) = t.transform(&mut dst[p_dst..], &src[p_src..], true);
        p_dst += n_dst;
        p_src += n_src;
        if err != Err(Error::ShortDst) {
            dst.truncate(p_dst);
            return (dst, p_src, err);
        }
        if n_dst == 0 {
            dst = grow(&dst, p_dst);
        }
    }
}
