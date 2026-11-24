use super::tables::{EXCEPTIONS, XOR_DATA, lookup};
use super::trieval::{
    C_IGNORABLE_CASED, C_LOWER, C_TITLE, C_UNCASED, C_UPPER, C_XOR_CASE, EXCEPTION_BIT,
    EXCEPTION_SHIFT, HAS_MAPPING_MASK, Info, LENGTH_BITS, LENGTH_MASK, NO_CHANGE, XOR_INDEX_BIT,
    XOR_SHIFT,
};
use crate::deps::xtext::norm::transform::Error;

pub(super) struct Context<'d, 's> {
    pub(super) dst: &'d mut [u8],
    pub(super) src: &'s [u8],
    pub(super) at_eof: bool,
    pub(super) p_dst: usize,
    pub(super) p_src: usize,
    pub(super) n_dst: usize,
    pub(super) n_src: usize,
    pub(super) err: Result<(), Error>,
    pub(super) sz: usize,
    pub(super) info: Info,
}

impl<'d, 's> Context<'d, 's> {
    pub(super) fn new(dst: &'d mut [u8], src: &'s [u8], at_eof: bool) -> Self {
        Context {
            dst,
            src,
            at_eof,
            p_dst: 0,
            p_src: 0,
            n_dst: 0,
            n_src: 0,
            err: Ok(()),
            sz: 0,
            info: Info(0),
        }
    }

    pub(super) fn ret(&self) -> (usize, usize, Result<(), Error>) {
        if self.err.is_err() || self.n_src == self.src.len() {
            return (self.n_dst, self.n_src, self.err);
        }
        if self.at_eof && self.p_src == self.src.len() {
            return (self.p_dst, self.p_src, Ok(()));
        }
        (self.n_dst, self.n_src, Err(Error::ShortSrc))
    }

    pub(super) fn ret_span(&self) -> (usize, Result<(), Error>) {
        let (_, n_src, err) = self.ret();
        (n_src, err)
    }

    pub(super) fn checkpoint(&mut self) {
        if self.err.is_ok() {
            self.n_dst = self.p_dst;
            self.n_src = self.p_src + self.sz;
        }
    }

    pub(super) fn unread_rune(&mut self) {
        self.sz = 0;
    }

    pub(super) fn next(&mut self) -> bool {
        self.p_src += self.sz;
        if self.p_src == self.src.len() || self.err.is_err() {
            self.info = Info(0);
            self.sz = 0;
            return false;
        }
        let (v, sz) = lookup(&self.src[self.p_src..]);
        self.info = Info(v);
        self.sz = sz;
        if self.sz == 0 {
            if self.at_eof {
                self.sz = 1;
            } else {
                self.err = Err(Error::ShortSrc);
                return false;
            }
        }
        true
    }

    pub(super) fn write_bytes(&mut self, b: &[u8]) -> bool {
        if self.dst.len() - self.p_dst < b.len() {
            self.err = Err(Error::ShortDst);
            return false;
        }
        for &ch in b {
            self.dst[self.p_dst] = ch;
            self.p_dst += 1;
        }
        true
    }

    pub(super) fn write_string(&mut self, s: &[u8]) -> bool {
        self.write_bytes(s)
    }

    pub(super) fn copy(&mut self) -> bool {
        let src = self.src;
        self.write_bytes(&src[self.p_src..self.p_src + self.sz])
    }

    pub(super) fn copy_xor(&mut self) -> bool {
        if !self.copy() {
            return false;
        }
        if self.info.0 & XOR_INDEX_BIT == 0 {
            self.dst[self.p_dst - 1] ^= (self.info.0 >> XOR_SHIFT) as u8;
        } else {
            let mut idx = usize::from(self.info.0 >> XOR_SHIFT);
            let mut p = self.p_dst - 1;
            loop {
                self.dst[p] ^= XOR_DATA[idx];
                idx = idx.wrapping_sub(1);
                if XOR_DATA[idx] == 0 {
                    break;
                }
                p = p.wrapping_sub(1);
            }
        }
        true
    }

    pub(super) fn has_prefix(&self, s: &[u8]) -> bool {
        let b = &self.src[self.p_src..];
        if b.len() < s.len() {
            return false;
        }
        b[..s.len()] == *s
    }

    pub(super) fn case_type(&self) -> Info {
        let cm = Info(self.info.0 & 0x7);
        if cm.0 < 4 {
            return cm;
        }
        if cm >= C_XOR_CASE {
            let b = self.src[self.p_src + self.sz - 1];
            return Info(u16::from(b & 1) ^ (cm.0 & 0x3));
        }
        if cm == C_IGNORABLE_CASED {
            return C_LOWER;
        }
        C_UNCASED
    }
}

fn exception(info: Info) -> &'static [u8] {
    &EXCEPTIONS[usize::from(info.0 >> EXCEPTION_SHIFT)..]
}

pub(super) fn lower(c: &mut Context<'_, '_>) -> bool {
    let ct = c.case_type();
    if c.info.0 & HAS_MAPPING_MASK == 0 || ct == C_LOWER {
        return c.copy();
    }
    if c.info.0 & EXCEPTION_BIT == 0 {
        return c.copy_xor();
    }
    let e = exception(c.info);
    let offset = usize::from(2 + (e[0] & LENGTH_MASK));
    let n_lower = (e[1] >> LENGTH_BITS) & LENGTH_MASK;
    if n_lower != NO_CHANGE {
        return c.write_string(&e[offset..offset + usize::from(n_lower)]);
    }
    c.copy()
}

pub(super) fn is_lower(c: &mut Context<'_, '_>) -> bool {
    let ct = c.case_type();
    if c.info.0 & HAS_MAPPING_MASK == 0 || ct == C_LOWER {
        return true;
    }
    if c.info.0 & EXCEPTION_BIT == 0 {
        c.err = Err(Error::EndOfSpan);
        return false;
    }
    let e = exception(c.info);
    let n_lower = (e[1] >> LENGTH_BITS) & LENGTH_MASK;
    if n_lower != NO_CHANGE {
        c.err = Err(Error::EndOfSpan);
        return false;
    }
    true
}

pub(super) fn upper(c: &mut Context<'_, '_>) -> bool {
    let ct = c.case_type();
    if c.info.0 & HAS_MAPPING_MASK == 0 || ct == C_UPPER {
        return c.copy();
    }
    if c.info.0 & EXCEPTION_BIT == 0 {
        return c.copy_xor();
    }
    let mut e = exception(c.info);
    let offset = usize::from(2 + (e[0] & LENGTH_MASK));
    let mut n = (e[1] >> LENGTH_BITS) & LENGTH_MASK;
    if ct == C_TITLE {
        if n == NO_CHANGE {
            n = 0;
        }
        let next_n = e[1] & LENGTH_MASK;
        e = &e[usize::from(n)..];
        n = next_n;
    }
    if n != NO_CHANGE {
        return c.write_string(&e[offset..offset + usize::from(n)]);
    }
    c.copy()
}

pub(super) fn is_upper(c: &mut Context<'_, '_>) -> bool {
    let ct = c.case_type();
    if c.info.0 & HAS_MAPPING_MASK == 0 || ct == C_UPPER {
        return true;
    }
    if c.info.0 & EXCEPTION_BIT == 0 {
        c.err = Err(Error::EndOfSpan);
        return false;
    }
    let e = exception(c.info);
    let mut n = (e[1] >> LENGTH_BITS) & LENGTH_MASK;
    if ct == C_TITLE {
        n = e[1] & LENGTH_MASK;
    }
    if n != NO_CHANGE {
        c.err = Err(Error::EndOfSpan);
        return false;
    }
    true
}
