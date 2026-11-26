use gocompat::utf8;

use super::composition::{
    HANGUL_UTF8_SIZE, MAX_BYTE_BUFFER_SIZE, ReorderBuffer, SsState, StreamSafe, copy_into,
    decompose_hangul, hangul,
};
use super::forminfo::Properties;
use super::normalize::Form;

pub const MAX_SEGMENT_SIZE: usize = MAX_BYTE_BUFFER_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IterFn {
    Done,
    Ascii,
    Hangul,
    Multi,
    MultiNorm,
    Decomposed,
    CgjDecompose,
    Composed,
    CgjCompose,
}

enum Seg {
    Nil,
    Src(usize, usize),
    Buf(usize),
    Static(&'static [u8]),
}

pub struct Iter<'a> {
    rb: ReorderBuffer<'a>,
    buf: [u8; MAX_BYTE_BUFFER_SIZE],
    info: Properties,
    next: IterFn,
    ascii_f: IterFn,
    p: usize,
    multi_seg: Option<&'static [u8]>,
}

impl Default for Iter<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Iter<'a> {
    pub fn new() -> Self {
        Iter {
            rb: ReorderBuffer::new(Form::Nfc, &[]),
            buf: [0; MAX_BYTE_BUFFER_SIZE],
            info: Properties::default(),
            next: IterFn::Done,
            ascii_f: IterFn::Done,
            p: 0,
            multi_seg: None,
        }
    }

    pub fn init(&mut self, f: Form, src: &'a [u8]) {
        self.p = 0;
        if src.is_empty() {
            self.set_done();
            self.rb.nsrc = 0;
            return;
        }
        self.multi_seg = None;
        self.rb.init(f, src);
        self.next = self.rb.f.next_main;
        self.ascii_f = IterFn::Ascii;
        self.info = self.rb.f.info(self.rb.src, self.p);
        self.rb.ss.first(self.info);
    }

    pub fn init_string(&mut self, f: Form, src: &'a [u8]) {
        self.init(f, src);
    }

    pub fn seek(&mut self, offset: i64, whence: i32) -> Result<i64, &'static str> {
        let abs = match whence {
            0 => offset,
            1 => self.p as i64 + offset,
            2 => self.rb.nsrc as i64 + offset,
            _ => return Err("norm: invalid whence"),
        };
        if abs < 0 {
            return Err("norm: negative position");
        }
        if abs as usize >= self.rb.nsrc {
            self.set_done();
            return Ok(self.p as i64);
        }
        self.p = abs as usize;
        self.multi_seg = None;
        self.next = self.rb.f.next_main;
        self.info = self.rb.f.info(self.rb.src, self.p);
        self.rb.ss.first(self.info);
        Ok(abs)
    }

    pub fn pos(&self) -> usize {
        self.p
    }

    fn set_done(&mut self) {
        self.next = IterFn::Done;
        self.p = self.rb.nsrc;
    }

    pub fn done(&self) -> bool {
        self.p >= self.rb.nsrc
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> &[u8] {
        let seg = self.call(self.next);
        match seg {
            Seg::Nil => &[],
            Seg::Src(a, b) => &self.rb.src[a..b],
            Seg::Buf(n) => &self.buf[..n],
            Seg::Static(d) => d,
        }
    }

    fn call(&mut self, f: IterFn) -> Seg {
        match f {
            IterFn::Done => Seg::Nil,
            IterFn::Ascii => self.next_ascii(),
            IterFn::Hangul => self.next_hangul(),
            IterFn::Multi => self.next_multi(),
            IterFn::MultiNorm => self.next_multi_norm(),
            IterFn::Decomposed => self.next_decomposed(),
            IterFn::CgjDecompose => self.next_cgj_decompose(),
            IterFn::Composed => self.next_composed(),
            IterFn::CgjCompose => self.next_cgj_compose(),
        }
    }

    fn next_ascii(&mut self) -> Seg {
        let p = self.p + 1;
        if p >= self.rb.nsrc {
            let p0 = self.p;
            self.set_done();
            return Seg::Src(p0, p);
        }
        if self.rb.src[p] < utf8::RUNE_SELF {
            let p0 = self.p;
            self.p = p;
            return Seg::Src(p0, p);
        }
        self.info = self.rb.f.info(self.rb.src, self.p);
        self.next = self.rb.f.next_main;
        self.call(self.next)
    }

    fn next_hangul(&mut self) -> Seg {
        let p = self.p;
        let next = p + HANGUL_UTF8_SIZE;
        if next >= self.rb.nsrc {
            self.set_done();
        } else if hangul(self.rb.src, next) == 0 {
            self.rb.ss.next(self.info);
            self.info = self.rb.f.info(self.rb.src, self.p);
            self.next = self.rb.f.next_main;
            return self.call(self.next);
        }
        self.p = next;
        let r = hangul(self.rb.src, p);
        Seg::Buf(decompose_hangul(&mut self.buf, r))
    }

    fn next_multi(&mut self) -> Seg {
        let d = self.multi_seg.unwrap_or(&[]);
        let mut j = 1;
        while j < d.len() && !utf8::rune_start(d[j]) {
            j += 1;
        }
        while j < d.len() {
            let info = self.rb.f.info(d, j);
            if info.boundary_before() {
                self.multi_seg = Some(&d[j..]);
                return Seg::Static(&d[..j]);
            }
            j += usize::from(info.size);
        }
        self.next = self.rb.f.next_main;
        self.call(self.next)
    }

    fn next_multi_norm(&mut self) -> Seg {
        let d = self.multi_seg.unwrap_or(&[]);
        let mut j = 0;
        while j < d.len() {
            let info = self.rb.f.info(d, j);
            if info.boundary_before() {
                self.rb.compose();
                let n = self.rb.flush_copy(&mut self.buf);
                self.rb.insert_unsafe(d, j, info);
                self.multi_seg = Some(&d[j + usize::from(info.size)..]);
                return Seg::Buf(n);
            }
            self.rb.insert_unsafe(d, j, info);
            j += usize::from(info.size);
        }
        self.multi_seg = None;
        self.next = IterFn::Composed;
        self.do_norm_composed()
    }

    fn next_decomposed(&mut self) -> Seg {
        let mut outp: usize = 0;
        let mut in_copy_start = self.p;
        let mut out_copy_start: usize = 0;
        let src = self.rb.src;
        'outer: {
            loop {
                let sz = usize::from(self.info.size);
                if sz <= 1 {
                    self.rb.ss = StreamSafe(0);
                    let p = self.p;
                    self.p += 1;
                    if self.p >= self.rb.nsrc {
                        self.set_done();
                        return Seg::Src(p, self.p);
                    } else if src[self.p] < utf8::RUNE_SELF {
                        self.next = self.ascii_f;
                        return Seg::Src(p, self.p);
                    }
                    outp += 1;
                } else if let Some(mut d) = self.info.decomposition() {
                    let mut p = outp + d.len();
                    if outp > 0 {
                        copy_into(&mut self.buf[out_copy_start..], &src[in_copy_start..self.p]);
                        if p > self.buf.len() {
                            return Seg::Buf(outp);
                        }
                    } else if self.info.multi_segment() {
                        match self.multi_seg {
                            None => {
                                self.multi_seg = Some(d);
                                self.next = IterFn::Multi;
                                return self.next_multi();
                            }
                            Some(m) => {
                                d = m;
                                self.multi_seg = None;
                                p = d.len();
                            }
                        }
                    }
                    let prev_cc = self.info.tccc;
                    self.p += sz;
                    if self.p >= self.rb.nsrc {
                        self.set_done();
                        self.info = Properties::default();
                    } else {
                        self.info = self.rb.f.info(src, self.p);
                    }
                    match self.rb.ss.next(self.info) {
                        SsState::Overflow => {
                            self.next = IterFn::CgjDecompose;
                            if outp > 0 {
                                copy_into(&mut self.buf[outp..], d);
                                return Seg::Buf(p);
                            }
                            return Seg::Static(d);
                        }
                        SsState::Starter => {
                            if outp > 0 {
                                copy_into(&mut self.buf[outp..], d);
                                return Seg::Buf(p);
                            }
                            return Seg::Static(d);
                        }
                        SsState::Success => {}
                    }
                    copy_into(&mut self.buf[outp..], d);
                    outp = p;
                    in_copy_start = self.p;
                    out_copy_start = outp;
                    if self.info.ccc < prev_cc {
                        break 'outer;
                    }
                    continue;
                } else {
                    let r = hangul(src, self.p);
                    if r != 0 {
                        outp = decompose_hangul(&mut self.buf, r);
                        self.p += HANGUL_UTF8_SIZE;
                        in_copy_start = self.p;
                        out_copy_start = outp;
                        if self.p >= self.rb.nsrc {
                            self.set_done();
                            break;
                        } else if hangul(src, self.p) != 0 {
                            self.next = IterFn::Hangul;
                            return Seg::Buf(outp);
                        }
                    } else {
                        let p = outp + sz;
                        if p > self.buf.len() {
                            break;
                        }
                        outp = p;
                        self.p += sz;
                    }
                }
                if self.p >= self.rb.nsrc {
                    self.set_done();
                    break;
                }
                let prev_cc = self.info.tccc;
                self.info = self.rb.f.info(src, self.p);
                match self.rb.ss.next(self.info) {
                    SsState::Starter => break,
                    SsState::Overflow => {
                        self.next = IterFn::CgjDecompose;
                        break;
                    }
                    SsState::Success => {}
                }
                if self.info.ccc < prev_cc {
                    break 'outer;
                }
            }
            if out_copy_start == 0 {
                return Seg::Src(in_copy_start, self.p);
            } else if in_copy_start < self.p {
                copy_into(&mut self.buf[out_copy_start..], &src[in_copy_start..self.p]);
            }
            return Seg::Buf(outp);
        }
        copy_into(&mut self.buf[out_copy_start..], &src[in_copy_start..self.p]);
        self.rb.insert_decomposed(&self.buf[..outp]);
        self.do_norm_decomposed()
    }

    fn do_norm_decomposed(&mut self) -> Seg {
        let src = self.rb.src;
        loop {
            self.rb.insert_unsafe(src, self.p, self.info);
            self.p += usize::from(self.info.size);
            if self.p >= self.rb.nsrc {
                self.set_done();
                break;
            }
            self.info = self.rb.f.info(src, self.p);
            if self.info.ccc == 0 {
                break;
            }
            if self.rb.ss.next(self.info) == SsState::Overflow {
                self.next = IterFn::CgjDecompose;
                break;
            }
        }
        Seg::Buf(self.rb.flush_copy(&mut self.buf))
    }

    fn next_cgj_decompose(&mut self) -> Seg {
        self.rb.ss = StreamSafe(0);
        self.rb.insert_cgj();
        self.next = IterFn::Decomposed;
        self.rb.ss.first(self.info);
        self.do_norm_decomposed()
    }

    fn next_composed(&mut self) -> Seg {
        let mut outp: usize = 0;
        let startp = self.p;
        let src = self.rb.src;
        'norm: {
            loop {
                if !self.info.is_yes_c() {
                    break 'norm;
                }
                let prev_cc = self.info.tccc;
                let mut sz = usize::from(self.info.size);
                if sz == 0 {
                    sz = 1;
                }
                let p = outp + sz;
                if p > self.buf.len() {
                    break;
                }
                outp = p;
                self.p += sz;
                if self.p >= self.rb.nsrc {
                    self.set_done();
                    break;
                } else if src[self.p] < utf8::RUNE_SELF {
                    self.rb.ss = StreamSafe(0);
                    self.next = self.ascii_f;
                    break;
                }
                self.info = self.rb.f.info(src, self.p);
                match self.rb.ss.next(self.info) {
                    SsState::Starter => break,
                    SsState::Overflow => {
                        self.next = IterFn::CgjCompose;
                        break;
                    }
                    SsState::Success => {}
                }
                if self.info.ccc < prev_cc {
                    break 'norm;
                }
            }
            return Seg::Src(startp, self.p);
        }
        self.p = startp;
        self.info = self.rb.f.info(src, self.p);
        self.rb.ss.first(self.info);
        if self.info.multi_segment() {
            let d = self.info.decomposition().unwrap_or(&[]);
            let info = self.rb.f.info(d, 0);
            self.rb.insert_unsafe(d, 0, info);
            self.multi_seg = Some(&d[usize::from(info.size)..]);
            self.next = IterFn::MultiNorm;
            return self.next_multi_norm();
        }
        self.rb.ss.first(self.info);
        self.rb.insert_unsafe(src, self.p, self.info);
        self.do_norm_composed()
    }

    fn do_norm_composed(&mut self) -> Seg {
        let src = self.rb.src;
        loop {
            self.p += usize::from(self.info.size);
            if self.p >= self.rb.nsrc {
                self.set_done();
                break;
            }
            self.info = self.rb.f.info(src, self.p);
            match self.rb.ss.next(self.info) {
                SsState::Starter => break,
                SsState::Overflow => {
                    self.next = IterFn::CgjCompose;
                    break;
                }
                SsState::Success => {}
            }
            self.rb.insert_unsafe(src, self.p, self.info);
        }
        self.rb.compose();
        Seg::Buf(self.rb.flush_copy(&mut self.buf))
    }

    fn next_cgj_compose(&mut self) -> Seg {
        self.rb.ss = StreamSafe(0);
        self.rb.insert_cgj();
        self.next = IterFn::Composed;
        self.rb.ss.first(self.info);
        let src = self.rb.src;
        self.rb.insert_unsafe(src, self.p, self.info);
        self.do_norm_composed()
    }
}
