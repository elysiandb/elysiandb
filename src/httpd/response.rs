use std::io::{self, Read, Write};

use super::bytesconv::LOWER_HEX;
use super::header::ResponseHeader;

pub type StreamWriterFn = Box<dyn FnOnce(&mut StreamWriter<'_>) + Send>;

const STREAM_CHUNK_SIZE: usize = 4096;

enum BodyStream {
    Writer(StreamWriterFn),
    Reader(Box<dyn Read + Send>),
}

#[derive(Default)]
pub struct Response {
    pub header: ResponseHeader,
    body: Vec<u8>,
    body_stream: Option<BodyStream>,
    pub skip_body: bool,
    pub immediate_header_flush: bool,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("header", &self.header)
            .field("body", &String::from_utf8_lossy(&self.body))
            .field("stream", &self.body_stream.is_some())
            .finish()
    }
}

enum Sink<'a> {
    Chunked(&'a mut dyn Write),
    Raw(&'a mut Vec<u8>),
    Discard,
}

pub struct StreamWriter<'a> {
    sink: Sink<'a>,
    buf: Vec<u8>,
    err: Option<io::ErrorKind>,
    written: u64,
}

impl<'a> StreamWriter<'a> {
    fn new(sink: Sink<'a>) -> StreamWriter<'a> {
        StreamWriter {
            sink,
            buf: Vec::with_capacity(STREAM_CHUNK_SIZE),
            err: None,
            written: 0,
        }
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    fn emit(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some(kind) = self.err {
            return Err(io::Error::from(kind));
        }
        let res = match &mut self.sink {
            Sink::Chunked(w) => write_chunk(&mut **w, data),
            Sink::Raw(v) => {
                v.extend_from_slice(data);
                Ok(())
            }
            Sink::Discard => Ok(()),
        };
        if let Err(e) = &res {
            self.err = Some(e.kind());
        }
        res
    }

    fn drain(&mut self) -> io::Result<()> {
        while !self.buf.is_empty() {
            let n = self.buf.len().min(STREAM_CHUNK_SIZE);
            let chunk: Vec<u8> = self.buf.drain(..n).collect();
            self.emit(&chunk)?;
        }
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.drain()?;
        if let Sink::Chunked(w) = &mut self.sink {
            if let Some(kind) = self.err {
                return Err(io::Error::from(kind));
            }
            w.write_all(b"0\r\n")?;
            w.flush()?;
        }
        Ok(())
    }
}

impl Write for StreamWriter<'_> {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        if let Some(kind) = self.err {
            return Err(io::Error::from(kind));
        }
        self.buf.extend_from_slice(p);
        self.written += p.len() as u64;
        while self.buf.len() >= STREAM_CHUNK_SIZE {
            let chunk: Vec<u8> = self.buf.drain(..STREAM_CHUNK_SIZE).collect();
            self.emit(&chunk)?;
        }
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.drain()
    }
}

fn write_chunk(w: &mut dyn Write, b: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(18);
    let mut n = b.len();
    let mut digits = Vec::new();
    loop {
        digits.push(LOWER_HEX[n & 0xf]);
        n >>= 4;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    head.extend_from_slice(&digits);
    head.extend_from_slice(b"\r\n");
    let mut out = head;
    out.extend_from_slice(b);
    if !b.is_empty() {
        out.extend_from_slice(b"\r\n");
    }
    w.write_all(&out)?;
    w.flush()
}

impl Response {
    pub fn new() -> Response {
        Response::default()
    }

    pub fn reset(&mut self) {
        *self = Response::default();
    }

    pub fn status_code(&self) -> u16 {
        self.header.status_code()
    }

    pub fn set_status_code(&mut self, status_code: u16) {
        self.header.set_status_code(status_code);
    }

    pub fn connection_close(&self) -> bool {
        self.header.connection_close()
    }

    pub fn set_connection_close(&mut self) {
        self.header.set_connection_close();
    }

    pub fn is_body_stream(&self) -> bool {
        self.body_stream.is_some()
    }

    fn close_body_stream(&mut self) {
        self.body_stream = None;
    }

    pub fn body(&mut self) -> &[u8] {
        if let Some(stream) = self.body_stream.take() {
            let mut out = Vec::new();
            match stream {
                BodyStream::Writer(f) => {
                    let mut sw = StreamWriter::new(Sink::Raw(&mut out));
                    f(&mut sw);
                    let _ = sw.finish();
                }
                BodyStream::Reader(mut r) => {
                    if let Err(e) = r.read_to_end(&mut out) {
                        out = e.to_string().into_bytes();
                    }
                }
            }
            self.body = out;
        }
        &self.body
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn set_body(&mut self, body: impl AsRef<[u8]>) {
        self.close_body_stream();
        self.body.clear();
        self.body.extend_from_slice(body.as_ref());
    }

    pub fn set_body_string(&mut self, body: impl AsRef<str>) {
        self.set_body(body.as_ref().as_bytes());
    }

    pub fn append_body(&mut self, p: impl AsRef<[u8]>) {
        self.close_body_stream();
        self.body.extend_from_slice(p.as_ref());
    }

    pub fn reset_body(&mut self) {
        self.close_body_stream();
        self.body = Vec::new();
    }

    pub fn set_body_stream(&mut self, body_stream: Box<dyn Read + Send>, body_size: i64) {
        self.reset_body();
        self.body_stream = Some(BodyStream::Reader(body_stream));
        self.header.set_content_length(body_size);
    }

    pub fn set_body_stream_writer(&mut self, sw: StreamWriterFn) {
        self.reset_body();
        self.body_stream = Some(BodyStream::Writer(sw));
        self.header.set_content_length(-1);
    }

    fn must_skip_body(&self) -> bool {
        let status = self.header.status_code();
        self.skip_body || status == 304 || status == 204 || (100..200).contains(&status)
    }

    pub fn write<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        let send_body = !self.must_skip_body();
        if self.body_stream.is_some() {
            return self.write_body_stream(w, send_body);
        }
        let body_len = self.body.len() as i64;
        if send_body || body_len > 0 {
            self.header.set_content_length(body_len);
        }
        w.write_all(&self.header.header())?;
        if send_body {
            w.write_all(&self.body)?;
        }
        Ok(())
    }

    fn write_body_stream<W: Write>(&mut self, w: &mut W, send_body: bool) -> io::Result<()> {
        let Some(stream) = self.body_stream.take() else {
            return Ok(());
        };
        let content_length = self.header.content_length();
        if content_length >= 0 {
            w.write_all(&self.header.header())?;
            if self.immediate_header_flush {
                w.flush()?;
            }
            if send_body {
                let copied = match stream {
                    BodyStream::Reader(mut r) => io::copy(&mut r, w)?,
                    BodyStream::Writer(f) => {
                        let mut raw = Vec::new();
                        let mut sw = StreamWriter::new(Sink::Raw(&mut raw));
                        f(&mut sw);
                        sw.finish()?;
                        w.write_all(&raw)?;
                        raw.len() as u64
                    }
                };
                if copied != content_length as u64 {
                    return Err(io::Error::other(format!(
                        "copied {copied} bytes from body stream instead of {content_length} bytes"
                    )));
                }
            }
            return Ok(());
        }

        self.header.set_content_length(-1);
        w.write_all(&self.header.header())?;
        if self.immediate_header_flush {
            w.flush()?;
        }
        if send_body {
            match stream {
                BodyStream::Writer(f) => {
                    let mut sw = StreamWriter::new(Sink::Chunked(w));
                    f(&mut sw);
                    sw.finish()?;
                }
                BodyStream::Reader(mut r) => {
                    let mut buf = vec![0u8; STREAM_CHUNK_SIZE];
                    loop {
                        let n = match r.read(&mut buf) {
                            Ok(n) => n,
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                            Err(e) => return Err(e),
                        };
                        write_chunk(w, &buf[..n])?;
                        if n == 0 {
                            break;
                        }
                    }
                }
            }
        } else if let BodyStream::Writer(f) = stream {
            let mut sw = StreamWriter::new(Sink::Discard);
            f(&mut sw);
        }
        w.write_all(&self.header.trailer_header())
    }
}
