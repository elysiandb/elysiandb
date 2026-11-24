use std::cell::OnceCell;
use std::fmt;
use std::io::{self, Read};

use super::bufio::{BufReader, PeekErr, ReadErr};
use super::bytesconv::{MAX_HEX_INT_CHARS, hex2int};
use super::header::{ParseError, RequestHeader, TrailerError};
use super::multipart::{Form, MultipartError, read_multipart_form, write_multipart_form};
use super::uri::URI;

pub const DEFAULT_MAX_REQUEST_BODY_SIZE: usize = 4 * 1024 * 1024;
const DEFAULT_MAX_IN_MEMORY_FILE_SIZE: i64 = 16 * 1024 * 1024;

pub const MSG_BAD_REQUEST: &str = "bad request";
pub const MSG_BODY_TOO_LARGE: &str = "request body too large";
pub const MSG_REQUEST_TIMEOUT: &str = "request timeout";
pub const MSG_HEADER_TOO_LARGE: &str = "request header fields too large";

pub const REASON_INVALID_REQUEST_LINE: &str = "invalid request line";
pub const REASON_UNSUPPORTED_VERSION: &str = "unsupported HTTP version";
pub const REASON_INVALID_HEADER_LINE: &str = "invalid header line";
pub const REASON_DUPLICATE_CONTENT_LENGTH: &str = "duplicate Content-Length header";
pub const REASON_INVALID_CONTENT_LENGTH: &str = "invalid Content-Length";
pub const REASON_INVALID_CHUNKED_BODY: &str = "invalid chunked body";
pub const REASON_BODY_TOO_LARGE: &str = "request body too large";
pub const REASON_INVALID_MULTIPART_BODY: &str = "invalid multipart body";
pub const REASON_INCOMPLETE_HEAD: &str = "unexpected end of the request head";

#[derive(Debug)]
pub(crate) enum RequestError {
    Silent,
    Respond {
        status: u16,
        body: &'static str,
        log: Option<String>,
    },
}

impl RequestError {
    fn bad(reason: &str) -> RequestError {
        RequestError::Respond {
            status: 400,
            body: MSG_BAD_REQUEST,
            log: Some(reason.to_string()),
        }
    }

    fn bad_quiet() -> RequestError {
        RequestError::Respond {
            status: 400,
            body: MSG_BAD_REQUEST,
            log: None,
        }
    }

    fn too_large() -> RequestError {
        RequestError::Respond {
            status: 400,
            body: MSG_BODY_TOO_LARGE,
            log: Some(REASON_BODY_TOO_LARGE.to_string()),
        }
    }

    pub(crate) fn small_buffer(logged: Option<&str>) -> RequestError {
        RequestError::Respond {
            status: 431,
            body: MSG_HEADER_TOO_LARGE,
            log: logged.map(str::to_string),
        }
    }

    fn direct_io(e: io::Error) -> RequestError {
        if e.kind() == io::ErrorKind::TimedOut {
            return RequestError::Respond {
                status: 408,
                body: MSG_REQUEST_TIMEOUT,
                log: None,
            };
        }
        RequestError::Respond {
            status: 400,
            body: MSG_BAD_REQUEST,
            log: if quiet_io(&e) {
                None
            } else {
                Some(e.to_string())
            },
        }
    }

    fn wrapped(reason: &str, cause: Option<&io::Error>) -> RequestError {
        match cause {
            Some(e) if quiet_io(e) => RequestError::bad_quiet(),
            _ => RequestError::bad(reason),
        }
    }

    fn wrapped_io(e: &io::Error) -> RequestError {
        if quiet_io(e) {
            RequestError::bad_quiet()
        } else {
            RequestError::bad(&e.to_string())
        }
    }
}

pub(crate) fn quiet_io(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
    )
}

#[derive(Debug)]
pub enum MultipartFormError {
    NoMultipartForm,
    UnsupportedContentEncoding,
    Multipart(MultipartError),
}

impl fmt::Display for MultipartFormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MultipartFormError::NoMultipartForm => {
                f.write_str("request has no multipart/form-data Content-Type")
            }
            MultipartFormError::UnsupportedContentEncoding => {
                f.write_str("unsupported Content-Encoding")
            }
            MultipartFormError::Multipart(e) => {
                write!(f, "cannot read multipart/form-data body: {e}")
            }
        }
    }
}

impl std::error::Error for MultipartFormError {}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub header: RequestHeader,
    uri: OnceCell<URI>,
    body: Vec<u8>,
    multipart_form: Option<Form>,
    multipart_form_boundary: Vec<u8>,
    marshaled_form: OnceCell<Vec<u8>>,
    is_tls: bool,
}

impl Request {
    pub fn new() -> Request {
        Request::default()
    }

    pub fn reset(&mut self) {
        *self = Request::default();
    }

    pub fn copy_to(&self, dst: &mut Request) {
        *dst = self.clone();
    }

    pub fn is_tls(&self) -> bool {
        self.is_tls
    }

    pub fn set_tls(&mut self, is_tls: bool) {
        self.is_tls = is_tls;
        self.uri = OnceCell::new();
    }

    pub fn uri(&self) -> &URI {
        self.uri.get_or_init(|| {
            let mut u = URI::new();
            let _ = u.parse_tls(self.header.host(), self.header.request_uri(), self.is_tls);
            u
        })
    }

    pub fn uri_mut(&mut self) -> &mut URI {
        self.uri();
        match self.uri.get_mut() {
            Some(u) => u,
            None => unreachable!("uri is initialized"),
        }
    }

    pub fn set_uri(&mut self, uri: Option<URI>) {
        self.uri = OnceCell::new();
        if let Some(u) = uri {
            let _ = self.uri.set(u);
        }
    }

    pub fn host(&self) -> &[u8] {
        self.uri().host()
    }

    pub fn set_host(&mut self, host: impl AsRef<[u8]>) {
        self.uri_mut().set_host(host);
    }

    pub fn request_uri(&self) -> &[u8] {
        self.header.request_uri()
    }

    pub fn set_request_uri(&mut self, request_uri: impl AsRef<[u8]>) {
        self.header.set_request_uri(request_uri);
        self.uri = OnceCell::new();
    }

    pub fn connection_close(&self) -> bool {
        self.header.connection_close()
    }

    pub fn set_connection_close(&mut self) {
        self.header.set_connection_close();
    }

    fn only_multipart_form(&self) -> bool {
        self.multipart_form.is_some() && self.body.is_empty()
    }

    pub fn body(&self) -> &[u8] {
        if self.only_multipart_form() {
            return self.marshaled_form.get_or_init(|| {
                let mut out = Vec::new();
                match self.multipart_form.as_ref() {
                    Some(form) => {
                        match write_multipart_form(&mut out, form, &self.multipart_form_boundary) {
                            Ok(()) => out,
                            Err(msg) => msg.into_bytes(),
                        }
                    }
                    None => out,
                }
            });
        }
        &self.body
    }

    pub fn set_body(&mut self, body: impl AsRef<[u8]>) {
        self.remove_multipart_form_files();
        self.body.clear();
        self.body.extend_from_slice(body.as_ref());
    }

    pub fn set_body_string(&mut self, body: impl AsRef<str>) {
        self.set_body(body.as_ref().as_bytes());
    }

    pub fn append_body(&mut self, p: impl AsRef<[u8]>) {
        self.remove_multipart_form_files();
        self.body.extend_from_slice(p.as_ref());
    }

    pub fn reset_body(&mut self) {
        self.remove_multipart_form_files();
        self.body = Vec::new();
    }

    pub fn remove_multipart_form_files(&mut self) {
        self.multipart_form = None;
        self.marshaled_form = OnceCell::new();
        self.multipart_form_boundary.clear();
    }

    pub fn multipart_form(&mut self) -> Result<&Form, MultipartFormError> {
        if self.multipart_form.is_none() {
            self.multipart_form_boundary = self.header.multipart_form_boundary().to_vec();
            if self.multipart_form_boundary.is_empty() {
                return Err(MultipartFormError::NoMultipartForm);
            }
            if !self.header.content_encoding().is_empty() {
                return Err(MultipartFormError::UnsupportedContentEncoding);
            }
            let body = self.body.clone();
            let size = body.len() as i64;
            let form =
                read_multipart_form(body.as_slice(), &self.multipart_form_boundary, size, size)
                    .map_err(MultipartFormError::Multipart)?;
            self.multipart_form = Some(form);
            self.marshaled_form = OnceCell::new();
        }
        match self.multipart_form.as_ref() {
            Some(f) => Ok(f),
            None => unreachable!("multipart form is set"),
        }
    }

    pub fn may_continue(&self) -> bool {
        self.header.may_continue()
    }

    pub(crate) fn read_limit_body<R: Read>(
        &mut self,
        r: &mut BufReader<R>,
        max_body_size: usize,
        pre_parse_multipart_form: bool,
    ) -> Result<(), RequestError> {
        if self.may_continue() {
            return Ok(());
        }
        self.continue_read_body(r, max_body_size, pre_parse_multipart_form)
    }

    pub(crate) fn continue_read_body<R: Read>(
        &mut self,
        r: &mut BufReader<R>,
        max_body_size: usize,
        pre_parse_multipart_form: bool,
    ) -> Result<(), RequestError> {
        let max = i64::try_from(max_body_size).unwrap_or(i64::MAX);
        let content_length = self.header.content_length();
        if content_length > 0 {
            if max > 0 && content_length > max {
                return Err(RequestError::too_large());
            }
            if pre_parse_multipart_form {
                self.multipart_form_boundary = self.header.multipart_form_boundary().to_vec();
                if !self.multipart_form_boundary.is_empty()
                    && self.header.content_encoding().is_empty()
                {
                    let boundary = self.multipart_form_boundary.clone();
                    return match read_multipart_form(
                        &mut *r,
                        &boundary,
                        content_length,
                        DEFAULT_MAX_IN_MEMORY_FILE_SIZE,
                    ) {
                        Ok(form) => {
                            self.multipart_form = Some(form);
                            self.marshaled_form = OnceCell::new();
                            Ok(())
                        }
                        Err(e) => {
                            let quiet = e.is_quiet();
                            self.reset();
                            if quiet {
                                Err(RequestError::bad_quiet())
                            } else {
                                Err(RequestError::bad(REASON_INVALID_MULTIPART_BODY))
                            }
                        }
                    };
                }
            }
        }

        if content_length == -2 {
            if !self.header.ignore_body() {
                self.header.set_content_length(0);
            }
            return Ok(());
        }

        let mut body = std::mem::take(&mut self.body);
        body.clear();
        let res = if content_length >= 0 {
            read_body_fixed(r, &mut body, content_length, max)
        } else {
            match read_body_chunked(r, &mut body, max) {
                Ok(()) => {
                    if body.is_empty() {
                        self.header.set_content_length(0);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        };
        if let Err(e) = res {
            self.reset();
            return Err(e);
        }
        self.body = body;

        if content_length == -1 {
            match self.read_trailer(r) {
                Ok(()) | Err(TrailerOutcome::Eof) => {}
                Err(TrailerOutcome::Fail(e)) => return Err(e),
            }
        }
        Ok(())
    }

    fn read_trailer<R: Read>(&mut self, r: &mut BufReader<R>) -> Result<(), TrailerOutcome> {
        let mut n = 1;
        loop {
            let (b, err) = r.peek(n);
            if b.is_empty() {
                return match err {
                    Some(PeekErr::Read(e)) if e.is_timeout() => Err(TrailerOutcome::Fail(
                        RequestError::bad(REASON_INVALID_CHUNKED_BODY),
                    )),
                    Some(PeekErr::Read(ReadErr::Eof)) => Err(TrailerOutcome::Eof),
                    _ if n == 1 => Err(TrailerOutcome::Eof),
                    Some(PeekErr::BufferFull) => Err(TrailerOutcome::Fail(
                        RequestError::small_buffer(Some(REASON_INVALID_CHUNKED_BODY)),
                    )),
                    Some(PeekErr::Read(ReadErr::Io(e))) => {
                        Err(TrailerOutcome::Fail(RequestError::wrapped_io(&e)))
                    }
                    None => Err(TrailerOutcome::Eof),
                };
            }
            let read_eof = matches!(err, Some(PeekErr::Read(ReadErr::Eof)));
            let peek_err = err;
            let buf = r.buffer().to_vec();
            match self.header.parse_trailer_section(&buf) {
                Ok(len) => {
                    let _ = r.discard(len);
                    return Ok(());
                }
                Err(_) if read_eof => return Err(TrailerOutcome::Eof),
                Err(TrailerError::Eof | TrailerError::Invalid) => {
                    return Err(TrailerOutcome::Fail(RequestError::bad(
                        REASON_INVALID_CHUNKED_BODY,
                    )));
                }
                Err(TrailerError::NeedMore) => match peek_err {
                    None => {}
                    Some(_) if is_only_crlf(&buf) => return Err(TrailerOutcome::Eof),
                    Some(PeekErr::BufferFull) => {
                        if buf.len() >= r.size() {
                            return Err(TrailerOutcome::Fail(RequestError::small_buffer(Some(
                                REASON_INVALID_CHUNKED_BODY,
                            ))));
                        }
                    }
                    Some(PeekErr::Read(ReadErr::Io(e))) => {
                        return Err(TrailerOutcome::Fail(RequestError::wrapped_io(&e)));
                    }
                    Some(PeekErr::Read(ReadErr::Eof)) => return Err(TrailerOutcome::Eof),
                },
            }
            n = r.buffered() + 1;
        }
    }

    pub(crate) fn read_header<R: Read>(
        &mut self,
        r: &mut BufReader<R>,
        wait_for_more: bool,
    ) -> Result<(), HeadError> {
        let mut n = 1;
        loop {
            match self.try_read_header(r, n) {
                Ok(()) => return Ok(()),
                Err(HeadError::NeedMore) if wait_for_more => {
                    n = r.buffered() + 1;
                }
                Err(e) => {
                    self.header.reset();
                    return Err(e);
                }
            }
        }
    }

    fn try_read_header<R: Read>(
        &mut self,
        r: &mut BufReader<R>,
        n: usize,
    ) -> Result<(), HeadError> {
        self.header.reset();
        let (b, err) = r.peek(n);
        if b.is_empty() {
            return Err(match err {
                Some(PeekErr::Read(ReadErr::Eof)) => HeadError::Eof,
                Some(PeekErr::BufferFull) => HeadError::SmallBuffer,
                Some(PeekErr::Read(ReadErr::Io(e))) => {
                    if n == 1 {
                        HeadError::NothingRead(e)
                    } else {
                        HeadError::Io(e)
                    }
                }
                None => HeadError::Eof,
            });
        }
        let buf = r.buffer().to_vec();
        match self.header.parse(&buf) {
            Ok(len) => {
                let _ = r.discard(len);
                Ok(())
            }
            Err(ParseError::NeedMore) => match err {
                None => Err(HeadError::NeedMore),
                Some(_) if is_only_crlf(&buf) => Err(HeadError::Eof),
                Some(PeekErr::BufferFull) => {
                    if buf.len() >= r.size() {
                        Err(HeadError::SmallBuffer)
                    } else {
                        Err(HeadError::NeedMore)
                    }
                }
                Some(PeekErr::Read(ReadErr::Eof)) => Err(HeadError::Incomplete),
                Some(PeekErr::Read(ReadErr::Io(e))) => Err(HeadError::Io(e)),
            },
            Err(ParseError::RequestLine) => Err(HeadError::Parse(REASON_INVALID_REQUEST_LINE)),
            Err(ParseError::Version) => Err(HeadError::Parse(REASON_UNSUPPORTED_VERSION)),
            Err(ParseError::HeaderLine) => Err(HeadError::Parse(REASON_INVALID_HEADER_LINE)),
            Err(ParseError::DuplicateContentLength) => {
                Err(HeadError::Parse(REASON_DUPLICATE_CONTENT_LENGTH))
            }
            Err(ParseError::InvalidContentLength) => {
                Err(HeadError::Parse(REASON_INVALID_CONTENT_LENGTH))
            }
            Err(ParseError::BadTrailer) => Err(HeadError::BadTrailer),
        }
    }
}

enum TrailerOutcome {
    Eof,
    Fail(RequestError),
}

#[derive(Debug)]
pub(crate) enum HeadError {
    NeedMore,
    Eof,
    NothingRead(io::Error),
    SmallBuffer,
    Incomplete,
    Io(io::Error),
    Parse(&'static str),
    BadTrailer,
}

impl HeadError {
    pub(crate) fn into_request_error(self, first_request: bool) -> RequestError {
        match self {
            HeadError::NeedMore | HeadError::Eof => RequestError::Silent,
            HeadError::NothingRead(e) => {
                if first_request {
                    RequestError::direct_io(e)
                } else {
                    RequestError::Silent
                }
            }
            HeadError::SmallBuffer => RequestError::small_buffer(None),
            HeadError::Incomplete => RequestError::bad(REASON_INCOMPLETE_HEAD),
            HeadError::Io(e) => RequestError::wrapped_io(&e),
            HeadError::Parse(reason) => RequestError::bad(reason),
            HeadError::BadTrailer => RequestError::bad_quiet(),
        }
    }
}

fn is_only_crlf(b: &[u8]) -> bool {
    b.iter().all(|&c| c == b'\r' || c == b'\n')
}

fn read_full<R: Read>(
    r: &mut BufReader<R>,
    dst: &mut Vec<u8>,
    n: usize,
) -> Result<(), RequestError> {
    let start = dst.len();
    dst.resize(start + n, 0);
    let mut offset = start;
    while offset < dst.len() {
        match r.read_buf(&mut dst[offset..]) {
            Ok(0) => {}
            Ok(k) => offset += k,
            Err(ReadErr::Eof) => {
                dst.truncate(offset);
                return Err(RequestError::bad_quiet());
            }
            Err(ReadErr::Io(e)) => {
                dst.truncate(offset);
                return Err(RequestError::direct_io(e));
            }
        }
    }
    Ok(())
}

fn read_body_fixed<R: Read>(
    r: &mut BufReader<R>,
    dst: &mut Vec<u8>,
    content_length: i64,
    max_body_size: i64,
) -> Result<(), RequestError> {
    if max_body_size > 0 && content_length > max_body_size {
        return Err(RequestError::too_large());
    }
    if content_length == 0 {
        return Ok(());
    }
    read_full(
        r,
        dst,
        usize::try_from(content_length).unwrap_or(usize::MAX),
    )
}

enum HexErr {
    Eof,
    Io(io::Error),
    Empty,
    TooLarge,
}

fn read_hex_int<R: Read>(r: &mut BufReader<R>) -> Result<i64, HexErr> {
    let mut i = 0;
    let mut n: i64 = 0;
    loop {
        let c = match r.read_byte() {
            Ok(c) => c,
            Err(ReadErr::Eof) => {
                if i > 0 {
                    return Ok(n);
                }
                return Err(HexErr::Eof);
            }
            Err(ReadErr::Io(e)) => return Err(HexErr::Io(e)),
        };
        let k = hex2int(c);
        if k == 16 {
            if i == 0 {
                return Err(HexErr::Empty);
            }
            r.unread_byte();
            return Ok(n);
        }
        if i >= MAX_HEX_INT_CHARS {
            return Err(HexErr::TooLarge);
        }
        n = (n << 4) | i64::from(k);
        i += 1;
    }
}

fn broken_chunk(cause: Option<&io::Error>) -> RequestError {
    RequestError::wrapped(REASON_INVALID_CHUNKED_BODY, cause)
}

fn parse_chunk_size<R: Read>(r: &mut BufReader<R>) -> Result<i64, RequestError> {
    let n = match read_hex_int(r) {
        Ok(n) => n,
        Err(HexErr::Eof) => return Err(RequestError::Silent),
        Err(HexErr::Io(e)) => return Err(RequestError::direct_io(e)),
        Err(HexErr::Empty | HexErr::TooLarge) => return Err(broken_chunk(None)),
    };
    loop {
        let c = match r.read_byte() {
            Ok(c) => c,
            Err(ReadErr::Eof) => return Err(broken_chunk(None)),
            Err(ReadErr::Io(e)) => return Err(broken_chunk(Some(&e))),
        };
        if c != b'\r' {
            if c == b'\n' {
                return Err(broken_chunk(None));
            }
            continue;
        }
        r.unread_byte();
        break;
    }
    for exp in [b'\r', b'\n'] {
        match r.read_byte() {
            Ok(c) if c == exp => {}
            Ok(_) => return Err(broken_chunk(None)),
            Err(ReadErr::Eof) => return Err(broken_chunk(None)),
            Err(ReadErr::Io(e)) => return Err(broken_chunk(Some(&e))),
        }
    }
    Ok(n)
}

fn read_body_chunked<R: Read>(
    r: &mut BufReader<R>,
    dst: &mut Vec<u8>,
    max_body_size: i64,
) -> Result<(), RequestError> {
    loop {
        let chunk_size = parse_chunk_size(r)?;
        if chunk_size == 0 {
            return Ok(());
        }
        if max_body_size > 0 && dst.len() as i64 + chunk_size > max_body_size {
            return Err(RequestError::too_large());
        }
        read_full(
            r,
            dst,
            usize::try_from(chunk_size + 2).unwrap_or(usize::MAX),
        )?;
        if !dst.ends_with(b"\r\n") {
            return Err(broken_chunk(None));
        }
        dst.truncate(dst.len() - 2);
    }
}
