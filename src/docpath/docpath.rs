use crate::gocompat::json::{Map, Value};
use crate::gocompat::strings;
use crate::gocompat::time::{self, Time};

pub const CORE_FIELDS_PREFIX: &str = "_elysiandb_core_";

pub fn split(path: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..=path.len() {
        if i == path.len() || path[i] == b'.' {
            out.push(&path[start..i]);
            start = i + 1;
        }
    }

    out
}

pub trait Node: Sized {
    fn is_object(&self) -> bool;
    fn field(&self, key: &[u8]) -> Option<&Self>;
    fn items(&self) -> Option<&[Self]>;
    fn text(&self) -> Option<&[u8]>;
    fn is_null(&self) -> bool;
}

impl Node for Value {
    fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    fn field(&self, key: &[u8]) -> Option<&Value> {
        match self {
            Value::Object(m) => std::str::from_utf8(key).ok().and_then(|k| m.get(k)),
            _ => None,
        }
    }

    fn items(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    fn text(&self) -> Option<&[u8]> {
        match self {
            Value::String(s) => Some(s.as_bytes()),
            _ => None,
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

pub fn resolve_values<'a, N: Node>(data: &'a N, path: &[u8]) -> Vec<&'a N> {
    let mut out = Vec::new();
    resolve(data, &split(path), &mut out);
    out
}

fn resolve<'a, N: Node>(root: &'a N, parts: &[&[u8]], out: &mut Vec<&'a N>) {
    let mut stack: Vec<(&'a N, usize)> = vec![(root, 0)];
    while let Some((current, i)) = stack.pop() {
        if i == parts.len() {
            out.push(current);
            continue;
        }

        if current.is_object() {
            if let Some(next) = current.field(parts[i]) {
                stack.push((next, i + 1));
            }
            continue;
        }

        if let Some(items) = current.items() {
            for item in items.iter().rev() {
                stack.push((item, i));
            }
        }
    }
}

pub enum SortValue<'a, N> {
    Nil,
    Time(Time),
    Value(&'a N),
}

pub fn sort_value<'a, N: Node>(m: &'a N, path: &[u8]) -> SortValue<'a, N> {
    let mut cur = Some(m);
    for p in path.split(|&b| b == b'.') {
        match cur {
            Some(mm) if mm.is_object() => cur = mm.field(p),
            _ => return SortValue::Nil,
        }
    }

    sort_value_at(cur)
}

pub fn sort_map_value<'a>(m: &'a Map, path: &[u8]) -> SortValue<'a, Value> {
    let mut parts = path.split(|&b| b == b'.');
    let mut cur = parts.next().and_then(|p| map_field(m, p));
    for p in parts {
        match cur {
            Some(mm) if mm.is_object() => cur = mm.field(p),
            _ => return SortValue::Nil,
        }
    }

    sort_value_at(cur)
}

fn map_field<'a>(m: &'a Map, key: &[u8]) -> Option<&'a Value> {
    std::str::from_utf8(key).ok().and_then(|k| m.get(k))
}

pub enum Resolved<'a> {
    Value(&'a Value),
    Map(&'a Map),
}

pub fn resolve_map_values<'a>(data: &'a Map, path: &[u8]) -> Vec<Resolved<'a>> {
    let parts = split(path);
    let mut out = Vec::new();
    let mut stack: Vec<(Resolved<'a>, usize)> = vec![(Resolved::Map(data), 0)];
    while let Some((current, i)) = stack.pop() {
        if i == parts.len() {
            out.push(current);
            continue;
        }

        match current {
            Resolved::Map(m) | Resolved::Value(Value::Object(m)) => {
                if let Some(next) = map_field(m, parts[i]) {
                    stack.push((Resolved::Value(next), i + 1));
                }
            }
            Resolved::Value(Value::Array(items)) => {
                for item in items.iter().rev() {
                    stack.push((Resolved::Value(item), i));
                }
            }
            Resolved::Value(Value::DocList(maps)) => {
                for m in maps.iter().rev() {
                    stack.push((Resolved::Map(m), i));
                }
            }
            Resolved::Value(_) => {}
        }
    }

    out
}

fn sort_value_at<'a, N: Node>(cur: Option<&'a N>) -> SortValue<'a, N> {
    let Some(cur) = cur else {
        return SortValue::Nil;
    };
    if cur.is_null() {
        return SortValue::Nil;
    }

    if let Some(s) = cur.text() {
        if let Some(t) = parse_sort_date(s) {
            return SortValue::Time(t);
        }

        return SortValue::Value(cur);
    }

    SortValue::Value(cur)
}

pub fn parse_sort_date(s: &[u8]) -> Option<Time> {
    if !looks_like_date(s) {
        return None;
    }

    if let Ok(t) = time::parse(time::RFC3339, s) {
        return Some(t);
    }

    if let Ok(t) = time::parse("2006-01-02", s) {
        return Some(t);
    }

    None
}

pub fn parse_date(s: &[u8]) -> Option<(Time, bool)> {
    if !looks_like_date(s) {
        return None;
    }

    if let Ok(t) = time::parse(time::RFC3339, s) {
        return Some((t, false));
    }

    if let Ok(t) = time::parse("2006-01-02", s) {
        return Some((t, true));
    }

    None
}

fn looks_like_date(s: &[u8]) -> bool {
    if s.len() < 10 {
        return false;
    }

    if !s[..4].iter().all(u8::is_ascii_digit) {
        return false;
    }

    s[4] == b'-'
}

pub fn is_searchable_key(key: &[u8]) -> bool {
    key != b"id" && key != b"@entity" && !key.starts_with(CORE_FIELDS_PREFIX.as_bytes())
}

pub fn match_glob(pattern: &[u8], s: &[u8]) -> bool {
    match_glob_lower(&strings::to_lower(pattern), &strings::to_lower(s))
}

pub fn match_glob_lower(p: &[u8], s: &[u8]) -> bool {
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut matched = 0usize;

    while j < s.len() {
        if i < p.len() && p[i] == b'\\' {
            if i + 1 >= p.len() {
                return false;
            }

            if p[i + 1] != s[j] {
                if let Some(st) = star {
                    i = st + 1;
                    matched += 1;
                    j = matched;

                    continue;
                }

                return false;
            }

            i += 2;
            j += 1;

            continue;
        }

        if i < p.len() && p[i] == b'*' {
            star = Some(i);
            i += 1;
            matched = j;

            continue;
        }

        if i < p.len() && p[i] == s[j] {
            i += 1;
            j += 1;

            continue;
        }

        if let Some(st) = star {
            i = st + 1;
            matched += 1;
            j = matched;

            continue;
        }

        return false;
    }

    while i < p.len() {
        if p[i] == b'\\' {
            return false;
        }

        if p[i] != b'*' {
            return false;
        }

        i += 1;
    }

    true
}
