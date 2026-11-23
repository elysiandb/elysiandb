use super::Arg;
use super::format::{Fmt, LDIGITS, SIGNED, UDIGITS, UNSIGNED};
use crate::gocompat::utf8::{RUNE_SELF, decode_rune};
use std::cmp::Ordering;

const COMMA_SPACE_STRING: &[u8] = b", ";
const NIL_ANGLE_STRING: &[u8] = b"<nil>";
const NIL_PAREN_STRING: &[u8] = b"(nil)";
const MAP_STRING: &[u8] = b"map[";
const PERCENT_BANG_STRING: &[u8] = b"%!";
const MISSING_STRING: &[u8] = b"(MISSING)";
const BAD_INDEX_STRING: &[u8] = b"(BADINDEX)";
const EXTRA_STRING: &[u8] = b"%!(EXTRA ";
const BAD_WIDTH_STRING: &[u8] = b"%!(BADWIDTH)";
const BAD_PREC_STRING: &[u8] = b"%!(BADPREC)";
const NO_VERB_STRING: &[u8] = b"%!(NOVERB)";

struct Printer {
    fmt: Fmt,
    reordered: bool,
    good_arg_num: bool,
    erroring: bool,
}

pub fn sprintf(format: &str, a: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    appendf(&mut out, format, a);
    out
}

pub fn appendf(dst: &mut Vec<u8>, format: &str, a: &[Arg]) {
    let mut p = Printer::new(std::mem::take(dst));
    p.do_printf(format.as_bytes(), a);
    *dst = p.fmt.buf;
}

pub fn sprint(a: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    append(&mut out, a);
    out
}

pub fn append(dst: &mut Vec<u8>, a: &[Arg]) {
    let mut p = Printer::new(std::mem::take(dst));
    p.do_print(a);
    *dst = p.fmt.buf;
}

pub fn sprintln(a: &[Arg]) -> Vec<u8> {
    let mut out = Vec::new();
    appendln(&mut out, a);
    out
}

pub fn appendln(dst: &mut Vec<u8>, a: &[Arg]) {
    let mut p = Printer::new(std::mem::take(dst));
    p.do_println(a);
    *dst = p.fmt.buf;
}

fn too_large(x: isize) -> bool {
    const MAX: isize = 1_000_000;
    !(-MAX..=MAX).contains(&x)
}

fn parsenum(s: &[u8], start: usize, end: usize) -> (isize, bool, usize) {
    if start >= end {
        return (0, false, end);
    }
    let mut num: isize = 0;
    let mut isnum = false;
    let mut newi = start;
    while newi < end && s[newi].is_ascii_digit() {
        if too_large(num) {
            return (0, false, end);
        }
        num = num * 10 + isize::from(s[newi] - b'0');
        isnum = true;
        newi += 1;
    }
    (num, isnum, newi)
}

fn int_value(arg: &Arg) -> Option<isize> {
    match arg {
        Arg::Int(n) => isize::try_from(*n).ok(),
        Arg::Uint(n) => isize::try_from(*n).ok(),
        Arg::Stringer(_, inner) => int_value(inner),
        _ => None,
    }
}

fn int_from_arg(a: &[Arg], arg_num: usize) -> (isize, bool, usize) {
    let mut num = 0;
    let mut is_int = false;
    let mut new_arg_num = arg_num;
    if arg_num < a.len() {
        if let Some(n) = int_value(&a[arg_num]) {
            num = n;
            is_int = true;
        }
        new_arg_num = arg_num + 1;
        if too_large(num) {
            num = 0;
            is_int = false;
        }
    }
    (num, is_int, new_arg_num)
}

fn parse_arg_number(format: &[u8]) -> (isize, usize, bool) {
    if format.len() < 3 {
        return (0, 1, false);
    }
    for (i, &c) in format.iter().enumerate().skip(1) {
        if c == b']' {
            let (width, ok, newi) = parsenum(format, 1, i);
            if !ok || newi != i {
                return (0, i + 1, false);
            }
            return (width - 1, i + 1, true);
        }
    }
    (0, 1, false)
}

fn is_string_kind(arg: &Arg) -> bool {
    match arg {
        Arg::Str(_) => true,
        Arg::Stringer(_, inner) => is_string_kind(inner),
        _ => false,
    }
}

fn kind_rank(arg: &Arg) -> u8 {
    match arg {
        Arg::Nil => 0,
        Arg::Bool(_) => 1,
        Arg::Int(_) => 2,
        Arg::Uint(_) => 3,
        Arg::Float(_) => 4,
        Arg::Float32(_) => 5,
        Arg::Str(_) => 6,
        Arg::Bytes(_) => 7,
        Arg::Slice(_) => 8,
        Arg::Map(_) => 9,
        Arg::Struct(_) => 10,
        Arg::Stringer(_, _) => 11,
        Arg::Error(_) => 12,
    }
}

fn compare_floats(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

fn compare_slices(a: &[Arg], b: &[Arg]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let c = compare(x, y);
        if c != Ordering::Equal {
            return c;
        }
    }
    a.len().cmp(&b.len())
}

fn compare_fields(a: &[(&str, Arg)], b: &[(&str, Arg)]) -> Ordering {
    for ((_, x), (_, y)) in a.iter().zip(b) {
        let c = compare(x, y);
        if c != Ordering::Equal {
            return c;
        }
    }
    a.len().cmp(&b.len())
}

fn compare(a: &Arg, b: &Arg) -> Ordering {
    match (a, b) {
        (Arg::Nil, Arg::Nil) => Ordering::Equal,
        (Arg::Bool(x), Arg::Bool(y)) => x.cmp(y),
        (Arg::Int(x), Arg::Int(y)) => x.cmp(y),
        (Arg::Uint(x), Arg::Uint(y)) => x.cmp(y),
        (Arg::Float(x), Arg::Float(y)) => compare_floats(*x, *y),
        (Arg::Float32(x), Arg::Float32(y)) => compare_floats(f64::from(*x), f64::from(*y)),
        (Arg::Str(x), Arg::Str(y)) | (Arg::Bytes(x), Arg::Bytes(y)) => x.cmp(y),
        (Arg::Error(x), Arg::Error(y)) => x.cmp(y),
        (Arg::Slice(x), Arg::Slice(y)) => compare_slices(x, y),
        (Arg::Struct(x), Arg::Struct(y)) => compare_fields(x, y),
        (Arg::Map(x), Arg::Map(y)) => x.len().cmp(&y.len()),
        (Arg::Stringer(_, x), Arg::Stringer(_, y)) => compare(x, y),
        _ => kind_rank(a).cmp(&kind_rank(b)),
    }
}

impl Printer {
    fn new(buf: Vec<u8>) -> Printer {
        Printer {
            fmt: Fmt::new(buf),
            reordered: false,
            good_arg_num: true,
            erroring: false,
        }
    }

    fn write_rune(&mut self, r: char) {
        let mut tmp = [0u8; 4];
        self.fmt
            .buf
            .extend_from_slice(r.encode_utf8(&mut tmp).as_bytes());
    }

    fn bad_verb(&mut self, verb: char, arg: &Arg) {
        self.erroring = true;
        self.fmt.buf.extend_from_slice(PERCENT_BANG_STRING);
        self.write_rune(verb);
        self.fmt.buf.push(b'(');
        match arg {
            Arg::Nil => self.fmt.buf.extend_from_slice(NIL_ANGLE_STRING),
            _ => self.print_arg(arg, 'v'),
        }
        self.fmt.buf.push(b')');
        self.erroring = false;
    }

    fn fmt_bool(&mut self, v: bool, verb: char, arg: &Arg) {
        match verb {
            't' | 'v' => self.fmt.fmt_boolean(v),
            _ => self.bad_verb(verb, arg),
        }
    }

    fn fmt_0x64(&mut self, v: u64, leading0x: bool) {
        let sharp = self.fmt.flags.sharp;
        self.fmt.flags.sharp = leading0x;
        self.fmt.fmt_integer(v, 16, UNSIGNED, 'v', LDIGITS);
        self.fmt.flags.sharp = sharp;
    }

    fn fmt_integer(&mut self, v: u64, is_signed: bool, verb: char, arg: &Arg) {
        match verb {
            'v' => {
                if self.fmt.flags.sharp_v && !is_signed {
                    self.fmt_0x64(v, true);
                } else {
                    self.fmt.fmt_integer(v, 10, is_signed, verb, LDIGITS);
                }
            }
            'd' => self.fmt.fmt_integer(v, 10, is_signed, verb, LDIGITS),
            'b' => self.fmt.fmt_integer(v, 2, is_signed, verb, LDIGITS),
            'o' | 'O' => self.fmt.fmt_integer(v, 8, is_signed, verb, LDIGITS),
            'x' => self.fmt.fmt_integer(v, 16, is_signed, verb, LDIGITS),
            'X' => self.fmt.fmt_integer(v, 16, is_signed, verb, UDIGITS),
            'c' => self.fmt.fmt_c(v),
            'q' => self.fmt.fmt_qc(v),
            'U' => self.fmt.fmt_unicode(v),
            _ => self.bad_verb(verb, arg),
        }
    }

    fn fmt_float(&mut self, v: f64, size: u32, verb: char, arg: &Arg) {
        match verb {
            'v' => self.fmt.fmt_float(v, size, 'g', -1),
            'b' | 'g' | 'G' | 'x' | 'X' => self.fmt.fmt_float(v, size, verb, -1),
            'f' | 'e' | 'E' => self.fmt.fmt_float(v, size, verb, 6),
            'F' => self.fmt.fmt_float(v, size, 'f', 6),
            _ => self.bad_verb(verb, arg),
        }
    }

    fn fmt_string(&mut self, v: &[u8], verb: char, arg: &Arg) {
        match verb {
            'v' => {
                if self.fmt.flags.sharp_v {
                    self.fmt.fmt_q(v);
                } else {
                    self.fmt.fmt_s(v);
                }
            }
            's' => self.fmt.fmt_s(v),
            'x' => self.fmt.fmt_sx(v, LDIGITS),
            'X' => self.fmt.fmt_sx(v, UDIGITS),
            'q' => self.fmt.fmt_q(v),
            _ => self.bad_verb(verb, arg),
        }
    }

    fn fmt_bytes(&mut self, v: &[u8], verb: char, arg: &Arg) {
        match verb {
            'v' | 'd' => {
                if self.fmt.flags.sharp_v {
                    self.fmt.buf.push(b'{');
                    for (i, &c) in v.iter().enumerate() {
                        if i > 0 {
                            self.fmt.buf.extend_from_slice(COMMA_SPACE_STRING);
                        }
                        self.fmt_0x64(u64::from(c), true);
                    }
                    self.fmt.buf.push(b'}');
                } else {
                    self.fmt.buf.push(b'[');
                    for (i, &c) in v.iter().enumerate() {
                        if i > 0 {
                            self.fmt.buf.push(b' ');
                        }
                        self.fmt
                            .fmt_integer(u64::from(c), 10, UNSIGNED, verb, LDIGITS);
                    }
                    self.fmt.buf.push(b']');
                }
            }
            's' => self.fmt.fmt_bs(v),
            'x' => self.fmt.fmt_bx(v, LDIGITS),
            'X' => self.fmt.fmt_bx(v, UDIGITS),
            'q' => self.fmt.fmt_q(v),
            _ => self.print_value(arg, verb, 0),
        }
    }

    fn handle_methods(&mut self, arg: &Arg, verb: char) -> bool {
        if self.erroring {
            return false;
        }
        if verb == 'w' {
            self.bad_verb(verb, arg);
            return true;
        }
        if self.fmt.flags.sharp_v {
            return false;
        }
        if matches!(verb, 'v' | 's' | 'x' | 'X' | 'q')
            && let Arg::Error(text) | Arg::Stringer(text, _) = arg
        {
            self.fmt_string(text, verb, arg);
            return true;
        }
        false
    }

    fn print_arg(&mut self, arg: &Arg, verb: char) {
        match arg {
            Arg::Nil => match verb {
                'T' | 'v' => self.fmt.pad_string(NIL_ANGLE_STRING),
                _ => self.bad_verb(verb, arg),
            },
            Arg::Bool(b) => self.fmt_bool(*b, verb, arg),
            Arg::Float32(f) => self.fmt_float(f64::from(*f), 32, verb, arg),
            Arg::Float(f) => self.fmt_float(*f, 64, verb, arg),
            Arg::Int(i) => self.fmt_integer(*i as u64, SIGNED, verb, arg),
            Arg::Uint(u) => self.fmt_integer(*u, UNSIGNED, verb, arg),
            Arg::Str(s) => self.fmt_string(s, verb, arg),
            Arg::Bytes(b) => self.fmt_bytes(b, verb, arg),
            _ => {
                if !self.handle_methods(arg, verb) {
                    self.print_value(arg, verb, 0);
                }
            }
        }
    }

    fn print_value(&mut self, arg: &Arg, verb: char, depth: usize) {
        if depth > 0 && self.handle_methods(arg, verb) {
            return;
        }
        self.print_kind(arg, verb, depth);
    }

    fn print_kind(&mut self, arg: &Arg, verb: char, depth: usize) {
        match arg {
            Arg::Nil => {
                if self.fmt.flags.sharp_v {
                    self.fmt.buf.extend_from_slice(NIL_PAREN_STRING);
                } else {
                    self.fmt.buf.extend_from_slice(NIL_ANGLE_STRING);
                }
            }
            Arg::Bool(b) => self.fmt_bool(*b, verb, arg),
            Arg::Int(i) => self.fmt_integer(*i as u64, SIGNED, verb, arg),
            Arg::Uint(u) => self.fmt_integer(*u, UNSIGNED, verb, arg),
            Arg::Float32(f) => self.fmt_float(f64::from(*f), 32, verb, arg),
            Arg::Float(f) => self.fmt_float(*f, 64, verb, arg),
            Arg::Str(s) => self.fmt_string(s, verb, arg),
            Arg::Map(entries) => {
                if self.fmt.flags.sharp_v {
                    self.fmt.buf.push(b'{');
                } else {
                    self.fmt.buf.extend_from_slice(MAP_STRING);
                }
                let mut sorted: Vec<&(Arg, Arg)> = entries.iter().collect();
                sorted.sort_by(|x, y| compare(&x.0, &y.0));
                for (i, (key, value)) in sorted.into_iter().enumerate() {
                    if i > 0 {
                        if self.fmt.flags.sharp_v {
                            self.fmt.buf.extend_from_slice(COMMA_SPACE_STRING);
                        } else {
                            self.fmt.buf.push(b' ');
                        }
                    }
                    self.print_value(key, verb, depth + 1);
                    self.fmt.buf.push(b':');
                    self.print_value(value, verb, depth + 1);
                }
                if self.fmt.flags.sharp_v {
                    self.fmt.buf.push(b'}');
                } else {
                    self.fmt.buf.push(b']');
                }
            }
            Arg::Struct(fields) => {
                self.fmt.buf.push(b'{');
                for (i, (name, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        if self.fmt.flags.sharp_v {
                            self.fmt.buf.extend_from_slice(COMMA_SPACE_STRING);
                        } else {
                            self.fmt.buf.push(b' ');
                        }
                    }
                    if (self.fmt.flags.plus_v || self.fmt.flags.sharp_v) && !name.is_empty() {
                        self.fmt.buf.extend_from_slice(name.as_bytes());
                        self.fmt.buf.push(b':');
                    }
                    self.print_value(value, verb, depth + 1);
                }
                self.fmt.buf.push(b'}');
            }
            Arg::Bytes(b) => {
                if matches!(verb, 's' | 'q' | 'x' | 'X') {
                    self.fmt_bytes(b, verb, arg);
                    return;
                }
                let items: Vec<Arg> = b.iter().map(|&c| Arg::Uint(u64::from(c))).collect();
                self.print_list(&items, verb, depth);
            }
            Arg::Slice(items) => self.print_list(items, verb, depth),
            Arg::Stringer(_, inner) => self.print_kind(inner, verb, depth),
            Arg::Error(text) => {
                self.fmt.buf.extend_from_slice(b"&{");
                self.print_value(&Arg::Str(text), verb, depth + 2);
                self.fmt.buf.push(b'}');
            }
        }
    }

    fn print_list(&mut self, items: &[Arg], verb: char, depth: usize) {
        if self.fmt.flags.sharp_v {
            self.fmt.buf.push(b'{');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    self.fmt.buf.extend_from_slice(COMMA_SPACE_STRING);
                }
                self.print_value(item, verb, depth + 1);
            }
            self.fmt.buf.push(b'}');
        } else {
            self.fmt.buf.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    self.fmt.buf.push(b' ');
                }
                self.print_value(item, verb, depth + 1);
            }
            self.fmt.buf.push(b']');
        }
    }

    fn arg_number(
        &mut self,
        arg_num: usize,
        format: &[u8],
        i: usize,
        num_args: usize,
    ) -> (usize, usize, bool) {
        if format.len() <= i || format[i] != b'[' {
            return (arg_num, i, false);
        }
        self.reordered = true;
        let (index, wid, ok) = parse_arg_number(&format[i..]);
        if ok && 0 <= index && (index as usize) < num_args {
            return (index as usize, i + wid, true);
        }
        self.good_arg_num = false;
        (arg_num, i + wid, ok)
    }

    fn bad_arg_num(&mut self, verb: char) {
        self.fmt.buf.extend_from_slice(PERCENT_BANG_STRING);
        self.write_rune(verb);
        self.fmt.buf.extend_from_slice(BAD_INDEX_STRING);
    }

    fn missing_arg(&mut self, verb: char) {
        self.fmt.buf.extend_from_slice(PERCENT_BANG_STRING);
        self.write_rune(verb);
        self.fmt.buf.extend_from_slice(MISSING_STRING);
    }

    fn do_printf(&mut self, format: &[u8], a: &[Arg]) {
        let end = format.len();
        let mut arg_num: usize = 0;
        let mut after_index: bool;
        self.reordered = false;
        let mut i = 0;
        'format_loop: while i < end {
            self.good_arg_num = true;
            let lasti = i;
            while i < end && format[i] != b'%' {
                i += 1;
            }
            if i > lasti {
                self.fmt.buf.extend_from_slice(&format[lasti..i]);
            }
            if i >= end {
                break;
            }

            i += 1;

            self.fmt.clearflags();
            while i < end {
                let c = format[i];
                match c {
                    b'#' => self.fmt.flags.sharp = true,
                    b'0' => self.fmt.flags.zero = true,
                    b'+' => self.fmt.flags.plus = true,
                    b'-' => self.fmt.flags.minus = true,
                    b' ' => self.fmt.flags.space = true,
                    _ => {
                        if c.is_ascii_lowercase() && arg_num < a.len() {
                            if c == b'w' || c == b'v' {
                                self.fmt.flags.sharp_v = self.fmt.flags.sharp;
                                self.fmt.flags.sharp = false;
                                self.fmt.flags.plus_v = self.fmt.flags.plus;
                                self.fmt.flags.plus = false;
                            }
                            self.print_arg(&a[arg_num], char::from(c));
                            arg_num += 1;
                            i += 1;
                            continue 'format_loop;
                        }
                        break;
                    }
                }
                i += 1;
            }

            (arg_num, i, after_index) = self.arg_number(arg_num, format, i, a.len());

            if i < end && format[i] == b'*' {
                i += 1;
                let (wid, present, next) = int_from_arg(a, arg_num);
                self.fmt.wid = wid;
                self.fmt.flags.wid_present = present;
                arg_num = next;

                if !self.fmt.flags.wid_present {
                    self.fmt.buf.extend_from_slice(BAD_WIDTH_STRING);
                }

                if self.fmt.wid < 0 {
                    self.fmt.wid = -self.fmt.wid;
                    self.fmt.flags.minus = true;
                    self.fmt.flags.zero = false;
                }
                after_index = false;
            } else {
                let (wid, present, next) = parsenum(format, i, end);
                self.fmt.wid = wid;
                self.fmt.flags.wid_present = present;
                i = next;
                if after_index && self.fmt.flags.wid_present {
                    self.good_arg_num = false;
                }
            }

            if i + 1 < end && format[i] == b'.' {
                i += 1;
                if after_index {
                    self.good_arg_num = false;
                }
                (arg_num, i, after_index) = self.arg_number(arg_num, format, i, a.len());
                if i < end && format[i] == b'*' {
                    i += 1;
                    let (prec, present, next) = int_from_arg(a, arg_num);
                    self.fmt.prec = prec;
                    self.fmt.flags.prec_present = present;
                    arg_num = next;
                    if self.fmt.prec < 0 {
                        self.fmt.prec = 0;
                        self.fmt.flags.prec_present = false;
                    }
                    if !self.fmt.flags.prec_present {
                        self.fmt.buf.extend_from_slice(BAD_PREC_STRING);
                    }
                    after_index = false;
                } else {
                    let (prec, present, next) = parsenum(format, i, end);
                    self.fmt.prec = prec;
                    self.fmt.flags.prec_present = present;
                    i = next;
                    if !self.fmt.flags.prec_present {
                        self.fmt.prec = 0;
                        self.fmt.flags.prec_present = true;
                    }
                }
            }

            if !after_index {
                let (next_arg, next_i, _) = self.arg_number(arg_num, format, i, a.len());
                arg_num = next_arg;
                i = next_i;
            }

            if i >= end {
                self.fmt.buf.extend_from_slice(NO_VERB_STRING);
                break;
            }

            let (verb, size) = if format[i] < RUNE_SELF {
                (char::from(format[i]), 1)
            } else {
                let (r, size) = decode_rune(&format[i..]);
                (
                    char::from_u32(r as u32).unwrap_or(char::REPLACEMENT_CHARACTER),
                    size,
                )
            };
            i += size;

            if verb == '%' {
                self.fmt.buf.push(b'%');
            } else if !self.good_arg_num {
                self.bad_arg_num(verb);
            } else if arg_num >= a.len() {
                self.missing_arg(verb);
            } else {
                if verb == 'w' || verb == 'v' {
                    self.fmt.flags.sharp_v = self.fmt.flags.sharp;
                    self.fmt.flags.sharp = false;
                    self.fmt.flags.plus_v = self.fmt.flags.plus;
                    self.fmt.flags.plus = false;
                }
                self.print_arg(&a[arg_num], verb);
                arg_num += 1;
            }
        }

        if !self.reordered && arg_num < a.len() {
            self.fmt.clearflags();
            self.fmt.buf.extend_from_slice(EXTRA_STRING);
            for (k, arg) in a[arg_num..].iter().enumerate() {
                if k > 0 {
                    self.fmt.buf.extend_from_slice(COMMA_SPACE_STRING);
                }
                if matches!(arg, Arg::Nil) {
                    self.fmt.buf.extend_from_slice(NIL_ANGLE_STRING);
                } else {
                    self.print_arg(arg, 'v');
                }
            }
            self.fmt.buf.push(b')');
        }
    }

    fn do_print(&mut self, a: &[Arg]) {
        let mut prev_string = false;
        for (arg_num, arg) in a.iter().enumerate() {
            let is_string = is_string_kind(arg);
            if arg_num > 0 && !is_string && !prev_string {
                self.fmt.buf.push(b' ');
            }
            self.print_arg(arg, 'v');
            prev_string = is_string;
        }
    }

    fn do_println(&mut self, a: &[Arg]) {
        for (arg_num, arg) in a.iter().enumerate() {
            if arg_num > 0 {
                self.fmt.buf.push(b' ');
            }
            self.print_arg(arg, 'v');
        }
        self.fmt.buf.push(b'\n');
    }
}
