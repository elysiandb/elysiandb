use crate::heap::{FxHashMap, FxHasher, Tracer};
use crate::string::JsString;
use crate::value::Value;
use std::hash::Hasher;

#[derive(Clone, Debug)]
struct MapEntry {
    key: Value,
    value: Value,
    h_next: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct OrderedMap {
    hash_table: FxHashMap<u64, u32>,
    entries: Vec<MapEntry>,
    size: usize,
}

fn hash_string(s: &JsString) -> u64 {
    let mut h = FxHasher::default();
    match s {
        JsString::Ascii(a) => h.write(a.as_bytes()),
        _ => {
            let u = s.string();
            match u.as_utf16() {
                Some(b) => {
                    for c in b {
                        h.write_u16(*c);
                    }
                }
                None => h.write(&u.as_bytes()),
            }
        }
    }
    h.finish()
}

const HASH_FALSE: u64 = 0x9e37_79b9_7f4a_7c15;
const HASH_TRUE: u64 = 0xbf58_476d_1ce4_e5b9;
const HASH_NULL: u64 = 0x94d0_49bb_1331_11eb;
const HASH_UNDEF: u64 = 0x2545_f491_4f6c_dd1d;

pub fn value_hash(v: &Value) -> u64 {
    match v {
        Value::Int(i) => *i as u64,
        Value::Float(f) => {
            if *f == 0.0 && f.is_sign_negative() {
                0
            } else {
                f.to_bits()
            }
        }
        Value::Bool(true) => HASH_TRUE,
        Value::Bool(false) => HASH_FALSE,
        Value::Null => HASH_NULL,
        Value::Undefined => HASH_UNDEF,
        Value::String(s) => hash_string(s),
        Value::BigInt(b) => {
            let mut h = FxHasher::default();
            h.write(b.string().as_bytes());
            h.finish()
        }
        Value::Symbol(s) => u64::from(s.0).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5bd1_e995,
        Value::Object(o) => o.get_id(),
        _ => 0,
    }
}

fn normalize_key(key: &Value) -> Value {
    if let Value::Float(f) = key
        && *f == 0.0
        && f.is_sign_negative()
    {
        return Value::Int(0);
    }
    key.clone()
}

impl OrderedMap {
    pub fn new() -> OrderedMap {
        OrderedMap::default()
    }

    fn lookup(&self, key: &Value) -> (u64, Option<u32>, Option<u32>) {
        let key = normalize_key(key);
        let h = value_hash(&key);
        let mut prev = None;
        let mut cur = self.hash_table.get(&h).copied();
        while let Some(i) = cur {
            let e = &self.entries[i as usize];
            if e.key.same_as(&key) {
                break;
            }
            prev = cur;
            cur = e.h_next;
        }
        (h, cur, prev)
    }

    pub fn set(&mut self, key: Value, value: Value) {
        let (h, entry, prev) = self.lookup(&key);
        if let Some(i) = entry {
            self.entries[i as usize].value = value;
            return;
        }
        let key = normalize_key(&key);
        let idx = self.entries.len() as u32;
        self.entries.push(MapEntry {
            key,
            value,
            h_next: None,
        });
        match prev {
            None => {
                self.hash_table.insert(h, idx);
            }
            Some(p) => self.entries[p as usize].h_next = Some(idx),
        }
        self.size += 1;
    }

    pub fn get(&self, key: &Value) -> Option<&Value> {
        let (_, entry, _) = self.lookup(key);
        entry.map(|i| &self.entries[i as usize].value)
    }

    pub fn get_mut(&mut self, key: &Value) -> Option<&mut Value> {
        let (_, entry, _) = self.lookup(key);
        entry.map(|i| &mut self.entries[i as usize].value)
    }

    pub fn remove(&mut self, key: &Value) -> bool {
        let (h, entry, prev) = self.lookup(key);
        let Some(i) = entry else {
            return false;
        };
        let next = self.entries[i as usize].h_next;
        match prev {
            None => match next {
                None => {
                    self.hash_table.remove(&h);
                }
                Some(n) => {
                    self.hash_table.insert(h, n);
                }
            },
            Some(p) => self.entries[p as usize].h_next = next,
        }
        let e = &mut self.entries[i as usize];
        e.key = Value::Empty;
        e.value = Value::Empty;
        e.h_next = None;
        self.size -= 1;
        true
    }

    pub fn has(&self, key: &Value) -> bool {
        self.lookup(key).1.is_some()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn clear(&mut self) {
        for e in &mut self.entries {
            e.key = Value::Empty;
            e.value = Value::Empty;
            e.h_next = None;
        }
        self.hash_table.clear();
        self.size = 0;
    }

    pub fn next_entry(&self, pos: &mut usize) -> Option<(&Value, &Value)> {
        while *pos < self.entries.len() {
            let e = &self.entries[*pos];
            *pos += 1;
            if !e.key.is_empty() {
                return Some((&e.key, &e.value));
            }
        }
        None
    }

    pub fn trace(&self, t: &mut Tracer) {
        for e in &self.entries {
            t.value(&e.key);
            t.value(&e.value);
        }
    }
}
