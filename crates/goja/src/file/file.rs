use crate::deps::sourcemap::url::{self, Url};
use crate::deps::sourcemap::{Consumer, path};
use gocompat::strings::trim_space;
use gocompat::utf8;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

pub type Idx = i64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub filename: String,
    pub line: i64,
    pub column: i64,
}

impl Position {
    fn is_valid(&self) -> bool {
        self.line > 0
    }

    pub fn string(&self) -> String {
        let mut s = self.filename.clone();
        if self.is_valid() {
            if !s.is_empty() {
                s.push(':');
            }
            s.push_str(&format!("{}:{}", self.line, self.column));
        }
        if s.is_empty() {
            s = "-".to_string();
        }
        s
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

#[derive(Default)]
pub struct FileSet {
    files: Vec<Arc<File>>,
    last: Option<Arc<File>>,
}

impl FileSet {
    pub fn new() -> FileSet {
        FileSet::default()
    }

    pub fn add_file(&mut self, filename: &str, src: &[u8]) -> i64 {
        let base = self.next_base();
        let file = Arc::new(File::new(filename, src, base));
        self.files.push(Arc::clone(&file));
        self.last = Some(file);
        base
    }

    fn next_base(&self) -> i64 {
        match &self.last {
            None => 1,
            Some(last) => last.base + last.src.len() as i64 + 1,
        }
    }

    pub fn file(&self, idx: Idx) -> Option<Arc<File>> {
        for file in &self.files {
            if idx <= file.base + file.src.len() as i64 {
                return Some(Arc::clone(file));
            }
        }
        None
    }

    pub fn position(&self, idx: Idx) -> Position {
        for file in &self.files {
            if idx <= file.base + file.src.len() as i64 {
                return file.position(idx - file.base);
            }
        }
        Position::default()
    }
}

#[derive(Default)]
struct Lines {
    line_offsets: Vec<i64>,
    last_scanned_offset: i64,
}

pub struct File {
    name: String,
    src: Vec<u8>,
    base: i64,
    source_map: RwLock<Option<Arc<Consumer>>>,
    lines: Mutex<Lines>,
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            .field("name", &self.name)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

fn search(n: usize, mut f: impl FnMut(usize) -> bool) -> usize {
    let (mut i, mut j) = (0usize, n);
    while i < j {
        let h = (i + j) >> 1;
        if !f(h) {
            i = h + 1;
        } else {
            j = h;
        }
    }
    i
}

impl File {
    pub fn new(filename: &str, src: &[u8], base: i64) -> File {
        File {
            name: filename.to_string(),
            src: src.to_vec(),
            base,
            source_map: RwLock::new(None),
            lines: Mutex::new(Lines::default()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &[u8] {
        &self.src
    }

    pub fn base(&self) -> i64 {
        self.base
    }

    pub fn set_source_map(&self, m: Option<Consumer>) {
        *self
            .source_map
            .write()
            .unwrap_or_else(PoisonError::into_inner) = m.map(Arc::new);
    }

    pub fn source_map(&self) -> Option<Arc<Consumer>> {
        self.source_map
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn position(&self, offset: i64) -> Position {
        let line: i64;
        let line_offsets: Vec<i64>;
        {
            let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
            if offset > lines.last_scanned_offset {
                line = self.scan_to(&mut lines, offset);
                line_offsets = lines.line_offsets.clone();
            } else {
                line_offsets = lines.line_offsets.clone();
                line = search(line_offsets.len(), |x| line_offsets[x] > offset) as i64 - 1;
            }
        }
        let mut line_start = 0;
        if line >= 0 {
            line_start = line_offsets[line as usize];
        }
        let row = line + 2;
        let col = offset - line_start + 1;
        if let Some(sm) = self.source_map() {
            let (source, _, row, col, ok) = sm.source(row, col);
            if ok {
                let source_url_str =
                    match resolve_sourcemap_url(self.name.as_bytes(), source.as_bytes()) {
                        Some(u) => u.string(),
                        None => source,
                    };
                return Position {
                    filename: source_url_str,
                    line: row,
                    column: col,
                };
            }
        }
        Position {
            filename: self.name.clone(),
            line: row,
            column: col,
        }
    }

    fn scan_to(&self, lines: &mut Lines, offset: i64) -> i64 {
        let mut o = lines.last_scanned_offset;
        while o < offset {
            let p = find_next_line_start(&self.src[o as usize..]);
            if p == -1 {
                lines.last_scanned_offset = self.src.len() as i64;
                return lines.line_offsets.len() as i64 - 1;
            }
            o += p;
            lines.line_offsets.push(o);
        }
        lines.last_scanned_offset = o;
        if o == offset {
            return lines.line_offsets.len() as i64 - 1;
        }
        lines.line_offsets.len() as i64 - 2
    }
}

pub fn resolve_sourcemap_url(basename: &[u8], source: &[u8]) -> Option<Url> {
    let sm_url = url::parse(trim_space(source));
    match sm_url {
        Ok(sm_url) if !sm_url.is_abs() => {
            let basename = trim_space(basename);
            match url::parse(basename) {
                Ok(base_url) if path::is_abs(&base_url.path) => {
                    Some(base_url.resolve_reference(&sm_url))
                }
                _ => url::parse(&path::join(&[&path::dir(basename), &sm_url.path])).ok(),
            }
        }
        Ok(sm_url) => Some(sm_url),
        Err(_) => None,
    }
}

fn find_next_line_start(s: &[u8]) -> i64 {
    let mut pos = 0;
    while pos < s.len() {
        let (ch, size) = utf8::decode_rune(&s[pos..]);
        match ch {
            0x0D => {
                if pos < s.len() - 1 && s[pos + 1] == b'\n' {
                    return pos as i64 + 2;
                }
                return pos as i64 + 1;
            }
            0x0A => return pos as i64 + 1,
            0x2028 | 0x2029 => return pos as i64 + 3,
            _ => {}
        }
        pos += size;
    }
    -1
}
