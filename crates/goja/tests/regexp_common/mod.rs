#![allow(dead_code)]

use gocompat::json::{self, Value};
use gocompat::unicode;
use gocompat::utf8::{self, Rune};
use goja::deps::regexp::syntax::{self, ErrorCode, Op};
use goja::deps::regexp::{Regexp, RuneReader};
use std::path::PathBuf;

pub fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/regexp")
        .join(rel)
}

pub fn read_lines(rel: &str) -> Vec<Vec<u8>> {
    let data = std::fs::read(golden_path(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
    let mut out: Vec<Vec<u8>> = data.split(|&b| b == b'\n').map(|l| l.to_vec()).collect();
    if out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

pub fn read_jsonl(rel: &str) -> Vec<Value> {
    read_lines(rel)
        .iter()
        .enumerate()
        .map(|(i, line)| {
            json::unmarshal_value(line).unwrap_or_else(|e| panic!("{rel}:{}: {e}", i + 1))
        })
        .collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|e| panic!("bad hex {s}: {e}"))
        })
        .collect()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn fnv(b: &[u8]) -> String {
    let mut h: u64 = 14695981039346656037;
    for &c in b {
        h ^= u64::from(c);
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

pub fn val<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing field {key} in {row:?}"))
}

pub fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {key} in {row:?}"))
}

pub fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

pub fn bool_field(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

pub fn bytes_field(row: &Value, key: &str) -> Option<Vec<u8>> {
    if let Some(v) = row.get(key).and_then(Value::as_str) {
        return Some(v.as_bytes().to_vec());
    }
    row.get(&format!("{key}_hex"))
        .and_then(Value::as_str)
        .map(unhex)
}

pub fn pattern(row: &Value) -> Vec<u8> {
    if let Some(p) = bytes_field(row, "pattern") {
        return p;
    }
    let parts = row
        .get("pattern_parts")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing pattern in {row:?}"));
    let mut out = Vec::new();
    for part in parts {
        let unit = part.index(0).and_then(Value::as_str).expect("unit");
        let count = part.index(1).and_then(Value::as_i64).expect("count");
        for _ in 0..count {
            out.extend_from_slice(unit.as_bytes());
        }
    }
    out
}

pub fn ints(v: &Value) -> Option<Vec<isize>> {
    if v.is_null() {
        return None;
    }
    Some(
        v.as_array()
            .unwrap_or_else(|| panic!("not an array: {v:?}"))
            .iter()
            .map(|x| x.as_i64().expect("integer") as isize)
            .collect(),
    )
}

pub fn intss(v: &Value) -> Option<Vec<Vec<isize>>> {
    if v.is_null() {
        return None;
    }
    Some(
        v.as_array()
            .unwrap_or_else(|| panic!("not an array: {v:?}"))
            .iter()
            .map(|x| ints(x).expect("inner array"))
            .collect(),
    )
}

pub fn pieces_field(row: &Value, key: &str) -> Option<Vec<Vec<u8>>> {
    if let Some(v) = row.get(key) {
        if v.is_null() {
            return None;
        }
        return Some(
            v.as_array()
                .expect("array")
                .iter()
                .map(|x| x.as_str().expect("string").as_bytes().to_vec())
                .collect(),
        );
    }
    let v = row
        .get(&format!("{key}_hex"))
        .unwrap_or_else(|| panic!("missing {key} in {row:?}"));
    Some(
        v.as_array()
            .expect("array")
            .iter()
            .map(|x| unhex(x.as_str().expect("hex")))
            .collect(),
    )
}

pub struct Sink {
    pub buf: Vec<u8>,
    pub n: usize,
    pub h: u64,
}

pub const MAX_INLINE: usize = 2048;

impl Default for Sink {
    fn default() -> Sink {
        Sink {
            buf: Vec::new(),
            n: 0,
            h: 14695981039346656037,
        }
    }
}

impl Sink {
    pub fn write(&mut self, p: &[u8]) {
        for &c in p {
            self.h ^= u64::from(c);
            self.h = self.h.wrapping_mul(1099511628211);
        }
        if self.n <= MAX_INLINE {
            self.buf.extend_from_slice(p);
        }
        self.n += p.len();
    }

    pub fn write_str(&mut self, p: &str) {
        self.write(p.as_bytes());
    }

    pub fn repr(&self) -> Vec<u8> {
        if self.n <= MAX_INLINE {
            return self.buf.clone();
        }
        format!("#{:016x}", self.h).into_bytes()
    }

    pub fn check(&self, row: &Value, key: &str, context: &str) {
        if self.n <= MAX_INLINE {
            let want = bytes_field(row, key).unwrap_or_else(|| panic!("{context}: missing {key}"));
            assert_eq!(
                String::from_utf8_lossy(&self.buf),
                String::from_utf8_lossy(&want),
                "{context}: {key}"
            );
        } else {
            let want = row
                .get(&format!("{key}_fnv"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{context}: missing {key}_fnv, have {} bytes", self.n));
            assert_eq!(format!("{:016x}", self.h), want, "{context}: {key}");
        }
    }
}

pub fn check_big(have: &[u8], row: &Value, key: &str, context: &str) {
    let mut s = Sink::default();
    s.write(have);
    s.check(row, key, context);
}

pub struct StringReader<'a> {
    pub s: &'a [u8],
    pub i: usize,
}

impl<'a> StringReader<'a> {
    pub fn new(s: &'a [u8]) -> StringReader<'a> {
        StringReader { s, i: 0 }
    }
}

impl RuneReader for StringReader<'_> {
    fn read_rune(&mut self) -> Option<(Rune, usize)> {
        if self.i >= self.s.len() {
            return None;
        }
        let c = self.s[self.i];
        if c < utf8::RUNE_SELF {
            self.i += 1;
            return Some((Rune::from(c), 1));
        }
        let (r, size) = utf8::decode_rune(&self.s[self.i..]);
        self.i += size;
        Some((r, size))
    }
}

pub fn error_class(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InternalError => "internal_error",
        ErrorCode::InvalidCharClass => "invalid_char_class",
        ErrorCode::InvalidCharRange => "invalid_char_range",
        ErrorCode::InvalidEscape => "invalid_escape",
        ErrorCode::InvalidNamedCapture => "invalid_named_capture",
        ErrorCode::InvalidPerlOp => "invalid_perl_op",
        ErrorCode::InvalidRepeatOp => "invalid_repeat_op",
        ErrorCode::InvalidRepeatSize => "invalid_repeat_size",
        ErrorCode::InvalidUtf8 => "invalid_utf8",
        ErrorCode::MissingBracket => "missing_bracket",
        ErrorCode::MissingParen => "missing_paren",
        ErrorCode::MissingRepeatArgument => "missing_repeat_argument",
        ErrorCode::TrailingBackslash => "trailing_backslash",
        ErrorCode::UnexpectedParen => "unexpected_paren",
        ErrorCode::NestingDepth => "nesting_depth",
        ErrorCode::Large => "large",
    }
}

pub fn check_error(err: &syntax::Error, row: &Value, context: &str) {
    assert_eq!(
        error_class(err.code),
        str_field(row, "error"),
        "{context}: error class"
    );
    check_big(&err.expr, row, "expr", &format!("{context}: expr"));
}

pub type InstRow<'a> = (u8, u32, u32, &'a [Rune], Option<&'a [u32]>);

fn dump_insts(w: &mut Sink, start: usize, num_cap: usize, insts: &[InstRow<'_>]) {
    w.write_str(&format!("start {start} numcap {num_cap}\n"));
    for (i, (op, out, arg, runes, next)) in insts.iter().enumerate() {
        w.write_str(&format!("{i} {op} {out} {arg} ["));
        let rs: Vec<String> = runes.iter().map(|r| r.to_string()).collect();
        w.write_str(&rs.join(" "));
        w.write_str("]");
        if let Some(next) = next {
            w.write_str(" [");
            let ns: Vec<String> = next.iter().map(|r| r.to_string()).collect();
            w.write_str(&ns.join(" "));
            w.write_str("]");
        }
        w.write_str("\n");
    }
}

pub fn prog_dump(re: &Regexp) -> Sink {
    let p = re.prog();
    let insts: Vec<_> = p
        .inst
        .iter()
        .map(|i| (i.op as u8, i.out, i.arg, &i.rune[..], None))
        .collect();
    let mut w = Sink::default();
    dump_insts(&mut w, p.start, p.num_cap, &insts);
    w
}

pub fn onepass_dump(re: &Regexp) -> Option<Sink> {
    let p = re.onepass()?;
    let insts: Vec<_> = p
        .inst
        .iter()
        .map(|i| {
            (
                i.inst.op as u8,
                i.inst.out,
                i.inst.arg,
                &i.inst.rune[..],
                Some(&i.next[..]),
            )
        })
        .collect();
    let mut w = Sink::default();
    dump_insts(&mut w, p.start, p.num_cap, &insts);
    Some(w)
}

pub fn prog_size(re: &Regexp) -> (usize, usize) {
    let p = re.prog();
    (p.inst.len(), p.inst.iter().map(|i| i.rune.len()).sum())
}

pub fn huge_prog(re: &Regexp) -> bool {
    let (insts, runes) = prog_size(re);
    insts > 100000 || runes > 1000000
}

pub fn compile_summary(result: &Result<Regexp, syntax::Error>) -> Vec<u8> {
    let re = match result {
        Err(err) => {
            return format!("error {} {}", error_class(err.code), hex(&err.expr)).into_bytes();
        }
        Ok(re) => re,
    };
    let op = onepass_dump(re).map(|s| s.repr()).unwrap_or_default();
    let (prefix, complete) = re.literal_prefix();
    let names: Vec<Vec<u8>> = re.subexp_names().to_vec();
    let parts: Vec<Vec<u8>> = vec![
        prog_dump(re).repr(),
        op,
        prefix.to_vec(),
        bool_text(complete).as_bytes().to_vec(),
        re.cond().to_string().into_bytes(),
        re.min_input_len().to_string().into_bytes(),
        re.max_bit_state_len().to_string().into_bytes(),
        re.num_subexp().to_string().into_bytes(),
        re.matchcap().to_string().into_bytes(),
        re.prefix_rune().to_string().into_bytes(),
        re.prefix_end().to_string().into_bytes(),
        re.mpool().to_string().into_bytes(),
        names.join(&b","[..]),
    ];
    parts.join(&b"\x00"[..])
}

pub fn bool_text(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

pub fn res_text(a: &Option<Vec<isize>>) -> String {
    match a {
        None => "-;".to_string(),
        Some(a) => {
            let s: Vec<String> = a.iter().map(|v| v.to_string()).collect();
            format!("{};", s.join(" "))
        }
    }
}

pub fn all_text(a: &Option<Vec<Vec<isize>>>) -> String {
    match a {
        None => "-;".to_string(),
        Some(a) => {
            let s: Vec<String> = a
                .iter()
                .map(|m| {
                    m.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            format!("{};", s.join(","))
        }
    }
}

pub fn op_name(op: Op) -> &'static str {
    match op {
        Op::NoMatch => "no",
        Op::EmptyMatch => "emp",
        Op::Literal => "lit",
        Op::CharClass => "cc",
        Op::AnyCharNotNL => "dnl",
        Op::AnyChar => "dot",
        Op::BeginLine => "bol",
        Op::EndLine => "eol",
        Op::BeginText => "bot",
        Op::EndText => "eot",
        Op::WordBoundary => "wb",
        Op::NoWordBoundary => "nwb",
        Op::Capture => "cap",
        Op::Star => "star",
        Op::Plus => "plus",
        Op::Quest => "que",
        Op::Repeat => "rep",
        Op::Concat => "cat",
        Op::Alternate => "alt",
        Op::LeftParen | Op::VerticalBar => "",
    }
}

pub fn dump(re: &syntax::Regexp) -> Vec<u8> {
    let mut b = Vec::new();
    dump_regexp(&mut b, re);
    b
}

fn dump_regexp(b: &mut Vec<u8>, re: &syntax::Regexp) {
    let name = op_name(re.op);
    if name.is_empty() {
        b.extend_from_slice(format!("op{}", re.op as u8).as_bytes());
    } else {
        match re.op {
            Op::Star | Op::Plus | Op::Quest | Op::Repeat => {
                if re.flags & syntax::NON_GREEDY != 0 {
                    b.push(b'n');
                }
                b.extend_from_slice(name.as_bytes());
            }
            Op::Literal => {
                if re.rune.len() > 1 {
                    b.extend_from_slice(b"str");
                } else {
                    b.extend_from_slice(b"lit");
                }
                if re.flags & syntax::FOLD_CASE != 0
                    && re.rune.iter().any(|&r| unicode::simple_fold(r) != r)
                {
                    b.extend_from_slice(b"fold");
                }
            }
            _ => b.extend_from_slice(name.as_bytes()),
        }
    }
    b.push(b'{');
    match re.op {
        Op::EndText => {
            if re.flags & syntax::WAS_DOLLAR == 0 {
                b.extend_from_slice(br"\z");
            }
        }
        Op::Literal => {
            for &r in re.rune.iter() {
                utf8::append_rune(b, r);
            }
        }
        Op::Concat | Op::Alternate => {
            for sub in &re.sub {
                dump_regexp(b, sub);
            }
        }
        Op::Star | Op::Plus | Op::Quest => dump_regexp(b, &re.sub[0]),
        Op::Repeat => {
            b.extend_from_slice(format!("{},{} ", re.min, re.max).as_bytes());
            dump_regexp(b, &re.sub[0]);
        }
        Op::Capture => {
            if !re.name.is_empty() {
                b.extend_from_slice(&re.name);
                b.push(b':');
            }
            dump_regexp(b, &re.sub[0]);
        }
        Op::CharClass => {
            let mut sep = "";
            for p in re.rune.chunks_exact(2) {
                b.extend_from_slice(sep.as_bytes());
                sep = " ";
                if p[0] == p[1] {
                    b.extend_from_slice(format!("{:#x}", p[0]).as_bytes());
                } else {
                    b.extend_from_slice(format!("{:#x}-{:#x}", p[0], p[1]).as_bytes());
                }
            }
        }
        _ => {}
    }
    b.push(b'}');
}

pub struct FindTest {
    pub pat: &'static [u8],
    pub text: &'static [u8],
    pub matches: Option<Vec<Vec<isize>>>,
}

fn build(pat: &'static [u8], text: &'static [u8], n: usize, x: &[isize]) -> FindTest {
    if n == 0 {
        return FindTest {
            pat,
            text,
            matches: None,
        };
    }
    let run = x.len() / n;
    let mut ret = Vec::new();
    let mut j = 0;
    for _ in 0..n {
        ret.push(x[j..j + run].to_vec());
        j += run;
    }
    FindTest {
        pat,
        text,
        matches: Some(ret),
    }
}

pub fn find_tests() -> Vec<FindTest> {
    vec![
        build(b"", b"", 1, &[0, 0]),
        build(b"^abcdefg", b"abcdefg", 1, &[0, 7]),
        build(b"a+", b"baaab", 1, &[1, 4]),
        build(b"abcd..", b"abcdef", 1, &[0, 6]),
        build(b"a", b"a", 1, &[0, 1]),
        build(b"x", b"y", 0, &[]),
        build(b"b", b"abc", 1, &[1, 2]),
        build(b".", b"a", 1, &[0, 1]),
        build(b".*", b"abcdef", 1, &[0, 6]),
        build(b"^", b"abcde", 1, &[0, 0]),
        build(b"$", b"abcde", 1, &[5, 5]),
        build(b"^abcd$", b"abcd", 1, &[0, 4]),
        build(b"^bcd'", b"abcdef", 0, &[]),
        build(b"^abcd$", b"abcde", 0, &[]),
        build(b"a+", b"baaab", 1, &[1, 4]),
        build(b"a*", b"baaab", 3, &[0, 0, 1, 4, 5, 5]),
        build(b"[a-z]+", b"abcd", 1, &[0, 4]),
        build(b"[^a-z]+", b"ab1234cd", 1, &[2, 6]),
        build(br"[a\-\]z]+", b"az]-bcz", 2, &[0, 4, 6, 7]),
        build(br"[^\n]+", b"abcd\n", 1, &[0, 4]),
        build(
            "[日本語]+".as_bytes(),
            "日本語日本語".as_bytes(),
            1,
            &[0, 18],
        ),
        build("日本語+".as_bytes(), "日本語".as_bytes(), 1, &[0, 9]),
        build("日本語+".as_bytes(), "日本語語語語".as_bytes(), 1, &[0, 18]),
        build(b"()", b"", 1, &[0, 0, 0, 0]),
        build(b"(a)", b"a", 1, &[0, 1, 0, 1]),
        build(b"(.)(.)", "日a".as_bytes(), 1, &[0, 4, 0, 3, 3, 4]),
        build(b"(.*)", b"", 1, &[0, 0, 0, 0]),
        build(b"(.*)", b"abcd", 1, &[0, 4, 0, 4]),
        build(b"(..)(..)", b"abcd", 1, &[0, 4, 0, 2, 2, 4]),
        build(b"(([^xyz]*)(d))", b"abcd", 1, &[0, 4, 0, 4, 0, 3, 3, 4]),
        build(b"((a|b|c)*(d))", b"abcd", 1, &[0, 4, 0, 4, 2, 3, 3, 4]),
        build(
            b"(((a|b|c)*)(d))",
            b"abcd",
            1,
            &[0, 4, 0, 4, 0, 3, 2, 3, 3, 4],
        ),
        build(br"\a\f\n\r\t\v", b"\x07\x0c\n\r\t\x0b", 1, &[0, 6]),
        build(br"[\a\f\n\r\t\v]+", b"\x07\x0c\n\r\t\x0b", 1, &[0, 6]),
        build(b"a*(|(b))c*", b"aacc", 1, &[0, 4, 2, 2, -1, -1]),
        build(b"(.*).*", b"ab", 1, &[0, 2, 0, 2]),
        build(b"[.]", b".", 1, &[0, 1]),
        build(b"/$", b"/abc/", 1, &[4, 5]),
        build(b"/$", b"/abc", 0, &[]),
        build(b".", b"abc", 3, &[0, 1, 1, 2, 2, 3]),
        build(b"(.)", b"abc", 3, &[0, 1, 0, 1, 1, 2, 1, 2, 2, 3, 2, 3]),
        build(b".(.)", b"abcd", 2, &[0, 2, 1, 2, 2, 4, 3, 4]),
        build(b"ab*", b"abbaab", 3, &[0, 3, 3, 4, 4, 6]),
        build(
            b"a(b*)",
            b"abbaab",
            3,
            &[0, 3, 1, 3, 3, 4, 4, 4, 4, 6, 5, 6],
        ),
        build(b"ab$", b"cab", 1, &[1, 3]),
        build(b"axxb$", b"axxcb", 0, &[]),
        build(b"data", b"daXY data", 1, &[5, 9]),
        build(b"da(.)a$", b"daXY data", 1, &[5, 9, 7, 8]),
        build(b"zx+", b"zzx", 1, &[1, 3]),
        build(b"ab$", b"abcab", 1, &[3, 5]),
        build(b"(aa)*$", b"a", 1, &[1, 1, -1, -1]),
        build(b"(?:.|(?:.a))", b"", 0, &[]),
        build(b"(?:A(?:A|a))", b"Aa", 1, &[0, 2]),
        build(b"(?:A|(?:A|a))", b"a", 1, &[0, 1]),
        build(b"(a){0}", b"", 1, &[0, 0, -1, -1]),
        build(b"(?-s)(?:(?:^).)", b"\n", 0, &[]),
        build(b"(?s)(?:(?:^).)", b"\n", 1, &[0, 1]),
        build(b"(?:(?:^).)", b"\n", 0, &[]),
        build(br"\b", b"x", 2, &[0, 0, 1, 1]),
        build(br"\b", b"xx", 2, &[0, 0, 2, 2]),
        build(br"\b", b"x y", 4, &[0, 0, 1, 1, 2, 2, 3, 3]),
        build(br"\b", b"xx yy", 4, &[0, 0, 2, 2, 3, 3, 5, 5]),
        build(br"\B", b"x", 0, &[]),
        build(br"\B", b"xx", 1, &[1, 1]),
        build(br"\B", b"x y", 0, &[]),
        build(br"\B", b"xx yy", 2, &[1, 1, 4, 4]),
        build(b"(|a)*", b"aa", 3, &[0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2]),
        build(br"[^\S\s]", b"abcd", 0, &[]),
        build(br"[^\S[:space:]]", b"abcd", 0, &[]),
        build(br"[^\D\d]", b"abcd", 0, &[]),
        build(br"[^\D[:digit:]]", b"abcd", 0, &[]),
        build(br"(?i)\W", b"x", 0, &[]),
        build(br"(?i)\W", b"k", 0, &[]),
        build(br"(?i)\W", b"s", 0, &[]),
        build(
            br##"\!\"\#\$\%\&\'\(\)\*\+\,\-\.\/\:\;\<\=\>\?\@\[\\\]\^\_\{\|\}\~"##,
            br##"!"#$%&'()*+,-./:;<=>?@[\]^_{|}~"##,
            1,
            &[0, 31],
        ),
        build(
            br##"[\!\"\#\$\%\&\'\(\)\*\+\,\-\.\/\:\;\<\=\>\?\@\[\\\]\^\_\{\|\}\~]+"##,
            br##"!"#$%&'()*+,-./:;<=>?@[\]^_{|}~"##,
            1,
            &[0, 31],
        ),
        build(b"\\`", b"`", 1, &[0, 1]),
        build(b"[\\`]+", b"`", 1, &[0, 1]),
        build("\u{fffd}".as_bytes(), b"\xff", 1, &[0, 1]),
        build("\u{fffd}".as_bytes(), b"hello\xffworld", 1, &[5, 6]),
        build(b".*", b"hello\xffworld", 1, &[0, 11]),
        build(br"\x{fffd}", b"\xc2\x00", 1, &[0, 1]),
        build("[\u{fffd}]".as_bytes(), b"\xff", 1, &[0, 1]),
        build(br"[\x{fffd}]", b"\xc2\x00", 1, &[0, 1]),
        build(
            b".",
            b"qwertyuiopasdfghjklzxcvbnm1234567890",
            36,
            &[
                0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
                13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23,
                23, 24, 24, 25, 25, 26, 26, 27, 27, 28, 28, 29, 29, 30, 30, 31, 31, 32, 32, 33, 33,
                34, 34, 35, 35, 36,
            ],
        ),
    ]
}
