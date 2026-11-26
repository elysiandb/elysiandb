use std::error::Error as StdError;
use std::fmt;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use arc_swap::{ArcSwapOption, Guard};

use crate::gocompat::errors::{self, BoxError};

pub(super) const MAX_ENTRIES: usize = 64;
pub(super) const MIN_ENTRIES: usize = 16;
pub(super) const MAX_BYTES: usize = 8 << 10;

const MAX_VARINT_LEN64: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorruptPage;

impl fmt::Display for CorruptPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pagetree: corrupt page")
    }
}

impl StdError for CorruptPage {}

pub const ERR_CORRUPT_PAGE: CorruptPage = CorruptPage;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

struct TokenInner {
    id: u64,
    frozen: AtomicBool,
}

#[derive(Clone)]
pub struct Token(Arc<TokenInner>);

impl Token {
    pub fn new() -> Token {
        Token(Arc::new(TokenInner {
            id: NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
            frozen: AtomicBool::new(false),
        }))
    }

    pub fn freeze(&self) {
        self.0.frozen.store(true, Ordering::SeqCst);
    }

    pub(super) fn active(&self) -> bool {
        !self.0.frozen.load(Ordering::SeqCst)
    }

    pub(super) fn id(&self) -> u64 {
        self.0.id
    }
}

impl Default for Token {
    fn default() -> Token {
        Token::new()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("id", &self.0.id)
            .field("frozen", &!self.active())
            .finish()
    }
}

pub struct Ref {
    id: AtomicU64,
    size: AtomicU32,
    pub(super) hot: AtomicBool,
    pub(super) mem: ArcSwapOption<Page>,
}

impl Ref {
    pub fn take_hot(&self) -> bool {
        self.hot.swap(false, Ordering::Relaxed)
    }

    pub fn new(p: Arc<Page>) -> Arc<Ref> {
        Arc::new(Ref {
            id: AtomicU64::new(0),
            size: AtomicU32::new(0),
            hot: AtomicBool::new(false),
            mem: ArcSwapOption::new(Some(p)),
        })
    }

    pub fn stored(id: u64, size: u32) -> Arc<Ref> {
        Arc::new(Ref {
            id: AtomicU64::new(id),
            size: AtomicU32::new(size),
            hot: AtomicBool::new(false),
            mem: ArcSwapOption::empty(),
        })
    }

    pub(super) fn empty() -> Arc<Ref> {
        Ref::stored(0, 0)
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::SeqCst)
    }

    pub fn size(&self) -> u32 {
        self.size.load(Ordering::SeqCst)
    }

    pub fn resident(&self) -> Option<Arc<Page>> {
        self.mem.load_full()
    }

    pub fn persisted(&self, id: u64, size: u32) {
        self.size.store(size, Ordering::SeqCst);
        self.id.store(id, Ordering::SeqCst);
    }

    pub fn attach(&self, p: Arc<Page>) -> Arc<Page> {
        let prev = self
            .mem
            .compare_and_swap(ptr::null::<Page>(), Some(p.clone()));
        match &*prev {
            None => p,
            Some(cur) => cur.clone(),
        }
    }

    pub fn evict(&self) -> bool {
        if self.id() == 0 {
            return false;
        }

        self.mem.swap(None).is_some()
    }

    pub fn replace(&self, old: &Arc<Page>, next: Arc<Page>) -> bool {
        let current = Arc::as_ptr(old);
        let prev = self.mem.compare_and_swap(current, Some(next));
        match &*prev {
            Some(p) => ptr::eq(Arc::as_ptr(p), current),
            None => false,
        }
    }

    pub(super) fn load(&self) -> Guard<Option<Arc<Page>>> {
        self.mem.load()
    }
}

impl fmt::Debug for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ref")
            .field("id", &self.id())
            .field("size", &self.size())
            .field("resident", &self.mem.load().is_some())
            .finish()
    }
}

pub struct Page {
    pub(super) leaf: bool,
    pub(super) owner: u64,
    pub(super) keys: Vec<u8>,
    pub(super) koff: Vec<u32>,
    pub(super) vals: Vec<u8>,
    pub(super) voff: Vec<u32>,
    pub(super) kids: Vec<Arc<Ref>>,
    pub(super) sizes: Vec<u64>,
    pub(super) total: u64,
}

impl Page {
    pub(super) fn blank(leaf: bool, owner: u64) -> Page {
        Page {
            leaf,
            owner,
            keys: Vec::new(),
            koff: Vec::new(),
            vals: Vec::new(),
            voff: Vec::new(),
            kids: Vec::new(),
            sizes: Vec::new(),
            total: 0,
        }
    }

    pub(super) fn internal(owner: u64) -> Page {
        let mut p = Page::blank(false, owner);
        p.koff = vec![0];
        p
    }

    pub(super) fn empty_leaf(owner: u64) -> Page {
        let mut p = Page::blank(true, owner);
        p.koff = vec![0];
        p.voff = vec![0];
        p
    }

    pub fn leaf(&self) -> bool {
        self.leaf
    }

    pub fn len(&self) -> usize {
        self.koff.len().wrapping_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn kids(&self) -> &[Arc<Ref>] {
        &self.kids
    }

    pub fn bytes(&self) -> i64 {
        (self.keys.capacity()
            + self.vals.capacity()
            + 4 * (self.koff.capacity() + self.voff.capacity())
            + 16 * self.kids.capacity()
            + 8 * self.sizes.capacity()
            + 72 * self.kids.len()
            + 160) as i64
    }

    pub fn compact(&self) -> Page {
        let mut c = Page::blank(self.leaf, self.owner);
        c.total = self.total;
        c.keys = exact(&self.keys);
        c.koff = exact(&self.koff);
        if self.leaf {
            c.vals = exact(&self.vals);
            c.voff = exact(&self.voff);
            return c;
        }

        c.kids = exact(&self.kids);
        c.sizes = exact(&self.sizes);

        c
    }

    pub(super) fn key(&self, i: usize) -> &[u8] {
        &self.keys[self.koff[i] as usize..self.koff[i + 1] as usize]
    }

    pub(super) fn val(&self, i: usize) -> &[u8] {
        &self.vals[self.voff[i] as usize..self.voff[i + 1] as usize]
    }

    pub(super) fn val_mut(&mut self, i: usize) -> &mut [u8] {
        let (start, end) = (self.voff[i] as usize, self.voff[i + 1] as usize);
        &mut self.vals[start..end]
    }

    pub(super) fn key_range(&self, i: usize) -> (u32, u32) {
        (self.koff[i], self.koff[i + 1])
    }

    pub(super) fn val_range(&self, i: usize) -> (u32, u32) {
        (self.voff[i], self.voff[i + 1])
    }

    pub(super) fn min_key(&self) -> &[u8] {
        self.key(0)
    }

    pub(super) fn search(&self, key: &[u8]) -> (usize, bool) {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let m = (lo + hi) >> 1;
            if compare_keys(self.key(m), key).is_lt() {
                lo = m + 1;
            } else {
                hi = m;
            }
        }

        (lo, lo < self.len() && self.key(lo) == key)
    }

    pub(super) fn child_index(&self, key: &[u8]) -> usize {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let m = (lo + hi) >> 1;
            if compare_keys(self.key(m), key).is_gt() {
                hi = m;
            } else {
                lo = m + 1;
            }
        }

        if lo == 0 {
            return 0;
        }

        lo - 1
    }

    pub(super) fn clone_owned(&self, owner: u64) -> Page {
        let mut c = Page::blank(self.leaf, owner);
        c.total = self.total;
        c.keys = spare(&self.keys, 64);
        c.koff = spare(&self.koff, 2);
        if self.leaf {
            c.vals = spare(&self.vals, 64);
            c.voff = spare(&self.voff, 2);
            return c;
        }

        c.kids = spare(&self.kids, 2);
        c.sizes = spare(&self.sizes, 2);

        c
    }

    pub(super) fn insert_key(&mut self, i: usize, k: &[u8]) {
        insert_bytes(&mut self.keys, self.koff[i] as usize, k);
        insert_offset(&mut self.koff, i, k.len());
    }

    pub(super) fn remove_key(&mut self, i: usize) {
        let (at, end) = (self.koff[i] as usize, self.koff[i + 1] as usize);
        remove_bytes(&mut self.keys, at, end - at);
        remove_offset(&mut self.koff, i);
    }

    pub(super) fn set_key(&mut self, i: usize, k: &[u8]) {
        if self.key(i) == k {
            return;
        }

        self.remove_key(i);
        self.insert_key(i, k);
    }

    pub(super) fn insert_val(&mut self, i: usize, v: &[u8]) {
        insert_bytes(&mut self.vals, self.voff[i] as usize, v);
        insert_offset(&mut self.voff, i, v.len());
    }

    pub(super) fn remove_val(&mut self, i: usize) {
        let (at, end) = (self.voff[i] as usize, self.voff[i + 1] as usize);
        remove_bytes(&mut self.vals, at, end - at);
        remove_offset(&mut self.voff, i);
    }

    pub(super) fn set_val(&mut self, i: usize, v: &[u8]) {
        let cur = self.val_mut(i);
        if cur.len() == v.len() {
            cur.copy_from_slice(v);
            return;
        }

        self.remove_val(i);
        self.insert_val(i, v);
    }

    pub(super) fn full(&self) -> bool {
        self.len() > MAX_ENTRIES
            || (self.keys.len() + self.vals.len() > MAX_BYTES && self.len() > 2)
    }

    pub(super) fn split(&mut self, owner: u64) -> Page {
        let mid = self.len() / 2;
        let mut right = Page::blank(self.leaf, owner);

        let kmid = self.koff[mid];
        right.keys = self.keys.split_off(kmid as usize);
        right.koff = Vec::with_capacity(self.len() - mid + 1);
        for &o in &self.koff[mid..] {
            right.koff.push(o - kmid);
        }
        self.koff.truncate(mid + 1);

        if self.leaf {
            let vmid = self.voff[mid];
            right.vals = self.vals.split_off(vmid as usize);
            right.voff = Vec::with_capacity(self.voff.len() - mid);
            for &o in &self.voff[mid..] {
                right.voff.push(o - vmid);
            }
            self.voff.truncate(mid + 1);
            right.total = right.len() as u64;
            self.total = self.len() as u64;

            return right;
        }

        right.kids = self.kids.split_off(mid);
        right.sizes = self.sizes.split_off(mid);
        self.total = self.sizes.iter().sum();
        right.total = right.sizes.iter().sum();

        right
    }
}

fn compare_keys(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    if a.len() == 4 && b.len() == 4 {
        let x = u32::from_be_bytes([a[0], a[1], a[2], a[3]]);
        let y = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        return x.cmp(&y);
    }

    a.cmp(b)
}

fn exact<T: Clone>(s: &[T]) -> Vec<T> {
    let mut v = Vec::with_capacity(s.len());
    v.extend_from_slice(s);
    v
}

fn spare<T: Clone>(s: &[T], extra: usize) -> Vec<T> {
    let mut v = Vec::with_capacity(s.len() + extra);
    v.extend_from_slice(s);
    v
}

fn insert_bytes(b: &mut Vec<u8>, at: usize, x: &[u8]) {
    let old = b.len();
    b.extend_from_slice(x);
    b.copy_within(at..old, at + x.len());
    b[at..at + x.len()].copy_from_slice(x);
}

fn remove_bytes(b: &mut Vec<u8>, at: usize, n: usize) {
    let len = b.len();
    b.copy_within(at + n..len, at);
    b.truncate(len - n);
}

fn insert_offset(off: &mut Vec<u32>, i: usize, n: usize) {
    let v = off[i];
    off.insert(i + 1, v);
    for o in &mut off[i + 1..] {
        *o = o.wrapping_add(n as u32);
    }
}

fn remove_offset(off: &mut Vec<u32>, i: usize) {
    let n = off[i + 1] - off[i];
    off.remove(i + 1);
    for o in &mut off[i + 1..] {
        *o = o.wrapping_sub(n);
    }
}

fn append_uvarint(dst: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        dst.push(x as u8 | 0x80);
        x >>= 7;
    }
    dst.push(x as u8);
}

fn uvarint(buf: &[u8]) -> (u64, isize) {
    let mut x: u64 = 0;
    let mut s: u32 = 0;
    for (i, &b) in buf.iter().enumerate() {
        if i == MAX_VARINT_LEN64 {
            return (0, -(i as isize + 1));
        }
        if b < 0x80 {
            if i == MAX_VARINT_LEN64 - 1 && b > 1 {
                return (0, -(i as isize + 1));
            }
            return (x | u64::from(b) << s, i as isize + 1);
        }
        x |= u64::from(b & 0x7f) << s;
        s += 7;
    }

    (0, 0)
}

pub fn encode(mut dst: Vec<u8>, p: &Page) -> Result<Vec<u8>, BoxError> {
    dst.push(if p.leaf { 1 } else { 2 });
    append_uvarint(&mut dst, p.len() as u64);

    for i in 0..p.len() {
        let k = p.key(i);
        append_uvarint(&mut dst, k.len() as u64);
        dst.extend_from_slice(k);

        if p.leaf {
            let v = p.val(i);
            append_uvarint(&mut dst, v.len() as u64);
            dst.extend_from_slice(v);
            continue;
        }

        let id = p.kids[i].id();
        if id == 0 {
            return Err(errors::new("pagetree: a child page is not written yet"));
        }
        dst.extend_from_slice(&id.to_le_bytes());
        append_uvarint(&mut dst, u64::from(p.kids[i].size()));
        append_uvarint(&mut dst, p.sizes[i]);
    }

    Ok(dst)
}

fn payload_sizes(b: &[u8], mut pos: usize, n: u64, leaf: bool) -> Option<(usize, usize)> {
    let (mut keys, mut vals) = (0usize, 0usize);
    let skip = |pos: &mut usize| -> Option<usize> {
        let (v, w) = uvarint(&b[*pos..]);
        if w <= 0 || ((b.len() - *pos - w as usize) as u64) < v {
            return None;
        }
        *pos += w as usize + v as usize;
        Some(v as usize)
    };

    for _ in 0..n {
        keys += skip(&mut pos)?;

        if leaf {
            vals += skip(&mut pos)?;
            continue;
        }

        if b.len() - pos < 8 {
            return None;
        }
        pos += 8;
        for _ in 0..2 {
            let (_, w) = uvarint(&b[pos..]);
            if w <= 0 {
                return None;
            }
            pos += w as usize;
        }
    }

    Some((keys, vals))
}

pub fn decode(b: &[u8]) -> Result<Page, CorruptPage> {
    if b.len() < 2 || (b[0] != 1 && b[0] != 2) {
        return Err(ERR_CORRUPT_PAGE);
    }

    let mut p = Page::blank(b[0] == 1, 0);
    let mut pos = 1usize;
    let uv = |pos: &mut usize| -> Option<u64> {
        let (v, n) = uvarint(&b[*pos..]);
        if n <= 0 {
            return None;
        }
        *pos += n as usize;
        Some(v)
    };

    let n = match uv(&mut pos) {
        Some(n) if n <= 1 << 16 => n,
        _ => return Err(ERR_CORRUPT_PAGE),
    };

    let Some((key_bytes, val_bytes)) = payload_sizes(b, pos, n, p.leaf) else {
        return Err(ERR_CORRUPT_PAGE);
    };

    let n = n as usize;
    p.koff = Vec::with_capacity(n + 1);
    p.koff.push(0);
    p.keys = Vec::with_capacity(key_bytes);
    if p.leaf {
        p.voff = Vec::with_capacity(n + 1);
        p.voff.push(0);
        p.vals = Vec::with_capacity(val_bytes);
    } else {
        p.kids = Vec::with_capacity(n);
        p.sizes = Vec::with_capacity(n);
    }

    for _ in 0..n {
        let kl = match uv(&mut pos) {
            Some(kl) if ((b.len() - pos) as u64) >= kl => kl as usize,
            _ => return Err(ERR_CORRUPT_PAGE),
        };
        p.keys.extend_from_slice(&b[pos..pos + kl]);
        p.koff.push(p.keys.len() as u32);
        pos += kl;

        if p.leaf {
            let vl = match uv(&mut pos) {
                Some(vl) if ((b.len() - pos) as u64) >= vl => vl as usize,
                _ => return Err(ERR_CORRUPT_PAGE),
            };
            p.vals.extend_from_slice(&b[pos..pos + vl]);
            p.voff.push(p.vals.len() as u32);
            pos += vl;
            continue;
        }

        if b.len() - pos < 8 {
            return Err(ERR_CORRUPT_PAGE);
        }
        let mut id = [0u8; 8];
        id.copy_from_slice(&b[pos..pos + 8]);
        let id = u64::from_le_bytes(id);
        pos += 8;
        let sz = uv(&mut pos);
        let total = uv(&mut pos);
        let (Some(sz), Some(total)) = (sz, total) else {
            return Err(ERR_CORRUPT_PAGE);
        };
        if id == 0 {
            return Err(ERR_CORRUPT_PAGE);
        }
        p.kids.push(Ref::stored(id, sz as u32));
        p.sizes.push(total);
    }

    if pos != b.len() {
        return Err(ERR_CORRUPT_PAGE);
    }

    if p.leaf {
        p.total = n as u64;
    } else {
        p.total = p.sizes.iter().fold(0u64, |a, &s| a.wrapping_add(s));
    }

    Ok(p)
}
