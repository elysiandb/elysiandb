use crate::heap::{FxHashMap, FxHasher, Tracer};
use crate::string::JsString;
use crate::value::Value;
use std::hash::Hasher;

#[derive(Clone, Debug)]
struct MapEntry {
    key: Value,
    value: Value,
    iter_prev: Option<u32>,
    iter_next: Option<u32>,
    h_next: Option<u32>,
    pins: u32,
    live: bool,
    allocated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OrderedMap {
    hash_table: FxHashMap<u64, u32>,
    entries: Vec<MapEntry>,
    free: Vec<u32>,
    iter_first: Option<u32>,
    iter_last: Option<u32>,
    size: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct OrderedMapIter {
    open: bool,
    cur: Option<u32>,
}

impl OrderedMapIter {
    pub fn new() -> OrderedMapIter {
        OrderedMapIter {
            open: true,
            cur: None,
        }
    }
}

impl Default for OrderedMapIter {
    fn default() -> OrderedMapIter {
        OrderedMapIter::new()
    }
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
        let mut h_prev = None;
        let mut entry = self.hash_table.get(&h).copied();
        while let Some(i) = entry {
            let e = &self.entries[i as usize];
            if e.key.same_as(&key) {
                break;
            }
            h_prev = entry;
            entry = e.h_next;
        }
        (h, entry, h_prev)
    }

    fn alloc_entry(&mut self, e: MapEntry) -> u32 {
        if let Some(i) = self.free.pop() {
            self.entries[i as usize] = e;
            return i;
        }
        self.entries.push(e);
        (self.entries.len() - 1) as u32
    }

    fn try_free(&mut self, mut i: u32) {
        loop {
            let e = &mut self.entries[i as usize];
            if !e.allocated || e.live || e.pins > 0 {
                return;
            }
            e.allocated = false;
            let prev = e.iter_prev.take();
            e.iter_next = None;
            e.h_next = None;
            e.key = Value::Empty;
            e.value = Value::Empty;
            self.free.push(i);
            match prev {
                Some(p) => {
                    let pe = &mut self.entries[p as usize];
                    pe.pins -= 1;
                    i = p;
                }
                None => return,
            }
        }
    }

    pub fn set(&mut self, key: Value, value: Value) {
        let (h, entry, h_prev) = self.lookup(&key);
        if let Some(i) = entry {
            self.entries[i as usize].value = value;
            return;
        }
        let key = normalize_key(&key);
        let i = self.alloc_entry(MapEntry {
            key,
            value,
            iter_prev: self.iter_last,
            iter_next: None,
            h_next: None,
            pins: 0,
            live: true,
            allocated: true,
        });
        match h_prev {
            None => {
                self.hash_table.insert(h, i);
            }
            Some(p) => self.entries[p as usize].h_next = Some(i),
        }
        match self.iter_last {
            Some(l) => self.entries[l as usize].iter_next = Some(i),
            None => self.iter_first = Some(i),
        }
        self.iter_last = Some(i);
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

    fn unlink(&mut self, i: u32) {
        let (prev, next) = {
            let e = &mut self.entries[i as usize];
            e.key = Value::Empty;
            e.value = Value::Empty;
            e.live = false;
            (e.iter_prev, e.iter_next)
        };
        match prev {
            Some(p) => self.entries[p as usize].iter_next = next,
            None => self.iter_first = next,
        }
        match next {
            Some(n) => self.entries[n as usize].iter_prev = prev,
            None => self.iter_last = prev,
        }
        if let Some(p) = prev {
            self.entries[p as usize].pins += 1;
        }
    }

    pub fn remove(&mut self, key: &Value) -> bool {
        let (h, entry, h_prev) = self.lookup(key);
        let Some(i) = entry else {
            return false;
        };
        let h_next = self.entries[i as usize].h_next.take();
        match h_prev {
            None => match h_next {
                None => {
                    self.hash_table.remove(&h);
                }
                Some(n) => {
                    self.hash_table.insert(h, n);
                }
            },
            Some(p) => self.entries[p as usize].h_next = h_next,
        }
        self.unlink(i);
        self.size -= 1;
        self.try_free(i);
        true
    }

    pub fn has(&self, key: &Value) -> bool {
        self.lookup(key).1.is_some()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn allocated_entries(&self) -> usize {
        self.entries.len() - self.free.len()
    }

    pub fn hash_table_len(&self) -> usize {
        self.hash_table.len()
    }

    pub fn new_iter(&self) -> OrderedMapIter {
        OrderedMapIter::new()
    }

    pub fn iter_next(&mut self, iter: &mut OrderedMapIter) -> Option<(Value, Value)> {
        if !iter.open {
            return None;
        }
        let mut cur = iter.cur;
        while let Some(c) = cur {
            let e = &self.entries[c as usize];
            if e.live {
                break;
            }
            cur = e.iter_prev;
        }
        let next = match cur {
            Some(c) => self.entries[c as usize].iter_next,
            None => self.iter_first,
        };
        let old = iter.cur;
        let res = match next {
            None => {
                iter.open = false;
                iter.cur = None;
                None
            }
            Some(n) => {
                let e = &mut self.entries[n as usize];
                e.pins += 1;
                iter.cur = Some(n);
                Some((e.key.clone(), e.value.clone()))
            }
        };
        if let Some(o) = old {
            self.entries[o as usize].pins -= 1;
            self.try_free(o);
        }
        res
    }

    pub fn iter_close(&mut self, iter: &mut OrderedMapIter) {
        if let Some(o) = iter.cur.take() {
            self.entries[o as usize].pins -= 1;
            self.try_free(o);
        }
        iter.open = false;
    }

    pub fn entries(&self) -> OrderedMapEntries<'_> {
        OrderedMapEntries {
            m: self,
            cur: self.iter_first,
        }
    }

    pub fn clear(&mut self) {
        let mut cur = self.iter_first;
        let mut order = Vec::with_capacity(self.size);
        while let Some(i) = cur {
            order.push(i);
            let e = &mut self.entries[i as usize];
            cur = e.iter_next;
            e.key = Value::Empty;
            e.value = Value::Empty;
            e.live = false;
            e.h_next = None;
            e.iter_next = None;
        }
        for &i in &order {
            if let Some(p) = self.entries[i as usize].iter_prev {
                self.entries[p as usize].pins += 1;
            }
        }
        for &i in order.iter().rev() {
            self.try_free(i);
        }
        self.iter_first = None;
        self.iter_last = None;
        self.hash_table = FxHashMap::default();
        self.size = 0;
    }

    pub fn trace(&self, t: &mut Tracer) {
        for e in &self.entries {
            t.value(&e.key);
            t.value(&e.value);
        }
    }
}

pub struct OrderedMapEntries<'a> {
    m: &'a OrderedMap,
    cur: Option<u32>,
}

impl<'a> Iterator for OrderedMapEntries<'a> {
    type Item = (&'a Value, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        let i = self.cur?;
        let e = &self.m.entries[i as usize];
        self.cur = e.iter_next;
        Some((&e.key, &e.value))
    }
}
