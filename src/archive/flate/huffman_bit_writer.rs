use super::deflate::MAX_STORE_BLOCK_SIZE;
use super::huffman_code::{Hcode, HuffmanEncoder};
use super::inflate::{MAX_NUM_LIT, internal_error};
use super::token::{
    MATCH_TYPE, Token, length_code, offset_code, token_length, token_literal, token_offset,
};
use crate::archive::Fail;
use std::io::Write;
use std::sync::OnceLock;

const OFFSET_CODE_COUNT: usize = 30;
pub(super) const END_BLOCK_MARKER: usize = 256;
const LENGTH_CODES_START: usize = 257;
const CODEGEN_CODE_COUNT: usize = 19;
const BAD_CODE: u8 = 255;
const BUFFER_FLUSH_SIZE: usize = 240;
const BUFFER_SIZE: usize = BUFFER_FLUSH_SIZE + 8;

const LENGTH_EXTRA_BITS: [i8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

const LENGTH_BASE: [u32; 29] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224, 255,
];

const OFFSET_EXTRA_BITS: [i8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const OFFSET_BASE: [u32; 30] = [
    0x000000, 0x000001, 0x000002, 0x000003, 0x000004, 0x000006, 0x000008, 0x00000c, 0x000010,
    0x000018, 0x000020, 0x000030, 0x000040, 0x000060, 0x000080, 0x0000c0, 0x000100, 0x000180,
    0x000200, 0x000300, 0x000400, 0x000600, 0x000800, 0x000c00, 0x001000, 0x001800, 0x002000,
    0x003000, 0x004000, 0x006000,
];

const CODEGEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn huff_offset() -> &'static HuffmanEncoder {
    static HUFF_OFFSET: OnceLock<HuffmanEncoder> = OnceLock::new();
    HUFF_OFFSET.get_or_init(|| {
        let mut offset_freq = [0i32; OFFSET_CODE_COUNT];
        offset_freq[0] = 1;
        let mut h = HuffmanEncoder::new(OFFSET_CODE_COUNT);
        h.generate(&offset_freq, 15);
        h
    })
}

pub(crate) fn write_once<W: Write + ?Sized>(w: &mut W, b: &[u8]) -> std::io::Result<()> {
    if b.is_empty() {
        return w.write(b).map(|_| ());
    }
    w.write_all(b)
}

pub(super) struct BitSink<W> {
    pub(super) writer: W,
    bits: u64,
    nbits: u32,
    bytes: [u8; BUFFER_SIZE],
    nbytes: usize,
    pub(super) err: Option<Fail>,
}

impl<W: Write> BitSink<W> {
    pub(super) fn bits_reset(&mut self) {
        self.bits = 0;
        self.nbits = 0;
        self.nbytes = 0;
        self.err = None;
    }

    pub(super) fn flush(&mut self) {
        if self.err.is_some() {
            self.nbits = 0;
            return;
        }
        let mut n = self.nbytes;
        while self.nbits != 0 {
            self.bytes[n] = self.bits as u8;
            self.bits >>= 8;
            if self.nbits > 8 {
                self.nbits -= 8;
            } else {
                self.nbits = 0;
            }
            n += 1;
        }
        self.bits = 0;
        self.write_buffered(n);
        self.nbytes = 0;
    }

    fn write_buffered(&mut self, n: usize) {
        if self.err.is_some() {
            return;
        }
        if let Err(e) = write_once(&mut self.writer, &self.bytes[..n]) {
            self.err = Some(Fail::from_io(&e));
        }
    }

    fn write(&mut self, b: &[u8]) {
        if self.err.is_some() {
            return;
        }
        if let Err(e) = write_once(&mut self.writer, b) {
            self.err = Some(Fail::from_io(&e));
        }
    }

    fn put48(&mut self) {
        let bits = self.bits;
        self.bits >>= 48;
        self.nbits -= 48;
        let n = self.nbytes;
        self.bytes[n..n + 6].copy_from_slice(&bits.to_le_bytes()[..6]);
        let n = n + 6;
        if n >= BUFFER_FLUSH_SIZE {
            self.write_buffered(n);
            self.nbytes = 0;
        } else {
            self.nbytes = n;
        }
    }

    pub(super) fn write_bits(&mut self, b: i32, nb: u32) {
        if self.err.is_some() {
            return;
        }
        self.bits |= (b as i64 as u64) << self.nbits;
        self.nbits += nb;
        if self.nbits >= 48 {
            self.put48();
        }
    }

    pub(super) fn write_bytes(&mut self, bytes: &[u8]) {
        if self.err.is_some() {
            return;
        }
        let mut n = self.nbytes;
        if self.nbits & 7 != 0 {
            self.err = Some(internal_error("writeBytes with unfinished bits"));
            return;
        }
        while self.nbits != 0 {
            self.bytes[n] = self.bits as u8;
            self.bits >>= 8;
            self.nbits -= 8;
            n += 1;
        }
        if n != 0 {
            self.write_buffered(n);
        }
        self.nbytes = 0;
        self.write(bytes);
    }

    fn write_code(&mut self, c: Hcode) {
        if self.err.is_some() {
            return;
        }
        self.bits |= u64::from(c.code) << self.nbits;
        self.nbits += u32::from(c.len);
        if self.nbits >= 48 {
            self.put48();
        }
    }

    pub(super) fn write_stored_header(&mut self, length: usize, is_eof: bool) {
        if self.err.is_some() {
            return;
        }
        let flag = i32::from(is_eof);
        self.write_bits(flag, 3);
        self.flush();
        self.write_bits(length as i32, 16);
        self.write_bits(i32::from(!(length as u16)), 16);
    }
}

#[doc(hidden)]
pub struct HuffmanBitWriter<W> {
    pub(super) s: BitSink<W>,
    codegen_freq: [i32; CODEGEN_CODE_COUNT],
    literal_freq: Vec<i32>,
    offset_freq: Vec<i32>,
    codegen: Vec<u8>,
    literal_encoding: HuffmanEncoder,
    offset_encoding: HuffmanEncoder,
    codegen_encoding: HuffmanEncoder,
}

fn generate_codegen(
    codegen: &mut [u8],
    codegen_freq: &mut [i32; CODEGEN_CODE_COUNT],
    num_literals: usize,
    num_offsets: usize,
    lit_enc: &HuffmanEncoder,
    off_enc: &HuffmanEncoder,
) {
    *codegen_freq = [0; CODEGEN_CODE_COUNT];
    for (cg, code) in codegen[..num_literals].iter_mut().zip(&lit_enc.codes) {
        *cg = code.len as u8;
    }
    for (cg, code) in codegen[num_literals..num_literals + num_offsets]
        .iter_mut()
        .zip(&off_enc.codes)
    {
        *cg = code.len as u8;
    }
    codegen[num_literals + num_offsets] = BAD_CODE;

    let mut size = codegen[0];
    let mut count: i32 = 1;
    let mut out_index = 0;
    let mut in_index = 1;
    while size != BAD_CODE {
        let next_size = codegen[in_index];
        if next_size == size {
            count += 1;
            in_index += 1;
            continue;
        }
        if size != 0 {
            codegen[out_index] = size;
            out_index += 1;
            codegen_freq[size as usize] += 1;
            count -= 1;
            while count >= 3 {
                let n = count.min(6);
                codegen[out_index] = 16;
                out_index += 1;
                codegen[out_index] = (n - 3) as u8;
                out_index += 1;
                codegen_freq[16] += 1;
                count -= n;
            }
        } else {
            while count >= 11 {
                let n = count.min(138);
                codegen[out_index] = 18;
                out_index += 1;
                codegen[out_index] = (n - 11) as u8;
                out_index += 1;
                codegen_freq[18] += 1;
                count -= n;
            }
            if count >= 3 {
                codegen[out_index] = 17;
                out_index += 1;
                codegen[out_index] = (count - 3) as u8;
                out_index += 1;
                codegen_freq[17] += 1;
                count = 0;
            }
        }
        count -= 1;
        while count >= 0 {
            codegen[out_index] = size;
            out_index += 1;
            codegen_freq[size as usize] += 1;
            count -= 1;
        }
        size = next_size;
        count = 1;
        in_index += 1;
    }
    codegen[out_index] = BAD_CODE;
}

impl<W: Write> HuffmanBitWriter<W> {
    pub fn new(w: W) -> HuffmanBitWriter<W> {
        HuffmanBitWriter {
            s: BitSink {
                writer: w,
                bits: 0,
                nbits: 0,
                bytes: [0; BUFFER_SIZE],
                nbytes: 0,
                err: None,
            },
            codegen_freq: [0; CODEGEN_CODE_COUNT],
            literal_freq: vec![0; MAX_NUM_LIT],
            offset_freq: vec![0; OFFSET_CODE_COUNT],
            codegen: vec![0; MAX_NUM_LIT + OFFSET_CODE_COUNT + 1],
            literal_encoding: HuffmanEncoder::new(MAX_NUM_LIT),
            offset_encoding: HuffmanEncoder::new(OFFSET_CODE_COUNT),
            codegen_encoding: HuffmanEncoder::new(CODEGEN_CODE_COUNT),
        }
    }

    pub fn reset(&mut self, writer: W) {
        self.s.writer = writer;
        self.s.bits_reset();
    }

    pub fn flush(&mut self) {
        self.s.flush();
    }

    pub fn writer(&self) -> &W {
        &self.s.writer
    }

    pub fn error(&self) -> Option<std::io::Error> {
        self.s.err.as_ref().map(Fail::to_io)
    }

    fn dynamic_size(
        &self,
        lit_enc: &HuffmanEncoder,
        off_enc: &HuffmanEncoder,
        extra_bits: i64,
    ) -> (i64, usize) {
        let mut num_codegens = self.codegen_freq.len();
        while num_codegens > 4 && self.codegen_freq[CODEGEN_ORDER[num_codegens - 1]] == 0 {
            num_codegens -= 1;
        }
        let header = 3
            + 5
            + 5
            + 4
            + (3 * num_codegens as i64)
            + self.codegen_encoding.bit_length(&self.codegen_freq)
            + i64::from(self.codegen_freq[16]) * 2
            + i64::from(self.codegen_freq[17]) * 3
            + i64::from(self.codegen_freq[18]) * 7;
        let size = header
            + lit_enc.bit_length(&self.literal_freq)
            + off_enc.bit_length(&self.offset_freq)
            + extra_bits;

        (size, num_codegens)
    }

    fn stored_size(input: Option<&[u8]>) -> (i64, bool) {
        match input {
            None => (0, false),
            Some(input) if input.len() <= MAX_STORE_BLOCK_SIZE => {
                (((input.len() + 5) * 8) as i64, true)
            }
            Some(_) => (0, false),
        }
    }

    fn write_dynamic_header(
        &mut self,
        num_literals: usize,
        num_offsets: usize,
        num_codegens: usize,
        is_eof: bool,
    ) {
        if self.s.err.is_some() {
            return;
        }
        let first_bits = if is_eof { 5 } else { 4 };
        self.s.write_bits(first_bits, 3);
        self.s.write_bits(num_literals as i32 - 257, 5);
        self.s.write_bits(num_offsets as i32 - 1, 5);
        self.s.write_bits(num_codegens as i32 - 4, 4);

        for &order in CODEGEN_ORDER.iter().take(num_codegens) {
            let value = self.codegen_encoding.codes[order].len;
            self.s.write_bits(i32::from(value), 3);
        }

        let mut i = 0;
        loop {
            let code_word = self.codegen[i];
            i += 1;
            if code_word == BAD_CODE {
                break;
            }
            self.s
                .write_code(self.codegen_encoding.codes[code_word as usize]);

            match code_word {
                16 => {
                    self.s.write_bits(i32::from(self.codegen[i]), 2);
                    i += 1;
                }
                17 => {
                    self.s.write_bits(i32::from(self.codegen[i]), 3);
                    i += 1;
                }
                18 => {
                    self.s.write_bits(i32::from(self.codegen[i]), 7);
                    i += 1;
                }
                _ => {}
            }
        }
    }

    pub fn write_block_dynamic(&mut self, tokens: &[Token], eof: bool, input: Option<&[u8]>) {
        if self.s.err.is_some() {
            return;
        }

        let (num_literals, num_offsets) = self.index_tokens(tokens);

        generate_codegen(
            &mut self.codegen,
            &mut self.codegen_freq,
            num_literals,
            num_offsets,
            &self.literal_encoding,
            &self.offset_encoding,
        );
        self.codegen_encoding.generate(&self.codegen_freq, 7);
        let (size, num_codegens) =
            self.dynamic_size(&self.literal_encoding, &self.offset_encoding, 0);

        let (ssize, storable) = Self::stored_size(input);
        if storable && ssize < size + (size >> 4) {
            let input = input.unwrap_or(&[]);
            self.s.write_stored_header(input.len(), eof);
            self.s.write_bytes(input);
            return;
        }

        self.write_dynamic_header(num_literals, num_offsets, num_codegens, eof);

        self.write_tokens(tokens);
    }

    fn index_tokens(&mut self, tokens: &[Token]) -> (usize, usize) {
        self.literal_freq.fill(0);
        self.offset_freq.fill(0);

        for t in tokens.iter().copied().chain([END_BLOCK_MARKER as Token]) {
            if t < MATCH_TYPE {
                self.literal_freq[token_literal(t) as usize] += 1;
                continue;
            }
            let length = token_length(t);
            let offset = token_offset(t);
            self.literal_freq[LENGTH_CODES_START + length_code(length) as usize] += 1;
            self.offset_freq[offset_code(offset) as usize] += 1;
        }

        let mut num_literals = self.literal_freq.len();
        while self.literal_freq[num_literals - 1] == 0 {
            num_literals -= 1;
        }
        let mut num_offsets = self.offset_freq.len();
        while num_offsets > 0 && self.offset_freq[num_offsets - 1] == 0 {
            num_offsets -= 1;
        }
        if num_offsets == 0 {
            self.offset_freq[0] = 1;
            num_offsets = 1;
        }
        self.literal_encoding.generate(&self.literal_freq, 15);
        self.offset_encoding.generate(&self.offset_freq, 15);
        (num_literals, num_offsets)
    }

    fn write_tokens(&mut self, tokens: &[Token]) {
        if self.s.err.is_some() {
            return;
        }
        let le_codes = &self.literal_encoding.codes;
        let oe_codes = &self.offset_encoding.codes;
        for t in tokens.iter().copied().chain([END_BLOCK_MARKER as Token]) {
            if t < MATCH_TYPE {
                self.s.write_code(le_codes[token_literal(t) as usize]);
                continue;
            }
            let length = token_length(t);
            let length_code = length_code(length) as usize;
            self.s
                .write_code(le_codes[length_code + LENGTH_CODES_START]);
            let extra_length_bits = LENGTH_EXTRA_BITS[length_code] as u32;
            if extra_length_bits > 0 {
                let extra_length = (length - LENGTH_BASE[length_code]) as i32;
                self.s.write_bits(extra_length, extra_length_bits);
            }
            let offset = token_offset(t);
            let offset_code = offset_code(offset) as usize;
            self.s.write_code(oe_codes[offset_code]);
            let extra_offset_bits = OFFSET_EXTRA_BITS[offset_code] as u32;
            if extra_offset_bits > 0 {
                let extra_offset = (offset - OFFSET_BASE[offset_code]) as i32;
                self.s.write_bits(extra_offset, extra_offset_bits);
            }
        }
    }

    pub fn write_block_huff(&mut self, eof: bool, input: &[u8]) {
        if self.s.err.is_some() {
            return;
        }

        self.literal_freq.fill(0);

        histogram(input, &mut self.literal_freq);

        self.literal_freq[END_BLOCK_MARKER] = 1;

        const NUM_LITERALS: usize = END_BLOCK_MARKER + 1;
        self.offset_freq[0] = 1;
        const NUM_OFFSETS: usize = 1;

        self.literal_encoding.generate(&self.literal_freq, 15);

        generate_codegen(
            &mut self.codegen,
            &mut self.codegen_freq,
            NUM_LITERALS,
            NUM_OFFSETS,
            &self.literal_encoding,
            huff_offset(),
        );
        self.codegen_encoding.generate(&self.codegen_freq, 7);
        let (size, num_codegens) = self.dynamic_size(&self.literal_encoding, huff_offset(), 0);

        let (ssize, storable) = Self::stored_size(Some(input));
        if storable && ssize < size + (size >> 4) {
            self.s.write_stored_header(input.len(), eof);
            self.s.write_bytes(input);
            return;
        }

        self.write_dynamic_header(NUM_LITERALS, NUM_OFFSETS, num_codegens, eof);
        let encoding = &self.literal_encoding.codes[..257];
        let s = &mut self.s;
        let mut n = s.nbytes;
        for &t in input {
            let c = encoding[t as usize];
            s.bits |= u64::from(c.code) << s.nbits;
            s.nbits += u32::from(c.len);
            if s.nbits < 48 {
                continue;
            }
            let bits = s.bits;
            s.bits >>= 48;
            s.nbits -= 48;
            s.bytes[n..n + 6].copy_from_slice(&bits.to_le_bytes()[..6]);
            n += 6;
            if n < BUFFER_FLUSH_SIZE {
                continue;
            }
            s.write_buffered(n);
            if s.err.is_some() {
                return;
            }
            n = 0;
        }
        s.nbytes = n;
        s.write_code(encoding[END_BLOCK_MARKER]);
    }
}

fn histogram(b: &[u8], h: &mut [i32]) {
    let h = &mut h[..256];
    for &t in b {
        h[t as usize] += 1;
    }
}
