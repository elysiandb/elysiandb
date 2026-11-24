use std::collections::BTreeMap;
use std::io::Read;

use super::bufio::{BufReader, ReadErr};
use super::bytesconv::valid_header_field_byte;
use super::headerscanner::trim;

pub type MimeHeader = BTreeMap<String, Vec<Vec<u8>>>;

#[derive(Debug)]
pub enum TextprotoError {
    Read(ReadErr),
    MessageTooLarge,
    Protocol(String),
}

impl std::fmt::Display for TextprotoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextprotoError::Read(ReadErr::Eof) => f.write_str("unexpected end of input"),
            TextprotoError::Read(ReadErr::Io(e)) => write!(f, "{e}"),
            TextprotoError::MessageTooLarge => f.write_str("message too large"),
            TextprotoError::Protocol(s) => f.write_str(s),
        }
    }
}

pub fn canonical_mime_header_key(s: &[u8]) -> String {
    canonical_key(s).0
}

fn canonical_key(a: &[u8]) -> (String, bool) {
    if a.is_empty() {
        return (String::new(), false);
    }
    let mut no_canon = false;
    for &c in a {
        if valid_header_field_byte(c) {
            continue;
        }
        if c == b' ' {
            no_canon = true;
            continue;
        }
        return (String::from_utf8_lossy(a).into_owned(), false);
    }
    if no_canon {
        return (String::from_utf8_lossy(a).into_owned(), true);
    }
    let mut out = Vec::with_capacity(a.len());
    let mut upper = true;
    for &c in a {
        let c = if upper && c.is_ascii_lowercase() {
            c - 32
        } else if !upper && c.is_ascii_uppercase() {
            c + 32
        } else {
            c
        };
        out.push(c);
        upper = c == b'-';
    }
    (String::from_utf8_lossy(&out).into_owned(), true)
}

fn valid_header_value_byte(c: u8) -> bool {
    c >= 0x80 || (0x20..0x7f).contains(&c) || c == 0x09
}

pub fn header_get<'a>(h: &'a MimeHeader, key: &str) -> &'a [u8] {
    let k = canonical_mime_header_key(key.as_bytes());
    h.get(&k)
        .and_then(|v| v.first())
        .map_or(&[], |v| v.as_slice())
}

pub fn header_del(h: &mut MimeHeader, key: &str) {
    let k = canonical_mime_header_key(key.as_bytes());
    h.remove(&k);
}

fn read_line_slice<R: Read>(r: &mut BufReader<R>, lim: i64) -> Result<Vec<u8>, TextprotoError> {
    let mut line: Option<Vec<u8>> = None;
    loop {
        let (l, more) = r.read_line().map_err(TextprotoError::Read)?;
        let cur = line.as_ref().map_or(0, Vec::len);
        if lim >= 0 && (cur + l.len()) as i64 > lim {
            return Err(TextprotoError::MessageTooLarge);
        }
        if line.is_none() && !more {
            return Ok(l);
        }
        line.get_or_insert_with(Vec::new).extend_from_slice(&l);
        if !more {
            break;
        }
    }
    Ok(line.unwrap_or_default())
}

fn skip_space<R: Read>(r: &mut BufReader<R>) -> usize {
    let mut n = 0;
    loop {
        match r.read_byte() {
            Err(_) => break,
            Ok(c) => {
                if c != b' ' && c != b'\t' {
                    r.unread_byte();
                    break;
                }
                n += 1;
            }
        }
    }
    n
}

fn read_continued_line_slice<R: Read>(
    r: &mut BufReader<R>,
    lim: i64,
) -> Result<Vec<u8>, TextprotoError> {
    let line = read_line_slice(r, lim)?;
    if line.is_empty() {
        return Ok(line);
    }
    if !line.contains(&b':') {
        return Err(TextprotoError::Protocol(
            "malformed MIME header: missing colon".to_string(),
        ));
    }
    if r.buffered() > 1 {
        let (peek, _) = r.peek(2);
        if (!peek.is_empty() && (is_ascii_letter(peek[0]) || peek[0] == b'\n'))
            || (peek.len() == 2 && peek[0] == b'\r' && peek[1] == b'\n')
        {
            return Ok(trim(&line).to_vec());
        }
    }
    let mut buf = trim(&line).to_vec();
    let mut lim = if lim < 0 { i64::MAX } else { lim };
    lim -= buf.len() as i64;
    while skip_space(r) > 0 {
        buf.push(b' ');
        if buf.len() as i64 >= lim {
            return Err(TextprotoError::MessageTooLarge);
        }
        match read_line_slice(r, lim - buf.len() as i64) {
            Err(_) => break,
            Ok(line) => buf.extend_from_slice(trim(&line)),
        }
    }
    Ok(buf)
}

fn is_ascii_letter(b: u8) -> bool {
    (b | 0x20).is_ascii_lowercase()
}

pub fn read_mime_header<R: Read>(
    r: &mut BufReader<R>,
    max_memory: i64,
    max_headers: i64,
) -> Result<MimeHeader, TextprotoError> {
    let mut m = MimeHeader::new();
    let mut max_memory = max_memory - 400;
    let mut max_headers = max_headers;
    const MAP_ENTRY_OVERHEAD: i64 = 200;

    let (first, err) = r.peek(1);
    let first_byte = first.first().copied();
    if err.is_none() && matches!(first_byte, Some(b' ' | b'\t')) {
        let line = read_line_slice(r, 80)?;
        return Err(TextprotoError::Protocol(format!(
            "malformed MIME header initial line: {}",
            String::from_utf8_lossy(&line)
        )));
    }

    loop {
        let kv = read_continued_line_slice(r, max_memory)?;
        if kv.is_empty() {
            return Ok(m);
        }
        let Some(colon) = kv.iter().position(|&c| c == b':') else {
            return Err(TextprotoError::Protocol(
                "malformed MIME header line".to_string(),
            ));
        };
        let (k, v) = (&kv[..colon], &kv[colon + 1..]);
        let (key, ok) = canonical_key(k);
        if !ok {
            return Err(TextprotoError::Protocol(
                "malformed MIME header line".to_string(),
            ));
        }
        if !v.iter().all(|&c| valid_header_value_byte(c)) {
            return Err(TextprotoError::Protocol(
                "malformed MIME header line".to_string(),
            ));
        }
        max_headers -= 1;
        if max_headers < 0 {
            return Err(TextprotoError::MessageTooLarge);
        }
        let mut value = v;
        while let Some((&(b' ' | b'\t'), rest)) = value.split_first() {
            value = rest;
        }
        let exists = m.contains_key(&key);
        if !exists {
            max_memory -= key.len() as i64;
            max_memory -= MAP_ENTRY_OVERHEAD;
        }
        max_memory -= value.len() as i64;
        if max_memory < 0 {
            return Err(TextprotoError::MessageTooLarge);
        }
        m.entry(key).or_default().push(value.to_vec());
    }
}
