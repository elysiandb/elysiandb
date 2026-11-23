use crate::strconv::{
    append_float, append_quote, append_quote_rune, append_quote_rune_to_ascii,
    append_quote_to_ascii, can_backquote, is_print,
};
use crate::utf8::{RUNE_ERROR, RUNE_SELF, Rune, UTF_MAX, append_rune, decode_rune};

pub(super) const LDIGITS: &[u8; 17] = b"0123456789abcdefx";
pub(super) const UDIGITS: &[u8; 17] = b"0123456789ABCDEFX";

pub(super) const SIGNED: bool = true;
pub(super) const UNSIGNED: bool = false;

const MAX_RUNE: u64 = 0x10FFFF;
const INTBUF_LEN: usize = 68;

#[derive(Clone, Copy, Default)]
pub(super) struct FmtFlags {
    pub(super) wid_present: bool,
    pub(super) prec_present: bool,
    pub(super) minus: bool,
    pub(super) plus: bool,
    pub(super) sharp: bool,
    pub(super) space: bool,
    pub(super) zero: bool,
    pub(super) plus_v: bool,
    pub(super) sharp_v: bool,
}

pub(super) struct Fmt {
    pub(super) buf: Vec<u8>,
    pub(super) flags: FmtFlags,
    pub(super) wid: isize,
    pub(super) prec: isize,
}

pub(super) fn rune_count(b: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] < RUNE_SELF {
            i += 1;
        } else {
            i += decode_rune(&b[i..]).1.max(1);
        }
        n += 1;
    }
    n
}

impl Fmt {
    pub(super) fn new(buf: Vec<u8>) -> Fmt {
        Fmt {
            buf,
            flags: FmtFlags::default(),
            wid: 0,
            prec: 0,
        }
    }

    pub(super) fn clearflags(&mut self) {
        self.flags = FmtFlags::default();
        self.wid = 0;
        self.prec = 0;
    }

    pub(super) fn write_padding(&mut self, n: isize) {
        if n <= 0 {
            return;
        }
        let pad_byte = if self.flags.zero && !self.flags.minus {
            b'0'
        } else {
            b' '
        };
        let new_len = self.buf.len() + n as usize;
        self.buf.resize(new_len, pad_byte);
    }

    pub(super) fn pad(&mut self, b: &[u8]) {
        if !self.flags.wid_present || self.wid == 0 {
            self.buf.extend_from_slice(b);
            return;
        }
        let width = self.wid - rune_count(b) as isize;
        if !self.flags.minus {
            self.write_padding(width);
            self.buf.extend_from_slice(b);
        } else {
            self.buf.extend_from_slice(b);
            self.write_padding(width);
        }
    }

    pub(super) fn pad_string(&mut self, s: &[u8]) {
        self.pad(s);
    }

    pub(super) fn fmt_boolean(&mut self, v: bool) {
        if v {
            self.pad_string(b"true");
        } else {
            self.pad_string(b"false");
        }
    }

    pub(super) fn fmt_unicode(&mut self, u: u64) {
        let mut size = INTBUF_LEN;

        let mut prec: isize = 4;
        if self.flags.prec_present && self.prec > 4 {
            prec = self.prec;
            let width = (2 + prec + 2 + UTF_MAX as isize + 1) as usize;
            if width > size {
                size = width;
            }
        }
        let mut buf = vec![0u8; size];

        let mut i = buf.len();

        let mut u = u;
        if self.flags.sharp && u <= MAX_RUNE && is_print(u as Rune) {
            i -= 1;
            buf[i] = b'\'';
            let mut enc = Vec::with_capacity(UTF_MAX);
            append_rune(&mut enc, u as Rune);
            i -= enc.len();
            buf[i..i + enc.len()].copy_from_slice(&enc);
            i -= 1;
            buf[i] = b'\'';
            i -= 1;
            buf[i] = b' ';
        }
        while u >= 16 {
            i -= 1;
            buf[i] = UDIGITS[(u & 0xF) as usize];
            prec -= 1;
            u >>= 4;
        }
        i -= 1;
        buf[i] = UDIGITS[u as usize];
        prec -= 1;
        while prec > 0 {
            i -= 1;
            buf[i] = b'0';
            prec -= 1;
        }
        i -= 1;
        buf[i] = b'+';
        i -= 1;
        buf[i] = b'U';

        let old_zero = self.flags.zero;
        self.flags.zero = false;
        self.pad(&buf[i..]);
        self.flags.zero = old_zero;
    }

    pub(super) fn fmt_integer(
        &mut self,
        u: u64,
        base: u32,
        is_signed: bool,
        verb: char,
        digits: &[u8; 17],
    ) {
        let negative = is_signed && (u as i64) < 0;
        let mut u = u;
        if negative {
            u = u.wrapping_neg();
        }

        let mut size = INTBUF_LEN;
        if self.flags.wid_present || self.flags.prec_present {
            let width = (3 + self.wid + self.prec) as usize;
            if width > size {
                size = width;
            }
        }
        let mut small = [0u8; INTBUF_LEN];
        let mut big;
        let buf: &mut [u8] = if size > INTBUF_LEN {
            big = vec![0u8; size];
            &mut big
        } else {
            &mut small
        };

        let mut prec: isize = 0;
        if self.flags.prec_present {
            prec = self.prec;
            if prec == 0 && u == 0 {
                let old_zero = self.flags.zero;
                self.flags.zero = false;
                self.write_padding(self.wid);
                self.flags.zero = old_zero;
                return;
            }
        } else if self.flags.zero && !self.flags.minus && self.flags.wid_present {
            prec = self.wid;
            if negative || self.flags.plus || self.flags.space {
                prec -= 1;
            }
        }

        let mut i = buf.len();
        match base {
            10 => {
                while u >= 10 {
                    i -= 1;
                    let next = u / 10;
                    buf[i] = b'0' + (u - next * 10) as u8;
                    u = next;
                }
            }
            16 => {
                while u >= 16 {
                    i -= 1;
                    buf[i] = digits[(u & 0xF) as usize];
                    u >>= 4;
                }
            }
            8 => {
                while u >= 8 {
                    i -= 1;
                    buf[i] = b'0' + (u & 7) as u8;
                    u >>= 3;
                }
            }
            2 => {
                while u >= 2 {
                    i -= 1;
                    buf[i] = b'0' + (u & 1) as u8;
                    u >>= 1;
                }
            }
            _ => panic!("unknown integer base {base}"),
        }
        i -= 1;
        buf[i] = digits[u as usize];
        while i > 0 && prec > (buf.len() - i) as isize {
            i -= 1;
            buf[i] = b'0';
        }

        if self.flags.sharp {
            match base {
                2 => {
                    i -= 1;
                    buf[i] = b'b';
                    i -= 1;
                    buf[i] = b'0';
                }
                8 => {
                    if buf[i] != b'0' {
                        i -= 1;
                        buf[i] = b'0';
                    }
                }
                16 => {
                    i -= 1;
                    buf[i] = digits[16];
                    i -= 1;
                    buf[i] = b'0';
                }
                _ => {}
            }
        }
        if verb == 'O' {
            i -= 1;
            buf[i] = b'o';
            i -= 1;
            buf[i] = b'0';
        }

        if negative {
            i -= 1;
            buf[i] = b'-';
        } else if self.flags.plus {
            i -= 1;
            buf[i] = b'+';
        } else if self.flags.space {
            i -= 1;
            buf[i] = b' ';
        }

        let old_zero = self.flags.zero;
        self.flags.zero = false;
        self.pad(&buf[i..]);
        self.flags.zero = old_zero;
    }

    pub(super) fn truncate<'b>(&self, b: &'b [u8]) -> &'b [u8] {
        if self.flags.prec_present {
            let mut n = self.prec;
            let mut i = 0;
            while i < b.len() {
                n -= 1;
                if n < 0 {
                    return &b[..i];
                }
                let mut wid = 1;
                if b[i] >= RUNE_SELF {
                    wid = decode_rune(&b[i..]).1.max(1);
                }
                i += wid;
            }
        }
        b
    }

    pub(super) fn fmt_s(&mut self, s: &[u8]) {
        let s = self.truncate(s);
        self.pad_string(s);
    }

    pub(super) fn fmt_bs(&mut self, b: &[u8]) {
        let b = self.truncate(b);
        self.pad(b);
    }

    pub(super) fn fmt_sbx(&mut self, s: &[u8], digits: &[u8; 17]) {
        let mut length = s.len() as isize;
        if self.flags.prec_present && self.prec < length {
            length = self.prec;
        }
        let mut width = 2 * length;
        if width > 0 {
            if self.flags.space {
                if self.flags.sharp {
                    width *= 2;
                }
                width += length - 1;
            } else if self.flags.sharp {
                width += 2;
            }
        } else {
            if self.flags.wid_present {
                self.write_padding(self.wid);
            }
            return;
        }
        if self.flags.wid_present && self.wid > width && !self.flags.minus {
            self.write_padding(self.wid - width);
        }
        if self.flags.sharp {
            self.buf.push(b'0');
            self.buf.push(digits[16]);
        }
        for (i, &c) in s[..length as usize].iter().enumerate() {
            if self.flags.space && i > 0 {
                self.buf.push(b' ');
                if self.flags.sharp {
                    self.buf.push(b'0');
                    self.buf.push(digits[16]);
                }
            }
            self.buf.push(digits[usize::from(c >> 4)]);
            self.buf.push(digits[usize::from(c & 0xF)]);
        }
        if self.flags.wid_present && self.wid > width && self.flags.minus {
            self.write_padding(self.wid - width);
        }
    }

    pub(super) fn fmt_sx(&mut self, s: &[u8], digits: &[u8; 17]) {
        self.fmt_sbx(s, digits);
    }

    pub(super) fn fmt_bx(&mut self, b: &[u8], digits: &[u8; 17]) {
        self.fmt_sbx(b, digits);
    }

    pub(super) fn fmt_q(&mut self, s: &[u8]) {
        let s = self.truncate(s);
        if self.flags.sharp && can_backquote(s) {
            let mut quoted = Vec::with_capacity(s.len() + 2);
            quoted.push(b'`');
            quoted.extend_from_slice(s);
            quoted.push(b'`');
            self.pad_string(&quoted);
            return;
        }
        let mut buf = Vec::with_capacity(s.len() + 2);
        if self.flags.plus {
            append_quote_to_ascii(&mut buf, s);
        } else {
            append_quote(&mut buf, s);
        }
        self.pad(&buf);
    }

    pub(super) fn fmt_c(&mut self, c: u64) {
        let r = if c > MAX_RUNE { RUNE_ERROR } else { c as Rune };
        let mut buf = Vec::with_capacity(UTF_MAX);
        append_rune(&mut buf, r);
        self.pad(&buf);
    }

    pub(super) fn fmt_qc(&mut self, c: u64) {
        let r = if c > MAX_RUNE { RUNE_ERROR } else { c as Rune };
        let mut buf = Vec::with_capacity(12);
        if self.flags.plus {
            append_quote_rune_to_ascii(&mut buf, r);
        } else {
            append_quote_rune(&mut buf, r);
        }
        self.pad(&buf);
    }

    pub(super) fn fmt_float(&mut self, v: f64, size: u32, verb: char, prec: isize) {
        let prec = if self.flags.prec_present {
            self.prec
        } else {
            prec
        };
        let mut num = Vec::with_capacity(32);
        num.push(b'+');
        append_float(&mut num, v, verb as u8, prec as i32, size);
        if num[1] == b'-' || num[1] == b'+' {
            num.remove(0);
        } else {
            num[0] = b'+';
        }
        if self.flags.space && num[0] == b'+' && !self.flags.plus {
            num[0] = b' ';
        }
        if num[1] == b'I' || num[1] == b'N' {
            let old_zero = self.flags.zero;
            self.flags.zero = false;
            if num[1] == b'N' && !self.flags.space && !self.flags.plus {
                num.remove(0);
            }
            self.pad(&num);
            self.flags.zero = old_zero;
            return;
        }
        if self.flags.sharp && verb != 'b' {
            let mut digits: isize = 0;
            if matches!(verb, 'v' | 'g' | 'G' | 'x') {
                digits = prec;
                if digits == -1 {
                    digits = 6;
                }
            }

            let mut tail: Vec<u8> = Vec::with_capacity(6);

            let mut has_decimal_point = false;
            let mut saw_nonzero_digit = false;
            let mut i = 1;
            while i < num.len() {
                match num[i] {
                    b'.' => has_decimal_point = true,
                    b'p' | b'P' => {
                        tail.extend_from_slice(&num[i..]);
                        num.truncate(i);
                    }
                    b'e' | b'E' if verb != 'x' && verb != 'X' => {
                        tail.extend_from_slice(&num[i..]);
                        num.truncate(i);
                    }
                    c => {
                        if c != b'0' {
                            saw_nonzero_digit = true;
                        }
                        if saw_nonzero_digit {
                            digits -= 1;
                        }
                    }
                }
                i += 1;
            }
            if !has_decimal_point {
                if num.len() == 2 && num[1] == b'0' {
                    digits -= 1;
                }
                num.push(b'.');
            }
            while digits > 0 {
                num.push(b'0');
                digits -= 1;
            }
            num.extend_from_slice(&tail);
        }
        if self.flags.plus || num[0] != b'+' {
            if self.flags.zero
                && !self.flags.minus
                && self.flags.wid_present
                && self.wid > num.len() as isize
            {
                self.buf.push(num[0]);
                self.write_padding(self.wid - num.len() as isize);
                self.buf.extend_from_slice(&num[1..]);
                return;
            }
            self.pad(&num);
            return;
        }
        self.pad(&num[1..]);
    }
}
