use super::decode::{Base64Bytes, MapKey, RawMessage};
use super::error::{Error, ErrorKind};
use super::quote::{HEX, append_html_escape, append_string};
use super::scanner::{Scan, Scanner, SyntaxError};
use crate::base64::STD_ENCODING;
use crate::strconv;
use std::collections::BTreeMap;

pub struct EncodeState {
    buf: Vec<u8>,
    escape_html: bool,
}

impl EncodeState {
    pub fn new(escape_html: bool) -> EncodeState {
        EncodeState {
            buf: Vec::new(),
            escape_html,
        }
    }

    pub fn escape_html(&self) -> bool {
        self.escape_html
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_byte(&mut self, b: u8) {
        self.buf.push(b);
    }

    pub fn write_raw(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    pub fn write_null(&mut self) {
        self.buf.extend_from_slice(b"null");
    }

    pub fn write_bool(&mut self, b: bool) {
        self.buf
            .extend_from_slice(if b { b"true" as &[u8] } else { b"false" });
    }

    pub fn write_i64(&mut self, n: i64) {
        strconv::append_int(&mut self.buf, n, 10);
    }

    pub fn write_u64(&mut self, n: u64) {
        strconv::append_uint(&mut self.buf, n, 10);
    }

    pub fn write_f64(&mut self, f: f64) -> Result<(), Error> {
        append_float(&mut self.buf, f, 64)
    }

    pub fn write_f32(&mut self, f: f32) -> Result<(), Error> {
        append_float(&mut self.buf, f64::from(f), 32)
    }

    pub fn write_string(&mut self, s: &[u8]) {
        append_string(&mut self.buf, s, self.escape_html);
    }

    pub fn write_marshaled(&mut self, json: &[u8]) -> Result<(), Error> {
        append_compact(&mut self.buf, json, self.escape_html)
            .map_err(|e| Error::new(ErrorKind::InvalidRaw(e), 0))
    }

    pub fn write_field_name(&mut self, first: &mut bool, name: &str) {
        self.buf.push(if *first { b'{' } else { b',' });
        *first = false;
        self.buf.push(b'"');
        if self.escape_html {
            append_html_escape(&mut self.buf, name.as_bytes());
        } else {
            self.buf.extend_from_slice(name.as_bytes());
        }
        self.buf.extend_from_slice(b"\":");
    }

    pub fn end_struct(&mut self, first: bool) {
        if first {
            self.buf.extend_from_slice(b"{}");
        } else {
            self.buf.push(b'}');
        }
    }
}

fn append_float(dst: &mut Vec<u8>, f: f64, bits: u32) -> Result<(), Error> {
    if f.is_nan() || f.is_infinite() {
        return Err(Error::new(ErrorKind::UnsupportedNumber(f), 0));
    }
    let abs = f.abs();
    let mut fmt = b'f';
    if abs != 0.0
        && (bits == 64 && !(1e-6..1e21).contains(&abs)
            || bits == 32 && !(1e-6f32..1e21f32).contains(&(abs as f32)))
    {
        fmt = b'e';
    }
    strconv::append_float(dst, f, fmt, -1, bits);
    if fmt == b'e' {
        let n = dst.len();
        if n >= 4 && dst[n - 4] == b'e' && dst[n - 3] == b'-' && dst[n - 2] == b'0' {
            dst[n - 2] = dst[n - 1];
            dst.truncate(n - 1);
        }
    }
    Ok(())
}

pub(crate) fn append_compact(
    dst: &mut Vec<u8>,
    src: &[u8],
    escape: bool,
) -> Result<(), SyntaxError> {
    let orig = dst.len();
    let mut scan = Scanner::new();
    let mut start = 0;
    for (i, &c) in src.iter().enumerate() {
        if escape && (c == b'<' || c == b'>' || c == b'&') {
            if start < i {
                dst.extend_from_slice(&src[start..i]);
            }
            dst.extend_from_slice(b"\\u00");
            dst.push(HEX[usize::from(c >> 4)]);
            dst.push(HEX[usize::from(c & 0xF)]);
            start = i + 1;
        }
        if escape && c == 0xE2 && i + 2 < src.len() && src[i + 1] == 0x80 && src[i + 2] & !1 == 0xA8
        {
            if start < i {
                dst.extend_from_slice(&src[start..i]);
            }
            dst.extend_from_slice(b"\\u202");
            dst.push(HEX[usize::from(src[i + 2] & 0xF)]);
            start = i + 3;
        }
        let v = scan.step(c);
        if matches!(v, Scan::SkipSpace | Scan::End | Scan::Error) {
            if v == Scan::Error {
                break;
            }
            if start < i {
                dst.extend_from_slice(&src[start..i]);
            }
            start = i + 1;
        }
    }
    if scan.eof() == Scan::Error {
        dst.truncate(orig);
        return Err(scan
            .take_err()
            .unwrap_or_else(|| SyntaxError::unexpected_end(0)));
    }
    if start < src.len() {
        dst.extend_from_slice(&src[start..]);
    }
    Ok(())
}

fn append_newline(dst: &mut Vec<u8>, prefix: &[u8], indent: &[u8], depth: usize) {
    dst.push(b'\n');
    dst.extend_from_slice(prefix);
    for _ in 0..depth {
        dst.extend_from_slice(indent);
    }
}

pub(crate) fn append_indent(
    dst: &mut Vec<u8>,
    src: &[u8],
    prefix: &[u8],
    indent: &[u8],
) -> Result<(), SyntaxError> {
    let orig = dst.len();
    let mut scan = Scanner::new();
    let mut need_indent = false;
    let mut depth = 0usize;
    for &c in src {
        scan.bytes += 1;
        let v = scan.step(c);
        if v == Scan::SkipSpace {
            continue;
        }
        if v == Scan::Error {
            break;
        }
        if need_indent && v != Scan::EndObject && v != Scan::EndArray {
            need_indent = false;
            depth += 1;
            append_newline(dst, prefix, indent, depth);
        }
        if v == Scan::Continue {
            dst.push(c);
            continue;
        }
        match c {
            b'{' | b'[' => {
                need_indent = true;
                dst.push(c);
            }
            b',' => {
                dst.push(c);
                append_newline(dst, prefix, indent, depth);
            }
            b':' => {
                dst.push(c);
                dst.push(b' ');
            }
            b'}' | b']' => {
                if need_indent {
                    need_indent = false;
                } else {
                    depth = depth.saturating_sub(1);
                    append_newline(dst, prefix, indent, depth);
                }
                dst.push(c);
            }
            _ => dst.push(c),
        }
    }
    if scan.eof() == Scan::Error {
        dst.truncate(orig);
        return Err(scan
            .take_err()
            .map(|e| e.with_input(src))
            .unwrap_or_else(|| SyntaxError::unexpected_end(0)));
    }
    Ok(())
}

pub fn compact(dst: &mut Vec<u8>, src: &[u8]) -> Result<(), Error> {
    append_compact(dst, src, false).map_err(Error::from)
}

pub fn indent(dst: &mut Vec<u8>, src: &[u8], prefix: &str, indent: &str) -> Result<(), Error> {
    append_indent(dst, src, prefix.as_bytes(), indent.as_bytes()).map_err(Error::from)
}

pub trait Encode {
    const NILABLE: bool = false;
    const SLICE_NILABLE: bool = true;

    fn encode(&self, e: &mut EncodeState) -> Result<(), Error>;

    fn is_empty_value(&self) -> bool {
        false
    }

    fn encode_slice(items: &[Self], e: &mut EncodeState) -> Result<(), Error>
    where
        Self: Sized,
    {
        e.write_byte(b'[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            item.encode(e)?;
        }
        e.write_byte(b']');
        Ok(())
    }
}

pub trait EncodeQuoted: Encode {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error>;
}

impl Encode for bool {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_bool(*self);
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        !*self
    }
}

impl EncodeQuoted for bool {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_byte(b'"');
        e.write_bool(*self);
        e.write_byte(b'"');
        Ok(())
    }
}

macro_rules! signed_encode {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
                e.write_i64(*self as i64);
                Ok(())
            }

            fn is_empty_value(&self) -> bool {
                *self == 0
            }
        }

        impl EncodeQuoted for $t {
            fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
                e.write_byte(b'"');
                e.write_i64(*self as i64);
                e.write_byte(b'"');
                Ok(())
            }
        }
    )*};
}

macro_rules! unsigned_encode {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
                e.write_u64(*self as u64);
                Ok(())
            }

            fn is_empty_value(&self) -> bool {
                *self == 0
            }
        }

        impl EncodeQuoted for $t {
            fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
                e.write_byte(b'"');
                e.write_u64(*self as u64);
                e.write_byte(b'"');
                Ok(())
            }
        }
    )*};
}

signed_encode!(i8, i16, i32, i64, isize);
unsigned_encode!(u16, u32, u64, usize);

impl Encode for u8 {
    const SLICE_NILABLE: bool = false;

    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_u64(u64::from(*self));
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        *self == 0
    }

    fn encode_slice(items: &[u8], e: &mut EncodeState) -> Result<(), Error> {
        e.write_string(items);
        Ok(())
    }
}

impl EncodeQuoted for u8 {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_byte(b'"');
        e.write_u64(u64::from(*self));
        e.write_byte(b'"');
        Ok(())
    }
}

impl Encode for f64 {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_f64(*self)
    }

    fn is_empty_value(&self) -> bool {
        *self == 0.0
    }
}

impl EncodeQuoted for f64 {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        let mut inner = Vec::new();
        append_float(&mut inner, *self, 64)?;
        e.write_byte(b'"');
        e.write_raw(&inner);
        e.write_byte(b'"');
        Ok(())
    }
}

impl Encode for f32 {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_f32(*self)
    }

    fn is_empty_value(&self) -> bool {
        *self == 0.0
    }
}

impl EncodeQuoted for f32 {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        let mut inner = Vec::new();
        append_float(&mut inner, f64::from(*self), 32)?;
        e.write_byte(b'"');
        e.write_raw(&inner);
        e.write_byte(b'"');
        Ok(())
    }
}

impl Encode for str {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_string(self.as_bytes());
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }
}

impl EncodeQuoted for str {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        let mut inner = Vec::new();
        append_string(&mut inner, self.as_bytes(), e.escape_html());
        let mut outer = Vec::new();
        append_string(&mut outer, &inner, false);
        e.write_raw(&outer);
        Ok(())
    }
}

impl Encode for String {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        self.as_str().encode(e)
    }

    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }
}

impl EncodeQuoted for String {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        self.as_str().encode_quoted(e)
    }
}

impl<T: Encode> Encode for [T] {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        T::encode_slice(self, e)
    }

    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }
}

impl<T: Encode> Encode for Vec<T> {
    const NILABLE: bool = T::SLICE_NILABLE;

    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        T::encode_slice(self, e)
    }

    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        match self {
            Some(v) => v.encode(e),
            None => {
                e.write_null();
                Ok(())
            }
        }
    }

    fn is_empty_value(&self) -> bool {
        match self {
            None => true,
            Some(v) => T::NILABLE && v.is_empty_value(),
        }
    }
}

impl<T: EncodeQuoted> EncodeQuoted for Option<T> {
    fn encode_quoted(&self, e: &mut EncodeState) -> Result<(), Error> {
        match self {
            Some(v) => v.encode_quoted(e),
            None => {
                e.write_null();
                Ok(())
            }
        }
    }
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        (**self).encode(e)
    }

    fn is_empty_value(&self) -> bool {
        (**self).is_empty_value()
    }
}

impl<T: Encode + ?Sized> Encode for Box<T> {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        (**self).encode(e)
    }

    fn is_empty_value(&self) -> bool {
        (**self).is_empty_value()
    }
}

impl<K: MapKey, V: Encode> Encode for BTreeMap<K, V> {
    const NILABLE: bool = true;

    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_byte(b'{');
        for (i, (k, v)) in self.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            e.write_string(k.key_bytes());
            e.write_byte(b':');
            v.encode(e)?;
        }
        e.write_byte(b'}');
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }
}

impl Encode for RawMessage {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        if self.0.is_empty() {
            e.write_null();
            return Ok(());
        }
        e.write_marshaled(&self.0)
    }

    fn is_empty_value(&self) -> bool {
        self.0.is_empty()
    }
}

impl Encode for Base64Bytes {
    const NILABLE: bool = true;

    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        e.write_byte(b'"');
        STD_ENCODING.append_encode(&mut e.buf, &self.0);
        e.write_byte(b'"');
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        self.0.is_empty()
    }
}

pub fn marshal<T: Encode + ?Sized>(v: &T) -> Result<Vec<u8>, Error> {
    let mut e = EncodeState::new(true);
    v.encode(&mut e)?;
    Ok(e.into_bytes())
}

pub fn marshal_indent<T: Encode + ?Sized>(
    v: &T,
    prefix: &str,
    indent: &str,
) -> Result<Vec<u8>, Error> {
    let b = marshal(v)?;
    let mut out = Vec::with_capacity(2 * b.len());
    append_indent(&mut out, &b, prefix.as_bytes(), indent.as_bytes())?;
    Ok(out)
}
