use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use gocompat::utf8::{self, Rune};

use super::r#match::Match;
use super::replace::replace;
use super::runner::{Runner, Scanned};
use super::syntax::{self, Code, RIGHT_TO_LEFT, RegexOptions, decode_runes, new_replacer_data};

pub use super::syntax::{
    COMPILED, DEBUG, ECMASCRIPT, EXPLICIT_CAPTURE, IGNORE_CASE, IGNORE_PATTERN_WHITESPACE,
    MULTILINE, RE2, SINGLELINE, UNICODE,
};

pub const NONE: RegexOptions = 0x0;

pub static DEFAULT_UNMARSHAL_OPTIONS: AtomicI32 = AtomicI32::new(NONE);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Syntax(syntax::Error),
    StartAtTooLarge,
    StartAtNotAligned,
    CountTooSmall,
    UnknownRunnerState,
}

impl Error {
    pub fn reason(&self) -> String {
        match self {
            Error::Syntax(e) => e.reason(),
            Error::StartAtTooLarge => "start position is past the end of the input".to_string(),
            Error::StartAtNotAligned => {
                "start position is not at the beginning of a character".to_string()
            }
            Error::CountTooSmall => "replacement count is below -1".to_string(),
            Error::UnknownRunnerState => {
                "unknown state in the regular expression engine".to_string()
            }
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for Error {}

impl From<syntax::Error> for Error {
    fn from(e: syntax::Error) -> Error {
        Error::Syntax(e)
    }
}

#[derive(Debug)]
pub struct Regexp {
    pattern: Vec<u8>,
    options: RegexOptions,
    pub(crate) caps: Option<HashMap<isize, isize>>,
    capnames: Option<HashMap<String, isize>>,
    capslist: Option<Vec<String>>,
    capsize: isize,
    code: Code,
    runner: Mutex<Vec<Runner>>,
}

struct RunnerGuard<'a> {
    re: &'a Regexp,
    runner: Option<Runner>,
}

impl Drop for RunnerGuard<'_> {
    fn drop(&mut self) {
        if let Some(runner) = self.runner.take() {
            self.re.put_runner(runner);
        }
    }
}

impl Regexp {
    pub fn compile<S: AsRef<[u8]> + ?Sized>(expr: &S, opt: RegexOptions) -> Result<Regexp, Error> {
        let tree = syntax::parse(expr, opt)?;
        let code = syntax::write(&tree);
        Ok(Regexp {
            pattern: expr.as_ref().to_vec(),
            options: opt,
            caps: code.caps.clone(),
            capnames: tree.capnames,
            capslist: tree.caplist,
            capsize: code.capsize,
            code,
            runner: Mutex::new(Vec::new()),
        })
    }

    pub fn must_compile<S: AsRef<[u8]> + ?Sized>(expr: &S, opt: RegexOptions) -> Regexp {
        match Regexp::compile(expr, opt) {
            Ok(re) => re,
            Err(e) => panic!(
                "invalid regular expression /{}/: {}",
                String::from_utf8_lossy(expr.as_ref()),
                e
            ),
        }
    }

    pub fn string(&self) -> &[u8] {
        &self.pattern
    }

    pub fn options(&self) -> RegexOptions {
        self.options
    }

    pub fn capsize(&self) -> isize {
        self.capsize
    }

    pub fn code(&self) -> &Code {
        &self.code
    }

    pub fn right_to_left(&self) -> bool {
        self.options & RIGHT_TO_LEFT != 0
    }

    pub fn debug(&self) -> bool {
        self.options & DEBUG != 0
    }

    pub fn replace(
        &self,
        input: &str,
        replacement: &str,
        start_at: isize,
        count: isize,
    ) -> Result<String, Error> {
        let data = new_replacer_data(
            replacement,
            self.caps.as_ref(),
            self.capsize,
            self.capnames.as_ref(),
            self.options,
        )?;
        replace(self, Some(&data), None, input, start_at, count)
    }

    pub fn replace_func<F: FnMut(&Match<'_>) -> String>(
        &self,
        input: &str,
        mut evaluator: F,
        start_at: isize,
        count: isize,
    ) -> Result<String, Error> {
        replace(self, None, Some(&mut evaluator), input, start_at, count)
    }

    pub fn find_string_match<S: AsRef<[u8]> + ?Sized>(
        &self,
        s: &S,
    ) -> Result<Option<Match<'_>>, Error> {
        Ok(self.run(false, -1, decode_runes(s.as_ref()).into())?.1)
    }

    pub fn find_runes_match<R: Into<Arc<[Rune]>>>(&self, r: R) -> Result<Option<Match<'_>>, Error> {
        Ok(self.run(false, -1, r.into())?.1)
    }

    pub fn find_string_match_starting_at<S: AsRef<[u8]> + ?Sized>(
        &self,
        s: &S,
        start_at: isize,
    ) -> Result<Option<Match<'_>>, Error> {
        let s = s.as_ref();
        if start_at > s.len() as isize {
            return Err(Error::StartAtTooLarge);
        }
        let (r, start_at) = self.get_runes_and_start(s, start_at);
        if start_at == -1 {
            return Err(Error::StartAtNotAligned);
        }
        Ok(self.run(false, start_at, r.into())?.1)
    }

    pub fn find_runes_match_starting_at<R: Into<Arc<[Rune]>>>(
        &self,
        r: R,
        start_at: isize,
    ) -> Result<Option<Match<'_>>, Error> {
        Ok(self.run(false, start_at, r.into())?.1)
    }

    pub fn find_next_match(&self, m: &Match<'_>) -> Result<Option<Match<'_>>, Error> {
        let mut start_at = m.textpos;
        if m.length == 0 {
            if m.textpos == m.text().len() as isize {
                return Ok(None);
            }
            if self.right_to_left() {
                start_at -= 1;
            } else {
                start_at += 1;
            }
        }
        Ok(self.run(false, start_at, m.text().clone())?.1)
    }

    pub fn match_string<S: AsRef<[u8]> + ?Sized>(&self, s: &S) -> Result<bool, Error> {
        Ok(self.run(true, -1, decode_runes(s.as_ref()).into())?.0)
    }

    pub fn match_runes<R: Into<Arc<[Rune]>>>(&self, r: R) -> Result<bool, Error> {
        Ok(self.run(true, -1, r.into())?.0)
    }

    fn get_runes_and_start(&self, s: &[u8], start_at: isize) -> (Vec<Rune>, isize) {
        if start_at < 0 {
            let r = decode_runes(s);
            if self.right_to_left() {
                let l = r.len() as isize;
                return (r, l);
            }
            return (r, 0);
        }
        let mut ret = Vec::with_capacity(s.len());
        let mut rune_idx: isize = -1;
        let mut str_idx = 0usize;
        while str_idx < s.len() {
            if str_idx as isize == start_at {
                rune_idx = ret.len() as isize;
            }
            let (r, n) = utf8::decode_rune(&s[str_idx..]);
            ret.push(r);
            str_idx += n;
        }
        if start_at == s.len() as isize {
            rune_idx = ret.len() as isize;
        }
        (ret, rune_idx)
    }

    fn run(
        &self,
        quick: bool,
        textstart: isize,
        input: Arc<[Rune]>,
    ) -> Result<(bool, Option<Match<'_>>), Error> {
        let mut guard = RunnerGuard {
            re: self,
            runner: Some(self.get_runner()),
        };
        let textstart = if textstart < 0 {
            if self.right_to_left() {
                input.len() as isize
            } else {
                0
            }
        } else {
            textstart
        };
        let text = input.clone();
        let runner = guard.runner.as_mut().expect("a runner");
        match runner.scan(self, input, textstart, quick)? {
            Scanned::None => Ok((false, None)),
            Scanned::Quick => Ok((true, None)),
            Scanned::Found(state, textpos) => {
                Ok((true, Some(Match::from_state(self, state, text, textpos))))
            }
        }
    }

    fn get_runner(&self) -> Runner {
        let mut cache = self.runner.lock().unwrap_or_else(|e| e.into_inner());
        cache.pop().unwrap_or_default()
    }

    fn put_runner(&self, mut r: Runner) {
        r.release();
        let mut cache = self.runner.lock().unwrap_or_else(|e| e.into_inner());
        cache.push(r);
    }

    pub fn get_group_names(&self) -> Vec<String> {
        match &self.capslist {
            None => (0..self.capsize).map(|i| i.to_string()).collect(),
            Some(list) => list.clone(),
        }
    }

    pub fn get_group_numbers(&self) -> Vec<isize> {
        match &self.caps {
            None => (0..self.capsize).collect(),
            Some(caps) => {
                let mut result = vec![0; caps.len()];
                for (&k, &v) in caps {
                    result[v as usize] = k;
                }
                result
            }
        }
    }

    pub fn group_name_from_number(&self, i: isize) -> String {
        let Some(capslist) = &self.capslist else {
            if i >= 0 && i < self.capsize {
                return i.to_string();
            }
            return String::new();
        };
        let mut i = i;
        if let Some(caps) = &self.caps {
            match caps.get(&i) {
                Some(&v) => i = v,
                None => return String::new(),
            }
        }
        if i >= 0 && (i as usize) < capslist.len() {
            return capslist[i as usize].clone();
        }
        String::new()
    }

    pub fn group_number_from_name(&self, name: &str) -> isize {
        if let Some(capnames) = &self.capnames {
            return capnames.get(name).copied().unwrap_or(-1);
        }
        let mut result: isize = 0;
        for &ch in name.as_bytes() {
            if !ch.is_ascii_digit() {
                return -1;
            }
            result = result.wrapping_mul(10).wrapping_add(isize::from(ch - b'0'));
        }
        if result >= 0 && result < self.capsize {
            return result;
        }
        -1
    }

    pub fn marshal_text(&self) -> Vec<u8> {
        self.pattern.clone()
    }

    pub fn unmarshal_text(&mut self, text: &[u8]) -> Result<(), Error> {
        let new_re = Regexp::compile(text, DEFAULT_UNMARSHAL_OPTIONS.load(Ordering::SeqCst))?;
        *self = new_re;
        Ok(())
    }
}

pub fn compile<S: AsRef<[u8]> + ?Sized>(expr: &S, opt: RegexOptions) -> Result<Regexp, Error> {
    Regexp::compile(expr, opt)
}

pub fn must_compile<S: AsRef<[u8]> + ?Sized>(expr: &S, opt: RegexOptions) -> Regexp {
    Regexp::must_compile(expr, opt)
}

pub fn escape<S: AsRef<[u8]> + ?Sized>(input: &S) -> String {
    syntax::escape(input)
}

pub fn unescape(input: &str) -> Result<String, Error> {
    syntax::unescape(input).map_err(Error::Syntax)
}
