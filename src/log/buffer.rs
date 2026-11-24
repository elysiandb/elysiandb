use super::handler::{Field, level_label, text_value};
use super::log::Level;
use super::value::AttrValue;
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError, ErrorKind};
use crate::gocompat::strings::{to_lower, trim_space};
use crate::gocompat::time::Time;
use crate::sync::RwLock;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const DEFAULT_QUERY_LIMIT: i64 = 200;
pub const MAX_QUERY_LIMIT: i64 = 1000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: u64,
    pub time: Time,
    pub level: &'static str,
    pub component: Vec<u8>,
    pub message: String,
    pub attrs: BTreeMap<String, AttrValue>,
    level_value: Level,
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub min_level: Level,
    pub component: Vec<u8>,
    pub text: Vec<u8>,
    pub user: Vec<u8>,
    pub request_id: Vec<u8>,
    pub since: Time,
    pub until: Time,
    pub before: u64,
    pub after: u64,
    pub limit: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub entries: Vec<Arc<Entry>>,
    pub has_more: bool,
    pub oldest_seq: u64,
    pub latest_seq: u64,
    pub size: usize,
    pub capacity: usize,
    pub buffer_since: Option<Time>,
    pub components: Vec<Vec<u8>>,
}

struct Ring {
    entries: Vec<Option<Arc<Entry>>>,
    next: usize,
    size: usize,
    seq: u64,
}

static BUFFER: RwLock<Ring> = RwLock::new(Ring {
    entries: Vec::new(),
    next: 0,
    size: 0,
    seq: 0,
});

pub(super) fn resize_buffer(capacity: i64) {
    let capacity = usize::try_from(capacity).unwrap_or(0);

    let mut buffer = BUFFER.write();

    if capacity == buffer.entries.len() {
        return;
    }

    let mut kept = buffer.ordered_locked();
    if kept.len() > capacity {
        kept.drain(..kept.len() - capacity);
    }

    let size = kept.len();
    let mut entries: Vec<Option<Arc<Entry>>> = kept.into_iter().map(Some).collect();
    entries.resize(capacity, None);
    buffer.entries = entries;
    buffer.size = size;
    buffer.next = 0;
    if capacity > 0 {
        buffer.next = size % capacity;
    }
}

pub fn buffer_enabled() -> bool {
    !BUFFER.read().entries.is_empty()
}

pub(super) fn record(t: &Time, level: Level, msg: &str, fields: &[Field<'_>]) {
    let mut buffer = BUFFER.write();

    if buffer.entries.is_empty() {
        return;
    }

    buffer.seq += 1;
    let mut entry = Entry {
        seq: buffer.seq,
        time: t.clone(),
        level: level_label(level),
        component: Vec::new(),
        message: msg.to_string(),
        attrs: BTreeMap::new(),
        level_value: level,
    };

    for f in fields {
        if f.key == "component"
            && let AttrValue::Str(s) = &f.value
        {
            entry.component = s.clone();
            continue;
        }

        entry.attrs.insert(f.key.to_string(), f.value.clone());
    }

    let next = buffer.next;
    buffer.entries[next] = Some(Arc::new(entry));
    buffer.next = (next + 1) % buffer.entries.len();
    if buffer.size < buffer.entries.len() {
        buffer.size += 1;
    }
}

impl Ring {
    fn ordered_locked(&self) -> Vec<Arc<Entry>> {
        let mut out = Vec::with_capacity(self.size);
        if self.size == 0 {
            return out;
        }

        let len = self.entries.len();
        let start = (self.next + len - self.size) % len;
        for i in 0..self.size {
            if let Some(e) = &self.entries[(start + i) % len] {
                out.push(Arc::clone(e));
            }
        }

        out
    }
}

pub fn logs(q: &Query) -> Page {
    let (ordered, capacity) = {
        let buffer = BUFFER.read();
        (buffer.ordered_locked(), buffer.entries.len())
    };

    let mut limit = q.limit;
    if limit <= 0 {
        limit = DEFAULT_QUERY_LIMIT;
    }
    if limit > MAX_QUERY_LIMIT {
        limit = MAX_QUERY_LIMIT;
    }
    let limit = limit as usize;

    let mut page = Page {
        entries: Vec::new(),
        size: ordered.len(),
        capacity,
        ..Page::default()
    };

    let components: BTreeSet<&[u8]> = ordered
        .iter()
        .filter(|e| !e.component.is_empty())
        .map(|e| e.component.as_slice())
        .collect();
    page.components = components.into_iter().map(<[u8]>::to_vec).collect();

    if let (Some(first), Some(last)) = (ordered.first(), ordered.last()) {
        page.oldest_seq = first.seq;
        page.latest_seq = last.seq;
        page.buffer_since = Some(first.time.clone());
    }

    let text = to_lower(trim_space(&q.text));

    for e in ordered.iter().rev() {
        if !q.matches(e, &text) {
            continue;
        }

        if page.entries.len() == limit {
            page.has_more = true;
            break;
        }

        page.entries.push(Arc::clone(e));
    }

    page
}

impl Query {
    fn matches(&self, e: &Entry, text: &[u8]) -> bool {
        if e.level_value < self.min_level {
            return false;
        }
        if self.after > 0 && e.seq <= self.after {
            return false;
        }
        if self.before > 0 && e.seq >= self.before {
            return false;
        }
        if !self.since.is_zero() && e.time.before(&self.since) {
            return false;
        }
        if !self.until.is_zero() && e.time.after(&self.until) {
            return false;
        }
        if !self.component.is_empty() && e.component != self.component {
            return false;
        }
        if !self.user.is_empty() && !attr_equals(e, &self.user, &["user", "actor"]) {
            return false;
        }
        if !self.request_id.is_empty() && !attr_equals(e, &self.request_id, &["request_id"]) {
            return false;
        }
        if !text.is_empty() && !entry_contains(e, text) {
            return false;
        }

        true
    }
}

fn attr_equals(e: &Entry, want: &[u8], keys: &[&str]) -> bool {
    keys.iter().any(|k| {
        e.attrs
            .get(*k)
            .is_some_and(|v| text_value(v).as_ref() == want)
    })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

fn entry_contains(e: &Entry, text: &[u8]) -> bool {
    if contains(&to_lower(e.message.as_bytes()), text) || contains(&to_lower(&e.component), text) {
        return true;
    }

    e.attrs.iter().any(|(k, v)| {
        contains(&to_lower(k.as_bytes()), text) || contains(&to_lower(&text_value(v)), text)
    })
}

pub(super) fn encode_time(t: &Time, e: &mut EncodeState) -> Result<(), JsonError> {
    let mut b = Vec::with_capacity(40);
    b.push(b'"');
    t.append_strict_rfc3339(&mut b).map_err(|err| {
        JsonError::new(ErrorKind::Custom(format!("cannot encode JSON: {err}")), 0)
    })?;
    b.push(b'"');
    e.write_raw(&b);
    Ok(())
}

impl Encode for Entry {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "seq");
        e.write_u64(self.seq);
        e.write_field_name(&mut first, "time");
        encode_time(&self.time, e)?;
        e.write_field_name(&mut first, "level");
        e.write_string(self.level.as_bytes());
        if !self.component.is_empty() {
            e.write_field_name(&mut first, "component");
            e.write_string(&self.component);
        }
        e.write_field_name(&mut first, "msg");
        e.write_string(self.message.as_bytes());
        if !self.attrs.is_empty() {
            e.write_field_name(&mut first, "attrs");
            self.attrs.encode(e)?;
        }
        e.end_struct(first);
        Ok(())
    }
}

impl Encode for Page {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "entries");
        e.write_byte(b'[');
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            entry.encode(e)?;
        }
        e.write_byte(b']');
        e.write_field_name(&mut first, "has_more");
        e.write_bool(self.has_more);
        e.write_field_name(&mut first, "oldest_seq");
        e.write_u64(self.oldest_seq);
        e.write_field_name(&mut first, "latest_seq");
        e.write_u64(self.latest_seq);
        e.write_field_name(&mut first, "size");
        e.write_u64(self.size as u64);
        e.write_field_name(&mut first, "capacity");
        e.write_u64(self.capacity as u64);
        e.write_field_name(&mut first, "buffer_since");
        match &self.buffer_since {
            Some(t) => encode_time(t, e)?,
            None => e.write_null(),
        }
        e.write_field_name(&mut first, "components");
        e.write_byte(b'[');
        for (i, c) in self.components.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            e.write_string(c);
        }
        e.write_byte(b']');
        e.end_struct(first);
        Ok(())
    }
}
