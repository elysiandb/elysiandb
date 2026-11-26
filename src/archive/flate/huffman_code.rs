use super::inflate::MAX_NUM_LIT;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Hcode {
    pub(super) code: u16,
    pub(super) len: u16,
}

impl Hcode {
    fn set(&mut self, code: u16, length: u16) {
        self.len = length;
        self.code = code;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LiteralNode {
    literal: u16,
    freq: i32,
}

#[derive(Clone, Copy, Default)]
struct LevelInfo {
    level: i32,
    last_freq: i32,
    next_char_freq: i32,
    next_pair_freq: i32,
    needed: i32,
}

fn max_node() -> LiteralNode {
    LiteralNode {
        literal: u16::MAX,
        freq: i32::MAX,
    }
}

pub(super) struct HuffmanEncoder {
    pub(super) codes: Vec<Hcode>,
    freqcache: Vec<LiteralNode>,
    bit_count: [i32; 17],
}

const MAX_BITS_LIMIT: usize = 16;

impl HuffmanEncoder {
    pub(super) fn new(size: usize) -> HuffmanEncoder {
        HuffmanEncoder {
            codes: vec![Hcode::default(); size],
            freqcache: Vec::new(),
            bit_count: [0; 17],
        }
    }

    pub(super) fn bit_length(&self, freq: &[i32]) -> i64 {
        let mut total: i64 = 0;
        for (i, &f) in freq.iter().enumerate() {
            if f != 0 {
                total += i64::from(f) * i64::from(self.codes[i].len);
            }
        }
        total
    }

    pub(super) fn generate(&mut self, freq: &[i32], max_bits: i32) {
        if self.freqcache.is_empty() {
            self.freqcache = vec![LiteralNode::default(); MAX_NUM_LIT + 1];
        }
        let mut count = 0;
        for (i, &f) in freq.iter().enumerate() {
            if f != 0 {
                self.freqcache[count] = LiteralNode {
                    literal: i as u16,
                    freq: f,
                };
                count += 1;
            } else {
                self.codes[i].len = 0;
            }
        }

        if count <= 2 {
            for i in 0..count {
                let node = self.freqcache[i];
                self.codes[node.literal as usize].set(i as u16, 1);
            }
            return;
        }
        self.freqcache[..count].sort_unstable_by(|a, b| {
            if a.freq == b.freq {
                a.literal.cmp(&b.literal)
            } else {
                a.freq.cmp(&b.freq)
            }
        });

        let bits = bit_counts(
            &mut self.freqcache[..count + 1],
            max_bits,
            &mut self.bit_count,
        );
        assign_encoding_and_size(
            &mut self.codes,
            &self.bit_count[..bits],
            &mut self.freqcache[..count],
        );
    }
}

fn bit_counts(list: &mut [LiteralNode], mut max_bits: i32, bit_count: &mut [i32; 17]) -> usize {
    assert!(
        (max_bits as usize) < MAX_BITS_LIMIT,
        "the huffman code length limit is too large"
    );
    let n = (list.len() - 1) as i32;
    list[n as usize] = max_node();

    if max_bits > n - 1 {
        max_bits = n - 1;
    }

    let mut levels = [LevelInfo::default(); MAX_BITS_LIMIT];
    let mut leaf_counts = [[0i32; MAX_BITS_LIMIT]; MAX_BITS_LIMIT];

    for level in 1..=max_bits {
        let l = level as usize;
        levels[l] = LevelInfo {
            level,
            last_freq: list[1].freq,
            next_char_freq: list[2].freq,
            next_pair_freq: list[0].freq.wrapping_add(list[1].freq),
            needed: 0,
        };
        leaf_counts[l][l] = 2;
        if level == 1 {
            levels[l].next_pair_freq = i32::MAX;
        }
    }

    levels[max_bits as usize].needed = 2 * n - 4;

    let mut level = max_bits as usize;
    loop {
        if levels[level].next_pair_freq == i32::MAX && levels[level].next_char_freq == i32::MAX {
            levels[level].needed = 0;
            levels[level + 1].next_pair_freq = i32::MAX;
            level += 1;
            continue;
        }

        let prev_freq = levels[level].last_freq;
        if levels[level].next_char_freq < levels[level].next_pair_freq {
            let n = leaf_counts[level][level] + 1;
            levels[level].last_freq = levels[level].next_char_freq;
            leaf_counts[level][level] = n;
            levels[level].next_char_freq = list[n as usize].freq;
        } else {
            levels[level].last_freq = levels[level].next_pair_freq;
            let (below, above) = leaf_counts.split_at_mut(level);
            above[0][..level].copy_from_slice(&below[level - 1][..level]);
            let lower = (levels[level].level - 1) as usize;
            levels[lower].needed = 2;
        }

        levels[level].needed -= 1;
        if levels[level].needed == 0 {
            if levels[level].level == max_bits {
                break;
            }
            let next = (levels[level].level + 1) as usize;
            levels[next].next_pair_freq = prev_freq.wrapping_add(levels[level].last_freq);
            level += 1;
        } else {
            while levels[level - 1].needed > 0 {
                level -= 1;
            }
        }
    }

    let max = max_bits as usize;
    assert!(
        leaf_counts[max][max] == n,
        "the huffman leaf counts are inconsistent"
    );

    let mut bits = 1;
    let counts = &leaf_counts[max];
    let mut level = max;
    while level > 0 {
        bit_count[bits] = counts[level] - counts[level - 1];
        bits += 1;
        level -= 1;
    }
    max + 1
}

fn assign_encoding_and_size(codes: &mut [Hcode], bit_count: &[i32], list: &mut [LiteralNode]) {
    let mut code: u16 = 0;
    let mut end = list.len();
    for (n, &bits) in bit_count.iter().enumerate() {
        code <<= 1;
        if n == 0 || bits == 0 {
            continue;
        }
        let start = end - bits as usize;
        let chunk = &mut list[start..end];
        chunk.sort_unstable_by(|a, b| a.literal.cmp(&b.literal));
        for node in chunk.iter() {
            codes[node.literal as usize] = Hcode {
                code: reverse_bits(code, n as u8),
                len: n as u16,
            };
            code = code.wrapping_add(1);
        }
        end = start;
    }
}

#[doc(hidden)]
pub fn reverse_bits(number: u16, bit_length: u8) -> u16 {
    number
        .checked_shl(16 - u32::from(bit_length))
        .unwrap_or(0)
        .reverse_bits()
}
