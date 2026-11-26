use super::backtrack::max_bit_state_len;
use super::exec::{LazyFlag, Source, new_lazy_flag};
use super::onepass::{OnePassProg, compile_one_pass, one_pass_prefix};
use super::syntax::{self, EmptyOp, Error, Flags, Op, Prog};
use gocompat::strconv;
use gocompat::unicode;
use gocompat::utf8::{self, Rune};
use std::sync::Arc;

pub trait RuneReader {
    fn read_rune(&mut self) -> Option<(Rune, usize)>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Regexp {
    pub(crate) expr: Vec<u8>,
    pub(crate) prog: Arc<Prog>,
    pub(crate) onepass: Option<Arc<OnePassProg>>,
    pub(crate) num_subexp: usize,
    pub(crate) max_bit_state_len: usize,
    pub(crate) subexp_names: Vec<Vec<u8>>,
    pub(crate) prefix: Vec<u8>,
    pub(crate) prefix_rune: Rune,
    pub(crate) prefix_end: u32,
    pub(crate) mpool: usize,
    pub(crate) matchcap: usize,
    pub(crate) prefix_complete: bool,
    pub(crate) cond: EmptyOp,
    pub(crate) min_input_len: isize,
    pub(crate) longest: bool,
}

pub const MATCH_SIZE: [usize; 5] = [128, 512, 2048, 16384, 0];

pub fn compile(expr: &[u8]) -> Result<Regexp, Error> {
    compile_mode(expr, syntax::PERL, false)
}

pub fn compile_posix(expr: &[u8]) -> Result<Regexp, Error> {
    compile_mode(expr, syntax::POSIX, true)
}

pub fn compile_mode(expr: &[u8], mode: Flags, longest: bool) -> Result<Regexp, Error> {
    let re = syntax::parse(expr, mode)?;
    let max_cap = re.max_cap();
    let cap_names = re.cap_names();
    let re = re.simplify();
    let prog = syntax::compile(&re)?;
    let matchcap = prog.num_cap.max(2);
    let onepass = compile_one_pass(&prog);
    let cond = prog.start_cond();
    let mut regexp = Regexp {
        expr: expr.to_vec(),
        prog: Arc::new(prog),
        onepass: None,
        num_subexp: max_cap as usize,
        max_bit_state_len: 0,
        subexp_names: cap_names,
        prefix: Vec::new(),
        prefix_rune: 0,
        prefix_end: 0,
        mpool: 0,
        matchcap,
        prefix_complete: false,
        cond,
        min_input_len: min_input_len(&re),
        longest,
    };
    match onepass {
        None => {
            let (prefix, complete) = regexp.prog.prefix();
            regexp.prefix = prefix;
            regexp.prefix_complete = complete;
            regexp.max_bit_state_len = max_bit_state_len(&regexp.prog);
        }
        Some(op) => {
            let (prefix, complete, pc) = one_pass_prefix(&regexp.prog);
            regexp.prefix = prefix;
            regexp.prefix_complete = complete;
            regexp.prefix_end = pc;
            regexp.onepass = Some(Arc::new(op));
        }
    }
    if !regexp.prefix.is_empty() {
        regexp.prefix_rune = utf8::decode_rune(&regexp.prefix).0;
    }
    let n = regexp.prog.inst.len();
    let mut i = 0;
    while MATCH_SIZE[i] != 0 && MATCH_SIZE[i] < n {
        i += 1;
    }
    regexp.mpool = i;
    Ok(regexp)
}

pub fn min_input_len(re: &syntax::Regexp) -> isize {
    match re.op {
        Op::AnyChar | Op::AnyCharNotNL | Op::CharClass => 1,
        Op::Literal => {
            let mut l: isize = 0;
            for &r in re.rune.iter() {
                if r == utf8::RUNE_ERROR {
                    l += 1;
                } else {
                    l += utf8::rune_len(r).map_or(-1, |n| n as isize);
                }
            }
            l
        }
        Op::Capture | Op::Plus => min_input_len(&re.sub[0]),
        Op::Repeat => re.min as isize * min_input_len(&re.sub[0]),
        Op::Concat => re.sub.iter().map(|s| min_input_len(s)).sum(),
        Op::Alternate => {
            let mut l = min_input_len(&re.sub[0]);
            for sub in &re.sub[1..] {
                let lnext = min_input_len(sub);
                if lnext < l {
                    l = lnext;
                }
            }
            l
        }
        _ => 0,
    }
}

pub fn must_compile(expr: &[u8]) -> Regexp {
    match compile(expr) {
        Ok(re) => re,
        Err(err) => panic!("invalid regular expression {}: {}", quote(expr), err),
    }
}

pub fn must_compile_posix(expr: &[u8]) -> Regexp {
    match compile_posix(expr) {
        Ok(re) => re,
        Err(err) => panic!("invalid POSIX regular expression {}: {}", quote(expr), err),
    }
}

fn quote(s: &[u8]) -> String {
    if strconv::can_backquote(s) {
        return format!("`{}`", String::from_utf8_lossy(s));
    }
    strconv::quote(s)
}

pub fn match_reader(pattern: &[u8], r: &mut dyn RuneReader) -> Result<bool, Error> {
    Ok(compile(pattern)?.match_reader(r))
}

pub fn match_string(pattern: &[u8], s: &[u8]) -> Result<bool, Error> {
    Ok(compile(pattern)?.match_string(s))
}

pub fn r#match(pattern: &[u8], b: &[u8]) -> Result<bool, Error> {
    Ok(compile(pattern)?.r#match(b))
}

pub(crate) const END_OF_TEXT: Rune = -1;

pub(crate) trait Input {
    fn step(&mut self, pos: usize) -> (Rune, usize);
    fn can_check_prefix(&self) -> bool;
    fn has_prefix(&self, re: &Regexp) -> bool;
    fn index(&self, re: &Regexp, pos: usize) -> isize;
    fn context(&self, pos: usize) -> LazyFlag;
}

pub(crate) struct InputBytes<'a> {
    pub(crate) str: &'a [u8],
}

impl Input for InputBytes<'_> {
    fn step(&mut self, pos: usize) -> (Rune, usize) {
        if pos < self.str.len() {
            let c = self.str[pos];
            if c < utf8::RUNE_SELF {
                return (Rune::from(c), 1);
            }
            return utf8::decode_rune(&self.str[pos..]);
        }
        (END_OF_TEXT, 0)
    }

    fn can_check_prefix(&self) -> bool {
        true
    }

    fn has_prefix(&self, re: &Regexp) -> bool {
        self.str.starts_with(&re.prefix)
    }

    fn index(&self, re: &Regexp, pos: usize) -> isize {
        index(&self.str[pos..], &re.prefix)
    }

    fn context(&self, pos: usize) -> LazyFlag {
        let mut r1 = END_OF_TEXT;
        let mut r2 = END_OF_TEXT;
        if pos > 0 && pos - 1 < self.str.len() {
            r1 = Rune::from(self.str[pos - 1]);
            if r1 >= Rune::from(utf8::RUNE_SELF) {
                r1 = utf8::decode_last_rune(&self.str[..pos]).0;
            }
        }
        if pos < self.str.len() {
            r2 = Rune::from(self.str[pos]);
            if r2 >= Rune::from(utf8::RUNE_SELF) {
                r2 = utf8::decode_rune(&self.str[pos..]).0;
            }
        }
        new_lazy_flag(r1, r2)
    }
}

pub(crate) struct InputReader<'a> {
    pub(crate) r: &'a mut dyn RuneReader,
    pub(crate) at_eot: bool,
    pub(crate) pos: usize,
}

impl Input for InputReader<'_> {
    fn step(&mut self, pos: usize) -> (Rune, usize) {
        if !self.at_eot && pos != self.pos {
            return (END_OF_TEXT, 0);
        }
        match self.r.read_rune() {
            None => {
                self.at_eot = true;
                (END_OF_TEXT, 0)
            }
            Some((r, w)) => {
                self.pos += w;
                (r, w)
            }
        }
    }

    fn can_check_prefix(&self) -> bool {
        false
    }

    fn has_prefix(&self, _re: &Regexp) -> bool {
        false
    }

    fn index(&self, _re: &Regexp, _pos: usize) -> isize {
        -1
    }

    fn context(&self, _pos: usize) -> LazyFlag {
        0
    }
}

pub(crate) fn index(s: &[u8], substr: &[u8]) -> isize {
    let n = substr.len();
    if n == 0 {
        return 0;
    }
    if n > s.len() {
        return -1;
    }
    let c0 = substr[0];
    if n == 1 {
        return s.iter().position(|&c| c == c0).map_or(-1, |i| i as isize);
    }
    let c1 = substr[1];
    let t = s.len() - n + 1;
    let mut i = 0;
    let mut fails = 0;
    while i < t {
        if s[i] != c0 {
            match s[i + 1..t].iter().position(|&c| c == c0) {
                None => return -1,
                Some(o) => i += o + 1,
            }
        }
        if s[i + 1] == c1 && &s[i..i + n] == substr {
            return i as isize;
        }
        i += 1;
        fails += 1;
        if fails >= 4 + (i >> 4) && i < t {
            let j = index_rabin_karp(&s[i..], substr);
            if j < 0 {
                return -1;
            }
            return i as isize + j;
        }
    }
    -1
}

const PRIME_RK: u32 = 16777619;

fn index_rabin_karp(s: &[u8], sep: &[u8]) -> isize {
    let n = sep.len();
    if s.len() < n {
        return -1;
    }
    let mut hashsep: u32 = 0;
    for &c in sep {
        hashsep = hashsep.wrapping_mul(PRIME_RK).wrapping_add(u32::from(c));
    }
    let mut pow: u32 = 1;
    let mut sq: u32 = PRIME_RK;
    let mut i = n;
    while i > 0 {
        if i & 1 != 0 {
            pow = pow.wrapping_mul(sq);
        }
        sq = sq.wrapping_mul(sq);
        i >>= 1;
    }
    let mut h: u32 = 0;
    for &c in &s[..n] {
        h = h.wrapping_mul(PRIME_RK).wrapping_add(u32::from(c));
    }
    if h == hashsep && &s[..n] == sep {
        return 0;
    }
    let mut i = n;
    while i < s.len() {
        h = h.wrapping_mul(PRIME_RK).wrapping_add(u32::from(s[i]));
        h = h.wrapping_sub(pow.wrapping_mul(u32::from(s[i - n])));
        i += 1;
        if h == hashsep && &s[i - n..i] == sep {
            return (i - n) as isize;
        }
    }
    -1
}

impl Regexp {
    pub fn string(&self) -> &[u8] {
        &self.expr
    }

    pub fn copy(&self) -> Regexp {
        self.clone()
    }

    pub fn longest(&mut self) {
        self.longest = true;
    }

    pub fn num_subexp(&self) -> usize {
        self.num_subexp
    }

    pub fn subexp_names(&self) -> &[Vec<u8>] {
        &self.subexp_names
    }

    pub fn subexp_index(&self, name: &[u8]) -> isize {
        if !name.is_empty() {
            for (i, s) in self.subexp_names.iter().enumerate() {
                if name == s.as_slice() {
                    return i as isize;
                }
            }
        }
        -1
    }

    pub fn literal_prefix(&self) -> (&[u8], bool) {
        (&self.prefix, self.prefix_complete)
    }

    pub fn prog(&self) -> &Prog {
        &self.prog
    }

    pub fn onepass(&self) -> Option<&OnePassProg> {
        self.onepass.as_deref()
    }

    pub fn max_bit_state_len(&self) -> usize {
        self.max_bit_state_len
    }

    pub fn prefix_rune(&self) -> Rune {
        self.prefix_rune
    }

    pub fn prefix_end(&self) -> u32 {
        self.prefix_end
    }

    pub fn mpool(&self) -> usize {
        self.mpool
    }

    pub fn matchcap(&self) -> usize {
        self.matchcap
    }

    pub fn cond(&self) -> EmptyOp {
        self.cond
    }

    pub fn min_input_len(&self) -> isize {
        self.min_input_len
    }

    pub fn match_reader(&self, r: &mut dyn RuneReader) -> bool {
        self.do_execute(Source::Reader(r), 0, 0).is_some()
    }

    pub fn match_string(&self, s: &[u8]) -> bool {
        self.do_execute(Source::Bytes(s), 0, 0).is_some()
    }

    pub fn r#match(&self, b: &[u8]) -> bool {
        self.do_execute(Source::Bytes(b), 0, 0).is_some()
    }

    pub fn replace_all_string(&self, src: &[u8], repl: &[u8]) -> Vec<u8> {
        let mut n = 2;
        if repl.contains(&b'$') {
            n = 2 * (self.num_subexp + 1);
        }
        self.replace_all_impl(src, n, |dst, m| self.expand_impl(dst, repl, src, m))
    }

    pub fn replace_all_literal_string(&self, src: &[u8], repl: &[u8]) -> Vec<u8> {
        self.replace_all_impl(src, 2, |dst, _| dst.extend_from_slice(repl))
    }

    pub fn replace_all_string_func(
        &self,
        src: &[u8],
        mut repl: impl FnMut(&[u8]) -> Vec<u8>,
    ) -> Vec<u8> {
        self.replace_all_impl(src, 2, |dst, m| {
            dst.extend_from_slice(&repl(&src[m[0] as usize..m[1] as usize]))
        })
    }

    pub fn replace_all(&self, src: &[u8], repl: &[u8]) -> Vec<u8> {
        self.replace_all_string(src, repl)
    }

    pub fn replace_all_literal(&self, src: &[u8], repl: &[u8]) -> Vec<u8> {
        self.replace_all_literal_string(src, repl)
    }

    pub fn replace_all_func(&self, src: &[u8], repl: impl FnMut(&[u8]) -> Vec<u8>) -> Vec<u8> {
        self.replace_all_string_func(src, repl)
    }

    fn replace_all_impl(
        &self,
        src: &[u8],
        nmatch: usize,
        mut repl: impl FnMut(&mut Vec<u8>, &[isize]),
    ) -> Vec<u8> {
        let mut last_match_end = 0usize;
        let mut search_pos = 0usize;
        let mut buf = Vec::new();
        let end_pos = src.len();
        let nmatch = nmatch.min(self.prog.num_cap);
        while search_pos <= end_pos {
            let a = match self.do_execute(Source::Bytes(src), search_pos, nmatch) {
                Some(a) if !a.is_empty() => a,
                _ => break,
            };
            let (a0, a1) = (a[0] as usize, a[1] as usize);
            buf.extend_from_slice(&src[last_match_end..a0]);
            if a1 > last_match_end || a0 == 0 {
                repl(&mut buf, &a);
            }
            last_match_end = a1;
            let width = utf8::decode_rune(&src[search_pos..]).1;
            if search_pos + width > a1 {
                search_pos += width;
            } else if search_pos + 1 > a1 {
                search_pos += 1;
            } else {
                search_pos = a1;
            }
        }
        buf.extend_from_slice(&src[last_match_end..]);
        buf
    }

    pub fn find<'a>(&self, b: &'a [u8]) -> Option<&'a [u8]> {
        let a = self.do_execute(Source::Bytes(b), 0, 2)?;
        Some(&b[a[0] as usize..a[1] as usize])
    }

    pub fn find_index(&self, b: &[u8]) -> Option<Vec<isize>> {
        let a = self.do_execute(Source::Bytes(b), 0, 2)?;
        Some(a[0..2].to_vec())
    }

    pub fn find_string<'a>(&self, s: &'a [u8]) -> &'a [u8] {
        match self.do_execute(Source::Bytes(s), 0, 2) {
            None => &s[..0],
            Some(a) => &s[a[0] as usize..a[1] as usize],
        }
    }

    pub fn find_string_index(&self, s: &[u8]) -> Option<Vec<isize>> {
        let a = self.do_execute(Source::Bytes(s), 0, 2)?;
        Some(a[0..2].to_vec())
    }

    pub fn find_reader_index(&self, r: &mut dyn RuneReader) -> Option<Vec<isize>> {
        let a = self.do_execute(Source::Reader(r), 0, 2)?;
        Some(a[0..2].to_vec())
    }

    pub fn find_submatch<'a>(&self, b: &'a [u8]) -> Option<Vec<Option<&'a [u8]>>> {
        let a = self.do_execute(Source::Bytes(b), 0, self.prog.num_cap)?;
        let mut ret = vec![None; 1 + self.num_subexp];
        for (i, slot) in ret.iter_mut().enumerate() {
            if 2 * i < a.len() && a[2 * i] >= 0 {
                *slot = Some(&b[a[2 * i] as usize..a[2 * i + 1] as usize]);
            }
        }
        Some(ret)
    }

    pub fn expand(&self, dst: Vec<u8>, template: &[u8], src: &[u8], m: &[isize]) -> Vec<u8> {
        let mut dst = dst;
        self.expand_impl(&mut dst, template, src, m);
        dst
    }

    pub fn expand_string(&self, dst: Vec<u8>, template: &[u8], src: &[u8], m: &[isize]) -> Vec<u8> {
        self.expand(dst, template, src, m)
    }

    fn expand_impl(&self, dst: &mut Vec<u8>, template: &[u8], src: &[u8], m: &[isize]) {
        let mut template = template;
        while !template.is_empty() {
            let Some(k) = template.iter().position(|&c| c == b'$') else {
                break;
            };
            dst.extend_from_slice(&template[..k]);
            template = &template[k + 1..];
            if !template.is_empty() && template[0] == b'$' {
                dst.push(b'$');
                template = &template[1..];
                continue;
            }
            let Some((name, num, rest)) = extract(template) else {
                dst.push(b'$');
                continue;
            };
            template = rest;
            if num >= 0 {
                let num = num as usize;
                if 2 * num + 1 < m.len() && m[2 * num] >= 0 {
                    dst.extend_from_slice(&src[m[2 * num] as usize..m[2 * num + 1] as usize]);
                }
            } else {
                for (i, namei) in self.subexp_names.iter().enumerate() {
                    if name == namei.as_slice() && 2 * i + 1 < m.len() && m[2 * i] >= 0 {
                        dst.extend_from_slice(&src[m[2 * i] as usize..m[2 * i + 1] as usize]);
                        break;
                    }
                }
            }
        }
        dst.extend_from_slice(template);
    }

    fn pad(&self, a: Option<Vec<isize>>) -> Option<Vec<isize>> {
        let mut a = a?;
        let n = (1 + self.num_subexp) * 2;
        while a.len() < n {
            a.push(-1);
        }
        Some(a)
    }

    pub fn find_submatch_index(&self, b: &[u8]) -> Option<Vec<isize>> {
        self.pad(self.do_execute(Source::Bytes(b), 0, self.prog.num_cap))
    }

    pub fn find_string_submatch<'a>(&self, s: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        let a = self.do_execute(Source::Bytes(s), 0, self.prog.num_cap)?;
        let mut ret = vec![&s[..0]; 1 + self.num_subexp];
        for (i, slot) in ret.iter_mut().enumerate() {
            if 2 * i < a.len() && a[2 * i] >= 0 {
                *slot = &s[a[2 * i] as usize..a[2 * i + 1] as usize];
            }
        }
        Some(ret)
    }

    pub fn find_string_submatch_index(&self, s: &[u8]) -> Option<Vec<isize>> {
        self.pad(self.do_execute(Source::Bytes(s), 0, self.prog.num_cap))
    }

    pub fn find_reader_submatch_index(&self, r: &mut dyn RuneReader) -> Option<Vec<isize>> {
        self.pad(self.do_execute(Source::Reader(r), 0, self.prog.num_cap))
    }

    fn all_matches(&self, b: &[u8], n: usize, mut deliver: impl FnMut(Vec<isize>)) {
        let end = b.len();
        let mut pos = 0usize;
        let mut i = 0usize;
        let mut prev_match_end: isize = -1;
        while i < n && pos <= end {
            let matches = match self.do_execute(Source::Bytes(b), pos, self.prog.num_cap) {
                Some(m) if !m.is_empty() => m,
                _ => break,
            };
            let mut accept = true;
            if matches[1] == pos as isize {
                if matches[0] == prev_match_end {
                    accept = false;
                }
                let width = if pos < b.len() {
                    if b[pos] < utf8::RUNE_SELF {
                        1
                    } else {
                        utf8::decode_rune(&b[pos..]).1
                    }
                } else {
                    0
                };
                if width > 0 {
                    pos += width;
                } else {
                    pos = end + 1;
                }
            } else {
                pos = matches[1] as usize;
            }
            prev_match_end = matches[1];
            if accept {
                if let Some(m) = self.pad(Some(matches)) {
                    deliver(m);
                }
                i += 1;
            }
        }
    }

    fn limit(n: isize, len: usize) -> usize {
        if n < 0 { len + 1 } else { n as usize }
    }

    pub fn find_all<'a>(&self, b: &'a [u8], n: isize) -> Option<Vec<&'a [u8]>> {
        let mut result: Option<Vec<&'a [u8]>> = None;
        self.all_matches(b, Self::limit(n, b.len()), |m| {
            result
                .get_or_insert_with(Vec::new)
                .push(&b[m[0] as usize..m[1] as usize]);
        });
        result
    }

    pub fn find_all_index(&self, b: &[u8], n: isize) -> Option<Vec<Vec<isize>>> {
        let mut result: Option<Vec<Vec<isize>>> = None;
        self.all_matches(b, Self::limit(n, b.len()), |m| {
            result.get_or_insert_with(Vec::new).push(m[0..2].to_vec());
        });
        result
    }

    pub fn find_all_string<'a>(&self, s: &'a [u8], n: isize) -> Option<Vec<&'a [u8]>> {
        self.find_all(s, n)
    }

    pub fn find_all_string_index(&self, s: &[u8], n: isize) -> Option<Vec<Vec<isize>>> {
        self.find_all_index(s, n)
    }

    pub fn find_all_submatch<'a>(
        &self,
        b: &'a [u8],
        n: isize,
    ) -> Option<Vec<Vec<Option<&'a [u8]>>>> {
        let mut result: Option<Vec<Vec<Option<&'a [u8]>>>> = None;
        self.all_matches(b, Self::limit(n, b.len()), |m| {
            let mut slice = vec![None; m.len() / 2];
            for (j, slot) in slice.iter_mut().enumerate() {
                if m[2 * j] >= 0 {
                    *slot = Some(&b[m[2 * j] as usize..m[2 * j + 1] as usize]);
                }
            }
            result.get_or_insert_with(Vec::new).push(slice);
        });
        result
    }

    pub fn find_all_submatch_index(&self, b: &[u8], n: isize) -> Option<Vec<Vec<isize>>> {
        let mut result: Option<Vec<Vec<isize>>> = None;
        self.all_matches(b, Self::limit(n, b.len()), |m| {
            result.get_or_insert_with(Vec::new).push(m);
        });
        result
    }

    pub fn find_all_string_submatch<'a>(
        &self,
        s: &'a [u8],
        n: isize,
    ) -> Option<Vec<Vec<&'a [u8]>>> {
        let mut result: Option<Vec<Vec<&'a [u8]>>> = None;
        self.all_matches(s, Self::limit(n, s.len()), |m| {
            let mut slice = vec![&s[..0]; m.len() / 2];
            for (j, slot) in slice.iter_mut().enumerate() {
                if m[2 * j] >= 0 {
                    *slot = &s[m[2 * j] as usize..m[2 * j + 1] as usize];
                }
            }
            result.get_or_insert_with(Vec::new).push(slice);
        });
        result
    }

    pub fn find_all_string_submatch_index(&self, s: &[u8], n: isize) -> Option<Vec<Vec<isize>>> {
        self.find_all_submatch_index(s, n)
    }

    pub fn split<'a>(&self, s: &'a [u8], n: isize) -> Option<Vec<&'a [u8]>> {
        if n == 0 {
            return None;
        }
        if !self.expr.is_empty() && s.is_empty() {
            return Some(vec![&s[..0]]);
        }
        let matches = self.find_all_string_index(s, n).unwrap_or_default();
        let mut strings: Vec<&'a [u8]> = Vec::with_capacity(matches.len());
        let mut beg = 0usize;
        let mut end = 0usize;
        for m in &matches {
            if n > 0 && strings.len() as isize >= n - 1 {
                break;
            }
            end = m[0] as usize;
            if m[1] != 0 {
                strings.push(&s[beg..end]);
            }
            beg = m[1] as usize;
        }
        if end != s.len() {
            strings.push(&s[beg..]);
        }
        Some(strings)
    }

    pub fn append_text(&self, b: Vec<u8>) -> Vec<u8> {
        let mut b = b;
        b.extend_from_slice(self.string());
        b
    }

    pub fn marshal_text(&self) -> Vec<u8> {
        self.append_text(Vec::new())
    }

    pub fn unmarshal_text(&mut self, text: &[u8]) -> Result<(), Error> {
        *self = compile(text)?;
        Ok(())
    }
}

fn extract(s: &[u8]) -> Option<(&[u8], i64, &[u8])> {
    if s.is_empty() {
        return None;
    }
    let mut s = s;
    let mut brace = false;
    if s[0] == b'{' {
        brace = true;
        s = &s[1..];
    }
    let mut i = 0;
    while i < s.len() {
        let (r, size) = utf8::decode_rune(&s[i..]);
        if !unicode::is_letter(r) && !unicode::is_digit(r) && r != '_' as Rune {
            break;
        }
        i += size;
    }
    if i == 0 {
        return None;
    }
    let name = &s[..i];
    if brace {
        if i >= s.len() || s[i] != b'}' {
            return None;
        }
        i += 1;
    }
    let mut num: i64 = 0;
    for &c in name {
        if !c.is_ascii_digit() || num >= 100_000_000 {
            num = -1;
            break;
        }
        num = num * 10 + i64::from(c - b'0');
    }
    if name[0] == b'0' && name.len() > 1 {
        num = -1;
    }
    Some((name, num, &s[i..]))
}

fn special(b: u8) -> bool {
    br"\.+*?()|[]{}^$".contains(&b)
}

pub fn quote_meta(s: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(2 * s.len());
    for &c in s {
        if special(c) {
            b.push(b'\\');
        }
        b.push(c);
    }
    b
}
