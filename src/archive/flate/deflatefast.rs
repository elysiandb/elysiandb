use super::deflate::{
    BASE_MATCH_LENGTH, BASE_MATCH_OFFSET, MAX_MATCH_LENGTH, MAX_MATCH_OFFSET, MAX_STORE_BLOCK_SIZE,
};
use super::token::{Token, literal_token, match_token};

const TABLE_BITS: u32 = 14;
const TABLE_SIZE: usize = 1 << TABLE_BITS;
const TABLE_MASK: u32 = (TABLE_SIZE - 1) as u32;
const TABLE_SHIFT: u32 = 32 - TABLE_BITS;

#[doc(hidden)]
pub const BUFFER_RESET: i32 = i32::MAX - MAX_STORE_BLOCK_SIZE as i32 * 2;

#[doc(hidden)]
pub const INPUT_MARGIN: usize = 16 - 1;
const MIN_NON_LITERAL_BLOCK_SIZE: usize = 1 + 1 + INPUT_MARGIN;

fn load32(b: &[u8], i: i32) -> u32 {
    let i = i as usize;
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn load64(b: &[u8], i: i32) -> u64 {
    let i = i as usize;
    u64::from_le_bytes([
        b[i],
        b[i + 1],
        b[i + 2],
        b[i + 3],
        b[i + 4],
        b[i + 5],
        b[i + 6],
        b[i + 7],
    ])
}

fn hash(u: u32) -> u32 {
    u.wrapping_mul(0x1e35a7bd) >> TABLE_SHIFT
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct TableEntry {
    val: u32,
    offset: i32,
}

#[doc(hidden)]
pub struct DeflateFast {
    table: Box<[TableEntry]>,
    prev: Vec<u8>,
    cur: i32,
}

impl Default for DeflateFast {
    fn default() -> DeflateFast {
        DeflateFast::new()
    }
}

impl DeflateFast {
    pub fn new() -> DeflateFast {
        DeflateFast {
            table: vec![TableEntry::default(); TABLE_SIZE].into_boxed_slice(),
            prev: Vec::with_capacity(MAX_STORE_BLOCK_SIZE),
            cur: MAX_STORE_BLOCK_SIZE as i32,
        }
    }

    pub fn with_prev(prev: &[u8]) -> DeflateFast {
        DeflateFast {
            table: vec![TableEntry::default(); TABLE_SIZE].into_boxed_slice(),
            prev: prev.to_vec(),
            cur: 0,
        }
    }

    pub fn cur(&self) -> i32 {
        self.cur
    }

    pub fn set_cur(&mut self, cur: i32) {
        self.cur = cur;
    }

    pub fn encode(&mut self, dst: &mut Vec<Token>, src: &[u8]) {
        if self.cur >= BUFFER_RESET {
            self.shift_offsets();
        }

        if src.len() < MIN_NON_LITERAL_BLOCK_SIZE {
            self.cur += MAX_STORE_BLOCK_SIZE as i32;
            self.prev.clear();
            emit_literal(dst, src);
            return;
        }

        let s_limit = (src.len() - INPUT_MARGIN) as i32;

        let mut next_emit: i32 = 0;
        let mut s: i32 = 0;
        let mut cv = load32(src, s);
        let mut next_hash = hash(cv);
        let mut candidate: TableEntry;

        'encode: loop {
            let mut skip: i32 = 32;

            let mut next_s = s;
            loop {
                s = next_s;
                let bytes_between_hash_lookups = skip >> 5;
                next_s = s + bytes_between_hash_lookups;
                skip += bytes_between_hash_lookups;
                if next_s > s_limit {
                    break 'encode;
                }
                candidate = self.table[(next_hash & TABLE_MASK) as usize];
                let now = load32(src, next_s);
                self.table[(next_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: s + self.cur,
                    val: cv,
                };
                next_hash = hash(now);

                let offset = s - (candidate.offset - self.cur);
                if offset > MAX_MATCH_OFFSET as i32 || cv != candidate.val {
                    cv = now;
                    continue;
                }
                break;
            }

            emit_literal(dst, &src[next_emit as usize..s as usize]);

            loop {
                s += 4;
                let t = candidate.offset - self.cur + 4;
                let l = self.match_len(s, t, src);

                dst.push(match_token(
                    (l + 4 - BASE_MATCH_LENGTH as i32) as u32,
                    (s - t - BASE_MATCH_OFFSET as i32) as u32,
                ));
                s += l;
                next_emit = s;
                if s >= s_limit {
                    break 'encode;
                }

                let mut x = load64(src, s - 1);
                let prev_hash = hash(x as u32);
                self.table[(prev_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: self.cur + s - 1,
                    val: x as u32,
                };
                x >>= 8;
                let curr_hash = hash(x as u32);
                candidate = self.table[(curr_hash & TABLE_MASK) as usize];
                self.table[(curr_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: self.cur + s,
                    val: x as u32,
                };

                let offset = s - (candidate.offset - self.cur);
                if offset > MAX_MATCH_OFFSET as i32 || x as u32 != candidate.val {
                    cv = (x >> 8) as u32;
                    next_hash = hash(cv);
                    s += 1;
                    break;
                }
            }
        }

        if (next_emit as usize) < src.len() {
            emit_literal(dst, &src[next_emit as usize..]);
        }
        self.cur += src.len() as i32;
        self.prev.clear();
        self.prev.extend_from_slice(src);
    }

    pub fn match_len(&self, s: i32, t: i32, src: &[u8]) -> i32 {
        let mut s1 = s as usize + MAX_MATCH_LENGTH - 4;
        if s1 > src.len() {
            s1 = src.len();
        }

        if t >= 0 {
            let a = &src[s as usize..s1];
            let b = &src[t as usize..t as usize + a.len()];
            for i in 0..a.len() {
                if a[i] != b[i] {
                    return i as i32;
                }
            }
            return a.len() as i32;
        }

        let tp = self.prev.len() as i32 + t;
        if tp < 0 {
            return 0;
        }

        let mut a = &src[s as usize..s1];
        let mut b = &self.prev[tp as usize..];
        if b.len() > a.len() {
            b = &b[..a.len()];
        }
        a = &a[..b.len()];
        for i in 0..b.len() {
            if a[i] != b[i] {
                return i as i32;
            }
        }

        let n = b.len() as i32;
        if (s + n) as usize == s1 {
            return n;
        }

        let a = &src[(s + n) as usize..s1];
        let b = &src[..a.len()];
        for i in 0..a.len() {
            if a[i] != b[i] {
                return i as i32 + n;
            }
        }
        a.len() as i32 + n
    }

    pub fn reset(&mut self) {
        self.prev.clear();
        self.cur += MAX_MATCH_OFFSET as i32;

        if self.cur >= BUFFER_RESET {
            self.shift_offsets();
        }
    }

    pub fn shift_offsets(&mut self) {
        if self.prev.is_empty() {
            self.table.fill(TableEntry::default());
            self.cur = MAX_MATCH_OFFSET as i32 + 1;
            return;
        }

        for entry in self.table.iter_mut() {
            let mut v = entry.offset - self.cur + MAX_MATCH_OFFSET as i32 + 1;
            if v < 0 {
                v = 0;
            }
            entry.offset = v;
        }
        self.cur = MAX_MATCH_OFFSET as i32 + 1;
    }
}

fn emit_literal(dst: &mut Vec<Token>, lit: &[u8]) {
    for &v in lit {
        dst.push(literal_token(u32::from(v)));
    }
}
