use std::io::{self, Read};

#[derive(Debug)]
pub enum ReadErr {
    Eof,
    Io(io::Error),
}

impl ReadErr {
    pub fn is_eof(&self) -> bool {
        matches!(self, ReadErr::Eof)
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, ReadErr::Io(e) if e.kind() == io::ErrorKind::TimedOut)
    }

    pub fn into_io(self) -> io::Error {
        match self {
            ReadErr::Eof => io::Error::from(io::ErrorKind::UnexpectedEof),
            ReadErr::Io(e) => e,
        }
    }
}

#[derive(Debug)]
pub enum PeekErr {
    BufferFull,
    Read(ReadErr),
}

#[derive(Debug)]
pub enum SliceErr {
    BufferFull,
    Read(ReadErr),
}

pub struct BufReader<R> {
    buf: Box<[u8]>,
    rd: R,
    r: usize,
    w: usize,
    err: Option<ReadErr>,
    last_byte: Option<u8>,
}

impl<R: Read> BufReader<R> {
    pub fn with_capacity(size: usize, rd: R) -> BufReader<R> {
        BufReader {
            buf: vec![0; size.max(16)].into_boxed_slice(),
            rd,
            r: 0,
            w: 0,
            err: None,
            last_byte: None,
        }
    }

    pub fn with_buffer(size: usize, rd: R, initial: &[u8]) -> BufReader<R> {
        let mut b = BufReader::with_capacity(size.max(initial.len()), rd);
        b.buf[..initial.len()].copy_from_slice(initial);
        b.w = initial.len();
        b
    }

    pub fn get_ref(&self) -> &R {
        &self.rd
    }

    pub fn into_inner(self) -> R {
        self.rd
    }

    pub fn into_parts(self) -> (Vec<u8>, R) {
        (self.buf[self.r..self.w].to_vec(), self.rd)
    }

    pub fn get_mut(&mut self) -> &mut R {
        &mut self.rd
    }

    pub fn size(&self) -> usize {
        self.buf.len()
    }

    pub fn buffered(&self) -> usize {
        self.w - self.r
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf[self.r..self.w]
    }

    pub fn take_buffered(&mut self) -> Vec<u8> {
        let v = self.buf[self.r..self.w].to_vec();
        self.r = self.w;
        v
    }

    fn read_err(&mut self) -> Option<ReadErr> {
        self.err.take()
    }

    fn fill(&mut self) {
        if self.r > 0 {
            self.buf.copy_within(self.r..self.w, 0);
            self.w -= self.r;
            self.r = 0;
        }
        assert!(self.w < self.buf.len(), "bufio: tried to fill full buffer");
        let mut attempts = 0;
        loop {
            match self.rd.read(&mut self.buf[self.w..]) {
                Ok(0) => {
                    self.err = Some(ReadErr::Eof);
                    return;
                }
                Ok(n) => {
                    self.w += n;
                    return;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    attempts += 1;
                    if attempts >= 100 {
                        self.err = Some(ReadErr::Io(io::Error::other(
                            "multiple Read calls return no data or error",
                        )));
                        return;
                    }
                }
                Err(e) => {
                    self.err = Some(ReadErr::Io(e));
                    return;
                }
            }
        }
    }

    pub fn peek(&mut self, n: usize) -> (&[u8], Option<PeekErr>) {
        self.last_byte = None;
        while self.w - self.r < n && self.w - self.r < self.buf.len() && self.err.is_none() {
            self.fill();
        }
        if n > self.buf.len() {
            return (&self.buf[self.r..self.w], Some(PeekErr::BufferFull));
        }
        let mut n = n;
        let mut err = None;
        let avail = self.w - self.r;
        if avail < n {
            n = avail;
            err = Some(match self.read_err() {
                Some(e) => PeekErr::Read(e),
                None => PeekErr::BufferFull,
            });
        }
        (&self.buf[self.r..self.r + n], err)
    }

    pub fn discard(&mut self, n: usize) -> Result<usize, (usize, ReadErr)> {
        if n == 0 {
            return Ok(0);
        }
        self.last_byte = None;
        let mut remain = n;
        loop {
            let mut skip = self.buffered();
            if skip == 0 {
                self.fill();
                skip = self.buffered();
            }
            if skip > remain {
                skip = remain;
            }
            self.r += skip;
            remain -= skip;
            if remain == 0 {
                return Ok(n);
            }
            if self.err.is_some() {
                let e = self.read_err().unwrap_or(ReadErr::Eof);
                return Err((n - remain, e));
            }
        }
    }

    pub fn read_buf(&mut self, p: &mut [u8]) -> Result<usize, ReadErr> {
        let n = p.len();
        if n == 0 {
            if self.buffered() > 0 {
                return Ok(0);
            }
            return match self.read_err() {
                Some(e) => Err(e),
                None => Ok(0),
            };
        }
        if self.r == self.w {
            if let Some(e) = self.read_err() {
                return Err(e);
            }
            if p.len() >= self.buf.len() {
                loop {
                    match self.rd.read(p) {
                        Ok(0) => return Err(ReadErr::Eof),
                        Ok(n) => {
                            self.last_byte = Some(p[n - 1]);
                            return Ok(n);
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(ReadErr::Io(e)),
                    }
                }
            }
            self.r = 0;
            self.w = 0;
            loop {
                match self.rd.read(&mut self.buf) {
                    Ok(0) => return Err(ReadErr::Eof),
                    Ok(n) => {
                        self.w += n;
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(ReadErr::Io(e)),
                }
            }
        }
        let n = p.len().min(self.w - self.r);
        p[..n].copy_from_slice(&self.buf[self.r..self.r + n]);
        self.r += n;
        self.last_byte = Some(self.buf[self.r - 1]);
        Ok(n)
    }

    pub fn read_byte(&mut self) -> Result<u8, ReadErr> {
        while self.r == self.w {
            if let Some(e) = self.read_err() {
                return Err(e);
            }
            self.fill();
        }
        let c = self.buf[self.r];
        self.r += 1;
        self.last_byte = Some(c);
        Ok(c)
    }

    pub fn unread_byte(&mut self) -> bool {
        let Some(last) = self.last_byte else {
            return false;
        };
        if self.r == 0 && self.w > 0 {
            return false;
        }
        if self.r > 0 {
            self.r -= 1;
        } else {
            self.w = 1;
        }
        self.buf[self.r] = last;
        self.last_byte = None;
        true
    }

    pub fn read_slice(&mut self, delim: u8) -> (Vec<u8>, Option<SliceErr>) {
        let mut s = 0;
        let (line, err) = loop {
            if let Some(i) = self.buf[self.r + s..self.w]
                .iter()
                .position(|&c| c == delim)
            {
                let i = i + s;
                let line = self.buf[self.r..self.r + i + 1].to_vec();
                self.r += i + 1;
                break (line, None);
            }
            if self.err.is_some() {
                let line = self.buf[self.r..self.w].to_vec();
                self.r = self.w;
                let e = self.read_err().unwrap_or(ReadErr::Eof);
                break (line, Some(SliceErr::Read(e)));
            }
            if self.buffered() >= self.buf.len() {
                self.r = self.w;
                break (self.buf.to_vec(), Some(SliceErr::BufferFull));
            }
            s = self.w - self.r;
            self.fill();
        };
        if let Some(&last) = line.last() {
            self.last_byte = Some(last);
        }
        (line, err)
    }

    pub fn read_line(&mut self) -> Result<(Vec<u8>, bool), ReadErr> {
        let (mut line, err) = self.read_slice(b'\n');
        match err {
            Some(SliceErr::BufferFull) => {
                if line.last() == Some(&b'\r') {
                    assert!(self.r > 0, "bufio: tried to rewind past start of buffer");
                    self.r -= 1;
                    line.pop();
                }
                return Ok((line, true));
            }
            Some(SliceErr::Read(e)) if line.is_empty() => return Err(e),
            _ => {}
        }
        if line.is_empty() {
            return Ok((line, false));
        }
        if line.last() == Some(&b'\n') {
            let drop = if line.len() > 1 && line[line.len() - 2] == b'\r' {
                2
            } else {
                1
            };
            line.truncate(line.len() - drop);
        }
        Ok((line, false))
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.read_buf(buf) {
            Ok(n) => Ok(n),
            Err(ReadErr::Eof) => Ok(0),
            Err(ReadErr::Io(e)) => Err(e),
        }
    }
}
