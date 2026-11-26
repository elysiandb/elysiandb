use super::error::{Error, ErrorKind, Expected, Kind, Path, PathSegment};
use super::fold::fold_name;
use super::scanner::{check_valid, is_space};
use crate::base64::STD_ENCODING;
use crate::strconv::{self, ErrorKind as NumErrorKind};
use crate::{utf8, utf16};
use std::borrow::Cow;
use std::collections::BTreeMap;

fn getu4(s: &[u8]) -> i32 {
    if s.len() < 6 || s[0] != b'\\' || s[1] != b'u' {
        return -1;
    }
    let mut r: i32 = 0;
    for &c in &s[2..6] {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return -1,
        };
        r = r * 16 + i32::from(d);
    }
    r
}

fn unquote_slow(s: &[u8], mut r: usize) -> Option<Vec<u8>> {
    let mut b = Vec::with_capacity(s.len() + 2 * utf8::UTF_MAX);
    b.extend_from_slice(&s[..r]);
    while r < s.len() {
        let c = s[r];
        if c == b'\\' {
            r += 1;
            if r >= s.len() {
                return None;
            }
            match s[r] {
                b'"' | b'\\' | b'/' | b'\'' => {
                    b.push(s[r]);
                    r += 1;
                }
                b'b' => {
                    b.push(0x08);
                    r += 1;
                }
                b'f' => {
                    b.push(0x0c);
                    r += 1;
                }
                b'n' => {
                    b.push(b'\n');
                    r += 1;
                }
                b'r' => {
                    b.push(b'\r');
                    r += 1;
                }
                b't' => {
                    b.push(b'\t');
                    r += 1;
                }
                b'u' => {
                    r -= 1;
                    let mut rr = getu4(&s[r..]);
                    if rr < 0 {
                        return None;
                    }
                    r += 6;
                    if utf16::is_surrogate(rr) {
                        let rr1 = getu4(&s[r..]);
                        let dec = utf16::decode_rune(rr, rr1);
                        if dec != utf8::RUNE_ERROR {
                            r += 6;
                            utf8::append_rune(&mut b, dec);
                            continue;
                        }
                        rr = utf8::RUNE_ERROR;
                    }
                    utf8::append_rune(&mut b, rr);
                }
                _ => return None,
            }
        } else if c == b'"' || c < b' ' {
            return None;
        } else if c < utf8::RUNE_SELF {
            b.push(c);
            r += 1;
        } else {
            let (rr, size) = utf8::decode_rune(&s[r..]);
            r += size;
            utf8::append_rune(&mut b, rr);
        }
    }
    Some(b)
}

fn unquote_cow(s: &[u8]) -> Option<Cow<'_, [u8]>> {
    if s.len() < 2 || s[0] != b'"' || s[s.len() - 1] != b'"' {
        return None;
    }
    let s = &s[1..s.len() - 1];
    let mut r = 0;
    while r < s.len() {
        let c = s[r];
        if c == b'\\' || c == b'"' || c < b' ' {
            break;
        }
        if c < utf8::RUNE_SELF {
            r += 1;
            continue;
        }
        let (rr, size) = utf8::decode_rune(&s[r..]);
        if rr == utf8::RUNE_ERROR && size == 1 {
            break;
        }
        r += size;
    }
    if r == s.len() {
        return Some(Cow::Borrowed(s));
    }
    unquote_slow(s, r).map(Cow::Owned)
}

pub fn unquote_bytes(s: &[u8]) -> Option<Vec<u8>> {
    unquote_cow(s).map(Cow::into_owned)
}

fn text(bytes: Cow<'_, [u8]>) -> String {
    match bytes {
        Cow::Borrowed(b) => match std::str::from_utf8(b) {
            Ok(s) => s.to_owned(),
            Err(_) => String::from_utf8_lossy(b).into_owned(),
        },
        Cow::Owned(v) => match String::from_utf8(v) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        },
    }
}

pub(crate) fn unquote_text(item: &[u8]) -> String {
    text(unquote_cow(item).unwrap_or(Cow::Borrowed(item)))
}

fn string_end(data: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    data.len()
}

fn literal_end(data: &[u8], start: usize) -> usize {
    match data[start] {
        b'"' => string_end(data, start),
        b't' | b'n' => (start + 4).min(data.len()),
        b'f' => (start + 5).min(data.len()),
        _ => {
            let mut i = start + 1;
            while i < data.len()
                && matches!(data[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
            {
                i += 1;
            }
            i
        }
    }
}

fn has_fraction_or_exponent(lit: &[u8]) -> bool {
    lit.iter().any(|&c| c == b'.' || c == b'e' || c == b'E')
}

fn not_an_integer(lit: &[u8]) -> bool {
    has_fraction_or_exponent(lit) && strconv::parse_float(lit, 64).is_ok()
}

pub(crate) fn is_valid_number(s: &[u8]) -> bool {
    let mut s = s;
    if s.is_empty() {
        return false;
    }
    if s[0] == b'-' {
        s = &s[1..];
        if s.is_empty() {
            return false;
        }
    }
    if s[0] == b'0' {
        s = &s[1..];
    } else if s[0].is_ascii_digit() {
        s = &s[1..];
        while !s.is_empty() && s[0].is_ascii_digit() {
            s = &s[1..];
        }
    } else {
        return false;
    }
    if s.len() >= 2 && s[0] == b'.' && s[1].is_ascii_digit() {
        s = &s[2..];
        while !s.is_empty() && s[0].is_ascii_digit() {
            s = &s[1..];
        }
    }
    if s.len() >= 2 && (s[0] == b'e' || s[0] == b'E') {
        s = &s[1..];
        if s[0] == b'+' || s[0] == b'-' {
            s = &s[1..];
            if s.is_empty() {
                return false;
            }
        }
        while !s.is_empty() && s[0].is_ascii_digit() {
            s = &s[1..];
        }
    }
    s.is_empty()
}

pub(crate) fn path_at(data: &[u8], target: usize) -> Path {
    enum Frame {
        Object(Option<String>),
        Array(usize),
    }
    let mut stack: Vec<Frame> = Vec::new();
    let mut expect_key = false;
    let mut i = 0;
    while i < target && i < data.len() {
        match data[i] {
            b'{' => {
                stack.push(Frame::Object(None));
                expect_key = true;
                i += 1;
            }
            b'[' => {
                stack.push(Frame::Array(0));
                expect_key = false;
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                match stack.last_mut() {
                    Some(Frame::Array(n)) => *n += 1,
                    Some(Frame::Object(k)) => {
                        *k = None;
                        expect_key = true;
                    }
                    None => {}
                }
                i += 1;
            }
            b'"' => {
                let end = string_end(data, i);
                if expect_key {
                    if let Some(Frame::Object(k)) = stack.last_mut() {
                        *k = Some(unquote_text(&data[i..end]));
                    }
                    expect_key = false;
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    let mut segments = Vec::with_capacity(stack.len());
    for frame in stack {
        match frame {
            Frame::Object(Some(k)) => segments.push(PathSegment::Key(k)),
            Frame::Object(None) => {}
            Frame::Array(n) => segments.push(PathSegment::Index(n)),
        }
    }
    Path(segments)
}

pub struct DecodeState<'a> {
    data: &'a [u8],
    pos: usize,
    saved: Option<Error>,
    disallow_unknown_fields: bool,
    exact_integers: bool,
}

impl<'a> DecodeState<'a> {
    pub fn new(data: &'a [u8]) -> DecodeState<'a> {
        DecodeState {
            data,
            pos: 0,
            saved: None,
            disallow_unknown_fields: false,
            exact_integers: false,
        }
    }

    pub(crate) fn disallow_unknown_fields(&mut self, on: bool) {
        self.disallow_unknown_fields = on;
    }

    pub(crate) fn exact_integers(&mut self, on: bool) {
        self.exact_integers = on;
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    fn ws(&mut self) {
        while self.pos < self.data.len() && is_space(self.data[self.pos]) {
            self.pos += 1;
        }
    }

    pub fn peek(&mut self) -> Kind {
        self.ws();
        Kind::of(self.data.get(self.pos).copied().unwrap_or(b'n'))
    }

    pub fn start(&mut self) -> usize {
        self.ws();
        self.pos
    }

    pub fn literal(&mut self) -> &'a [u8] {
        self.ws();
        let data = self.data;
        let start = self.pos;
        let end = literal_end(data, start);
        self.pos = end;
        &data[start..end]
    }

    pub fn skip(&mut self) {
        self.ws();
        let data = self.data;
        let start = self.pos;
        match data.get(start) {
            Some(b'{') | Some(b'[') => {
                let mut depth = 0usize;
                let mut i = start;
                while i < data.len() {
                    match data[i] {
                        b'"' => {
                            i = string_end(data, i);
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                self.pos = i;
            }
            Some(_) => self.pos = literal_end(data, start),
            None => {}
        }
    }

    pub fn raw_value(&mut self) -> &'a [u8] {
        let data = self.data;
        let start = self.start();
        self.skip();
        &data[start..self.pos]
    }

    pub fn has_error(&self) -> bool {
        self.saved.is_some()
    }

    pub fn save_error(&mut self, err: Error) {
        if self.saved.is_none() {
            self.saved = Some(err);
        }
    }

    pub fn path_at(&self, start: usize) -> Path {
        path_at(self.data, start)
    }

    fn save_with(&mut self, start: usize, offset: usize, make: impl FnOnce(Path) -> ErrorKind) {
        if self.saved.is_none() {
            let path = path_at(self.data, start);
            self.saved = Some(Error::new(make(path), offset as i64));
        }
    }

    pub fn mismatch(&mut self, expected: Expected) -> Result<(), Error> {
        let start = self.start();
        let first = self.data.get(start).copied().unwrap_or(b'n');
        let got = Kind::of(first);
        let offset = if matches!(got, Kind::Object | Kind::Array) {
            start + 1
        } else {
            literal_end(self.data, start)
        };
        self.save_with(start, offset, |path| ErrorKind::Type {
            path,
            expected,
            got,
        });
        self.skip();
        Ok(())
    }

    fn save_range(&mut self, start: usize, offset: usize, literal: &[u8]) {
        self.save_with(start, offset, |path| ErrorKind::Range {
            path,
            literal: String::from_utf8_lossy(literal).into_owned(),
        });
    }

    fn save_quoted(&mut self, start: usize, offset: usize, expected: Expected) {
        self.save_with(start, offset, |path| ErrorKind::Quoted { path, expected });
    }

    pub fn object_members(
        &mut self,
        mut member: impl FnMut(&mut DecodeState<'a>, &[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.ws();
        self.pos += 1;
        loop {
            self.ws();
            if self.data.get(self.pos) != Some(&b'"') {
                self.pos += 1;
                return Ok(());
            }
            let data = self.data;
            let kstart = self.pos;
            let kend = string_end(data, kstart);
            self.pos = kend;
            let item = &data[kstart..kend];
            let key = unquote_cow(item).unwrap_or(Cow::Borrowed(item));
            self.ws();
            self.pos += 1;
            member(self, &key)?;
            self.ws();
            if self.data.get(self.pos) == Some(&b',') {
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            return Ok(());
        }
    }

    pub fn array_elements(
        &mut self,
        mut element: impl FnMut(&mut DecodeState<'a>, usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.ws();
        self.pos += 1;
        self.ws();
        if self.data.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(());
        }
        let mut i = 0;
        loop {
            element(self, i)?;
            i += 1;
            self.ws();
            if self.data.get(self.pos) == Some(&b',') {
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            return Ok(());
        }
    }

    pub fn decode_struct(
        &mut self,
        names: &[Option<&str>],
        mut field: impl FnMut(&mut DecodeState<'a>, usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        match self.peek() {
            Kind::Null => {
                self.skip();
                Ok(())
            }
            Kind::Object => {
                let disallow = self.disallow_unknown_fields;
                self.object_members(|d, key| match find_field(names, key) {
                    Some(i) => field(d, i),
                    None => {
                        if disallow && d.saved.is_none() {
                            d.saved = Some(Error::new(
                                ErrorKind::UnknownField {
                                    key: text(Cow::Borrowed(key)),
                                },
                                d.pos as i64,
                            ));
                        }
                        d.skip();
                        Ok(())
                    }
                })
            }
            _ => self.mismatch(Expected::Object),
        }
    }

    pub fn finish(self) -> Result<(), Error> {
        match self.saved {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub(crate) fn number_value(&mut self, start: usize, lit: &[u8]) -> Option<f64> {
        match strconv::parse_float(lit, 64) {
            Ok(f) => Some(f),
            Err(_) => {
                let end = start + lit.len();
                self.save_range(start, end + 1, lit);
                None
            }
        }
    }

    pub(crate) fn wants_exact_integers(&self) -> bool {
        self.exact_integers
    }

    pub(crate) fn advance_to(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }
}

fn find_field(names: &[Option<&str>], key: &[u8]) -> Option<usize> {
    if let Some(i) = names
        .iter()
        .position(|n| n.is_some_and(|n| n.as_bytes() == key))
    {
        return Some(i);
    }
    let folded = fold_name(key);
    names
        .iter()
        .position(|n| n.is_some_and(|n| fold_name(n.as_bytes()) == folded))
}

pub trait Decode {
    const EXPECTED: Expected;
    const SLICE_EXPECTED: Expected = Expected::array_of(Self::EXPECTED);
    const MAP_EXPECTED: Expected = Expected::object_of(Self::EXPECTED);

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error>;

    fn decode_slice(v: &mut Vec<Self>, d: &mut DecodeState<'_>) -> Result<(), Error>
    where
        Self: Sized + Default,
    {
        decode_slice_elements(v, d, Self::SLICE_EXPECTED)
    }

    fn decode_option(slot: &mut Option<Self>, d: &mut DecodeState<'_>) -> Result<(), Error>
    where
        Self: Sized + Default,
    {
        if d.peek() == Kind::Null {
            d.skip();
            *slot = None;
            return Ok(());
        }
        slot.get_or_insert_with(Self::default).decode(d)
    }
}

pub fn decode_slice_elements<T: Decode + Default>(
    v: &mut Vec<T>,
    d: &mut DecodeState<'_>,
    expected: Expected,
) -> Result<(), Error> {
    match d.peek() {
        Kind::Null => {
            d.skip();
            v.clear();
            Ok(())
        }
        Kind::Array => {
            let mut n = 0;
            d.array_elements(|d, i| {
                if i >= v.len() {
                    v.push(T::default());
                }
                n = i + 1;
                v[i].decode(d)
            })?;
            v.truncate(n);
            Ok(())
        }
        _ => d.mismatch(expected),
    }
}

impl Decode for bool {
    const EXPECTED: Expected = Expected::Boolean;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::Boolean => {
                *self = d.literal() == b"true";
                Ok(())
            }
            Kind::Null => {
                d.skip();
                Ok(())
            }
            _ => d.mismatch(Expected::Boolean),
        }
    }
}

fn integer_literal<'a>(d: &mut DecodeState<'a>) -> Result<Option<(usize, &'a [u8])>, Error> {
    match d.peek() {
        Kind::Number => {
            let start = d.start();
            Ok(Some((start, d.literal())))
        }
        Kind::Null => {
            d.skip();
            Ok(None)
        }
        _ => {
            d.mismatch(Expected::Integer)?;
            Ok(None)
        }
    }
}

fn integer_failure(d: &mut DecodeState<'_>, start: usize, lit: &[u8]) {
    let end = start + lit.len();
    if not_an_integer(lit) {
        d.save_with(start, end, |path| ErrorKind::Type {
            path,
            expected: Expected::Integer,
            got: Kind::Number,
        });
    } else {
        d.save_range(start, end, lit);
    }
}

macro_rules! signed_decode {
    ($($t:ty),*) => {$(
        impl Decode for $t {
            const EXPECTED: Expected = Expected::Integer;

            fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
                let Some((start, lit)) = integer_literal(d)? else {
                    return Ok(());
                };
                match strconv::parse_int(lit, 10, 64).ok().and_then(|n| <$t>::try_from(n).ok()) {
                    Some(n) => *self = n,
                    None => integer_failure(d, start, lit),
                }
                Ok(())
            }
        }
    )*};
}

macro_rules! unsigned_decode {
    ($($t:ty),*) => {$(
        impl Decode for $t {
            const EXPECTED: Expected = Expected::Integer;

            fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
                let Some((start, lit)) = integer_literal(d)? else {
                    return Ok(());
                };
                match strconv::parse_uint(lit, 10, 64).ok().and_then(|n| <$t>::try_from(n).ok()) {
                    Some(n) => *self = n,
                    None => integer_failure(d, start, lit),
                }
                Ok(())
            }
        }
    )*};
}

signed_decode!(i8, i16, i32, i64, isize);
unsigned_decode!(u16, u32, u64, usize);

impl Decode for u8 {
    const EXPECTED: Expected = Expected::Integer;
    const SLICE_EXPECTED: Expected = Expected::String;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        let Some((start, lit)) = integer_literal(d)? else {
            return Ok(());
        };
        match strconv::parse_uint(lit, 10, 64)
            .ok()
            .and_then(|n| u8::try_from(n).ok())
        {
            Some(n) => *self = n,
            None => integer_failure(d, start, lit),
        }
        Ok(())
    }

    fn decode_slice(v: &mut Vec<u8>, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::String => {
                let lit = d.literal();
                *v = unquote_cow(lit).unwrap_or(Cow::Borrowed(lit)).into_owned();
                Ok(())
            }
            Kind::Null => {
                d.skip();
                Ok(())
            }
            _ => d.mismatch(Expected::String),
        }
    }
}

fn float_literal(d: &mut DecodeState<'_>, bits: u32) -> Result<Option<f64>, Error> {
    match d.peek() {
        Kind::Number => {
            let start = d.start();
            let lit = d.literal();
            match strconv::parse_float(lit, bits) {
                Ok(f) => Ok(Some(f)),
                Err(_) => {
                    d.save_range(start, start + lit.len(), lit);
                    Ok(None)
                }
            }
        }
        Kind::Null => {
            d.skip();
            Ok(None)
        }
        _ => {
            d.mismatch(Expected::Number)?;
            Ok(None)
        }
    }
}

impl Decode for f64 {
    const EXPECTED: Expected = Expected::Number;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        if let Some(f) = float_literal(d, 64)? {
            *self = f;
        }
        Ok(())
    }
}

impl Decode for f32 {
    const EXPECTED: Expected = Expected::Number;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        if let Some(f) = float_literal(d, 32)? {
            *self = f as f32;
        }
        Ok(())
    }
}

impl Decode for String {
    const EXPECTED: Expected = Expected::String;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::String => {
                *self = unquote_text(d.literal());
                Ok(())
            }
            Kind::Null => {
                d.skip();
                Ok(())
            }
            _ => d.mismatch(Expected::String),
        }
    }
}

impl<T: Decode + Default> Decode for Vec<T> {
    const EXPECTED: Expected = T::SLICE_EXPECTED;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        T::decode_slice(self, d)
    }

    fn decode_option(slot: &mut Option<Vec<T>>, d: &mut DecodeState<'_>) -> Result<(), Error> {
        if T::SLICE_EXPECTED == Expected::String {
            if d.peek() == Kind::Null {
                d.skip();
                *slot = None;
                return Ok(());
            }
            return slot.get_or_insert_with(Vec::new).decode(d);
        }
        match d.peek() {
            Kind::Null => {
                d.skip();
                *slot = None;
                Ok(())
            }
            Kind::Array => slot.get_or_insert_with(Vec::new).decode(d),
            _ => d.mismatch(T::SLICE_EXPECTED),
        }
    }
}

impl<T: Decode + Default> Decode for Option<T> {
    const EXPECTED: Expected = T::EXPECTED;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        T::decode_option(self, d)
    }
}

impl<T: Decode + ?Sized> Decode for Box<T> {
    const EXPECTED: Expected = T::EXPECTED;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        (**self).decode(d)
    }
}

pub trait MapKey: Ord {
    fn key_bytes(&self) -> &[u8];
    fn from_key(key: &[u8]) -> Self;
}

impl MapKey for String {
    fn key_bytes(&self) -> &[u8] {
        self.as_bytes()
    }

    fn from_key(key: &[u8]) -> String {
        text(Cow::Borrowed(key))
    }
}

impl MapKey for Vec<u8> {
    fn key_bytes(&self) -> &[u8] {
        self
    }

    fn from_key(key: &[u8]) -> Vec<u8> {
        key.to_vec()
    }
}

impl<K: MapKey, T: Decode + Default> Decode for BTreeMap<K, T> {
    const EXPECTED: Expected = T::MAP_EXPECTED;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::Null => {
                d.skip();
                self.clear();
                Ok(())
            }
            Kind::Object => d.object_members(|d, key| {
                let mut elem = T::default();
                let result = elem.decode(d);
                self.insert(K::from_key(key), elem);
                result
            }),
            _ => d.mismatch(Self::EXPECTED),
        }
    }

    fn decode_option(slot: &mut Option<Self>, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::Null => {
                d.skip();
                *slot = None;
                Ok(())
            }
            Kind::Object => slot.get_or_insert_with(BTreeMap::new).decode(d),
            _ => d.mismatch(Self::EXPECTED),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RawMessage(pub Vec<u8>);

impl RawMessage {
    pub fn new(bytes: impl Into<Vec<u8>>) -> RawMessage {
        RawMessage(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_nil(&self) -> bool {
        self.0.is_empty()
    }
}

impl Decode for RawMessage {
    const EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        let raw = d.raw_value();
        self.0.clear();
        self.0.extend_from_slice(raw);
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Base64Bytes(pub Vec<u8>);

fn base64_literal(d: &mut DecodeState<'_>) -> Option<Vec<u8>> {
    let start = d.start();
    let lit = d.literal();
    let s = unquote_cow(lit).unwrap_or(Cow::Borrowed(lit));
    match STD_ENCODING.decode_string(&s) {
        Ok(b) => Some(b),
        Err(e) => {
            let byte = e.offset();
            d.save_with(start, 0, |path| ErrorKind::Base64 { path, byte });
            None
        }
    }
}

impl Decode for Base64Bytes {
    const EXPECTED: Expected = Expected::String;
    const MAP_EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::String => {
                if let Some(b) = base64_literal(d) {
                    self.0 = b;
                }
                Ok(())
            }
            Kind::Array => decode_slice_elements(&mut self.0, d, Expected::String),
            Kind::Null => {
                d.skip();
                self.0.clear();
                Ok(())
            }
            _ => d.mismatch(Expected::String),
        }
    }

    fn decode_option(slot: &mut Option<Self>, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::Null => {
                d.skip();
                *slot = None;
                Ok(())
            }
            Kind::String => {
                if let Some(b) = base64_literal(d) {
                    *slot = Some(Base64Bytes(b));
                }
                Ok(())
            }
            Kind::Array => slot.get_or_insert_with(Base64Bytes::default).decode(d),
            _ => d.mismatch(Expected::String),
        }
    }
}

pub trait DecodeQuoted: Decode {
    fn decode_quoted_null(&mut self) {}

    fn decode_quoted(
        &mut self,
        d: &mut DecodeState<'_>,
        start: usize,
        end: usize,
        item: &[u8],
    ) -> Result<(), Error>;
}

fn quoted_prelude(
    d: &mut DecodeState<'_>,
    start: usize,
    end: usize,
    item: &[u8],
    expected: Expected,
) -> Option<u8> {
    match item.first() {
        None => {
            d.save_quoted(start, end, expected);
            None
        }
        Some(b'n') => {
            if item != b"null" {
                d.save_quoted(start, end, expected);
            }
            None
        }
        Some(&c) => Some(c),
    }
}

fn quoted_abort(d: &DecodeState<'_>, start: usize, end: usize, expected: Expected) -> Error {
    Error::new(
        ErrorKind::Quoted {
            path: path_at(d.data, start),
            expected,
        },
        end as i64,
    )
}

fn quoted_non_number(
    d: &mut DecodeState<'_>,
    start: usize,
    end: usize,
    item: &[u8],
    c: u8,
    expected: Expected,
) -> Result<(), Error> {
    match c {
        b't' | b'f' => {
            d.save_quoted(start, end, expected);
            Ok(())
        }
        b'"' => {
            if unquote_cow(item).is_none() {
                return Err(quoted_abort(d, start, end, expected));
            }
            d.save_with(start, end, |path| ErrorKind::Type {
                path,
                expected,
                got: Kind::String,
            });
            Ok(())
        }
        _ => Err(quoted_abort(d, start, end, expected)),
    }
}

fn quoted_number_failure(
    d: &mut DecodeState<'_>,
    start: usize,
    end: usize,
    item: &[u8],
    expected: Expected,
    range: bool,
) {
    if !is_valid_number(item) {
        d.save_quoted(start, end, expected);
    } else if range || !not_an_integer(item) {
        d.save_range(start, end, item);
    } else {
        d.save_with(start, end, |path| ErrorKind::Type {
            path,
            expected,
            got: Kind::Number,
        });
    }
}

impl DecodeQuoted for bool {
    fn decode_quoted(
        &mut self,
        d: &mut DecodeState<'_>,
        start: usize,
        end: usize,
        item: &[u8],
    ) -> Result<(), Error> {
        let Some(c) = quoted_prelude(d, start, end, item, Expected::Boolean) else {
            return Ok(());
        };
        match c {
            b't' | b'f' => {
                if item == b"true" || item == b"false" {
                    *self = item == b"true";
                } else {
                    d.save_quoted(start, end, Expected::Boolean);
                }
                Ok(())
            }
            b'"' => quoted_non_number(d, start, end, item, c, Expected::Boolean),
            _ => Err(quoted_abort(d, start, end, Expected::Boolean)),
        }
    }
}

impl DecodeQuoted for String {
    fn decode_quoted(
        &mut self,
        d: &mut DecodeState<'_>,
        start: usize,
        end: usize,
        item: &[u8],
    ) -> Result<(), Error> {
        let Some(c) = quoted_prelude(d, start, end, item, Expected::String) else {
            return Ok(());
        };
        match c {
            b'"' => match unquote_cow(item) {
                Some(s) => {
                    *self = text(s);
                    Ok(())
                }
                None => Err(quoted_abort(d, start, end, Expected::String)),
            },
            b't' | b'f' => {
                d.save_quoted(start, end, Expected::String);
                Ok(())
            }
            _ => Err(quoted_abort(d, start, end, Expected::String)),
        }
    }
}

macro_rules! quoted_number {
    ($($t:ty => $parse:expr),* $(,)?) => {$(
        impl DecodeQuoted for $t {
            fn decode_quoted(
                &mut self,
                d: &mut DecodeState<'_>,
                start: usize,
                end: usize,
                item: &[u8],
            ) -> Result<(), Error> {
                let expected = <$t as Decode>::EXPECTED;
                let Some(c) = quoted_prelude(d, start, end, item, expected) else {
                    return Ok(());
                };
                if c != b'-' && !c.is_ascii_digit() {
                    return quoted_non_number(d, start, end, item, c, expected);
                }
                let parse: fn(&[u8]) -> Result<$t, bool> = $parse;
                match parse(item) {
                    Ok(n) => *self = n,
                    Err(range) => quoted_number_failure(d, start, end, item, expected, range),
                }
                Ok(())
            }
        }
    )*};
}

fn parse_signed<T: TryFrom<i64>>(item: &[u8]) -> Result<T, bool> {
    match strconv::parse_int(item, 10, 64) {
        Ok(n) => T::try_from(n).map_err(|_| true),
        Err(e) => Err(e.kind == NumErrorKind::Range),
    }
}

fn parse_unsigned<T: TryFrom<u64>>(item: &[u8]) -> Result<T, bool> {
    match strconv::parse_uint(item, 10, 64) {
        Ok(n) => T::try_from(n).map_err(|_| true),
        Err(e) => Err(e.kind == NumErrorKind::Range),
    }
}

quoted_number!(
    i8 => parse_signed::<i8>,
    i16 => parse_signed::<i16>,
    i32 => parse_signed::<i32>,
    i64 => parse_signed::<i64>,
    isize => parse_signed::<isize>,
    u8 => parse_unsigned::<u8>,
    u16 => parse_unsigned::<u16>,
    u32 => parse_unsigned::<u32>,
    u64 => parse_unsigned::<u64>,
    usize => parse_unsigned::<usize>,
    f64 => |s| strconv::parse_float(s, 64).map_err(|e| e.kind == NumErrorKind::Range),
    f32 => |s| strconv::parse_float(s, 32)
        .map(|f| f as f32)
        .map_err(|e| e.kind == NumErrorKind::Range),
);

impl<T: DecodeQuoted + Default> DecodeQuoted for Option<T> {
    fn decode_quoted_null(&mut self) {
        *self = None;
    }

    fn decode_quoted(
        &mut self,
        d: &mut DecodeState<'_>,
        start: usize,
        end: usize,
        item: &[u8],
    ) -> Result<(), Error> {
        if item == b"null" {
            *self = None;
            return Ok(());
        }
        if item.first() == Some(&b'n') {
            quoted_prelude(d, start, end, item, T::EXPECTED);
            return Ok(());
        }
        self.get_or_insert_with(T::default)
            .decode_quoted(d, start, end, item)
    }
}

pub fn decode_quoted<T: DecodeQuoted>(v: &mut T, d: &mut DecodeState<'_>) -> Result<(), Error> {
    let start = d.start();
    match d.peek() {
        Kind::Null => {
            d.skip();
            v.decode_quoted_null();
            Ok(())
        }
        Kind::String => {
            let lit = d.literal();
            let end = start + lit.len();
            let item = unquote_cow(lit).unwrap_or(Cow::Borrowed(lit));
            v.decode_quoted(d, start, end, &item)
        }
        Kind::Number => {
            let lit = d.literal();
            let end = start + lit.len();
            if strconv::parse_float(lit, 64).is_err() {
                d.save_range(start, end + 1, lit);
            }
            d.save_quoted(start, end, T::EXPECTED);
            Ok(())
        }
        Kind::Boolean => {
            let lit = d.literal();
            let end = start + lit.len();
            d.save_quoted(start, end, T::EXPECTED);
            Ok(())
        }
        Kind::Object | Kind::Array => {
            d.skip();
            let end = d.position();
            d.save_quoted(start, end, T::EXPECTED);
            Ok(())
        }
    }
}

pub fn unmarshal<T: Decode + ?Sized>(data: &[u8], v: &mut T) -> Result<(), Error> {
    check_valid(data)?;
    let mut d = DecodeState::new(data);
    v.decode(&mut d)?;
    d.finish()
}
