use super::deflatefast::DeflateFast;
use super::huffman_bit_writer::HuffmanBitWriter;
use super::token::Token;
use crate::archive::Fail;
use std::io::{self, Write};

pub const BEST_SPEED: i32 = 1;

pub(super) const BASE_MATCH_LENGTH: usize = 3;
pub(super) const MAX_MATCH_LENGTH: usize = 258;
pub(super) const BASE_MATCH_OFFSET: usize = 1;
pub(super) const MAX_MATCH_OFFSET: usize = 1 << 15;
pub(super) const MAX_STORE_BLOCK_SIZE: usize = 65535;

const CLOSED: &str = "the compressed stream is already closed";

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Failed(Fail),
    Closed,
}

impl State {
    fn to_io(&self) -> io::Error {
        match self {
            State::Failed(err) => err.to_io(),
            State::Closed => io::Error::other(CLOSED),
        }
    }
}

pub struct Writer<W: Write> {
    w: HuffmanBitWriter<W>,
    best_speed: DeflateFast,
    window: Vec<u8>,
    window_end: usize,
    sync: bool,
    tokens: Vec<Token>,
    err: Option<State>,
}

impl<W: Write> Writer<W> {
    pub fn new(w: W, level: i32) -> io::Result<Writer<W>> {
        if level != BEST_SPEED {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported compression level {level}"),
            ));
        }
        Ok(Writer {
            w: HuffmanBitWriter::new(w),
            best_speed: DeflateFast::new(),
            window: vec![0; MAX_STORE_BLOCK_SIZE],
            window_end: 0,
            sync: false,
            tokens: Vec::with_capacity(MAX_STORE_BLOCK_SIZE),
            err: None,
        })
    }

    fn fill_store(&mut self, b: &[u8]) -> usize {
        let n = (self.window.len() - self.window_end).min(b.len());
        self.window[self.window_end..self.window_end + n].copy_from_slice(&b[..n]);
        self.window_end += n;
        n
    }

    fn write_stored_block(&mut self, start: usize, end: usize) -> Option<Fail> {
        self.w.s.write_stored_header(end - start, false);
        if self.w.s.err.is_some() {
            return self.w.s.err.clone();
        }
        self.w.s.write_bytes(&self.window[start..end]);
        self.w.s.err.clone()
    }

    fn enc_speed(&mut self) {
        if self.window_end < MAX_STORE_BLOCK_SIZE {
            if !self.sync {
                return;
            }

            if self.window_end < 128 {
                match self.window_end {
                    0 => return,
                    1..=16 => {
                        if let Some(err) = self.write_stored_block(0, self.window_end) {
                            self.err = Some(State::Failed(err));
                        }
                    }
                    _ => {
                        self.w
                            .write_block_huff(false, &self.window[..self.window_end]);
                        if let Some(err) = &self.w.s.err {
                            self.err = Some(State::Failed(err.clone()));
                        }
                    }
                }
                self.window_end = 0;
                self.best_speed.reset();
                return;
            }
        }
        self.tokens.clear();
        self.best_speed
            .encode(&mut self.tokens, &self.window[..self.window_end]);

        if self.tokens.len() > self.window_end - (self.window_end >> 4) {
            self.w
                .write_block_huff(false, &self.window[..self.window_end]);
        } else {
            self.w
                .write_block_dynamic(&self.tokens, false, Some(&self.window[..self.window_end]));
        }
        if let Some(err) = &self.w.s.err {
            self.err = Some(State::Failed(err.clone()));
        }
        self.window_end = 0;
    }

    fn compress(&mut self, mut b: &[u8]) -> io::Result<usize> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        let n = b.len();
        while !b.is_empty() {
            self.enc_speed();
            let filled = self.fill_store(b);
            b = &b[filled..];
            if let Some(err) = &self.err {
                return Err(err.to_io());
            }
        }
        Ok(n)
    }

    fn sync_flush(&mut self) -> io::Result<()> {
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        self.sync = true;
        self.enc_speed();
        if self.err.is_none() {
            self.w.s.write_stored_header(0, false);
            self.w.s.flush();
            if let Some(err) = &self.w.s.err {
                self.err = Some(State::Failed(err.clone()));
            }
        }
        self.sync = false;
        match &self.err {
            None => Ok(()),
            Some(err) => Err(err.to_io()),
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.sync_flush()
    }

    pub fn close(&mut self) -> io::Result<()> {
        match &self.err {
            Some(State::Closed) => return Ok(()),
            Some(err) => return Err(err.to_io()),
            None => {}
        }
        self.sync = true;
        self.enc_speed();
        if let Some(err) = &self.err {
            return Err(err.to_io());
        }
        self.w.s.write_stored_header(0, true);
        if let Some(err) = &self.w.s.err {
            return Err(err.to_io());
        }
        self.w.s.flush();
        if let Some(err) = &self.w.s.err {
            return Err(err.to_io());
        }
        self.err = Some(State::Closed);
        Ok(())
    }

    pub fn reset(&mut self, w: W) {
        self.w.s.writer = w;
        self.reset_state();
    }

    #[doc(hidden)]
    pub fn reset_state(&mut self) {
        self.w.s.bits_reset();
        self.sync = false;
        self.err = None;
        self.window_end = 0;
        self.tokens.clear();
        self.best_speed.reset();
    }

    pub fn get_ref(&self) -> &W {
        self.w.writer()
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.w.s.writer
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.compress(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sync_flush()
    }
}
