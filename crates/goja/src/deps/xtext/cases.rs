mod context;
#[cfg(test)]
mod context_test;
#[cfg(test)]
mod example_test;
#[cfg(test)]
mod info;
mod map;
#[cfg(test)]
mod map_test;
mod tables;
#[cfg(test)]
mod tables_test;
mod trieval;

use std::borrow::Cow;

pub use tables::UNICODE_VERSION;

use super::language::Tag;
use super::norm::transform::{self, Error, SpanningTransformer, Transformer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UndCaser {
    Upper,
    Lower,
    LowerIgnoreSigma,
}

impl Transformer for UndCaser {
    fn transform(
        &mut self,
        dst: &mut [u8],
        src: &[u8],
        at_eof: bool,
    ) -> (usize, usize, Result<(), Error>) {
        match self {
            UndCaser::Upper => map::und_upper_transform(dst, src, at_eof),
            UndCaser::Lower => map::und_lower_transform(dst, src, at_eof),
            UndCaser::LowerIgnoreSigma => map::und_lower_ignore_sigma_transform(dst, src, at_eof),
        }
    }

    fn reset(&mut self) {}
}

impl SpanningTransformer for UndCaser {
    fn span(&mut self, src: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
        match self {
            UndCaser::Upper => map::und_upper_span(src, at_eof),
            UndCaser::Lower | UndCaser::LowerIgnoreSigma => map::und_lower_span(src, at_eof),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caser {
    t: UndCaser,
}

impl Caser {
    pub fn bytes(&self, b: &[u8]) -> Vec<u8> {
        let mut t = self.t;
        transform::bytes(&mut t, b).0
    }

    pub fn string<'a>(&self, s: &'a [u8]) -> Cow<'a, [u8]> {
        let mut t = self.t;
        transform::string(&mut t, s).0
    }

    pub fn reset(&self) {
        let mut t = self.t;
        t.reset();
    }

    pub fn transform(
        &self,
        dst: &mut [u8],
        src: &[u8],
        at_eof: bool,
    ) -> (usize, usize, Result<(), Error>) {
        let mut t = self.t;
        t.transform(dst, src, at_eof)
    }

    pub fn span(&self, src: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
        let mut t = self.t;
        t.span(src, at_eof)
    }
}

impl Transformer for Caser {
    fn transform(
        &mut self,
        dst: &mut [u8],
        src: &[u8],
        at_eof: bool,
    ) -> (usize, usize, Result<(), Error>) {
        self.t.transform(dst, src, at_eof)
    }

    fn reset(&mut self) {
        self.t.reset();
    }
}

impl SpanningTransformer for Caser {
    fn span(&mut self, src: &[u8], at_eof: bool) -> (usize, Result<(), Error>) {
        self.t.span(src, at_eof)
    }
}

pub fn upper(t: Tag, opts: &[Option]) -> Caser {
    Caser {
        t: map::make_upper(t, get_opts(opts)),
    }
}

pub fn lower(t: Tag, opts: &[Option]) -> Caser {
    Caser {
        t: map::make_lower(t, get_opts(opts)),
    }
}

#[derive(Clone, Copy)]
pub struct Option(fn(Options) -> Options);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Options {
    ignore_final_sigma: bool,
}

fn get_opts(o: &[Option]) -> Options {
    let mut res = Options::default();
    for f in o {
        res = (f.0)(res);
    }
    res
}

pub fn handle_final_sigma(enable: bool) -> Option {
    if enable {
        return Option(handle_final_sigma_f);
    }
    Option(ignore_final_sigma_f)
}

fn ignore_final_sigma_f(mut o: Options) -> Options {
    o.ignore_final_sigma = true;
    o
}

fn handle_final_sigma_f(mut o: Options) -> Options {
    o.ignore_final_sigma = false;
    o
}
