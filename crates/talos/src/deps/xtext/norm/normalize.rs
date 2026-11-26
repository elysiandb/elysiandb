use std::borrow::Cow;

use gocompat::utf8;

use super::composition::{
    Flusher, InsertErr, MAX_NON_STARTERS, ReorderBuffer, SsState, StreamSafe,
};
use super::forminfo::{FORM_TABLE, FormInfo, Properties};
use super::transform::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Form {
    Nfc = 0,
    Nfd = 1,
    Nfkc = 2,
    Nfkd = 3,
}

impl Form {
    pub(super) fn form_info(self) -> &'static FormInfo {
        &FORM_TABLE[self as usize]
    }

    pub fn bytes<'a>(&self, b: &'a [u8]) -> Cow<'a, [u8]> {
        let ft = self.form_info();
        let (n, ok) = ft.quick_span(b, 0, b.len(), true);
        if ok {
            return Cow::Borrowed(b);
        }
        let mut out = Vec::with_capacity(b.len());
        out.extend_from_slice(&b[..n]);
        let mut rb = ReorderBuffer::new(*self, b);
        rb.set_flusher(out, Flusher::Append);
        Cow::Owned(do_append_inner(&mut rb, n))
    }

    pub fn string<'a>(&self, s: &'a [u8]) -> Cow<'a, [u8]> {
        self.bytes(s)
    }

    pub fn is_normal(&self, b: &[u8]) -> bool {
        let ft = self.form_info();
        let (mut bp, ok) = ft.quick_span(b, 0, b.len(), true);
        if ok {
            return true;
        }
        let mut rb = ReorderBuffer::new(*self, b);
        rb.set_flusher(Vec::new(), Flusher::CmpNormalBytes);
        while bp < b.len() {
            rb.cmp_pos = bp;
            let r = decompose_segment(&mut rb, bp, true);
            if r < 0 {
                return false;
            }
            bp = ft.quick_span(b, r as usize, b.len(), true).0;
        }
        true
    }

    pub fn is_normal_string(&self, s: &[u8]) -> bool {
        let ft = self.form_info();
        let (mut bp, ok) = ft.quick_span(s, 0, s.len(), true);
        if ok {
            return true;
        }
        let mut rb = ReorderBuffer::new(*self, s);
        rb.set_flusher(Vec::new(), Flusher::CmpNormalString);
        while bp < s.len() {
            rb.cmp_pos = bp;
            let r = decompose_segment(&mut rb, bp, true);
            if r < 0 {
                return false;
            }
            bp = ft.quick_span(s, r as usize, s.len(), true).0;
        }
        true
    }

    pub fn append(&self, out: Vec<u8>, src: &[u8]) -> Vec<u8> {
        self.do_append(out, src, src.len())
    }

    pub fn append_string(&self, out: Vec<u8>, src: &[u8]) -> Vec<u8> {
        self.do_append(out, src, src.len())
    }

    fn do_append(&self, mut out: Vec<u8>, src: &[u8], n: usize) -> Vec<u8> {
        if n == 0 {
            return out;
        }
        let ft = self.form_info();
        if out.is_empty() {
            let (p, _) = ft.quick_span(src, 0, n, true);
            out.extend_from_slice(&src[..p]);
            if p == n {
                return out;
            }
            let mut rb = ReorderBuffer::new(*self, src);
            rb.nsrc = n;
            rb.set_flusher(out, Flusher::Append);
            return do_append_inner(&mut rb, p);
        }
        let mut rb = ReorderBuffer::new(*self, src);
        rb.nsrc = n;
        do_append(&mut rb, out, 0)
    }

    pub fn quick_span(&self, b: &[u8]) -> usize {
        self.form_info().quick_span(b, 0, b.len(), true).0
    }

    pub fn quick_span_string(&self, s: &[u8]) -> usize {
        self.quick_span(s)
    }

    pub fn span(&self, b: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
        let (n, ok) = self.form_info().quick_span(b, 0, b.len(), at_eof);
        if n < b.len() {
            if !ok {
                return (n, Err(Error::EndOfSpan));
            }
            return (n, Err(Error::ShortSrc));
        }
        (n, Ok(()))
    }

    pub fn span_string(&self, s: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
        self.span(s, at_eof)
    }

    pub fn first_boundary(&self, b: &[u8]) -> isize {
        self.first_boundary_impl(b, b.len())
    }

    pub fn first_boundary_in_string(&self, s: &[u8]) -> isize {
        self.first_boundary_impl(s, s.len())
    }

    fn first_boundary_impl(&self, src: &[u8], nsrc: usize) -> isize {
        let mut i = skip_continuation_bytes(src, 0);
        if i >= nsrc {
            return -1;
        }
        let fd = self.form_info();
        let mut ss = StreamSafe(0);
        loop {
            let info = fd.info(src, i);
            if info.size == 0 {
                return -1;
            }
            if ss.next(info) != SsState::Success {
                return i as isize;
            }
            i += usize::from(info.size);
            if i >= nsrc {
                if !info.boundary_after() && !ss.is_max() {
                    return -1;
                }
                return nsrc as isize;
            }
        }
    }

    pub fn next_boundary(&self, b: &[u8], at_eof: bool) -> isize {
        self.next_boundary_impl(b, b.len(), at_eof)
    }

    pub fn next_boundary_in_string(&self, s: &[u8], at_eof: bool) -> isize {
        self.next_boundary_impl(s, s.len(), at_eof)
    }

    fn next_boundary_impl(&self, src: &[u8], nsrc: usize, at_eof: bool) -> isize {
        if nsrc == 0 {
            if at_eof {
                return 0;
            }
            return -1;
        }
        let fd = self.form_info();
        let mut info = fd.info(src, 0);
        if info.size == 0 {
            if at_eof {
                return 1;
            }
            return -1;
        }
        let mut ss = StreamSafe(0);
        ss.first(info);
        let mut i = usize::from(info.size);
        while i < nsrc {
            info = fd.info(src, i);
            if info.size == 0 {
                if at_eof {
                    return i as isize;
                }
                return -1;
            }
            if ss.next(info) != SsState::Success {
                return i as isize;
            }
            i += usize::from(info.size);
        }
        if !at_eof && !info.boundary_after() && !ss.is_max() {
            return -1;
        }
        nsrc as isize
    }

    pub fn last_boundary(&self, b: &[u8]) -> isize {
        last_boundary(self.form_info(), b)
    }
}

impl FormInfo {
    pub(super) fn quick_span(
        &self,
        src: &[u8],
        mut i: usize,
        end: usize,
        at_eof: bool,
    ) -> (usize, bool) {
        let mut last_cc: u8 = 0;
        let mut ss = StreamSafe(0);
        let mut last_seg_start = i;
        let n = end;
        while i < n {
            let j = skip_ascii(src, i, n);
            if i != j {
                i = j;
                last_seg_start = i - 1;
                last_cc = 0;
                ss = StreamSafe(0);
                continue;
            }
            let info = self.info(src, i);
            if info.size == 0 {
                if at_eof {
                    return (n, true);
                }
                return (last_seg_start, true);
            }
            match ss.next(info) {
                SsState::Starter => last_seg_start = i,
                SsState::Overflow => return (last_seg_start, false),
                SsState::Success => {
                    if last_cc > info.ccc {
                        return (last_seg_start, false);
                    }
                }
            }
            if self.composing {
                if !info.is_yes_c() {
                    break;
                }
            } else if !info.is_yes_d() {
                break;
            }
            last_cc = info.ccc;
            i += usize::from(info.size);
        }
        if i == n {
            if !at_eof {
                return (last_seg_start, true);
            }
            return (n, true);
        }
        (last_seg_start, false)
    }
}

pub(super) fn skip_ascii(src: &[u8], mut p: usize, max: usize) -> usize {
    while p < max && src[p] < utf8::RUNE_SELF {
        p += 1;
    }
    p
}

pub(super) fn skip_continuation_bytes(src: &[u8], mut p: usize) -> usize {
    while p < src.len() && !utf8::rune_start(src[p]) {
        p += 1;
    }
    p
}

fn patch_tail(rb: &mut ReorderBuffer<'_>) -> bool {
    let (info, p) = last_rune_start(rb.f, &rb.out);
    if p == -1 || info.size == 0 {
        return true;
    }
    let p = p as usize;
    let end = p + usize::from(info.size);
    let extra = rb.out.len() - end;
    if extra > 0 {
        let x = rb.out[rb.out.len() - extra..].to_vec();
        rb.out.truncate(end);
        decompose_to_last_boundary(rb);
        rb.do_flush();
        rb.out.extend_from_slice(&x);
        return false;
    }
    let buf = rb.out[p..].to_vec();
    rb.out.truncate(p);
    decompose_to_last_boundary(rb);
    match rb.ss.next(info) {
        SsState::Starter => {
            rb.do_flush();
            rb.ss.first(info);
        }
        SsState::Overflow => {
            rb.do_flush();
            rb.insert_cgj();
            rb.ss = StreamSafe(0);
        }
        SsState::Success => {}
    }
    rb.insert_unsafe(&buf, 0, info);
    true
}

fn append_quick(rb: &mut ReorderBuffer<'_>, i: usize) -> usize {
    if rb.nsrc == i {
        return i;
    }
    let (end, _) = rb.f.quick_span(rb.src, i, rb.nsrc, true);
    let src = rb.src;
    rb.out.extend_from_slice(&src[i..end]);
    end
}

fn do_append(rb: &mut ReorderBuffer<'_>, out: Vec<u8>, mut p: usize) -> Vec<u8> {
    rb.set_flusher(out, Flusher::Append);
    let src = rb.src;
    let n = rb.nsrc;
    let mut do_merge = !rb.out.is_empty();
    let q = skip_continuation_bytes(src, p);
    if q > p {
        rb.out.extend_from_slice(&src[p..q]);
        p = q;
        do_merge = patch_tail(rb);
    }
    if do_merge {
        let mut info = Properties::default();
        if p < n {
            info = rb.f.info(src, p);
            if !info.boundary_before() || info.n_leading_non_starters() > 0 {
                if p == 0 {
                    decompose_to_last_boundary(rb);
                }
                p = decompose_segment(rb, p, true) as usize;
            }
        }
        if info.size == 0 {
            rb.do_flush();
            let mut out = std::mem::take(&mut rb.out);
            out.extend_from_slice(&src[p..n]);
            return out;
        }
        if rb.nrune > 0 {
            return do_append_inner(rb, p);
        }
    }
    p = append_quick(rb, p);
    do_append_inner(rb, p)
}

pub(super) fn do_append_inner(rb: &mut ReorderBuffer<'_>, mut p: usize) -> Vec<u8> {
    let n = rb.nsrc;
    while p < n {
        p = decompose_segment(rb, p, true) as usize;
        p = append_quick(rb, p);
    }
    std::mem::take(&mut rb.out)
}

fn last_boundary(fd: &FormInfo, b: &[u8]) -> isize {
    let mut i = b.len();
    let (mut info, mut p) = last_rune_start(fd, b);
    if p == -1 {
        return -1;
    }
    if info.size == 0 {
        if p == 0 {
            return -1;
        }
        i = p as usize;
        (info, p) = last_rune_start(fd, &b[..i]);
        if p == -1 {
            return i as isize;
        }
    }
    if p + isize::from(info.size) != i as isize {
        return i as isize;
    }
    if info.boundary_after() {
        return i as isize;
    }
    let mut ss = StreamSafe(0);
    let mut v = ss.backwards(info);
    let mut i = p;
    while i >= 0 && v != SsState::Starter {
        (info, p) = last_rune_start(fd, &b[..i as usize]);
        v = ss.backwards(info);
        if v == SsState::Overflow {
            break;
        }
        if p + isize::from(info.size) != i {
            if p == -1 {
                return -1;
            }
            return i;
        }
        i = p;
    }
    i
}

pub(super) fn decompose_segment(rb: &mut ReorderBuffer<'_>, sp: usize, at_eof: bool) -> isize {
    let src = rb.src;
    let mut sp = sp;
    let mut info = rb.f.info(src, sp);
    if info.size == 0 {
        return 0;
    }
    'body: {
        match rb.ss.next(info) {
            SsState::Starter => {
                if rb.nrune > 0 {
                    break 'body;
                }
            }
            SsState::Overflow => {
                rb.insert_cgj();
                break 'body;
            }
            SsState::Success => {}
        }
        let err = rb.insert_flush(src, sp, info);
        if err != InsertErr::Success {
            return err.code();
        }
        loop {
            sp += usize::from(info.size);
            if sp >= rb.nsrc {
                if !at_eof && !info.boundary_after() {
                    return InsertErr::ShortSrc.code();
                }
                break;
            }
            info = rb.f.info(src, sp);
            if info.size == 0 {
                if !at_eof {
                    return InsertErr::ShortSrc.code();
                }
                break;
            }
            match rb.ss.next(info) {
                SsState::Starter => break,
                SsState::Overflow => {
                    rb.insert_cgj();
                    break;
                }
                SsState::Success => {}
            }
            let err = rb.insert_flush(src, sp, info);
            if err != InsertErr::Success {
                return err.code();
            }
        }
    }
    if !rb.do_flush() {
        return InsertErr::ShortDst.code();
    }
    sp as isize
}

pub(super) fn last_rune_start(fd: &FormInfo, buf: &[u8]) -> (Properties, isize) {
    let mut p = buf.len() as isize - 1;
    while p >= 0 && !utf8::rune_start(buf[p as usize]) {
        p -= 1;
    }
    if p < 0 {
        return (Properties::default(), -1);
    }
    (fd.info(buf, p as usize), p)
}

pub(super) fn decompose_to_last_boundary(rb: &mut ReorderBuffer<'_>) {
    let fd = rb.f;
    let (mut info, mut i) = last_rune_start(fd, &rb.out);
    if isize::from(info.size) != rb.out.len() as isize - i {
        return;
    }
    if info.boundary_after() {
        return;
    }
    let mut add = [Properties::default(); MAX_NON_STARTERS + 1];
    let mut padd: usize = 0;
    let mut ss = StreamSafe(0);
    let mut p = rb.out.len() as isize;
    loop {
        add[padd] = info;
        let v = ss.backwards(info);
        if v == SsState::Overflow {
            break;
        }
        padd += 1;
        p -= isize::from(info.size);
        if v == SsState::Starter || p < 0 {
            break;
        }
        (info, i) = last_rune_start(fd, &rb.out[..p as usize]);
        if isize::from(info.size) != p - i {
            break;
        }
    }
    rb.ss = ss;
    let mut buf = [0u8; super::composition::MAX_BUFFER_SIZE * utf8::UTF_MAX];
    let tail = &rb.out[p as usize..];
    let n = tail.len().min(buf.len());
    buf[..n].copy_from_slice(&tail[..n]);
    let mut cp: &[u8] = &buf[..n];
    rb.out.truncate(p as usize);
    while padd > 0 {
        padd -= 1;
        info = add[padd];
        rb.insert_unsafe(cp, 0, info);
        cp = &cp[usize::from(info.size)..];
    }
}
