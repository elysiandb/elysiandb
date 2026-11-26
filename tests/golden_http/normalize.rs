use std::collections::BTreeMap;

use elysiandb::gocompat::json::{self, Map, Value};

pub struct Normalizer {
    data: String,
    tmp: String,
    uuids: BTreeMap<String, usize>,
    txids: BTreeMap<String, usize>,
}

fn is_hex_lower(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

fn digits(b: &[u8], at: usize, n: usize) -> bool {
    at + n <= b.len() && b[at..at + n].iter().all(u8::is_ascii_digit)
}

fn match_uuid(b: &[u8], i: usize) -> Option<usize> {
    let groups = [8, 4, 4, 4, 12];
    let mut p = i;
    for (gi, n) in groups.iter().enumerate() {
        if p + n > b.len() || !b[p..p + n].iter().all(|&c| is_hex_lower(c)) {
            return None;
        }
        p += n;
        if gi < 4 {
            if b.get(p) != Some(&b'-') {
                return None;
            }
            p += 1;
        }
    }
    Some(p)
}

fn match_txid(b: &[u8], i: usize) -> Option<usize> {
    if digits(b, i, 14) && b.get(i + 14) == Some(&b'.') && digits(b, i + 15, 9) {
        return Some(i + 24);
    }
    None
}

fn match_zone(b: &[u8], p: usize) -> Option<usize> {
    match b.get(p) {
        Some(b'Z') => Some(p + 1),
        Some(b'+' | b'-')
            if digits(b, p + 1, 2) && b.get(p + 3) == Some(&b':') && digits(b, p + 4, 2) =>
        {
            Some(p + 6)
        }
        _ => None,
    }
}

fn match_time(b: &[u8], i: usize) -> Option<usize> {
    let shape = [
        (4, Some(b'-')),
        (2, Some(b'-')),
        (2, Some(b'T')),
        (2, Some(b':')),
        (2, Some(b':')),
        (2, None),
    ];
    let mut p = i;
    for (n, sep) in shape {
        if !digits(b, p, n) {
            return None;
        }
        p += n;
        if let Some(s) = sep {
            if b.get(p) != Some(&s) {
                return None;
            }
            p += 1;
        }
    }
    if b.get(p) == Some(&b'.') && digits(b, p + 1, 1) {
        let mut q = p + 1;
        while q < b.len() && b[q].is_ascii_digit() {
            q += 1;
        }
        if let Some(end) = match_zone(b, q) {
            return Some(end);
        }
    }
    match_zone(b, p)
}

fn match_stamp(b: &[u8], i: usize) -> Option<usize> {
    if !digits(b, i, 8) {
        return None;
    }
    if b.get(i + 8) == Some(&b'-') && digits(b, i + 9, 6) {
        return Some(i + 15);
    }
    if b.get(i + 8) == Some(&b'T') && digits(b, i + 9, 6) {
        let end = i + 15;
        return Some(if b.get(end) == Some(&b'Z') {
            end + 1
        } else {
            end
        });
    }
    None
}

fn match_session(b: &[u8], i: usize) -> Option<usize> {
    let prefix = b"edb_session=";
    if !b[i..].starts_with(prefix) {
        return None;
    }
    let mut p = i + prefix.len();
    let start = p;
    while p < b.len() && is_hex_lower(b[p]) {
        p += 1;
    }
    if p == start { None } else { Some(p) }
}

fn match_stats_version(b: &[u8], i: usize) -> Option<usize> {
    let prefix = b"\"version\":\"";
    if !b[i..].starts_with(prefix) {
        return None;
    }
    let p = i + prefix.len();
    let q = b[p..].iter().position(|&c| c == b'"')?;
    Some(p + q + 1)
}

fn replace_all(
    b: &[u8],
    matcher: impl Fn(&[u8], usize) -> Option<usize>,
    mut repl: impl FnMut(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = matcher(b, i) {
            out.extend_from_slice(&repl(&b[i..end]));
            i = end;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn replace_text(b: &[u8], from: &str, to: &str) -> Vec<u8> {
    if from.is_empty() {
        return b.to_vec();
    }
    String::from_utf8_lossy(b).replace(from, to).into_bytes()
}

pub fn is_request_id(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() < 10 || !b[..8].iter().all(|&c| is_hex_lower(c)) || b[8] != b'-' {
        return false;
    }
    b[9..]
        .iter()
        .all(|&c| c.is_ascii_digit() || c.is_ascii_lowercase())
}

fn shape_of(v: &Value) -> Value {
    match v {
        Value::Null => Value::String("<null>".to_string()),
        Value::Bool(_) => Value::String("<bool>".to_string()),
        Value::Number(_) | Value::Int(_) => Value::String("<number>".to_string()),
        Value::String(_) => Value::String("<string>".to_string()),
        Value::Array(a) => match a.first() {
            None => Value::Array(Vec::new()),
            Some(x) => Value::Array(vec![shape_of(x)]),
        },
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                out.insert(k.clone(), shape_of(x));
            }
            Value::Object(out)
        }
        Value::DocList(list) => match list.first() {
            None => Value::Array(Vec::new()),
            Some(m) => Value::Array(vec![shape_of(&Value::Object(m.clone()))]),
        },
    }
}

fn json_shape(body: &[u8]) -> Vec<u8> {
    match json::unmarshal_value_with_integers(body) {
        Err(_) => b"<NOT JSON>".to_vec(),
        Ok(v) => json::marshal(&shape_of(&v)).unwrap_or_else(|_| b"<NOT JSON>".to_vec()),
    }
}

fn decode_chunked(b: &[u8]) -> (Vec<u8>, usize, bool) {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let Some(eol) = b[pos..].windows(2).position(|w| w == b"\r\n") else {
            return (out, b.len(), false);
        };
        let size_text = String::from_utf8_lossy(&b[pos..pos + eol]).into_owned();
        let Ok(size) = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
        else {
            return (out, b.len(), false);
        };
        pos += eol + 2;
        if size == 0 {
            if b[pos..].starts_with(b"\r\n") {
                pos += 2;
            }
            return (out, pos, true);
        }
        if pos + size + 2 > b.len() {
            out.extend_from_slice(&b[pos..(pos + size).min(b.len())]);
            return (out, b.len(), false);
        }
        out.extend_from_slice(&b[pos..pos + size]);
        pos += size + 2;
    }
}

impl Normalizer {
    pub fn new(data: &str, tmp: &str) -> Normalizer {
        Normalizer {
            data: data.to_string(),
            tmp: tmp.trim_end_matches('/').to_string(),
            uuids: BTreeMap::new(),
            txids: BTreeMap::new(),
        }
    }

    pub fn replace_body(&mut self, body: &[u8]) -> Vec<u8> {
        let mut s = replace_text(body, &self.data, "<DATA>");
        s = replace_text(&s, &self.tmp, "<TMP>");
        let uuids = &mut self.uuids;
        s = replace_all(&s, match_uuid, |m| {
            let key = String::from_utf8_lossy(m).into_owned();
            let next = uuids.len() + 1;
            let n = *uuids.entry(key).or_insert(next);
            format!("<UUID:{n}>").into_bytes()
        });
        let txids = &mut self.txids;
        s = replace_all(&s, match_txid, |m| {
            let key = String::from_utf8_lossy(m).into_owned();
            let next = txids.len() + 1;
            let n = *txids.entry(key).or_insert(next);
            format!("<TXID:{n}>").into_bytes()
        });
        s = replace_all(&s, match_time, |_| b"<TIME>".to_vec());
        s = replace_all(&s, match_session, |_| b"edb_session=<SID>".to_vec());
        replace_all(&s, match_stats_version, |_| {
            b"\"version\":\"<VER>\"".to_vec()
        })
    }

    pub fn normalize(&mut self, raw: &[u8], head: bool, compare: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = raw;
        while !rest.is_empty() {
            let end = rest.windows(4).position(|w| w == b"\r\n\r\n");
            let Some(end) = end.filter(|_| rest.starts_with(b"HTTP/")) else {
                out.extend_from_slice(b"<UNPARSED>\r\n");
                out.extend_from_slice(&self.replace_body(rest));
                break;
            };
            let head_text = String::from_utf8_lossy(&rest[..end]).into_owned();
            rest = &rest[end + 4..];
            let mut lines = head_text.split("\r\n");
            let status = lines.next().unwrap_or("").to_string();
            let code: u16 = status
                .split(' ')
                .nth(1)
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let mut headers: Vec<(String, String)> = Vec::new();
            let mut length: Option<usize> = None;
            let mut chunked = false;
            for l in lines {
                let (k, v) = l.split_once(':').unwrap_or((l, ""));
                let v = v.trim().to_string();
                match k.to_ascii_lowercase().as_str() {
                    "content-length" => length = v.parse().ok(),
                    "transfer-encoding" => chunked = v.eq_ignore_ascii_case("chunked"),
                    _ => {}
                }
                headers.push((k.to_string(), v));
            }
            let mut body: Vec<u8> = Vec::new();
            let mut marker = "";
            let no_body = head || code == 100 || code == 204 || code == 304;
            if no_body {
                body.clear();
            } else if chunked {
                let (decoded, consumed, complete) = decode_chunked(rest);
                body = decoded;
                rest = &rest[consumed..];
                marker = if complete {
                    "<CHUNKED complete>\r\n"
                } else {
                    "<CHUNKED truncated>\r\n"
                };
            } else if let Some(n) = length {
                let n = n.min(rest.len());
                body = rest[..n].to_vec();
                rest = &rest[n..];
            } else {
                body = rest.to_vec();
                rest = &[];
            }
            let mut new_body = self.replace_body(&body);
            if compare == "d27" && code != 100 {
                new_body = b"<D27>".to_vec();
            }
            if compare == "json-shape" && code != 100 {
                new_body = json_shape(&new_body);
            }
            out.extend_from_slice(status.as_bytes());
            out.extend_from_slice(b"\r\n");
            for (k, v) in &headers {
                let mut v = v.clone();
                match k.to_ascii_lowercase().as_str() {
                    "date" => v = "<DATE>".to_string(),
                    "x-request-id" => {
                        if is_request_id(&v) {
                            v = "<RID>".to_string();
                        }
                    }
                    "x-elysian-version" => v = "<VER>".to_string(),
                    "content-length" => {
                        if compare == "d27" || compare == "head-only" || new_body != body {
                            v = "<LEN>".to_string();
                        }
                    }
                    "content-disposition" => {
                        v = String::from_utf8_lossy(&replace_all(
                            v.as_bytes(),
                            match_stamp,
                            |_| b"<STAMP>".to_vec(),
                        ))
                        .into_owned();
                    }
                    _ => {}
                }
                let v = replace_all(v.as_bytes(), match_session, |_| {
                    b"edb_session=<SID>".to_vec()
                });
                let v = self.replace_body(&v);
                out.extend_from_slice(k.as_bytes());
                out.extend_from_slice(b": ");
                out.extend_from_slice(&v);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(marker.as_bytes());
            if compare == "head-only" {
                out.extend_from_slice(b"<BODY OMITTED>");
                break;
            }
            out.extend_from_slice(&new_body);
            if code != 100 && no_body && !rest.is_empty() && !rest.starts_with(b"HTTP/") {
                break;
            }
        }
        out
    }
}
