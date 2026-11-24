use crate::pagetree::{Tree, UpsertOut};

use super::engine::IndexView;
use super::indexkey::{INDEX_BITMAP, seq_key};
use super::trigram::{
    Bitset, POSTING_CHUNK, POSTING_CHUNK_BITS, Posting, posting_add, posting_each, posting_remove,
};

#[doc(hidden)]
pub fn bitmap_key(value: &[u8], chunk: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + 4);
    out.extend_from_slice(value);
    out.extend_from_slice(&chunk.to_be_bytes());
    out
}

fn bitmap_value(key: &[u8]) -> &[u8] {
    &key[..key.len() - 4]
}

fn bitmap_chunk(key: &[u8]) -> u32 {
    let t = &key[key.len() - 4..];
    u32::from_be_bytes([t[0], t[1], t[2], t[3]])
}

#[doc(hidden)]
pub fn posting_count(c: &[u8]) -> usize {
    if c.is_empty() {
        return 0;
    }

    if c[0] != 1 {
        return (c.len() - 1) / 2;
    }

    let mut n = 0;
    let mut i = 1;
    while i + 8 <= c.len() {
        let mut b = [0u8; 8];
        b.copy_from_slice(&c[i..i + 8]);
        n += u64::from_le_bytes(b).count_ones() as usize;
        i += 8;
    }

    n
}

pub(crate) fn update_bitmap(
    mut tree: Tree,
    seq: u32,
    removed: &[Vec<u8>],
    added: &[Vec<u8>],
) -> Tree {
    let chunk = seq >> POSTING_CHUNK_BITS;
    let off = (seq & (POSTING_CHUNK as u32 - 1)) as u16;

    for v in removed {
        let key = bitmap_key(v, chunk);
        let next = {
            let Some(cur) = tree.get(&key) else {
                continue;
            };
            match posting_remove(&cur, off) {
                Posting::Empty => None,
                Posting::Same => Some(cur.to_vec()),
                Posting::New(n) => Some(n),
            }
        };
        let Some(next) = next else {
            tree = tree.delete(&key).0;
            continue;
        };

        tree = tree.update(&key, move |_| UpsertOut::Replace(next)).0;
    }

    for v in added {
        tree = tree.upsert(&bitmap_key(v, chunk), |c| match c {
            None => match posting_add(&[], off) {
                Posting::New(n) => UpsertOut::Replace(n),
                _ => UpsertOut::Replace(Vec::new()),
            },
            Some(c) => match posting_add(c, off) {
                Posting::New(n) => UpsertOut::Replace(n),
                _ => UpsertOut::InPlace,
            },
        });
    }

    tree
}

pub type Keep<'a> = &'a mut dyn FnMut(&[u8]) -> bool;

fn below(value: &[u8], tail: [u8; 4], p: &[u8]) -> bool {
    let n = value.len().min(p.len());
    match value[..n].cmp(&p[..n]) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => {
            let rest = &p[n..];
            if value.len() > n {
                return false;
            }
            let m = rest.len().min(4);
            match tail[..m].cmp(&rest[..m]) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => 4 < rest.len(),
            }
        }
    }
}

fn joined(value: &[u8], tail: [u8; 4]) -> Vec<u8> {
    let mut k = Vec::with_capacity(value.len() + 4);
    k.extend_from_slice(value);
    k.extend_from_slice(&tail);
    k
}

impl IndexView {
    pub(crate) fn bitmap(&self) -> bool {
        self.idx.spec.kind == INDEX_BITMAP
    }

    pub(crate) fn bitmap_len(&self) -> i64 {
        let mut n = 0usize;
        self.idx.tree.ascend_at(0, |_, c| {
            n += posting_count(c);
            true
        });
        n as i64
    }

    pub(crate) fn bitmap_rank(&self, p: &[u8]) -> i64 {
        let mut n = 0usize;
        self.idx.tree.ascend_at(0, |k, c| {
            if k >= p {
                return false;
            }

            let value = bitmap_value(k);
            if !below(value, [0; 4], p) {
                posting_each(c, bitmap_chunk(k) << POSTING_CHUNK_BITS, |seq| {
                    if below(value, seq_key(seq), p) {
                        n += 1;
                    }
                });
                return true;
            }

            n += posting_count(c);
            true
        });

        n as i64
    }

    pub(crate) fn bitmap_scan(
        &self,
        lo: Option<&[u8]>,
        hi: Option<&[u8]>,
        mut f: impl FnMut(&[u8], u32) -> bool,
    ) {
        let l = lo.unwrap_or_default();
        let start: &[u8] = if l.len() > 4 { &l[..l.len() - 4] } else { b"" };

        self.idx.tree.ascend_from(start, |k, c| {
            let value = bitmap_value(k);
            if let Some(hi) = hi
                && !below(value, [0; 4], hi)
            {
                return false;
            }

            let mut more = true;
            posting_each(c, bitmap_chunk(k) << POSTING_CHUNK_BITS, |seq| {
                if !more {
                    return;
                }

                let key = joined(value, seq_key(seq));
                if key.as_slice() < l {
                    return;
                }

                if let Some(hi) = hi
                    && key.as_slice() >= hi
                {
                    more = false;
                    return;
                }

                more = f(&key, seq);
            });

            more
        });
    }

    pub fn collect(
        &self,
        lo: Option<&[u8]>,
        hi: Option<&[u8]>,
        set: &mut Bitset,
        mut keep: Option<Keep<'_>>,
    ) {
        if !self.bitmap() {
            self.scan_range(lo, hi, |key, seq| {
                let take = match keep.as_mut() {
                    None => true,
                    Some(k) => k(key),
                };
                if take {
                    set.set(seq);
                }
                true
            });
            return;
        }

        let l = lo.unwrap_or_default();
        let start: &[u8] = if l.len() > 4 { &l[..l.len() - 4] } else { b"" };

        let mut last_value: Vec<u8> = Vec::new();
        let mut seen = false;
        let mut keep_value = false;
        self.idx.tree.ascend_from(start, |k, c| {
            let value = bitmap_value(k);
            if let Some(hi) = hi
                && !below(value, [0; 4], hi)
            {
                return false;
            }

            let whole = value >= l
                && match hi {
                    None => true,
                    Some(hi) => below(value, [0xff; 4], hi),
                };
            if !whole {
                let base = bitmap_chunk(k) << POSTING_CHUNK_BITS;
                posting_each(c, base, |seq| {
                    let key = joined(value, seq_key(seq));
                    let inside = key.as_slice() >= l
                        && match hi {
                            None => true,
                            Some(hi) => key.as_slice() < hi,
                        };
                    if !inside {
                        return;
                    }
                    let take = match keep.as_mut() {
                        None => true,
                        Some(kf) => kf(&key),
                    };
                    if take {
                        set.set(seq);
                    }
                });
                return true;
            }

            if let Some(kf) = keep.as_mut() {
                if !seen || value != last_value.as_slice() {
                    seen = true;
                    last_value = value.to_vec();
                    keep_value = kf(&joined(value, [0; 4]));
                }
                if !keep_value {
                    return true;
                }
            }

            set.add_container(bitmap_chunk(k), c);
            true
        });
    }

    pub fn distinct_values(&self, limit: i64) -> i64 {
        let mut n = 0i64;
        let mut last: Vec<u8> = Vec::new();
        self.idx.tree.ascend_at(0, |k, _| {
            let value = bitmap_value(k);
            if n > 0 && value == last.as_slice() {
                return true;
            }
            last = value.to_vec();
            n += 1;
            n <= limit
        });

        n
    }
}
