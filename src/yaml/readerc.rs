use super::yamlh::{Encoding, ErrorType, Parser};
use super::yamlprivateh::INPUT_RAW_BUFFER_SIZE;

const BOM_UTF8: &[u8] = b"\xef\xbb\xbf";
const BOM_UTF16LE: &[u8] = b"\xff\xfe";
const BOM_UTF16BE: &[u8] = b"\xfe\xff";

impl Parser<'_> {
    fn set_reader_error(&mut self, problem: &str, offset: usize, value: i64) -> bool {
        self.error = ErrorType::Reader;
        self.problem = problem.to_string();
        self.problem_offset = offset;
        self.problem_value = value;
        false
    }

    fn determine_encoding(&mut self) -> bool {
        while !self.eof && self.raw_buffer.len() - self.raw_buffer_pos < 3 {
            if !self.update_raw_buffer() {
                return false;
            }
        }
        let buf = &self.raw_buffer;
        let pos = self.raw_buffer_pos;
        let avail = buf.len() - pos;
        if avail >= 2 && buf[pos] == BOM_UTF16LE[0] && buf[pos + 1] == BOM_UTF16LE[1] {
            self.encoding = Encoding::Utf16Le;
            self.raw_buffer_pos += 2;
            self.offset += 2;
        } else if avail >= 2 && buf[pos] == BOM_UTF16BE[0] && buf[pos + 1] == BOM_UTF16BE[1] {
            self.encoding = Encoding::Utf16Be;
            self.raw_buffer_pos += 2;
            self.offset += 2;
        } else if avail >= 3
            && buf[pos] == BOM_UTF8[0]
            && buf[pos + 1] == BOM_UTF8[1]
            && buf[pos + 2] == BOM_UTF8[2]
        {
            self.encoding = Encoding::Utf8;
            self.raw_buffer_pos += 3;
            self.offset += 3;
        } else {
            self.encoding = Encoding::Utf8;
        }
        true
    }

    fn update_raw_buffer(&mut self) -> bool {
        if self.raw_buffer_pos == 0 && self.raw_buffer.len() == INPUT_RAW_BUFFER_SIZE {
            return true;
        }
        if self.eof {
            return true;
        }
        if self.raw_buffer_pos > 0 && self.raw_buffer_pos < self.raw_buffer.len() {
            self.raw_buffer.copy_within(self.raw_buffer_pos.., 0);
        }
        let kept = self.raw_buffer.len() - self.raw_buffer_pos;
        self.raw_buffer.truncate(kept);
        self.raw_buffer_pos = 0;
        if self.input_pos == self.input.len() {
            self.eof = true;
            return true;
        }
        let room = INPUT_RAW_BUFFER_SIZE - self.raw_buffer.len();
        let n = room.min(self.input.len() - self.input_pos);
        self.raw_buffer
            .extend_from_slice(&self.input[self.input_pos..self.input_pos + n]);
        self.input_pos += n;
        true
    }

    pub(super) fn update_buffer(&mut self, length: usize) -> bool {
        if self.unread >= length {
            return true;
        }
        if self.encoding == Encoding::Any && !self.determine_encoding() {
            return false;
        }
        let mut buffer_len = self.buffer.len();
        if self.buffer_pos > 0 && self.buffer_pos < buffer_len {
            self.buffer.copy_within(self.buffer_pos.., 0);
            buffer_len -= self.buffer_pos;
            self.buffer_pos = 0;
        } else if self.buffer_pos == buffer_len {
            buffer_len = 0;
            self.buffer_pos = 0;
        }
        self.buffer.truncate(buffer_len);
        let mut first = true;
        while self.unread < length {
            if (!first || self.raw_buffer_pos == self.raw_buffer.len()) && !self.update_raw_buffer()
            {
                return false;
            }
            first = false;
            while self.raw_buffer_pos != self.raw_buffer.len() {
                let raw_unread = self.raw_buffer.len() - self.raw_buffer_pos;
                let value: u32;
                let width: usize;
                match self.encoding {
                    Encoding::Utf16Le | Encoding::Utf16Be => {
                        let (low, high) = if self.encoding == Encoding::Utf16Le {
                            (0, 1)
                        } else {
                            (1, 0)
                        };
                        if raw_unread < 2 {
                            if self.eof {
                                return self.set_reader_error(
                                    "incomplete UTF-16 character",
                                    self.offset,
                                    -1,
                                );
                            }
                            break;
                        }
                        let p = self.raw_buffer_pos;
                        let first_unit = u32::from(self.raw_buffer[p + low])
                            + (u32::from(self.raw_buffer[p + high]) << 8);
                        if first_unit & 0xFC00 == 0xDC00 {
                            return self.set_reader_error(
                                "unexpected low surrogate area",
                                self.offset,
                                i64::from(first_unit),
                            );
                        }
                        if first_unit & 0xFC00 == 0xD800 {
                            if raw_unread < 4 {
                                if self.eof {
                                    return self.set_reader_error(
                                        "incomplete UTF-16 surrogate pair",
                                        self.offset,
                                        -1,
                                    );
                                }
                                break;
                            }
                            let second = u32::from(self.raw_buffer[p + low + 2])
                                + (u32::from(self.raw_buffer[p + high + 2]) << 8);
                            if second & 0xFC00 != 0xDC00 {
                                return self.set_reader_error(
                                    "expected low surrogate area",
                                    self.offset + 2,
                                    i64::from(second),
                                );
                            }
                            value = 0x10000 + ((first_unit & 0x3FF) << 10) + (second & 0x3FF);
                            width = 4;
                        } else {
                            value = first_unit;
                            width = 2;
                        }
                    }
                    _ => {
                        let octet = self.raw_buffer[self.raw_buffer_pos];
                        let w = if octet & 0x80 == 0x00 {
                            1
                        } else if octet & 0xE0 == 0xC0 {
                            2
                        } else if octet & 0xF0 == 0xE0 {
                            3
                        } else if octet & 0xF8 == 0xF0 {
                            4
                        } else {
                            return self.set_reader_error(
                                "invalid leading UTF-8 octet",
                                self.offset,
                                i64::from(octet),
                            );
                        };
                        if w > raw_unread {
                            if self.eof {
                                return self.set_reader_error(
                                    "incomplete UTF-8 octet sequence",
                                    self.offset,
                                    -1,
                                );
                            }
                            break;
                        }
                        let mut v = match w {
                            1 => u32::from(octet & 0x7F),
                            2 => u32::from(octet & 0x1F),
                            3 => u32::from(octet & 0x0F),
                            _ => u32::from(octet & 0x07),
                        };
                        for k in 1..w {
                            let trailing = self.raw_buffer[self.raw_buffer_pos + k];
                            if trailing & 0xC0 != 0x80 {
                                return self.set_reader_error(
                                    "invalid trailing UTF-8 octet",
                                    self.offset + k,
                                    i64::from(trailing),
                                );
                            }
                            v = (v << 6) + u32::from(trailing & 0x3F);
                        }
                        let length_ok = w == 1
                            || (w == 2 && v >= 0x80)
                            || (w == 3 && v >= 0x800)
                            || (w == 4 && v >= 0x10000);
                        if !length_ok {
                            return self.set_reader_error(
                                "invalid length of a UTF-8 sequence",
                                self.offset,
                                -1,
                            );
                        }
                        if (0xD800..=0xDFFF).contains(&v) || v > 0x10FFFF {
                            return self.set_reader_error(
                                "invalid Unicode character",
                                self.offset,
                                i64::from(v),
                            );
                        }
                        value = v;
                        width = w;
                    }
                }
                let allowed = value == 0x09
                    || value == 0x0A
                    || value == 0x0D
                    || (0x20..=0x7E).contains(&value)
                    || value == 0x85
                    || (0xA0..=0xD7FF).contains(&value)
                    || (0xE000..=0xFFFD).contains(&value)
                    || (0x10000..=0x10FFFF).contains(&value);
                if !allowed {
                    return self.set_reader_error(
                        "control characters are not allowed",
                        self.offset,
                        i64::from(value),
                    );
                }
                self.raw_buffer_pos += width;
                self.offset += width;
                if value <= 0x7F {
                    self.buffer.push(value as u8);
                } else if value <= 0x7FF {
                    self.buffer.push((0xC0 + (value >> 6)) as u8);
                    self.buffer.push((0x80 + (value & 0x3F)) as u8);
                } else if value <= 0xFFFF {
                    self.buffer.push((0xE0 + (value >> 12)) as u8);
                    self.buffer.push((0x80 + ((value >> 6) & 0x3F)) as u8);
                    self.buffer.push((0x80 + (value & 0x3F)) as u8);
                } else {
                    self.buffer.push((0xF0 + (value >> 18)) as u8);
                    self.buffer.push((0x80 + ((value >> 12) & 0x3F)) as u8);
                    self.buffer.push((0x80 + ((value >> 6) & 0x3F)) as u8);
                    self.buffer.push((0x80 + (value & 0x3F)) as u8);
                }
                self.unread += 1;
            }
            if self.eof {
                self.buffer.push(0);
                self.unread += 1;
                break;
            }
        }
        while self.buffer.len() < length {
            self.buffer.push(0);
        }
        true
    }
}
