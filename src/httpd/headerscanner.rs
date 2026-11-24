use super::bytesconv::{index_byte, index_of, valid_header_field_byte};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanError {
    NeedMore,
    Malformed,
}

pub(crate) struct HeaderScanner<'a> {
    initialized: bool,
    b: &'a [u8],
    pub(crate) r: usize,
    pub(crate) err: Option<ScanError>,
}

impl<'a> HeaderScanner<'a> {
    pub(crate) fn new(b: &'a [u8]) -> HeaderScanner<'a> {
        HeaderScanner {
            initialized: false,
            b,
            r: 0,
            err: None,
        }
    }

    pub(crate) fn next(&mut self) -> Option<(Vec<u8>, Vec<u8>)> {
        if !self.initialized {
            if self.b.starts_with(b"\r\n") {
                self.r = 2;
                return None;
            }
            let Some(i) = index_of(self.b, b"\r\n\r\n") else {
                self.err = Some(ScanError::NeedMore);
                return None;
            };
            self.b = &self.b[..i + 4];
            if matches!(self.b.first(), Some(b' ' | b'\t')) {
                self.err = Some(ScanError::Malformed);
                return None;
            }
            self.initialized = true;
        }

        let kv = match self.read_continued_line_slice() {
            Ok(kv) => kv,
            Err(e) => {
                self.err = Some(e);
                return None;
            }
        };
        if kv.is_empty() {
            return None;
        }
        let Some(colon) = index_byte(&kv, b':') else {
            self.err = Some(ScanError::Malformed);
            return None;
        };
        let k = &kv[..colon];
        if !is_valid_header_key(k) {
            self.err = Some(ScanError::Malformed);
            return None;
        }
        let mut v = &kv[colon + 1..];
        while let Some((&(b' ' | b'\t'), rest)) = v.split_first() {
            v = rest;
        }
        Some((k.to_vec(), v.to_vec()))
    }

    fn read_line(&mut self) -> &'a [u8] {
        let b = self.b;
        let line = match index_byte(&b[self.r..], b'\n') {
            Some(i) => {
                let line = &b[self.r..self.r + i + 1];
                self.r += i + 1;
                line
            }
            None => {
                let line = &b[self.r..];
                self.r = b.len();
                line
            }
        };
        if line.is_empty() {
            return line;
        }
        let mut line = line;
        if line[line.len() - 1] == b'\n' {
            let drop = if line.len() > 1 && line[line.len() - 2] == b'\r' {
                2
            } else {
                1
            };
            line = &line[..line.len() - drop];
        }
        line
    }

    fn read_continued_line_slice(&mut self) -> Result<Vec<u8>, ScanError> {
        let line = self.read_line();
        if line.is_empty() {
            return Ok(Vec::new());
        }
        if index_byte(line, b':').is_none() {
            return Err(ScanError::Malformed);
        }
        if self.b.len() - self.r > 1 {
            let peek = &self.b[self.r..self.r + 2];
            if is_ascii_letter(peek[0])
                || peek[0] == b'\n'
                || (peek[0] == b'\r' && peek[1] == b'\n')
            {
                return Ok(trim(line).to_vec());
            }
        }
        let mut mline = trim(line).to_vec();
        while self.skip_space() {
            mline.push(b' ');
            let line = self.read_line();
            mline.extend_from_slice(trim(line));
        }
        Ok(mline)
    }

    fn skip_space(&mut self) -> bool {
        let mut skipped = false;
        while self.r < self.b.len() {
            let c = self.b[self.r];
            if c != b' ' && c != b'\t' {
                break;
            }
            self.r += 1;
            skipped = true;
        }
        skipped
    }
}

pub(crate) fn is_valid_header_key(a: &[u8]) -> bool {
    if a.is_empty() {
        return false;
    }
    a.iter().all(|&c| valid_header_field_byte(c) || c == b' ')
}

fn is_ascii_letter(b: u8) -> bool {
    let b = b | 0x20;
    b.is_ascii_lowercase()
}

pub(crate) fn trim(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    let mut n = s.len();
    while n > i && (s[n - 1] == b' ' || s[n - 1] == b'\t') {
        n -= 1;
    }
    &s[i..n]
}
