use std::collections::BTreeSet;

use crate::docpath;
use crate::gocompat::json::Value;
use crate::gocompat::sort as gosort;
use crate::gocompat::strings;
use crate::pagetree::{Tree, UpsertOut};

use super::engine::Engine;
use super::indexkey::{INDEX_SEARCH, IndexSpec};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitset {
    words: Vec<u64>,
}

impl Bitset {
    pub fn new(capacity: u32) -> Bitset {
        Bitset {
            words: vec![0; capacity.div_ceil(64) as usize],
        }
    }

    pub fn set(&mut self, i: u32) {
        let w = (i / 64) as usize;
        if w >= self.words.len() {
            let mut grown = vec![0u64; w + 1 + self.words.len() / 2];
            grown[..self.words.len()].copy_from_slice(&self.words);
            self.words = grown;
        }

        self.words[w] |= 1 << (i % 64);
    }

    #[doc(hidden)]
    pub fn add_container(&mut self, chunk: u32, c: &[u8]) {
        if c.is_empty() {
            return;
        }

        if c[0] != 1 {
            posting_each(c, chunk << POSTING_CHUNK_BITS, |seq| self.set(seq));
            return;
        }

        let per = POSTING_CHUNK / 64;
        let first = chunk as usize * per;
        let need = first + per;
        if need > self.words.len() {
            let mut grown = vec![0u64; need + self.words.len() / 2];
            grown[..self.words.len()].copy_from_slice(&self.words);
            self.words = grown;
        }

        for w in 0..per {
            let at = 1 + 8 * w;
            let mut b = [0u8; 8];
            b.copy_from_slice(&c[at..at + 8]);
            self.words[first + w] |= u64::from_le_bytes(b);
        }
    }

    pub fn has(&self, i: u32) -> bool {
        let w = (i / 64) as usize;
        w < self.words.len() && self.words[w] & (1 << (i % 64)) != 0
    }

    pub fn and(&mut self, o: &Bitset) {
        for (i, w) in self.words.iter_mut().enumerate() {
            if i < o.words.len() {
                *w &= o.words[i];
            } else {
                *w = 0;
            }
        }
    }

    pub fn and_not(&mut self, o: &Bitset) {
        for (i, w) in self.words.iter_mut().enumerate() {
            if i < o.words.len() {
                *w &= !o.words[i];
            }
        }
    }

    pub fn or(&mut self, o: &Bitset) {
        if o.words.len() > self.words.len() {
            self.words.resize(o.words.len(), 0);
        }

        for (i, w) in o.words.iter().enumerate() {
            self.words[i] |= w;
        }
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    pub fn for_each(&self, mut f: impl FnMut(u32) -> bool) {
        for (wi, &w) in self.words.iter().enumerate() {
            let mut w = w;
            while w != 0 {
                let t = w.trailing_zeros();
                if !f((wi * 64) as u32 + t) {
                    return;
                }
                w &= w - 1;
            }
        }
    }
}

fn walk_strings<'a>(doc: Option<&'a Value>, mut f: impl FnMut(&'a str)) {
    let Some(doc) = doc else {
        return;
    };

    let mut stack: Vec<&'a Value> = vec![doc];
    while let Some(v) = stack.pop() {
        match v {
            Value::Object(m) => {
                for (k, child) in m.iter().rev() {
                    if docpath::is_searchable_key(k.as_bytes()) {
                        stack.push(child);
                    }
                }
            }
            Value::Array(items) => {
                for item in items.iter().rev() {
                    stack.push(item);
                }
            }
            Value::String(s) => f(s),
            _ => {}
        }
    }
}

pub fn document_trigrams(doc: Option<&Value>) -> BTreeSet<Vec<u8>> {
    let mut out = BTreeSet::new();
    walk_strings(doc, |s| {
        let lower = strings::to_lower(s.as_bytes());
        for w in lower.windows(3) {
            out.insert(w.to_vec());
        }
    });
    out
}

pub fn pattern_trigrams(pattern: &[u8]) -> Vec<Vec<u8>> {
    let lower = strings::to_lower(pattern);
    let mut set = BTreeSet::new();
    for run in lower.split(|&c| c == b'*') {
        if run.contains(&b'\\') {
            continue;
        }

        for w in run.windows(3) {
            set.insert(w.to_vec());
        }
    }

    set.into_iter().collect()
}

pub const STALE_SEARCH_SLACK: i64 = 1024;
pub const POSTING_CHUNK_BITS: u32 = 12;
pub const POSTING_CHUNK: usize = 1 << POSTING_CHUNK_BITS;
pub const POSTING_ARRAY_MAX: usize = 256;
pub const POSTING_BITMAP: usize = POSTING_CHUNK / 8;

pub fn search_spec() -> IndexSpec {
    IndexSpec::new(INDEX_SEARCH, Vec::new())
}

fn lower_ascii(t: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if t.iter().any(|&c| c >= 0x80 || c.is_ascii_uppercase()) {
        return std::borrow::Cow::Owned(strings::to_lower(t));
    }

    std::borrow::Cow::Borrowed(t)
}

fn gram_of(s: &[u8], i: usize) -> u32 {
    (u32::from(s[i]) << 16) | (u32::from(s[i + 1]) << 8) | u32::from(s[i + 2])
}

#[doc(hidden)]
pub fn search_grams(doc: Option<&Value>) -> Vec<u32> {
    let mut out = Vec::new();
    walk_strings(doc, |s| {
        let lower = lower_ascii(s.as_bytes());
        for i in 0..lower.len().saturating_sub(2) {
            out.push(gram_of(&lower, i));
        }
    });

    out.sort_unstable();
    out.dedup();
    out
}

pub(crate) fn diff_grams(old: &[u32], new: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if j >= new.len() || (i < old.len() && old[i] < new[j]) {
            removed.push(old[i]);
            i += 1;
        } else if i >= old.len() || new[j] < old[i] {
            added.push(new[j]);
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }

    (removed, added)
}

#[doc(hidden)]
pub fn posting_key(gram: u32, chunk: u32) -> [u8; 7] {
    let c = chunk.to_be_bytes();
    [
        (gram >> 16) as u8,
        (gram >> 8) as u8,
        gram as u8,
        c[0],
        c[1],
        c[2],
        c[3],
    ]
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Posting {
    Same,
    Empty,
    New(Vec<u8>),
}

impl Posting {
    pub fn resolve(self, c: &[u8]) -> Option<Vec<u8>> {
        match self {
            Posting::Same => Some(c.to_vec()),
            Posting::Empty => None,
            Posting::New(v) => Some(v),
        }
    }
}

fn array_get(c: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([c[1 + 2 * i], c[2 + 2 * i]])
}

fn array_search(c: &[u8], n: usize, off: u16) -> usize {
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if array_get(c, mid) >= off {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

#[doc(hidden)]
pub fn posting_add(c: &[u8], off: u16) -> Posting {
    if c.is_empty() {
        return Posting::New(vec![0, off as u8, (off >> 8) as u8]);
    }

    if c[0] == 1 {
        let mut out = c.to_vec();
        out[1 + (off / 8) as usize] |= 1 << (off % 8);
        return Posting::New(out);
    }

    let n = (c.len() - 1) / 2;
    let i = array_search(c, n, off);
    if i < n && array_get(c, i) == off {
        return Posting::Same;
    }

    if n + 1 > POSTING_ARRAY_MAX {
        let mut out = vec![0u8; 1 + POSTING_BITMAP];
        out[0] = 1;
        for j in 0..n {
            let v = array_get(c, j);
            out[1 + (v / 8) as usize] |= 1 << (v % 8);
        }
        out[1 + (off / 8) as usize] |= 1 << (off % 8);
        return Posting::New(out);
    }

    let mut out = Vec::with_capacity(c.len() + 2);
    out.extend_from_slice(&c[..1 + 2 * i]);
    out.extend_from_slice(&off.to_le_bytes());
    out.extend_from_slice(&c[1 + 2 * i..]);
    Posting::New(out)
}

#[doc(hidden)]
pub fn posting_remove(c: &[u8], off: u16) -> Posting {
    if c.is_empty() {
        return Posting::Empty;
    }

    if c[0] == 1 {
        let mut out = c.to_vec();
        out[1 + (off / 8) as usize] &= !(1 << (off % 8));
        if out[1..].iter().any(|&b| b != 0) {
            return Posting::New(out);
        }
        return Posting::Empty;
    }

    let n = (c.len() - 1) / 2;
    let i = array_search(c, n, off);
    if i >= n || array_get(c, i) != off {
        return Posting::Same;
    }

    if n == 1 {
        return Posting::Empty;
    }

    let mut out = Vec::with_capacity(c.len() - 2);
    out.extend_from_slice(&c[..1 + 2 * i]);
    out.extend_from_slice(&c[1 + 2 * i + 2..]);
    Posting::New(out)
}

#[doc(hidden)]
pub fn posting_each(c: &[u8], base: u32, mut f: impl FnMut(u32)) {
    if c.is_empty() {
        return;
    }

    if c[0] == 1 {
        for (i, &b) in c[1..].iter().enumerate() {
            let mut b = b;
            while b != 0 {
                let t = b.trailing_zeros();
                f(base.wrapping_add((i * 8) as u32 + t));
                b &= b - 1;
            }
        }
        return;
    }

    let mut j = 1;
    while j + 1 < c.len() {
        f(base.wrapping_add(u32::from(u16::from_le_bytes([c[j], c[j + 1]]))));
        j += 2;
    }
}

pub(crate) fn set_bit_in_place(c: &mut [u8], off: u16) {
    c[1 + (off / 8) as usize] |= 1 << (off % 8);
}

pub(crate) fn update_postings(mut tree: Tree, seq: u32, removed: &[u32], added: &[u32]) -> Tree {
    let chunk = seq >> POSTING_CHUNK_BITS;
    let off = (seq & (POSTING_CHUNK as u32 - 1)) as u16;

    for &g in removed {
        let key = posting_key(g, chunk);
        let empty = match tree.get(&key) {
            None => continue,
            Some(cur) => posting_remove(&cur, off) == Posting::Empty,
        };

        if empty {
            tree = tree.delete(&key).0;
            continue;
        }

        tree = tree
            .update(&key, |c| {
                if c[0] == 1 {
                    c[1 + (off / 8) as usize] &= !(1 << (off % 8));
                    return UpsertOut::InPlace;
                }
                match posting_remove(c, off) {
                    Posting::Same => UpsertOut::InPlace,
                    Posting::Empty => UpsertOut::Replace(Vec::new()),
                    Posting::New(v) => UpsertOut::Replace(v),
                }
            })
            .0;
    }

    for &g in added {
        tree = tree.upsert(&posting_key(g, chunk), |c| match c {
            None => UpsertOut::Replace(posting_add(&[], off).resolve(&[]).unwrap_or_default()),
            Some(c) => {
                if c[0] == 1 {
                    set_bit_in_place(c, off);
                    return UpsertOut::InPlace;
                }
                match posting_add(c, off) {
                    Posting::New(v) => UpsertOut::Replace(v),
                    _ => UpsertOut::InPlace,
                }
            }
        });
    }

    tree
}

fn gram_postings(tree: &Tree, gram: u32) -> Bitset {
    let mut out = Bitset::default();
    let lo = posting_key(gram, 0);
    let prefix = [lo[0], lo[1], lo[2]];
    tree.ascend_from(&lo, |key, val| {
        if key[..3] != prefix {
            return false;
        }
        out.add_container(u32::from_be_bytes([key[3], key[4], key[5], key[6]]), val);
        true
    });

    out
}

fn gram_chunks(tree: &Tree, gram: u32) -> i64 {
    let mut hi = posting_key(gram, u32::MAX).to_vec();
    hi.push(0xff);
    tree.rank(&hi) as i64 - tree.rank(&posting_key(gram, 0)) as i64
}

impl Engine {
    pub fn search_candidates(&self, name: &[u8], pattern: &[u8]) -> Option<Bitset> {
        let pattern_grams = pattern_trigrams(pattern);
        if pattern_grams.is_empty() {
            return None;
        }

        let mut grams: Vec<u32> = pattern_grams.iter().map(|g| gram_of(g, 0)).collect();

        if self.entity(name, false).is_none() {
            return Some(Bitset::default());
        }

        let spec = search_spec();
        let mut snap = self.indexed_snapshot(name);
        let mut view = snap.index(&spec);
        if view.is_none()
            && !snap.is_empty()
            && (!snap.has_index(&spec) || snap.index_dormant(&spec))
        {
            self.ensure_index(name, &spec);
            snap = self.indexed_snapshot(name);
            view = snap.index(&spec);
        }
        let view = view?;

        let tree = &view.idx.tree;
        gosort::slice(&mut grams, |a, b| {
            gram_chunks(tree, *a) < gram_chunks(tree, *b)
        });

        let mut out: Option<Bitset> = None;
        for g in grams {
            let set = gram_postings(tree, g);
            let o = match out.take() {
                None => set,
                Some(mut o) => {
                    o.and(&set);
                    o
                }
            };
            let empty = o.count() == 0;
            out = Some(o);
            if empty {
                return out;
            }
        }

        out
    }
}
