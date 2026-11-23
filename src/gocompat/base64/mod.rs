use crate::gocompat::utf8::Rune;
use std::fmt;

pub const STD_PADDING: Rune = '=' as Rune;
pub const NO_PADDING: Rune = -1;

const INVALID_INDEX: u8 = 0xFF;

const STD_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub static STD_ENCODING: Encoding = Encoding::new(STD_ALPHABET);
pub static URL_ENCODING: Encoding = Encoding::new(URL_ALPHABET);
pub static RAW_STD_ENCODING: Encoding = Encoding::new(STD_ALPHABET).with_padding(NO_PADDING);
pub static RAW_URL_ENCODING: Encoding = Encoding::new(URL_ALPHABET).with_padding(NO_PADDING);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorruptInputError(pub usize);

impl CorruptInputError {
    pub fn offset(&self) -> usize {
        self.0
    }
}

impl fmt::Display for CorruptInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid base64 at byte {}", self.0)
    }
}

impl std::error::Error for CorruptInputError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoding {
    encode: [u8; 64],
    decode_map: [u8; 256],
    pad_char: Rune,
    strict: bool,
}

fn is_newline(c: u8) -> bool {
    c == b'\n' || c == b'\r'
}

fn decoded_len(n: usize, pad_char: Rune) -> usize {
    if pad_char == NO_PADDING {
        return n / 4 * 3 + n % 4 * 6 / 8;
    }
    n / 4 * 3
}

impl Encoding {
    pub const fn new(encoder: &[u8; 64]) -> Encoding {
        let mut decode_map = [INVALID_INDEX; 256];
        let mut i = 0;
        while i < encoder.len() {
            let c = encoder[i];
            if c == b'\n' || c == b'\r' {
                panic!("the base64 alphabet contains a newline character");
            }
            if !c.is_ascii() {
                panic!("the base64 alphabet contains a non-ASCII byte");
            }
            if decode_map[c as usize] != INVALID_INDEX {
                panic!("the base64 alphabet contains a duplicate symbol");
            }
            decode_map[c as usize] = i as u8;
            i += 1;
        }
        Encoding {
            encode: *encoder,
            decode_map,
            pad_char: STD_PADDING,
            strict: false,
        }
    }

    pub const fn with_padding(self, padding: Rune) -> Encoding {
        if padding < NO_PADDING
            || padding == '\r' as Rune
            || padding == '\n' as Rune
            || padding > 0x7F
        {
            panic!("invalid base64 padding");
        }
        if padding != NO_PADDING && self.decode_map[padding as usize] != INVALID_INDEX {
            panic!("the base64 padding is part of the alphabet");
        }
        Encoding {
            pad_char: padding,
            ..self
        }
    }

    pub const fn strict(self) -> Encoding {
        Encoding {
            strict: true,
            ..self
        }
    }

    pub fn encode(&self, dst: &mut [u8], src: &[u8]) {
        if src.is_empty() {
            return;
        }
        let mut di = 0;
        let mut si = 0;
        let n = (src.len() / 3) * 3;
        while si < n {
            let val =
                (u32::from(src[si]) << 16) | (u32::from(src[si + 1]) << 8) | u32::from(src[si + 2]);
            dst[di] = self.symbol(val >> 18);
            dst[di + 1] = self.symbol(val >> 12);
            dst[di + 2] = self.symbol(val >> 6);
            dst[di + 3] = self.symbol(val);
            si += 3;
            di += 4;
        }
        let remain = src.len() - si;
        if remain == 0 {
            return;
        }
        let mut val = u32::from(src[si]) << 16;
        if remain == 2 {
            val |= u32::from(src[si + 1]) << 8;
        }
        dst[di] = self.symbol(val >> 18);
        dst[di + 1] = self.symbol(val >> 12);
        if remain == 2 {
            dst[di + 2] = self.symbol(val >> 6);
            if self.pad_char != NO_PADDING {
                dst[di + 3] = self.pad_char as u8;
            }
        } else if self.pad_char != NO_PADDING {
            dst[di + 2] = self.pad_char as u8;
            dst[di + 3] = self.pad_char as u8;
        }
    }

    fn symbol(&self, v: u32) -> u8 {
        self.encode[(v & 0x3F) as usize]
    }

    pub fn append_encode(&self, dst: &mut Vec<u8>, src: &[u8]) {
        let start = dst.len();
        dst.resize(start + self.encoded_len(src.len()), 0);
        self.encode(&mut dst[start..], src);
    }

    pub fn encode_to_string(&self, src: &[u8]) -> String {
        let mut buf = vec![0; self.encoded_len(src.len())];
        self.encode(&mut buf, src);
        buf.into_iter().map(char::from).collect()
    }

    pub fn encoded_len(&self, n: usize) -> usize {
        if self.pad_char == NO_PADDING {
            return n / 3 * 4 + (n % 3 * 8).div_ceil(6);
        }
        n.div_ceil(3) * 4
    }

    pub fn decoded_len(&self, n: usize) -> usize {
        decoded_len(n, self.pad_char)
    }

    fn decode_quantum(
        &self,
        dst: &mut [u8],
        src: &[u8],
        mut si: usize,
    ) -> (usize, usize, Result<(), CorruptInputError>) {
        let mut dbuf = [0u8; 4];
        let mut dlen = 4;
        let mut err = Ok(());
        let mut j = 0;
        while j < dbuf.len() {
            if src.len() == si {
                if j == 0 {
                    return (si, 0, Ok(()));
                }
                if j == 1 || self.pad_char != NO_PADDING {
                    return (si, 0, Err(CorruptInputError(si - j)));
                }
                dlen = j;
                break;
            }
            let input = src[si];
            si += 1;
            let out = self.decode_map[input as usize];
            if out != INVALID_INDEX {
                dbuf[j] = out;
                j += 1;
                continue;
            }
            if is_newline(input) {
                continue;
            }
            if Rune::from(input) != self.pad_char {
                return (si, 0, Err(CorruptInputError(si - 1)));
            }
            if j < 2 {
                return (si, 0, Err(CorruptInputError(si - 1)));
            }
            if j == 2 {
                while si < src.len() && is_newline(src[si]) {
                    si += 1;
                }
                if si == src.len() {
                    return (si, 0, Err(CorruptInputError(src.len())));
                }
                if Rune::from(src[si]) != self.pad_char {
                    return (si, 0, Err(CorruptInputError(si - 1)));
                }
                si += 1;
            }
            while si < src.len() && is_newline(src[si]) {
                si += 1;
            }
            if si < src.len() {
                err = Err(CorruptInputError(si));
            }
            dlen = j;
            break;
        }
        let val = (u32::from(dbuf[0]) << 18)
            | (u32::from(dbuf[1]) << 12)
            | (u32::from(dbuf[2]) << 6)
            | u32::from(dbuf[3]);
        let b0 = (val >> 16) as u8;
        let b1 = (val >> 8) as u8;
        let b2 = val as u8;
        match dlen {
            4 => {
                dst[2] = b2;
                dst[1] = b1;
                dst[0] = b0;
            }
            3 => {
                dst[1] = b1;
                if self.strict && b2 != 0 {
                    return (si, 0, Err(CorruptInputError(si - 1)));
                }
                dst[0] = b0;
            }
            _ => {
                dst[0] = b0;
                if self.strict && (b1 != 0 || b2 != 0) {
                    return (si, 0, Err(CorruptInputError(si - 2)));
                }
            }
        }
        (si, dlen - 1, err)
    }

    pub fn decode(&self, dst: &mut [u8], src: &[u8]) -> (usize, Result<(), CorruptInputError>) {
        let mut n = 0;
        let mut si = 0;
        while si < src.len() {
            let (nsi, ninc, err) = self.decode_quantum(&mut dst[n..], src, si);
            si = nsi;
            n += ninc;
            if err.is_err() {
                return (n, err);
            }
        }
        (n, Ok(()))
    }

    pub fn append_decode(&self, dst: &mut Vec<u8>, src: &[u8]) -> Result<(), CorruptInputError> {
        let mut n = src.len();
        while n > 0 && Rune::from(src[n - 1]) == self.pad_char {
            n -= 1;
        }
        let start = dst.len();
        dst.resize(start + decoded_len(n, NO_PADDING), 0);
        let (written, result) = self.decode(&mut dst[start..], src);
        dst.truncate(start + written);
        result
    }

    pub fn decode_string(&self, s: &[u8]) -> Result<Vec<u8>, CorruptInputError> {
        let mut dbuf = vec![0; self.decoded_len(s.len())];
        let (n, result) = self.decode(&mut dbuf, s);
        result?;
        dbuf.truncate(n);
        Ok(dbuf)
    }
}
