use super::deflate::MAX_MATCH_OFFSET;
use super::dict_decoder::DictDecoder;
use super::huffman_bit_writer::END_BLOCK_MARKER;
use crate::archive::{Fail, read_byte, read_full};
use std::fmt;
use std::io::{self, BufRead, BufReader, Read};
use std::sync::OnceLock;

const MAX_CODE_LEN: usize = 16;
pub(super) const MAX_NUM_LIT: usize = 286;
const MAX_NUM_DIST: usize = 30;
const NUM_CODES: usize = 19;

const HUFFMAN_CHUNK_BITS: u32 = 9;
const HUFFMAN_NUM_CHUNKS: usize = 1 << HUFFMAN_CHUNK_BITS;
const HUFFMAN_COUNT_MASK: u32 = 15;
const HUFFMAN_VALUE_SHIFT: u32 = 4;

const BUFIO_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorruptInputError(pub i64);

impl fmt::Display for CorruptInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "corrupt compressed data at byte {}", self.0)
    }
}

impl std::error::Error for CorruptInputError {}

pub(super) fn internal_error(msg: &str) -> Fail {
    Fail::Error(
        io::ErrorKind::Other,
        format!("internal error in the deflate coder: {msg}"),
    )
}

#[doc(hidden)]
#[derive(Clone)]
pub struct HuffmanDecoder {
    min: i32,
    chunks: [u32; HUFFMAN_NUM_CHUNKS],
    links: Vec<Vec<u32>>,
    link_mask: u32,
}

impl Default for HuffmanDecoder {
    fn default() -> HuffmanDecoder {
        HuffmanDecoder {
            min: 0,
            chunks: [0; HUFFMAN_NUM_CHUNKS],
            links: Vec::new(),
            link_mask: 0,
        }
    }
}

impl HuffmanDecoder {
    pub fn init(&mut self, lengths: &[i32]) -> bool {
        if self.min != 0 {
            *self = HuffmanDecoder::default();
        }

        let mut count = [0i32; MAX_CODE_LEN];
        let (mut min, mut max) = (0i32, 0i32);
        for &n in lengths {
            if n == 0 {
                continue;
            }
            if min == 0 || n < min {
                min = n;
            }
            if n > max {
                max = n;
            }
            count[n as usize] += 1;
        }

        if max == 0 {
            return true;
        }

        let mut code: i32 = 0;
        let mut nextcode = [0i32; MAX_CODE_LEN];
        for i in min..=max {
            code <<= 1;
            nextcode[i as usize] = code;
            code += count[i as usize];
        }

        if code != 1 << max && !(code == 1 && max == 1) {
            return false;
        }

        self.min = min;
        if max > HUFFMAN_CHUNK_BITS as i32 {
            let num_links = 1usize << (max as u32 - HUFFMAN_CHUNK_BITS);
            self.link_mask = (num_links - 1) as u32;

            let link = (nextcode[HUFFMAN_CHUNK_BITS as usize + 1] >> 1) as usize;
            self.links = vec![Vec::new(); HUFFMAN_NUM_CHUNKS - link];
            for j in link..HUFFMAN_NUM_CHUNKS {
                let reverse = ((j as u16).reverse_bits() >> (16 - HUFFMAN_CHUNK_BITS)) as usize;
                let off = j - link;
                self.chunks[reverse] =
                    ((off as u32) << HUFFMAN_VALUE_SHIFT) | (HUFFMAN_CHUNK_BITS + 1);
                self.links[off] = vec![0; num_links];
            }
        }

        for (i, &n) in lengths.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let code = nextcode[n as usize];
            nextcode[n as usize] += 1;
            let chunk = ((i as u32) << HUFFMAN_VALUE_SHIFT) | n as u32;
            let mut reverse = ((code as u16).reverse_bits() >> (16 - n as u32)) as usize;
            if n as u32 <= HUFFMAN_CHUNK_BITS {
                let mut off = reverse;
                while off < self.chunks.len() {
                    self.chunks[off] = chunk;
                    off += 1 << n;
                }
            } else {
                let j = reverse & (HUFFMAN_NUM_CHUNKS - 1);
                let value = (self.chunks[j] >> HUFFMAN_VALUE_SHIFT) as usize;
                let linktab = &mut self.links[value];
                reverse >>= HUFFMAN_CHUNK_BITS;
                let mut off = reverse;
                while off < linktab.len() {
                    linktab[off] = chunk;
                    off += 1 << (n as u32 - HUFFMAN_CHUNK_BITS);
                }
            }
        }

        true
    }
}

fn fixed_huffman_decoder() -> &'static HuffmanDecoder {
    static FIXED: OnceLock<HuffmanDecoder> = OnceLock::new();
    FIXED.get_or_init(|| {
        let mut bits = [0i32; 288];
        for b in bits.iter_mut().take(144) {
            *b = 8;
        }
        for b in bits.iter_mut().take(256).skip(144) {
            *b = 9;
        }
        for b in bits.iter_mut().take(280).skip(256) {
            *b = 7;
        }
        for b in bits.iter_mut().skip(280) {
            *b = 8;
        }
        let mut h = HuffmanDecoder::default();
        h.init(&bits);
        h
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    NextBlock,
    HuffmanBlock,
    CopyData,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepState {
    Init,
    Dict,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Literals {
    Fixed,
    Dynamic,
}

#[derive(Default)]
struct BitReader {
    roffset: i64,
    b: u32,
    nb: u32,
}

impl BitReader {
    fn more_bits<B: BufRead + ?Sized>(&mut self, r: &mut B) -> Result<(), Fail> {
        let c = read_byte(r).map_err(Fail::no_eof)?;
        self.roffset += 1;
        self.b |= u32::from(c) << self.nb;
        self.nb += 8;
        Ok(())
    }

    fn huff_sym<B: BufRead + ?Sized>(
        &mut self,
        r: &mut B,
        h: &HuffmanDecoder,
    ) -> Result<i32, Fail> {
        let mut n = h.min as u32;
        let (mut nb, mut b) = (self.nb, self.b);
        loop {
            while nb < n {
                let c = match read_byte(r) {
                    Ok(c) => c,
                    Err(err) => {
                        self.b = b;
                        self.nb = nb;
                        return Err(err.no_eof());
                    }
                };
                self.roffset += 1;
                b |= u32::from(c) << (nb & 31);
                nb += 8;
            }
            let mut chunk = h.chunks[(b as usize) & (HUFFMAN_NUM_CHUNKS - 1)];
            n = chunk & HUFFMAN_COUNT_MASK;
            if n > HUFFMAN_CHUNK_BITS {
                chunk = h.links[(chunk >> HUFFMAN_VALUE_SHIFT) as usize]
                    [((b >> HUFFMAN_CHUNK_BITS) & h.link_mask) as usize];
                n = chunk & HUFFMAN_COUNT_MASK;
            }
            if n <= nb {
                if n == 0 {
                    self.b = b;
                    self.nb = nb;
                    return Err(Fail::Corrupt(self.roffset));
                }
                self.b = b >> (n & 31);
                self.nb = nb - n;
                return Ok((chunk >> HUFFMAN_VALUE_SHIFT) as i32);
            }
        }
    }
}

#[doc(hidden)]
pub fn decode_symbol(input: &[u8], h: &HuffmanDecoder) -> io::Result<i32> {
    let mut r = input;
    BitReader::default()
        .huff_sym(&mut r, h)
        .map_err(|err| err.to_io())
}

const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

pub(crate) struct Decompressor {
    st: BitReader,
    h1: HuffmanDecoder,
    h2: HuffmanDecoder,
    bits: Box<[i32; MAX_NUM_LIT + MAX_NUM_DIST]>,
    codebits: [i32; NUM_CODES],
    dict: DictDecoder,
    buf: [u8; 4],
    step: Step,
    step_state: StepState,
    final_block: bool,
    err: Option<Fail>,
    to_read: (usize, usize),
    hl: Literals,
    hd_fixed: bool,
    copy_len: usize,
    copy_dist: usize,
}

impl Decompressor {
    pub(crate) fn new() -> Decompressor {
        let mut f = Decompressor {
            st: BitReader::default(),
            h1: HuffmanDecoder::default(),
            h2: HuffmanDecoder::default(),
            bits: Box::new([0; MAX_NUM_LIT + MAX_NUM_DIST]),
            codebits: [0; NUM_CODES],
            dict: DictDecoder::default(),
            buf: [0; 4],
            step: Step::NextBlock,
            step_state: StepState::Init,
            final_block: false,
            err: None,
            to_read: (0, 0),
            hl: Literals::Fixed,
            hd_fixed: false,
            copy_len: 0,
            copy_dist: 0,
        };
        f.dict.init(MAX_MATCH_OFFSET, None);
        f
    }

    pub(crate) fn reset(&mut self, dict: Option<&[u8]>) {
        self.st = BitReader::default();
        self.h1 = HuffmanDecoder::default();
        self.h2 = HuffmanDecoder::default();
        self.buf = [0; 4];
        self.step = Step::NextBlock;
        self.step_state = StepState::Init;
        self.final_block = false;
        self.err = None;
        self.to_read = (0, 0);
        self.hl = Literals::Fixed;
        self.hd_fixed = false;
        self.copy_len = 0;
        self.copy_dist = 0;
        self.dict.init(MAX_MATCH_OFFSET, dict);
    }

    pub(crate) fn read<B: BufRead + ?Sized>(
        &mut self,
        r: &mut B,
        b: &mut [u8],
    ) -> (usize, Option<Fail>) {
        loop {
            let pending = self.to_read.1 - self.to_read.0;
            if pending > 0 {
                let n = pending.min(b.len());
                let start = self.to_read.0;
                b[..n].copy_from_slice(self.dict.bytes((start, start + n)));
                self.to_read.0 += n;
                if self.to_read.0 == self.to_read.1 {
                    return (n, self.err.clone());
                }
                return (n, None);
            }
            if let Some(err) = &self.err {
                return (0, Some(err.clone()));
            }
            self.run_step(r);
            if self.err.is_some() && self.to_read.0 == self.to_read.1 {
                self.to_read = self.dict.read_flush();
            }
        }
    }

    pub(crate) fn close(&self) -> Option<Fail> {
        match &self.err {
            Some(Fail::Eof) => None,
            other => other.clone(),
        }
    }

    fn run_step<B: BufRead + ?Sized>(&mut self, r: &mut B) {
        match self.step {
            Step::NextBlock => self.next_block(r),
            Step::HuffmanBlock => self.huffman_block(r),
            Step::CopyData => self.copy_data(r),
        }
    }

    fn next_block<B: BufRead + ?Sized>(&mut self, r: &mut B) {
        while self.st.nb < 1 + 2 {
            if let Err(err) = self.st.more_bits(r) {
                self.err = Some(err);
                return;
            }
        }
        self.final_block = self.st.b & 1 == 1;
        self.st.b >>= 1;
        let typ = self.st.b & 3;
        self.st.b >>= 2;
        self.st.nb -= 1 + 2;
        match typ {
            0 => self.data_block(r),
            1 => {
                self.hl = Literals::Fixed;
                self.hd_fixed = true;
                self.huffman_block(r);
            }
            2 => {
                if let Err(err) = self.read_huffman(r) {
                    self.err = Some(err);
                    return;
                }
                self.hl = Literals::Dynamic;
                self.hd_fixed = false;
                self.huffman_block(r);
            }
            _ => self.err = Some(Fail::Corrupt(self.st.roffset)),
        }
    }

    fn read_huffman<B: BufRead + ?Sized>(&mut self, r: &mut B) -> Result<(), Fail> {
        while self.st.nb < 5 + 5 + 4 {
            self.st.more_bits(r)?;
        }
        let nlit = (self.st.b & 0x1F) as usize + 257;
        if nlit > MAX_NUM_LIT {
            return Err(Fail::Corrupt(self.st.roffset));
        }
        self.st.b >>= 5;
        let ndist = (self.st.b & 0x1F) as usize + 1;
        if ndist > MAX_NUM_DIST {
            return Err(Fail::Corrupt(self.st.roffset));
        }
        self.st.b >>= 5;
        let nclen = (self.st.b & 0xF) as usize + 4;
        self.st.b >>= 4;
        self.st.nb -= 5 + 5 + 4;

        for &order in CODE_ORDER.iter().take(nclen) {
            while self.st.nb < 3 {
                self.st.more_bits(r)?;
            }
            self.codebits[order] = (self.st.b & 0x7) as i32;
            self.st.b >>= 3;
            self.st.nb -= 3;
        }
        for &order in CODE_ORDER.iter().skip(nclen) {
            self.codebits[order] = 0;
        }
        if !self.h1.init(&self.codebits) {
            return Err(Fail::Corrupt(self.st.roffset));
        }

        let n = nlit + ndist;
        let mut i = 0;
        while i < n {
            let x = self.st.huff_sym(r, &self.h1)?;
            if x < 16 {
                self.bits[i] = x;
                i += 1;
                continue;
            }
            let (mut rep, nb, b) = match x {
                16 => {
                    if i == 0 {
                        return Err(Fail::Corrupt(self.st.roffset));
                    }
                    (3usize, 2u32, self.bits[i - 1])
                }
                17 => (3, 3, 0),
                18 => (11, 7, 0),
                _ => return Err(internal_error("unexpected length code")),
            };
            while self.st.nb < nb {
                self.st.more_bits(r)?;
            }
            rep += (self.st.b & ((1u32 << nb) - 1)) as usize;
            self.st.b >>= nb;
            self.st.nb -= nb;
            if i + rep > n {
                return Err(Fail::Corrupt(self.st.roffset));
            }
            for _ in 0..rep {
                self.bits[i] = b;
                i += 1;
            }
        }

        if !self.h1.init(&self.bits[0..nlit]) || !self.h2.init(&self.bits[nlit..nlit + ndist]) {
            return Err(Fail::Corrupt(self.st.roffset));
        }

        if self.h1.min < self.bits[END_BLOCK_MARKER] {
            self.h1.min = self.bits[END_BLOCK_MARKER];
        }

        Ok(())
    }

    fn huffman_block<B: BufRead + ?Sized>(&mut self, r: &mut B) {
        let mut copy_history = self.step_state == StepState::Dict;
        loop {
            if copy_history {
                let mut cnt = self.dict.try_write_copy(self.copy_dist, self.copy_len);
                if cnt == 0 {
                    cnt = self.dict.write_copy(self.copy_dist, self.copy_len);
                }
                self.copy_len -= cnt;

                if self.dict.avail_write() == 0 || self.copy_len > 0 {
                    self.to_read = self.dict.read_flush();
                    self.step = Step::HuffmanBlock;
                    self.step_state = StepState::Dict;
                    return;
                }
                copy_history = false;
                continue;
            }

            let hl = match self.hl {
                Literals::Fixed => fixed_huffman_decoder(),
                Literals::Dynamic => &self.h1,
            };
            let v = match self.st.huff_sym(r, hl) {
                Ok(v) => v,
                Err(err) => {
                    self.err = Some(err);
                    return;
                }
            };
            let n: u32;
            let mut length: usize;
            match v {
                0..=255 => {
                    self.dict.write_byte(v as u8);
                    if self.dict.avail_write() == 0 {
                        self.to_read = self.dict.read_flush();
                        self.step = Step::HuffmanBlock;
                        self.step_state = StepState::Init;
                        return;
                    }
                    continue;
                }
                256 => {
                    self.finish_block();
                    return;
                }
                257..=264 => {
                    length = (v - (257 - 3)) as usize;
                    n = 0;
                }
                265..=268 => {
                    length = (v * 2 - (265 * 2 - 11)) as usize;
                    n = 1;
                }
                269..=272 => {
                    length = (v * 4 - (269 * 4 - 19)) as usize;
                    n = 2;
                }
                273..=276 => {
                    length = (v * 8 - (273 * 8 - 35)) as usize;
                    n = 3;
                }
                277..=280 => {
                    length = (v * 16 - (277 * 16 - 67)) as usize;
                    n = 4;
                }
                281..=284 => {
                    length = (v * 32 - (281 * 32 - 131)) as usize;
                    n = 5;
                }
                285 => {
                    length = 258;
                    n = 0;
                }
                _ => {
                    self.err = Some(Fail::Corrupt(self.st.roffset));
                    return;
                }
            }
            if n > 0 {
                while self.st.nb < n {
                    if let Err(err) = self.st.more_bits(r) {
                        self.err = Some(err);
                        return;
                    }
                }
                length += (self.st.b & ((1u32 << n) - 1)) as usize;
                self.st.b >>= n;
                self.st.nb -= n;
            }

            let mut dist: i64;
            if self.hd_fixed {
                while self.st.nb < 5 {
                    if let Err(err) = self.st.more_bits(r) {
                        self.err = Some(err);
                        return;
                    }
                }
                dist = i64::from((((self.st.b & 0x1F) << 3) as u8).reverse_bits());
                self.st.b >>= 5;
                self.st.nb -= 5;
            } else {
                match self.st.huff_sym(r, &self.h2) {
                    Ok(d) => dist = i64::from(d),
                    Err(err) => {
                        self.err = Some(err);
                        return;
                    }
                }
            }

            if dist < 4 {
                dist += 1;
            } else if dist < MAX_NUM_DIST as i64 {
                let nb = ((dist - 2) as u32) >> 1;
                let mut extra = (dist & 1) << nb;
                while self.st.nb < nb {
                    if let Err(err) = self.st.more_bits(r) {
                        self.err = Some(err);
                        return;
                    }
                }
                extra |= i64::from(self.st.b & ((1u32 << nb) - 1));
                self.st.b >>= nb;
                self.st.nb -= nb;
                dist = (1i64 << (nb + 1)) + 1 + extra;
            } else {
                self.err = Some(Fail::Corrupt(self.st.roffset));
                return;
            }

            if dist > self.dict.hist_size() as i64 {
                self.err = Some(Fail::Corrupt(self.st.roffset));
                return;
            }

            self.copy_len = length;
            self.copy_dist = dist as usize;
            copy_history = true;
        }
    }

    fn data_block<B: BufRead + ?Sized>(&mut self, r: &mut B) {
        self.st.nb = 0;
        self.st.b = 0;

        let (nr, err) = read_full(r, &mut self.buf);
        self.st.roffset += nr as i64;
        if let Some(err) = err {
            self.err = Some(err.no_eof());
            return;
        }
        let n = usize::from(self.buf[0]) | (usize::from(self.buf[1]) << 8);
        let nn = usize::from(self.buf[2]) | (usize::from(self.buf[3]) << 8);
        if nn as u16 != !(n as u16) {
            self.err = Some(Fail::Corrupt(self.st.roffset));
            return;
        }

        if n == 0 {
            self.to_read = self.dict.read_flush();
            self.finish_block();
            return;
        }

        self.copy_len = n;
        self.copy_data(r);
    }

    fn copy_data<B: BufRead + ?Sized>(&mut self, r: &mut B) {
        let buf = self.dict.write_slice();
        let len = buf.len().min(self.copy_len);
        let (cnt, err) = read_full(r, &mut buf[..len]);
        self.st.roffset += cnt as i64;
        self.copy_len -= cnt;
        self.dict.write_mark(cnt);
        if let Some(err) = err {
            self.err = Some(err.no_eof());
            return;
        }

        if self.dict.avail_write() == 0 || self.copy_len > 0 {
            self.to_read = self.dict.read_flush();
            self.step = Step::CopyData;
            return;
        }
        self.finish_block();
    }

    fn finish_block(&mut self) {
        if self.final_block {
            if self.dict.avail_read() > 0 {
                self.to_read = self.dict.read_flush();
            }
            self.err = Some(Fail::Eof);
        }
        self.step = Step::NextBlock;
    }

    pub(crate) fn avail_write(&self) -> usize {
        self.dict.avail_write()
    }
}

pub struct Reader<B> {
    r: B,
    d: Decompressor,
}

pub fn new_reader<R: Read>(r: R) -> Reader<BufReader<R>> {
    Reader::new(BufReader::with_capacity(BUFIO_SIZE, r))
}

impl<B: BufRead> Reader<B> {
    pub fn new(r: B) -> Reader<B> {
        Reader {
            r,
            d: Decompressor::new(),
        }
    }

    pub fn reset(&mut self, r: B) {
        self.r = r;
        self.d.reset(None);
    }

    pub fn close(&mut self) -> io::Result<()> {
        match self.d.close() {
            None => Ok(()),
            Some(err) => Err(err.to_io()),
        }
    }

    pub fn get_ref(&self) -> &B {
        &self.r
    }

    pub fn get_mut(&mut self) -> &mut B {
        &mut self.r
    }

    pub fn into_inner(self) -> B {
        self.r
    }

    #[doc(hidden)]
    pub fn window_space(&self) -> usize {
        self.d.avail_write()
    }
}

impl<B: BufRead> Read for Reader<B> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (n, err) = self.d.read(&mut self.r, buf);
        if n > 0 {
            return Ok(n);
        }
        match err {
            None | Some(Fail::Eof) => Ok(0),
            Some(err) => Err(err.to_io()),
        }
    }
}
