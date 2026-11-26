use std::collections::BTreeMap;
use std::io::{self, Read};

use super::bufio::{BufReader, PeekErr, ReadErr, SliceErr};
use super::mime::parse_media_type;
use super::textproto::{MimeHeader, TextprotoError, header_del, header_get, read_mime_header};
use crate::gocompat::strings::equal_fold;

const PEEK_BUFFER_SIZE: usize = 4096;
const MAX_MIME_HEADER_SIZE: i64 = 10 << 20;
const MAX_MIME_HEADERS: i64 = 10000;
const MAX_PARTS: i64 = 1000;

#[derive(Debug)]
pub enum MultipartError {
    EmptyBoundary,
    NextPart(Option<io::Error>),
    LineTooLong,
    FormSize,
    ExpectingNewPart,
    UnexpectedLine,
    Header(TextprotoError),
    MessageTooLarge,
    Read(io::Error),
}

impl std::fmt::Display for MultipartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MultipartError::EmptyBoundary => f.write_str("multipart boundary is empty"),
            MultipartError::NextPart(None) => f.write_str("unexpected end of the multipart body"),
            MultipartError::NextPart(Some(e)) => write!(f, "cannot read the next part: {e}"),
            MultipartError::LineTooLong => f.write_str("multipart line too long"),
            MultipartError::FormSize => f.write_str("form size must be greater than 0"),
            MultipartError::ExpectingNewPart => f.write_str("expecting a new part"),
            MultipartError::UnexpectedLine => f.write_str("unexpected line between parts"),
            MultipartError::Header(e) => write!(f, "invalid part header: {e}"),
            MultipartError::MessageTooLarge => f.write_str("multipart message too large"),
            MultipartError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MultipartError {}

fn quiet_kind(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::TimedOut | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
    )
}

impl MultipartError {
    pub(crate) fn is_quiet(&self) -> bool {
        match self {
            MultipartError::NextPart(Some(e)) => quiet_kind(e.kind()),
            MultipartError::Header(TextprotoError::Read(ReadErr::Io(e))) => quiet_kind(e.kind()),
            MultipartError::Read(e) => {
                quiet_kind(e.kind()) || e.kind() == io::ErrorKind::UnexpectedEof
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
enum StickyErr {
    Eof,
    Io(io::ErrorKind, String),
}

struct StickyErrorReader<R> {
    r: R,
    err: Option<StickyErr>,
}

impl<R: Read> Read for StickyErrorReader<R> {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        match &self.err {
            Some(StickyErr::Eof) => return Ok(0),
            Some(StickyErr::Io(k, m)) => return Err(io::Error::new(*k, m.clone())),
            None => {}
        }
        match self.r.read(p) {
            Ok(0) if !p.is_empty() => {
                self.err = Some(StickyErr::Eof);
                Ok(0)
            }
            Ok(n) => Ok(n),
            Err(e) => {
                if e.kind() != io::ErrorKind::Interrupted {
                    self.err = Some(StickyErr::Io(e.kind(), e.to_string()));
                }
                Err(e)
            }
        }
    }
}

#[derive(Clone, Debug)]
enum PartErr {
    Eof,
    UnexpectedEof,
    BufferFull,
    Io(io::ErrorKind, String),
}

impl PartErr {
    fn into_io(self) -> io::Error {
        match self {
            PartErr::Eof => io::Error::from(io::ErrorKind::UnexpectedEof),
            PartErr::UnexpectedEof => io::Error::from(io::ErrorKind::UnexpectedEof),
            PartErr::BufferFull => io::Error::other("buffer full"),
            PartErr::Io(k, m) => io::Error::new(k, m),
        }
    }
}

#[derive(Default)]
struct PartState {
    n: usize,
    total: i64,
    err: Option<PartErr>,
    read_err: Option<PartErr>,
}

#[derive(Clone, Debug, Default)]
pub struct Part {
    pub header: MimeHeader,
    disposition: Vec<u8>,
    disposition_params: BTreeMap<Vec<u8>, Vec<u8>>,
    quoted_printable: bool,
}

impl Part {
    fn new(header: MimeHeader) -> Part {
        let mut header = header;
        let mut quoted_printable = false;
        if equal_fold(
            header_get(&header, "Content-Transfer-Encoding"),
            b"quoted-printable",
        ) {
            header_del(&mut header, "Content-Transfer-Encoding");
            quoted_printable = true;
        }
        let v = header_get(&header, "Content-Disposition").to_vec();
        let (disposition, disposition_params) = match parse_media_type(&v) {
            Ok((d, p)) => (d, p),
            Err(_) => (Vec::new(), BTreeMap::new()),
        };
        Part {
            header,
            disposition,
            disposition_params,
            quoted_printable,
        }
    }

    pub fn form_name(&self) -> &[u8] {
        if self.disposition != b"form-data" {
            return &[];
        }
        self.disposition_params
            .get(b"name".as_slice())
            .map_or(&[], Vec::as_slice)
    }

    pub fn file_name(&self) -> Vec<u8> {
        let filename = self
            .disposition_params
            .get(b"filename".as_slice())
            .map_or(&[][..], Vec::as_slice);
        if filename.is_empty() {
            return Vec::new();
        }
        filepath_base(filename)
    }
}

fn filepath_base(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }
    let mut path = path;
    while let Some((&b'/', rest)) = path.split_last() {
        path = rest;
    }
    if let Some(i) = path.iter().rposition(|&c| c == b'/') {
        path = &path[i + 1..];
    }
    if path.is_empty() {
        return b"/".to_vec();
    }
    path.to_vec()
}

pub struct Reader<R: Read> {
    br: BufReader<StickyErrorReader<R>>,
    current: Option<PartState>,
    current_qp: bool,
    parts_read: usize,
    nl: Vec<u8>,
    nl_dash_boundary: Vec<u8>,
    dash_boundary_dash: Vec<u8>,
    dash_boundary: Vec<u8>,
}

impl<R: Read> Reader<R> {
    pub fn new(r: R, boundary: &[u8]) -> Reader<R> {
        let mut b = b"\r\n--".to_vec();
        b.extend_from_slice(boundary);
        b.extend_from_slice(b"--");
        Reader {
            br: BufReader::with_capacity(PEEK_BUFFER_SIZE, StickyErrorReader { r, err: None }),
            current: None,
            current_qp: false,
            parts_read: 0,
            nl: b[..2].to_vec(),
            nl_dash_boundary: b[..b.len() - 2].to_vec(),
            dash_boundary_dash: b[2..].to_vec(),
            dash_boundary: b[2..b.len() - 2].to_vec(),
        }
    }

    pub fn into_inner(self) -> R {
        self.br.into_inner().r
    }

    fn close_current(&mut self) {
        if self.current.is_none() {
            return;
        }
        let mut sink = [0u8; 4096];
        loop {
            match self.raw_read(&mut sink) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }

    pub fn next_part(&mut self) -> Result<Option<Part>, MultipartError> {
        self.next_part_limited(MAX_MIME_HEADER_SIZE, MAX_MIME_HEADERS)
    }

    fn next_part_limited(
        &mut self,
        max_header_size: i64,
        max_headers: i64,
    ) -> Result<Option<Part>, MultipartError> {
        self.close_current();
        if self.dash_boundary == b"--" {
            return Err(MultipartError::EmptyBoundary);
        }
        let mut expect_new_part = false;
        loop {
            let (line, err) = self.br.read_slice(b'\n');
            match err {
                Some(SliceErr::Read(ReadErr::Eof)) if self.is_final_boundary(&line) => {
                    return Ok(None);
                }
                Some(SliceErr::Read(e)) => {
                    return Err(MultipartError::NextPart(match e {
                        ReadErr::Eof => None,
                        ReadErr::Io(e) => Some(e),
                    }));
                }
                Some(SliceErr::BufferFull) => return Err(MultipartError::LineTooLong),
                None => {}
            }
            if self.is_boundary_delimiter_line(&line) {
                self.parts_read += 1;
                let header = match read_mime_header(&mut self.br, max_header_size, max_headers) {
                    Ok(h) => h,
                    Err(TextprotoError::MessageTooLarge) => {
                        return Err(MultipartError::MessageTooLarge);
                    }
                    Err(e) => return Err(MultipartError::Header(e)),
                };
                let part = Part::new(header);
                self.current = Some(PartState::default());
                self.current_qp = part.quoted_printable;
                return Ok(Some(part));
            }
            if self.is_final_boundary(&line) {
                return Ok(None);
            }
            if expect_new_part {
                return Err(MultipartError::ExpectingNewPart);
            }
            if self.parts_read == 0 {
                continue;
            }
            if line == self.nl {
                expect_new_part = true;
                continue;
            }
            return Err(MultipartError::UnexpectedLine);
        }
    }

    fn is_final_boundary(&self, line: &[u8]) -> bool {
        let Some(rest) = line.strip_prefix(self.dash_boundary_dash.as_slice()) else {
            return false;
        };
        let rest = skip_lwsp_char(rest);
        rest.is_empty() || rest == self.nl.as_slice()
    }

    fn is_boundary_delimiter_line(&mut self, line: &[u8]) -> bool {
        let Some(rest) = line.strip_prefix(self.dash_boundary.as_slice()) else {
            return false;
        };
        let rest = skip_lwsp_char(rest);
        if self.parts_read == 0 && rest == b"\n" {
            self.nl.remove(0);
            self.nl_dash_boundary.remove(0);
        }
        rest == self.nl.as_slice()
    }

    fn raw_read(&mut self, d: &mut [u8]) -> io::Result<usize> {
        let Some(mut p) = self.current.take() else {
            return Ok(0);
        };
        let result = self.raw_read_state(&mut p, d);
        self.current = Some(p);
        result
    }

    fn raw_read_state(&mut self, p: &mut PartState, d: &mut [u8]) -> io::Result<usize> {
        while p.n == 0 && p.err.is_none() {
            let peek_len = self.br.buffered();
            let peek = self.br.buffer().to_vec();
            let (n, err) = scan_until_boundary(
                &peek,
                &self.dash_boundary,
                &self.nl_dash_boundary,
                p.total,
                p.read_err.clone(),
            );
            p.n = n;
            p.err = err;
            if p.n == 0 && p.err.is_none() {
                let (_, perr) = self.br.peek(peek_len + 1);
                p.read_err = match perr {
                    None => None,
                    Some(PeekErr::BufferFull) => Some(PartErr::BufferFull),
                    Some(PeekErr::Read(ReadErr::Eof)) => Some(PartErr::UnexpectedEof),
                    Some(PeekErr::Read(ReadErr::Io(e))) => {
                        Some(PartErr::Io(e.kind(), e.to_string()))
                    }
                };
            }
        }
        if p.n == 0 {
            return match p.err.clone() {
                None | Some(PartErr::Eof) => Ok(0),
                Some(e) => Err(e.into_io()),
            };
        }
        let n = d.len().min(p.n);
        let n = self.br.read_buf(&mut d[..n]).unwrap_or(0);
        p.total += n as i64;
        p.n -= n;
        Ok(n)
    }

    pub fn part_reader(&mut self) -> Box<dyn Read + '_> {
        if self.current_qp {
            Box::new(QpReader::new(PartReader { mr: self }))
        } else {
            Box::new(PartReader { mr: self })
        }
    }

    pub fn read_form(&mut self, max_memory: i64) -> Result<Form, MultipartError> {
        let mut form = Form::default();
        let mut max_parts = MAX_PARTS;
        let mut max_headers = MAX_MIME_HEADERS;
        let mut max_file_memory_bytes = max_memory;
        if max_file_memory_bytes == i64::MAX {
            max_file_memory_bytes -= 1;
        }
        let mut max_memory_bytes = max_memory.wrapping_add(10 << 20);
        if max_memory_bytes <= 0 {
            max_memory_bytes = if max_memory < 0 { 0 } else { i64::MAX };
        }
        loop {
            let Some(p) = self.next_part_limited(max_memory_bytes, max_headers)? else {
                break;
            };
            if max_parts <= 0 {
                return Err(MultipartError::MessageTooLarge);
            }
            max_parts -= 1;

            let name = p.form_name().to_vec();
            if name.is_empty() {
                continue;
            }
            let filename = p.file_name();

            const MAP_ENTRY_OVERHEAD: i64 = 200;
            max_memory_bytes -= name.len() as i64;
            max_memory_bytes -= MAP_ENTRY_OVERHEAD;
            if max_memory_bytes < 0 {
                return Err(MultipartError::MessageTooLarge);
            }

            if filename.is_empty() {
                let b = copy_n(&mut self.part_reader(), max_memory_bytes.saturating_add(1))?;
                max_memory_bytes -= b.len() as i64;
                if max_memory_bytes < 0 {
                    return Err(MultipartError::MessageTooLarge);
                }
                form.value.entry(name).or_default().push(b);
                continue;
            }

            const FILE_HEADER_SIZE: i64 = 100;
            max_memory_bytes -= mime_header_size(&p.header);
            max_memory_bytes -= MAP_ENTRY_OVERHEAD;
            max_memory_bytes -= FILE_HEADER_SIZE;
            if max_memory_bytes < 0 {
                return Err(MultipartError::MessageTooLarge);
            }
            for v in p.header.values() {
                max_headers -= v.len() as i64;
            }

            let mut content = copy_n(
                &mut self.part_reader(),
                max_file_memory_bytes.saturating_add(1),
            )?;
            let n = content.len() as i64;
            if n > max_file_memory_bytes {
                let mut rest = Vec::new();
                self.part_reader()
                    .read_to_end(&mut rest)
                    .map_err(MultipartError::Read)?;
                content.extend_from_slice(&rest);
            } else {
                max_file_memory_bytes -= n;
                max_memory_bytes -= n;
            }
            let size = content.len() as i64;
            form.file.entry(name).or_default().push(FileHeader {
                filename,
                header: p.header,
                size,
                content,
            });
        }
        Ok(form)
    }
}

fn copy_n(r: &mut dyn Read, limit: i64) -> Result<Vec<u8>, MultipartError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut remaining = limit.max(0) as u64;
    while remaining > 0 {
        let want = buf
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        match r.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                remaining -= n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(MultipartError::Read(e)),
        }
    }
    Ok(out)
}

fn mime_header_size(h: &MimeHeader) -> i64 {
    let mut size = 400;
    for (k, vs) in h {
        size += k.len() as i64;
        size += 200;
        for v in vs {
            size += v.len() as i64;
        }
    }
    size
}

struct PartReader<'a, R: Read> {
    mr: &'a mut Reader<R>,
}

impl<R: Read> Read for PartReader<'_, R> {
    fn read(&mut self, d: &mut [u8]) -> io::Result<usize> {
        self.mr.raw_read(d)
    }
}

fn skip_lwsp_char(mut b: &[u8]) -> &[u8] {
    while let Some((&(b' ' | b'\t'), rest)) = b.split_first() {
        b = rest;
    }
    b
}

fn scan_until_boundary(
    buf: &[u8],
    dash_boundary: &[u8],
    nl_dash_boundary: &[u8],
    total: i64,
    read_err: Option<PartErr>,
) -> (usize, Option<PartErr>) {
    if total == 0 {
        if buf.starts_with(dash_boundary) {
            match match_after_prefix(buf, dash_boundary, read_err.is_some()) {
                -1 => return (dash_boundary.len(), None),
                0 => return (0, None),
                _ => return (0, Some(PartErr::Eof)),
            }
        }
        if dash_boundary.starts_with(buf) {
            return (0, read_err);
        }
    }
    if let Some(i) = super::bytesconv::index_of(buf, nl_dash_boundary) {
        match match_after_prefix(&buf[i..], nl_dash_boundary, read_err.is_some()) {
            -1 => return (i + nl_dash_boundary.len(), None),
            0 => return (i, None),
            _ => return (i, Some(PartErr::Eof)),
        }
    }
    if nl_dash_boundary.starts_with(buf) {
        return (0, read_err);
    }
    if let Some(i) = buf.iter().rposition(|&c| c == nl_dash_boundary[0])
        && nl_dash_boundary.starts_with(&buf[i..])
    {
        return (i, None);
    }
    (buf.len(), read_err)
}

fn match_after_prefix(buf: &[u8], prefix: &[u8], has_read_err: bool) -> i32 {
    if buf.len() == prefix.len() {
        return if has_read_err { 1 } else { 0 };
    }
    let c = buf[prefix.len()];
    if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
        return 1;
    }
    if c == b'-' {
        if buf.len() == prefix.len() + 1 {
            return if has_read_err { -1 } else { 0 };
        }
        if buf[prefix.len() + 1] == b'-' {
            return 1;
        }
    }
    -1
}

struct QpReader<R: Read> {
    br: BufReader<R>,
    rerr: Option<io::Error>,
    rerr_eof: bool,
    line: Vec<u8>,
    pos: usize,
}

impl<R: Read> QpReader<R> {
    fn new(r: R) -> QpReader<R> {
        QpReader {
            br: BufReader::with_capacity(4096, r),
            rerr: None,
            rerr_eof: false,
            line: Vec::new(),
            pos: 0,
        }
    }
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn read_hex_byte(v: &[u8]) -> Result<u8, io::Error> {
    if v.len() < 2 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    match (from_hex(v[0]), from_hex(v[1])) {
        (Some(h), Some(l)) => Ok((h << 4) | l),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid quoted-printable hex byte",
        )),
    }
}

impl<R: Read> Read for QpReader<R> {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        while n < p.len() {
            if self.pos >= self.line.len() {
                if self.rerr_eof && self.rerr.is_none() {
                    return Ok(n);
                }
                if let Some(e) = self.rerr.take() {
                    if n > 0 {
                        self.rerr = Some(e);
                        return Ok(n);
                    }
                    return Err(e);
                }
                let (line, err) = self.br.read_slice(b'\n');
                match err {
                    None => {}
                    Some(SliceErr::Read(ReadErr::Eof)) => self.rerr_eof = true,
                    Some(SliceErr::Read(ReadErr::Io(e))) => self.rerr = Some(e),
                    Some(SliceErr::BufferFull) => self.rerr = Some(io::Error::other("buffer full")),
                }
                let has_lf = line.ends_with(b"\n");
                let has_cr = line.ends_with(b"\r\n");
                let mut end = line.len();
                while end > 0 && matches!(line[end - 1], b'\n' | b'\r' | b' ' | b'\t') {
                    end -= 1;
                }
                let mut trimmed = line[..end].to_vec();
                if trimmed.ends_with(b"=") {
                    let right_stripped = &line[end..];
                    trimmed.pop();
                    let soft_break = right_stripped.starts_with(b"\n")
                        || right_stripped.starts_with(b"\r\n")
                        || (right_stripped.is_empty() && !trimmed.is_empty() && self.rerr_eof);
                    if !soft_break {
                        self.rerr = Some(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid bytes after = in quoted-printable data",
                        ));
                        self.rerr_eof = false;
                    }
                } else if has_lf {
                    if has_cr {
                        trimmed.extend_from_slice(b"\r\n");
                    } else {
                        trimmed.push(b'\n');
                    }
                }
                self.line = trimmed;
                self.pos = 0;
                continue;
            }
            let rest = &self.line[self.pos..];
            let mut b = rest[0];
            let mut advance = 1;
            match b {
                b'=' => match read_hex_byte(&rest[1..]) {
                    Ok(v) => {
                        b = v;
                        advance = 3;
                    }
                    Err(e) => {
                        if rest.len() >= 2 && rest[1] != b'\r' && rest[1] != b'\n' {
                            b = b'=';
                        } else {
                            if n > 0 {
                                return Ok(n);
                            }
                            return Err(e);
                        }
                    }
                },
                b'\t' | b'\r' | b'\n' => {}
                0x80..=0xff => {}
                c if !(b' '..=b'~').contains(&c) => {
                    if n > 0 {
                        return Ok(n);
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid unescaped byte in quoted-printable body",
                    ));
                }
                _ => {}
            }
            p[n] = b;
            n += 1;
            self.pos += advance;
        }
        Ok(n)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Form {
    pub value: BTreeMap<Vec<u8>, Vec<Vec<u8>>>,
    pub file: BTreeMap<Vec<u8>, Vec<FileHeader>>,
}

#[derive(Clone, Debug, Default)]
pub struct FileHeader {
    pub filename: Vec<u8>,
    pub header: MimeHeader,
    pub size: i64,
    content: Vec<u8>,
}

impl FileHeader {
    pub fn open(&self) -> io::Cursor<&[u8]> {
        io::Cursor::new(self.content.as_slice())
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }
}

pub fn read_multipart_form<R: Read>(
    r: R,
    boundary: &[u8],
    size: i64,
    max_in_memory_file_size: i64,
) -> Result<Form, MultipartError> {
    if size <= 0 {
        return Err(MultipartError::FormSize);
    }
    let lr = r.take(size as u64);
    let mut mr = Reader::new(lr, boundary);
    mr.read_form(max_in_memory_file_size)
}

fn valid_boundary(boundary: &[u8]) -> Result<(), &'static str> {
    if boundary.is_empty() || boundary.len() > 70 {
        return Err("invalid boundary length");
    }
    let end = boundary.len() - 1;
    for (i, &b) in boundary.iter().enumerate() {
        if b.is_ascii_alphanumeric() {
            continue;
        }
        match b {
            b'\'' | b'(' | b')' | b'+' | b'_' | b',' | b'-' | b'.' | b'/' | b':' | b'=' | b'?' => {
                continue;
            }
            b' ' if i != end => continue,
            _ => {}
        }
        return Err("invalid boundary character");
    }
    Ok(())
}

fn escape_quotes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            _ => out.push(c),
        }
    }
    out
}

pub fn write_multipart_form(dst: &mut Vec<u8>, f: &Form, boundary: &[u8]) -> Result<(), String> {
    if boundary.is_empty() {
        return Err("form boundary cannot be empty".to_string());
    }
    if let Err(e) = valid_boundary(boundary) {
        return Err(format!(
            "cannot use form boundary {}: {e}",
            crate::gocompat::strconv::quote(boundary)
        ));
    }
    let mut first = true;
    let mut begin_part = |dst: &mut Vec<u8>, header: &MimeHeader| {
        if first {
            dst.extend_from_slice(b"--");
        } else {
            dst.extend_from_slice(b"\r\n--");
        }
        first = false;
        dst.extend_from_slice(boundary);
        dst.extend_from_slice(b"\r\n");
        for (k, vs) in header {
            for v in vs {
                dst.extend_from_slice(k.as_bytes());
                dst.extend_from_slice(b": ");
                dst.extend_from_slice(v);
                dst.extend_from_slice(b"\r\n");
            }
        }
        dst.extend_from_slice(b"\r\n");
    };
    for (k, vv) in &f.value {
        for v in vv {
            let mut h = MimeHeader::new();
            let mut cd = b"form-data; name=\"".to_vec();
            cd.extend_from_slice(&escape_quotes(k));
            cd.push(b'"');
            h.insert("Content-Disposition".to_string(), vec![cd]);
            begin_part(dst, &h);
            dst.extend_from_slice(v);
        }
    }
    for fvv in f.file.values() {
        for fv in fvv {
            begin_part(dst, &fv.header);
            dst.extend_from_slice(&fv.content);
        }
    }
    dst.extend_from_slice(b"\r\n--");
    dst.extend_from_slice(boundary);
    dst.extend_from_slice(b"--\r\n");
    Ok(())
}
