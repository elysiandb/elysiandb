use crate::path;
use crate::utf8::{self, RUNE_ERROR, Rune};
use std::ffi::OsStr;
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const SEPARATOR: u8 = b'/';
const PATH_SEPARATORS_LIMIT: usize = 10000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadPattern;

impl fmt::Display for BadPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("syntax error in pattern")
    }
}

impl std::error::Error for BadPattern {}

pub fn match_pattern(mut pattern: &[u8], mut name: &[u8]) -> Result<bool, BadPattern> {
    'pattern: while !pattern.is_empty() {
        let (star, chunk, rest) = scan_chunk(pattern);
        pattern = rest;
        if star && chunk.is_empty() {
            return Ok(!name.contains(&SEPARATOR));
        }

        let (t, ok, err) = match_chunk(chunk, name);
        if ok && (t.is_empty() || !pattern.is_empty()) {
            name = t;
            continue;
        }
        if let Some(err) = err {
            return Err(err);
        }

        if star {
            let mut i = 0;
            while i < name.len() && name[i] != SEPARATOR {
                let (t, ok, err) = match_chunk(chunk, &name[i + 1..]);
                if ok {
                    if pattern.is_empty() && !t.is_empty() {
                        i += 1;
                        continue;
                    }
                    name = t;
                    continue 'pattern;
                }
                if let Some(err) = err {
                    return Err(err);
                }
                i += 1;
            }
        }

        while !pattern.is_empty() {
            let (_, chunk, rest) = scan_chunk(pattern);
            pattern = rest;
            if let (_, _, Some(err)) = match_chunk(chunk, b"") {
                return Err(err);
            }
        }

        return Ok(false);
    }

    Ok(name.is_empty())
}

fn scan_chunk(mut pattern: &[u8]) -> (bool, &[u8], &[u8]) {
    let mut star = false;
    while !pattern.is_empty() && pattern[0] == b'*' {
        pattern = &pattern[1..];
        star = true;
    }

    let mut in_range = false;
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            b'\\' => {
                if i + 1 < pattern.len() {
                    i += 1;
                }
            }
            b'[' => in_range = true,
            b']' => in_range = false,
            b'*' => {
                if !in_range {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }

    (star, &pattern[..i], &pattern[i..])
}

fn match_chunk<'a>(mut chunk: &[u8], mut s: &'a [u8]) -> (&'a [u8], bool, Option<BadPattern>) {
    let mut failed = false;
    while !chunk.is_empty() {
        if !failed && s.is_empty() {
            failed = true;
        }

        match chunk[0] {
            b'[' => {
                let mut r: Rune = 0;
                if !failed {
                    let (decoded, n) = utf8::decode_rune(s);
                    r = decoded;
                    s = &s[n..];
                }
                chunk = &chunk[1..];

                let mut negated = false;
                if !chunk.is_empty() && chunk[0] == b'^' {
                    negated = true;
                    chunk = &chunk[1..];
                }

                let mut matched = false;
                let mut nrange = 0;
                loop {
                    if !chunk.is_empty() && chunk[0] == b']' && nrange > 0 {
                        chunk = &chunk[1..];
                        break;
                    }

                    let (lo, rest) = match get_esc(chunk) {
                        Ok(v) => v,
                        Err(err) => return (b"", false, Some(err)),
                    };
                    chunk = rest;
                    let mut hi = lo;
                    if chunk[0] == b'-' {
                        match get_esc(&chunk[1..]) {
                            Ok((h, rest)) => {
                                hi = h;
                                chunk = rest;
                            }
                            Err(err) => return (b"", false, Some(err)),
                        }
                    }

                    if lo <= r && r <= hi {
                        matched = true;
                    }
                    nrange += 1;
                }

                if matched == negated {
                    failed = true;
                }
            }
            b'?' => {
                if !failed {
                    if s[0] == SEPARATOR {
                        failed = true;
                    }
                    let (_, n) = utf8::decode_rune(s);
                    s = &s[n..];
                }
                chunk = &chunk[1..];
            }
            c => {
                let mut c = c;
                if c == b'\\' {
                    chunk = &chunk[1..];
                    if chunk.is_empty() {
                        return (b"", false, Some(BadPattern));
                    }
                    c = chunk[0];
                }

                if !failed {
                    if c != s[0] {
                        failed = true;
                    }
                    s = &s[1..];
                }
                chunk = &chunk[1..];
            }
        }
    }

    if failed {
        return (b"", false, None);
    }

    (s, true, None)
}

fn get_esc(mut chunk: &[u8]) -> Result<(Rune, &[u8]), BadPattern> {
    if chunk.is_empty() || chunk[0] == b'-' || chunk[0] == b']' {
        return Err(BadPattern);
    }

    if chunk[0] == b'\\' {
        chunk = &chunk[1..];
        if chunk.is_empty() {
            return Err(BadPattern);
        }
    }

    let (r, n) = utf8::decode_rune(chunk);
    let rest = &chunk[n..];
    if (r == RUNE_ERROR && n == 1) || rest.is_empty() {
        return Err(BadPattern);
    }

    Ok((r, rest))
}

fn has_meta(path: &[u8]) -> bool {
    path.iter().any(|c| b"*?[\\".contains(c))
}

fn split(path: &[u8]) -> (&[u8], &[u8]) {
    let mut i = path.len();
    while i > 0 && path[i - 1] != SEPARATOR {
        i -= 1;
    }

    (&path[..i], &path[i..])
}

fn clean_glob_path(path: &[u8]) -> Vec<u8> {
    match path {
        b"" => b".".to_vec(),
        b"/" => path.to_vec(),
        _ => path[..path.len() - 1].to_vec(),
    }
}

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() {
        return path::clean(name);
    }

    let mut s = Vec::with_capacity(dir.len() + 1 + name.len());
    s.extend_from_slice(dir);
    s.push(SEPARATOR);
    s.extend_from_slice(name);

    path::clean(&s)
}

fn as_path(b: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(b))
}

pub fn glob(pattern: &[u8]) -> Result<Vec<Vec<u8>>, BadPattern> {
    match_pattern(pattern, b"")?;

    if !has_meta(pattern) {
        if std::fs::symlink_metadata(as_path(pattern)).is_err() {
            return Ok(Vec::new());
        }
        return Ok(vec![pattern.to_vec()]);
    }

    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut current = pattern.to_vec();
    let mut depth = 0;
    let base = loop {
        if depth == PATH_SEPARATORS_LIMIT {
            return Err(BadPattern);
        }

        let (dir, file) = split(&current);
        let dir = clean_glob_path(dir);
        files.push(file.to_vec());

        if !has_meta(&dir) {
            break dir;
        }

        if dir == current {
            return Err(BadPattern);
        }

        match_pattern(&dir, b"")?;
        current = dir;
        depth += 1;
    };

    let mut matches = vec![base];
    for file in files.iter().rev() {
        let mut next = Vec::new();
        for dir in &matches {
            glob_dir(dir, file, &mut next)?;
        }
        matches = next;
    }

    Ok(matches)
}

fn glob_dir(dir: &[u8], pattern: &[u8], matches: &mut Vec<Vec<u8>>) -> Result<(), BadPattern> {
    let Ok(info) = std::fs::metadata(as_path(dir)) else {
        return Ok(());
    };
    if !info.is_dir() {
        return Ok(());
    }

    let Ok(entries) = std::fs::read_dir(as_path(dir)) else {
        return Ok(());
    };

    let mut names: Vec<Vec<u8>> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            break;
        };
        names.push(entry.file_name().as_bytes().to_vec());
    }
    names.sort();

    for name in names {
        if match_pattern(pattern, &name)? {
            matches.push(join(dir, &name));
        }
    }

    Ok(())
}
