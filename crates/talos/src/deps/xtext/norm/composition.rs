use gocompat::utf8::{self, Rune, UTF_MAX};

use super::forminfo::{FORM_TABLE, FormInfo, Properties, combine, recomp_map};
use super::normalize::Form;

pub(super) const MAX_NON_STARTERS: usize = 30;
pub(super) const MAX_BUFFER_SIZE: usize = MAX_NON_STARTERS + 2;
pub(super) const MAX_BYTE_BUFFER_SIZE: usize = UTF_MAX * MAX_BUFFER_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SsState {
    Success,
    Starter,
    Overflow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct StreamSafe(pub(super) u8);

impl StreamSafe {
    pub(super) fn first(&mut self, p: Properties) {
        self.0 = p.n_trailing_non_starters();
    }

    pub(super) fn next(&mut self, p: Properties) -> SsState {
        if usize::from(self.0) > MAX_NON_STARTERS {
            panic!("streamSafe was not reset");
        }
        let n = p.n_leading_non_starters();
        self.0 = self.0.wrapping_add(n);
        if usize::from(self.0) > MAX_NON_STARTERS {
            self.0 = 0;
            return SsState::Overflow;
        }
        if n == 0 {
            self.0 = p.n_trailing_non_starters();
            return SsState::Starter;
        }
        SsState::Success
    }

    pub(super) fn backwards(&mut self, p: Properties) -> SsState {
        if usize::from(self.0) > MAX_NON_STARTERS {
            panic!("streamSafe was not reset");
        }
        let c = self.0.wrapping_add(p.n_trailing_non_starters());
        if usize::from(c) > MAX_NON_STARTERS {
            return SsState::Overflow;
        }
        self.0 = c;
        if p.n_leading_non_starters() == 0 {
            return SsState::Starter;
        }
        SsState::Success
    }

    pub(super) fn is_max(&self) -> bool {
        usize::from(self.0) == MAX_NON_STARTERS
    }
}

pub const GRAPHEME_JOINER: &str = "\u{034F}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flusher {
    Nil,
    Append,
    CmpNormalBytes,
    CmpNormalString,
}

pub(super) struct ReorderBuffer<'a> {
    pub(super) rune: [Properties; MAX_BUFFER_SIZE],
    pub(super) byte: [u8; MAX_BYTE_BUFFER_SIZE],
    pub(super) nbyte: u8,
    pub(super) ss: StreamSafe,
    pub(super) nrune: usize,
    pub(super) f: &'static FormInfo,
    pub(super) src: &'a [u8],
    pub(super) nsrc: usize,
    pub(super) out: Vec<u8>,
    pub(super) flush_f: Flusher,
    pub(super) cmp_pos: usize,
}

#[inline]
pub(super) fn copy_into(dst: &mut [u8], src: &[u8]) -> usize {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InsertErr {
    Success,
    ShortDst,
    ShortSrc,
}

impl InsertErr {
    pub(super) fn code(self) -> isize {
        match self {
            InsertErr::Success => 0,
            InsertErr::ShortDst => -1,
            InsertErr::ShortSrc => -2,
        }
    }
}

impl<'a> ReorderBuffer<'a> {
    pub(super) fn new(f: Form, src: &'a [u8]) -> Self {
        ReorderBuffer {
            rune: [Properties::default(); MAX_BUFFER_SIZE],
            byte: [0; MAX_BYTE_BUFFER_SIZE],
            nbyte: 0,
            ss: StreamSafe(0),
            nrune: 0,
            f: &FORM_TABLE[f as usize],
            src,
            nsrc: src.len(),
            out: Vec::new(),
            flush_f: Flusher::Nil,
            cmp_pos: 0,
        }
    }

    pub(super) fn init(&mut self, f: Form, src: &'a [u8]) {
        self.f = &FORM_TABLE[f as usize];
        self.src = src;
        self.nsrc = src.len();
        self.ss = StreamSafe(0);
    }

    pub(super) fn set_flusher(&mut self, out: Vec<u8>, f: Flusher) {
        self.out = out;
        self.flush_f = f;
    }

    pub(super) fn reset(&mut self) {
        self.nrune = 0;
        self.nbyte = 0;
    }

    pub(super) fn do_flush(&mut self) -> bool {
        if self.f.composing {
            self.compose();
        }
        let res = match self.flush_f {
            Flusher::Nil => {
                panic!("runtime error: invalid memory address or nil pointer dereference")
            }
            Flusher::Append => append_flush(self),
            Flusher::CmpNormalBytes => cmp_normal_bytes(self),
            Flusher::CmpNormalString => cmp_normal_string(self),
        };
        self.reset();
        res
    }

    pub(super) fn flush_copy(&mut self, buf: &mut [u8]) -> usize {
        let mut p = 0;
        for i in 0..self.nrune {
            let runep = self.rune[i];
            let start = usize::from(runep.pos);
            let end = start + usize::from(runep.size);
            p += copy_into(&mut buf[p..], &self.byte[start..end]);
        }
        self.reset();
        p
    }

    pub(super) fn insert_ordered(&mut self, mut info: Properties) {
        let mut n = self.nrune;
        let cc = info.ccc;
        if cc > 0 {
            while n > 0 {
                if self.rune[n - 1].ccc <= cc {
                    break;
                }
                self.rune[n] = self.rune[n - 1];
                n -= 1;
            }
        }
        self.nrune += 1;
        let pos = self.nbyte;
        self.nbyte = self.nbyte.wrapping_add(UTF_MAX as u8);
        info.pos = pos;
        self.rune[n] = info;
    }

    pub(super) fn insert_flush(&mut self, src: &[u8], i: usize, info: Properties) -> InsertErr {
        let rune = hangul(src, i);
        if rune != 0 {
            self.decompose_hangul(rune);
            return InsertErr::Success;
        }
        if info.has_decomposition() {
            return self.insert_decomposed(info.decomposition().unwrap_or(&[]));
        }
        self.insert_single(src, i, info);
        InsertErr::Success
    }

    pub(super) fn insert_unsafe(&mut self, src: &[u8], i: usize, info: Properties) {
        let rune = hangul(src, i);
        if rune != 0 {
            self.decompose_hangul(rune);
        }
        if info.has_decomposition() {
            self.insert_decomposed(info.decomposition().unwrap_or(&[]));
        } else {
            self.insert_single(src, i, info);
        }
    }

    pub(super) fn insert_decomposed(&mut self, dcomp: &[u8]) -> InsertErr {
        let mut i = 0;
        while i < dcomp.len() {
            let info = self.f.info(dcomp, i);
            if info.boundary_before() && self.nrune > 0 && !self.do_flush() {
                return InsertErr::ShortDst;
            }
            let nb = usize::from(self.nbyte);
            i += copy_into(&mut self.byte[nb..], &dcomp[i..i + usize::from(info.size)]);
            self.insert_ordered(info);
        }
        InsertErr::Success
    }

    pub(super) fn insert_single(&mut self, src: &[u8], i: usize, info: Properties) {
        let nb = usize::from(self.nbyte);
        copy_into(&mut self.byte[nb..], &src[i..i + usize::from(info.size)]);
        self.insert_ordered(info);
    }

    pub(super) fn insert_cgj(&mut self) {
        let cgj = GRAPHEME_JOINER.as_bytes();
        self.insert_single(
            cgj,
            0,
            Properties {
                size: cgj.len() as u8,
                ..Properties::default()
            },
        );
    }

    pub(super) fn append_rune(&mut self, r: Rune) {
        let bn = self.nbyte;
        let sz = utf8::encode_rune(&mut self.byte[usize::from(bn)..], r);
        self.nbyte = self.nbyte.wrapping_add(UTF_MAX as u8);
        self.rune[self.nrune] = Properties {
            pos: bn,
            size: sz as u8,
            ..Properties::default()
        };
        self.nrune += 1;
    }

    pub(super) fn assign_rune(&mut self, pos: usize, r: Rune) {
        let bn = self.rune[pos].pos;
        let sz = utf8::encode_rune(&mut self.byte[usize::from(bn)..], r);
        self.rune[pos] = Properties {
            pos: bn,
            size: sz as u8,
            ..Properties::default()
        };
    }

    pub(super) fn rune_at(&self, n: usize) -> Rune {
        utf8::decode_rune(self.bytes_at(n)).0
    }

    pub(super) fn bytes_at(&self, n: usize) -> &[u8] {
        let inf = self.rune[n];
        let start = usize::from(inf.pos);
        &self.byte[start..start + usize::from(inf.size)]
    }

    pub(super) fn decompose_hangul(&mut self, r: Rune) {
        let mut r = r - HANGUL_BASE;
        let x = r % JAMO_T_COUNT;
        r /= JAMO_T_COUNT;
        self.append_rune(JAMO_L_BASE + r / JAMO_V_COUNT);
        self.append_rune(JAMO_V_BASE + r % JAMO_V_COUNT);
        if x != 0 {
            self.append_rune(JAMO_T_BASE + x);
        }
    }

    fn combine_hangul(&mut self, mut s: usize, mut i: usize, mut k: usize) {
        let bn = self.nrune;
        while i < bn {
            let ccc_b = self.rune[k - 1].ccc;
            let ccc_c = self.rune[i].ccc;
            if ccc_b == 0 {
                s = k - 1;
            }
            if s != k - 1 && ccc_b >= ccc_c {
                self.rune[k] = self.rune[i];
                k += 1;
            } else {
                let l = self.rune_at(s);
                let v = self.rune_at(i);
                if (JAMO_L_BASE..JAMO_L_END).contains(&l) && (JAMO_V_BASE..JAMO_V_END).contains(&v)
                {
                    self.assign_rune(
                        s,
                        HANGUL_BASE
                            + (l - JAMO_L_BASE) * JAMO_VT_COUNT
                            + (v - JAMO_V_BASE) * JAMO_T_COUNT,
                    );
                } else if (HANGUL_BASE..HANGUL_END).contains(&l)
                    && JAMO_T_BASE < v
                    && v < JAMO_T_END
                    && (l - HANGUL_BASE) % JAMO_T_COUNT == 0
                {
                    self.assign_rune(s, l + v - JAMO_T_BASE);
                } else {
                    self.rune[k] = self.rune[i];
                    k += 1;
                }
            }
            i += 1;
        }
        self.nrune = k;
    }

    pub(super) fn compose(&mut self) {
        let map = recomp_map();
        let bn = self.nrune;
        if bn == 0 {
            return;
        }
        let mut k = 1;
        let mut s = 0;
        let mut i = 1;
        while i < bn {
            if is_jamo_vt(self.bytes_at(i)) {
                self.combine_hangul(s, i, k);
                return;
            }
            let ii = self.rune[i];
            if ii.combines_backward() {
                let ccc_b = self.rune[k - 1].ccc;
                let ccc_c = ii.ccc;
                let mut blocked = false;
                if ccc_b == 0 {
                    s = k - 1;
                } else {
                    blocked = s != k - 1 && ccc_b >= ccc_c;
                }
                if !blocked {
                    let combined = combine(map, self.rune_at(s), self.rune_at(i));
                    if combined != 0 {
                        self.assign_rune(s, combined);
                        i += 1;
                        continue;
                    }
                }
            }
            self.rune[k] = self.rune[i];
            k += 1;
            i += 1;
        }
        self.nrune = k;
    }
}

pub(super) fn append_flush(rb: &mut ReorderBuffer<'_>) -> bool {
    for i in 0..rb.nrune {
        let start = usize::from(rb.rune[i].pos);
        let end = start + usize::from(rb.rune[i].size);
        rb.out.extend_from_slice(&rb.byte[start..end]);
    }
    true
}

fn cmp_normal_bytes(rb: &mut ReorderBuffer<'_>) -> bool {
    let mut b = &rb.src[rb.cmp_pos..];
    for i in 0..rb.nrune {
        let info = rb.rune[i];
        if usize::from(info.size) > b.len() {
            return false;
        }
        let mut p = usize::from(info.pos);
        let pe = p + usize::from(info.size);
        while p < pe {
            if b[0] != rb.byte[p] {
                return false;
            }
            b = &b[1..];
            p += 1;
        }
    }
    true
}

fn cmp_normal_string(rb: &mut ReorderBuffer<'_>) -> bool {
    let s = rb.src;
    for i in 0..rb.nrune {
        let info = rb.rune[i];
        if rb.cmp_pos + usize::from(info.size) > s.len() {
            return false;
        }
        let mut p = usize::from(info.pos);
        let pe = p + usize::from(info.size);
        while p < pe {
            if s[rb.cmp_pos] != rb.byte[p] {
                return false;
            }
            rb.cmp_pos += 1;
            p += 1;
        }
    }
    true
}

pub(super) const HANGUL_BASE: Rune = 0xAC00;
const HANGUL_BASE0: u8 = 0xEA;
const HANGUL_BASE1: u8 = 0xB0;
pub(super) const HANGUL_END: Rune = HANGUL_BASE + JAMO_LVT_COUNT;
const HANGUL_END0: u8 = 0xED;
const HANGUL_END1: u8 = 0x9E;
const HANGUL_END2: u8 = 0xA4;
pub(super) const JAMO_L_BASE: Rune = 0x1100;
const JAMO_L_BASE0: u8 = 0xE1;
const JAMO_L_BASE1: u8 = 0x84;
const JAMO_L_END: Rune = 0x1113;
pub(super) const JAMO_V_BASE: Rune = 0x1161;
const JAMO_V_END: Rune = 0x1176;
pub(super) const JAMO_T_BASE: Rune = 0x11A7;
const JAMO_T_END: Rune = 0x11C3;
const JAMO_T_COUNT: Rune = 28;
const JAMO_V_COUNT: Rune = 21;
const JAMO_VT_COUNT: Rune = 21 * 28;
const JAMO_LVT_COUNT: Rune = 19 * 21 * 28;

pub(super) const HANGUL_UTF8_SIZE: usize = 3;

pub(super) fn is_hangul(b: &[u8]) -> bool {
    if b.len() < HANGUL_UTF8_SIZE {
        return false;
    }
    let b0 = b[0];
    if b0 < HANGUL_BASE0 {
        return false;
    }
    let b1 = b[1];
    if b0 == HANGUL_BASE0 {
        return b1 >= HANGUL_BASE1;
    }
    if b0 < HANGUL_END0 {
        return true;
    }
    if b0 > HANGUL_END0 {
        return false;
    }
    if b1 < HANGUL_END1 {
        return true;
    }
    b1 == HANGUL_END1 && b[2] < HANGUL_END2
}

fn is_jamo_vt(b: &[u8]) -> bool {
    b[0] == JAMO_L_BASE0 && (b[1] & 0xFC) == JAMO_L_BASE1
}

pub(super) fn decompose_hangul(buf: &mut [u8], r: Rune) -> usize {
    const JAMO_UTF8_LEN: usize = 3;
    let mut r = r - HANGUL_BASE;
    let x = r % JAMO_T_COUNT;
    r /= JAMO_T_COUNT;
    utf8::encode_rune(buf, JAMO_L_BASE + r / JAMO_V_COUNT);
    utf8::encode_rune(&mut buf[JAMO_UTF8_LEN..], JAMO_V_BASE + r % JAMO_V_COUNT);
    if x != 0 {
        utf8::encode_rune(&mut buf[2 * JAMO_UTF8_LEN..], JAMO_T_BASE + x);
        return 3 * JAMO_UTF8_LEN;
    }
    2 * JAMO_UTF8_LEN
}

pub(super) fn hangul(src: &[u8], p: usize) -> Rune {
    if !is_hangul(&src[p..]) {
        return 0;
    }
    let (r, size) = utf8::decode_rune(&src[p..]);
    if size != HANGUL_UTF8_SIZE {
        return 0;
    }
    r
}
