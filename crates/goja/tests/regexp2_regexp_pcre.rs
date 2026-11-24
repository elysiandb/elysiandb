use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

use goja::deps::regexp2::{
    IGNORE_CASE, IGNORE_PATTERN_WHITESPACE, MULTILINE, Match, RegexOptions, Regexp, SINGLELINE,
};

#[derive(Default)]
struct Pcre {
    total_count: usize,
    fail_count: usize,
    failures: Vec<String>,
}

impl Pcre {
    fn problem(&mut self, msg: String) {
        self.fail_count += 1;
        self.failures.push(msg);
    }
}

fn scanner_lines(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = data
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
        .collect();
    if data.ends_with(b"\n") {
        out.pop();
    }
    out
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

fn trim_space(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|&c| !is_space(c)).unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |e| e + 1);
    &b[start..end.max(start)]
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn test_pcre_basics() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/regexp2/testoutput1");
    let data =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let lines = scanner_lines(&data);
    let mut t = Pcre::default();
    let mut pos = 0usize;
    while pos < lines.len() {
        let mut line = lines[pos].clone();
        pos += 1;
        let trim = trim_space(&line);
        if trim.is_empty() || trim.starts_with(b"#") {
            continue;
        }
        let pattern_start = line[0];
        assert!(
            pattern_start == b'/' || pattern_start == b'"',
            "Unknown file format, expected line to start with '/' or '\"', line in: {}",
            lossy(&line)
        );
        let mut pattern = line.clone();
        t.total_count += 1;
        let mut allow_first = false;
        while !contains_ender(&line, pattern_start, allow_first) {
            assert!(
                pos < lines.len(),
                "Unknown file format, expected more pattern text, but got EOF, pattern so far: {}",
                lossy(&pattern)
            );
            line = lines[pos].clone();
            pos += 1;
            pattern.push(b'\n');
            pattern.extend_from_slice(&line);
            allow_first = true;
        }
        let re = compile_raw_pattern(&mut t, &pattern);
        let mut cap_counts: Vec<(usize, usize)> = Vec::new();
        let mut m: Option<Match<'_>> = None;
        let mut to_match: Vec<u8> = Vec::new();
        while pos < lines.len() {
            let line = lines[pos].clone();
            pos += 1;
            if trim_space(&line).is_empty() {
                break;
            }
            if line.starts_with(b"\\= Expect") {
                continue;
            } else if line.starts_with(b"    ") {
                to_match = line[4..].to_vec();
                while to_match.last() == Some(&b' ') {
                    to_match.pop();
                }
                m = match_string(&mut t, re.as_ref(), &to_match);
                cap_counts.clear();
                continue;
            } else if line.starts_with(b"No match") {
                validate_no_match(&mut t, re.as_ref(), m.as_ref());
                continue;
            } else if let Some((g_idx, value)) = match_group(&line) {
                let cap_idx = match cap_counts.iter().position(|&(g, _)| g == g_idx) {
                    Some(i) => cap_counts[i].1,
                    None => {
                        cap_counts.push((g_idx, 0));
                        0
                    }
                };
                validate_match(
                    &mut t,
                    re.as_ref(),
                    m.as_ref(),
                    &to_match,
                    value,
                    g_idx,
                    cap_idx,
                );
                if let Some(entry) = cap_counts.iter_mut().find(|(g, _)| *g == g_idx) {
                    entry.1 += 1;
                }
                continue;
            } else {
                panic!(
                    "Unknown file format, expected match or match group but got '{}'",
                    lossy(&line)
                );
            }
        }
    }
    assert!(
        t.failures.is_empty(),
        "{} of {} patterns failed:\n{}",
        t.fail_count,
        t.total_count,
        t.failures.join("\n")
    );
}

fn match_group(line: &[u8]) -> Option<(usize, &[u8])> {
    let mut i = 0;
    while i < line.len() && matches!(line[i], b'\t' | b'\n' | 0x0c | b'\r' | b' ') {
        i += 1;
    }
    let start = i;
    while i < line.len() && line[i].is_ascii_digit() {
        i += 1;
    }
    if i == start || !line[i..].starts_with(b": ") {
        return None;
    }
    let n = std::str::from_utf8(&line[start..i]).ok()?.parse().ok()?;
    Some((n, &line[i + 2..]))
}

fn validate_no_match(t: &mut Pcre, re: Option<&Regexp>, m: Option<&Match<'_>>) {
    let (Some(re), Some(m)) = (re, m) else {
        return;
    };
    t.problem(format!(
        "Expected no match for pattern '{}', but got '{}'",
        lossy(re.string()),
        m.string()
    ));
}

fn validate_match(
    t: &mut Pcre,
    re: Option<&Regexp>,
    m: Option<&Match<'_>>,
    to_match: &[u8],
    value: &[u8],
    idx: usize,
    cap_idx: usize,
) {
    let Some(re) = re else {
        return;
    };
    let Some(m) = m else {
        t.problem(format!(
            "Expected match for pattern '{}' with input '{}', but got no match",
            lossy(re.string()),
            lossy(to_match)
        ));
        return;
    };
    let g = m.groups();
    if g.len() <= idx {
        t.problem(format!(
            "Expected group {idx} does not exist in pattern '{}' with input '{}'",
            lossy(re.string()),
            lossy(to_match)
        ));
        return;
    }
    if value == b"<unset>" {
        if !g[idx].captures.is_empty() {
            t.problem(format!(
                "Expected no cap {} in group {idx} in pattern '{}' with input '{}'",
                g[idx]
                    .captures
                    .get(cap_idx)
                    .map(|c| c.string())
                    .unwrap_or_default(),
                lossy(re.string()),
                lossy(to_match)
            ));
        }
        return;
    }
    if g[idx].captures.len() <= cap_idx {
        t.problem(format!(
            "Expected cap {cap_idx} does not exist in group {idx} in pattern '{}' with input '{}'",
            lossy(re.string()),
            lossy(to_match)
        ));
        return;
    }
    let escp = un_escape_group(g[idx].string().as_bytes());
    if escp.as_bytes() != value {
        t.problem(format!(
            "Expected '{}' but got '{escp}' for cap {cap_idx}, group {idx} for pattern '{}' with input '{}'",
            lossy(value),
            lossy(re.string()),
            lossy(to_match)
        ));
    }
}

fn compile_raw_pattern(t: &mut Pcre, pattern: &[u8]) -> Option<Regexp> {
    let index = pattern.iter().rposition(|&b| b == b'/' || b == b'"');
    let index = index.map_or(-1, |i| i as isize);
    let mut opts: RegexOptions = 0;
    let mut pattern = pattern.to_vec();
    if index + 1 < pattern.len() as isize {
        let text_options = pattern[(index + 1) as usize..].to_vec();
        pattern.truncate((index + 1) as usize);
        for text_opt in text_options.split(|&b| b == b',') {
            if text_opt == b"dupnames" {
                continue;
            }
            if text_opt.contains(&b'i') {
                opts |= IGNORE_CASE;
            }
            if text_opt.contains(&b's') {
                opts |= SINGLELINE;
            }
            if text_opt.contains(&b'm') {
                opts |= MULTILINE;
            }
            if text_opt.contains(&b'x') {
                opts |= IGNORE_PATTERN_WHITESPACE;
            }
        }
    }
    let pattern = pattern[1..pattern.len() - 1].to_vec();
    let compiled = panic::catch_unwind(AssertUnwindSafe(|| Regexp::compile(&pattern, opts)));
    match compiled {
        Err(_) => {
            t.problem(format!("PANIC in compiling \"{}\"", lossy(&pattern)));
            None
        }
        Ok(Err(err)) => {
            t.problem(format!("Error parsing \"{}\": {err}", lossy(&pattern)));
            None
        }
        Ok(Ok(re)) => Some(re),
    }
}

fn match_string<'r>(t: &mut Pcre, re: Option<&'r Regexp>, to_match: &[u8]) -> Option<Match<'r>> {
    let re = re?;
    let escp = if to_match != b"\\" {
        un_escape_to_match(to_match)
    } else {
        Vec::new()
    };
    match re.find_string_match(&escp) {
        Ok(m) => m,
        Err(err) => {
            t.problem(format!(
                "Error matching \"{}\" in pattern \"{}\": {err}",
                lossy(to_match),
                lossy(re.string())
            ));
            None
        }
    }
}

fn contains_ender(line: &[u8], ender: u8, allow_first: bool) -> bool {
    match line.iter().rposition(|&b| b == ender) {
        Some(index) if index > 0 => true,
        Some(0) => allow_first,
        _ => false,
    }
}

fn un_escape_to_match(line: &[u8]) -> Vec<u8> {
    let Some(idx) = line.iter().position(|&b| b == b'\\') else {
        return line.to_vec();
    };
    let mut buf = line[..idx].to_vec();
    let mut in_escape = false;
    let mut i = idx;
    while i < line.len() {
        let ch = line[i];
        if ch == b'\\' {
            if in_escape {
                buf.push(ch);
            }
            in_escape = !in_escape;
            i += 1;
            continue;
        }
        if in_escape {
            match ch {
                b'x' => buf.push(scan_hex(line, &mut i)),
                b'a' => buf.push(0x07),
                b'b' => buf.push(0x08),
                b'e' => buf.push(0x1b),
                b'f' => buf.push(0x0c),
                b'n' => buf.push(b'\n'),
                b'r' => buf.push(b'\r'),
                b't' => buf.push(b'\t'),
                b'v' => buf.push(0x0b),
                _ => {
                    if (b'0'..=b'7').contains(&ch) {
                        buf.push(scan_octal(line, &mut i));
                    } else {
                        buf.push(ch);
                    }
                }
            }
            in_escape = false;
        } else {
            buf.push(ch);
        }
        i += 1;
    }
    buf
}

fn un_escape_group(val: &[u8]) -> String {
    let mut buf = String::new();
    for &ch in val {
        if ch <= 0x1f || ch >= 0x7f {
            buf.push_str(&format!("\\x{ch:02x}"));
        } else {
            buf.push(ch as char);
        }
    }
    buf
}

fn scan_hex(line: &[u8], idx: &mut usize) -> u8 {
    if *idx as isize >= line.len() as isize - 2 {
        panic!("not enough hex chars in {} at {}", lossy(line), idx);
    }
    *idx += 1;
    let d1 = hex_digit(line[*idx]);
    *idx += 1;
    let d2 = hex_digit(line[*idx]);
    if d1 < 0 || d2 < 0 {
        panic!("bad hex chars");
    }
    (d1 * 0x10 + d2) as u8
}

fn hex_digit(ch: u8) -> i32 {
    let d = ch.wrapping_sub(b'0');
    if d <= 9 {
        return i32::from(d);
    }
    let d = ch.wrapping_sub(b'a');
    if d <= 5 {
        return i32::from(d) + 0xa;
    }
    let d = ch.wrapping_sub(b'A');
    if d <= 5 {
        return i32::from(d) + 0xa;
    }
    -1
}

fn scan_octal(line: &[u8], idx: &mut usize) -> u8 {
    let mut c = 3usize;
    let diff = line.len() - *idx;
    if c > diff {
        c = diff;
    }
    let mut i: i32 = 0;
    let mut d = i32::from(line[*idx].wrapping_sub(b'0'));
    while c > 0 && d <= 7 {
        i *= 8;
        i += d;
        c -= 1;
        *idx += 1;
        if *idx < line.len() {
            d = i32::from(line[*idx].wrapping_sub(b'0'));
        }
    }
    *idx -= 1;
    i &= 0xFF;
    i as u8
}
