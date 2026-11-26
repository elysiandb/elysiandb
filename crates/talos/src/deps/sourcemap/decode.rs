use super::Error;
use gocompat::json::{check_valid, unquote_bytes};
use gocompat::strconv::{ErrorKind, parse_int};
use gocompat::strings::equal_fold;

#[derive(Clone, Debug, Default)]
pub struct GoSlice<T> {
    backing: Vec<T>,
    len: usize,
}

const CLASS_TO_SIZE: [usize; 68] = [
    0, 8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320,
    352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 896, 1024, 1152, 1280, 1408, 1536, 1792,
    2048, 2304, 2688, 3072, 3200, 3456, 4096, 4864, 5376, 6144, 6528, 6784, 6912, 8192, 9472, 9728,
    10240, 10880, 12288, 13568, 14336, 16384, 18432, 19072, 20480, 21760, 24576, 27264, 28672,
    32768,
];

const MAX_SMALL_SIZE: usize = 32768;
const MALLOC_HEADER_SIZE: usize = 8;
const MIN_SIZE_FOR_MALLOC_HEADER: usize = 512;
const PAGE_SIZE: usize = 8192;

fn roundupsize(size: usize, noscan: bool) -> usize {
    let mut req_size = size;
    if req_size <= MAX_SMALL_SIZE - MALLOC_HEADER_SIZE {
        if !noscan && req_size > MIN_SIZE_FOR_MALLOC_HEADER {
            req_size += MALLOC_HEADER_SIZE;
        }
        let class = CLASS_TO_SIZE
            .iter()
            .copied()
            .find(|&c| c >= req_size)
            .unwrap_or(MAX_SMALL_SIZE);
        return class - (req_size - size);
    }
    req_size += PAGE_SIZE - 1;
    if req_size < size {
        return size;
    }
    req_size & !(PAGE_SIZE - 1)
}

fn nextslicecap(new_len: usize, old_cap: usize) -> usize {
    let mut newcap = old_cap;
    let doublecap = newcap + newcap;
    if new_len > doublecap {
        return new_len;
    }
    const THRESHOLD: usize = 256;
    if old_cap < THRESHOLD {
        return doublecap;
    }
    loop {
        newcap += (newcap + 3 * THRESHOLD) >> 2;
        if newcap >= new_len {
            break;
        }
    }
    newcap
}

fn grown_cap(old_cap: usize, elem_size: usize) -> usize {
    let newcap = nextslicecap(old_cap + 1, old_cap);
    let capmem = roundupsize(newcap * elem_size, false);
    capmem / elem_size
}

impl<T: Default> GoSlice<T> {
    pub fn as_slice(&self) -> &[T] {
        &self.backing[..self.len]
    }

    pub fn set_nil(&mut self) {
        self.backing = Vec::new();
        self.len = 0;
    }

    pub fn into_vec(mut self) -> Vec<T> {
        self.backing.truncate(self.len);
        self.backing
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    fn described(self) -> &'static str {
        match self {
            Kind::Null => "null",
            Kind::Bool => "a boolean",
            Kind::Number => "a number",
            Kind::String => "a string",
            Kind::Array => "an array",
            Kind::Object => "an object",
        }
    }
}

pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Result<Cursor<'a>, Error> {
        check_valid(data).map_err(Error::Json)?;
        let mut c = Cursor { data, pos: 0 };
        c.skip_ws();
        Ok(c)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.data.len()
            && matches!(self.data[self.pos], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.pos += 1;
        }
    }

    pub fn kind(&self) -> Kind {
        match self.data[self.pos] {
            b'n' => Kind::Null,
            b't' | b'f' => Kind::Bool,
            b'"' => Kind::String,
            b'[' => Kind::Array,
            b'{' => Kind::Object,
            _ => Kind::Number,
        }
    }

    fn skip_string(&mut self) {
        self.pos += 1;
        while self.data[self.pos] != b'"' {
            if self.data[self.pos] == b'\\' {
                self.pos += 1;
            }
            self.pos += 1;
        }
        self.pos += 1;
    }

    pub fn skip_value(&mut self) -> &'a [u8] {
        let start = self.pos;
        match self.data[self.pos] {
            b'"' => self.skip_string(),
            b'{' | b'[' => {
                let mut depth = 0usize;
                loop {
                    match self.data[self.pos] {
                        b'"' => {
                            self.skip_string();
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.pos += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
            }
            _ => {
                while self.pos < self.data.len()
                    && matches!(self.data[self.pos], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' | b'a'..=b'z')
                {
                    self.pos += 1;
                }
            }
        }
        let span = &self.data[start..self.pos];
        self.skip_ws();
        span
    }

    fn read_string(&mut self) -> Vec<u8> {
        let raw = self.skip_value();
        unquote_bytes(raw).unwrap_or_default()
    }

    pub fn object<F>(&mut self, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&mut Cursor<'a>, &[u8]) -> Result<(), Error>,
    {
        self.pos += 1;
        self.skip_ws();
        loop {
            if self.data[self.pos] == b'}' {
                self.pos += 1;
                self.skip_ws();
                return Ok(());
            }
            let key = self.read_string();
            self.pos += 1;
            self.skip_ws();
            f(self, &key)?;
            if self.data[self.pos] == b',' {
                self.pos += 1;
                self.skip_ws();
            }
        }
    }

    pub fn array<F>(&mut self, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&mut Cursor<'a>, usize) -> Result<(), Error>,
    {
        self.pos += 1;
        self.skip_ws();
        let mut i = 0;
        loop {
            if self.data[self.pos] == b']' {
                self.pos += 1;
                self.skip_ws();
                return Ok(());
            }
            f(self, i)?;
            i += 1;
            if self.data[self.pos] == b',' {
                self.pos += 1;
                self.skip_ws();
            }
        }
    }
}

pub fn type_error(path: &str, expected: &'static str, got: Kind) -> Error {
    Error::Type {
        path: path.to_string(),
        expected,
        got: got.described(),
    }
}

pub fn field_path(path: &str, key: &[u8]) -> String {
    let key = String::from_utf8_lossy(key);
    if path.is_empty() {
        key.into_owned()
    } else {
        format!("{path}.{key}")
    }
}

pub fn index_path(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

pub fn match_field<'n>(key: &[u8], names: &[&'n str]) -> Option<&'n str> {
    if let Some(n) = names.iter().find(|n| n.as_bytes() == key) {
        return Some(n);
    }
    names
        .iter()
        .find(|n| equal_fold(n.as_bytes(), key))
        .copied()
}

pub fn decode_int(c: &mut Cursor<'_>, path: &str, target: &mut i64) -> Result<(), Error> {
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            Ok(())
        }
        Kind::Number => {
            let lit = c.skip_value();
            match parse_int(lit, 10, 64) {
                Ok(n) => {
                    *target = n;
                    Ok(())
                }
                Err(e) if e.kind == ErrorKind::Range => Err(Error::Range {
                    path: path.to_string(),
                    literal: String::from_utf8_lossy(lit).into_owned(),
                }),
                Err(_) => Err(type_error(path, "an integer", kind)),
            }
        }
        _ => Err(type_error(path, "an integer", kind)),
    }
}

pub fn decode_string(c: &mut Cursor<'_>, path: &str, target: &mut Vec<u8>) -> Result<(), Error> {
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            Ok(())
        }
        Kind::String => {
            *target = c.read_string();
            Ok(())
        }
        _ => Err(type_error(path, "a string", kind)),
    }
}

pub fn decode_slice<'a, T, F>(
    c: &mut Cursor<'a>,
    path: &str,
    expected: &'static str,
    elem_size: usize,
    target: &mut GoSlice<T>,
    mut elem: F,
) -> Result<(), Error>
where
    T: Default,
    F: FnMut(&mut Cursor<'a>, &str, &mut T) -> Result<(), Error>,
{
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            target.set_nil();
            Ok(())
        }
        Kind::Array => {
            let mut n = 0;
            c.array(|c, i| {
                if i >= target.backing.len() {
                    let cap = grown_cap(target.backing.len(), elem_size);
                    target.backing.resize_with(cap, T::default);
                }
                if i >= target.len {
                    target.len = i + 1;
                }
                n = i + 1;
                elem(c, &index_path(path, i), &mut target.backing[i])
            })?;
            if n < target.len {
                target.len = n;
            }
            if n == 0 {
                target.set_nil();
            }
            Ok(())
        }
        _ => Err(type_error(path, expected, kind)),
    }
}
