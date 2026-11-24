use crate::deps::regexp;
use crate::deps::regexp2;
use crate::string::JsString;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Regexp2MatchCache {
    pub target: JsString,
    pub runes: Vec<i32>,
    pub pos_map: Vec<isize>,
}

#[derive(Clone, Debug)]
pub struct Regexp2Wrapper {
    pub rx: Arc<regexp2::Regexp>,
    pub cache: Option<Regexp2MatchCache>,
}

#[derive(Clone, Debug)]
pub struct RegexpWrapper(pub regexp::Regexp);

#[derive(Clone, Debug)]
pub struct RegexpPattern {
    pub src: Vec<u8>,
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub sticky: bool,
    pub unicode: bool,
    pub regexp_wrapper: Option<Arc<RegexpWrapper>>,
    pub regexp2_wrapper: Option<Regexp2Wrapper>,
}

pub fn compile_regexp2(
    src: &[u8],
    multiline: bool,
    dot_all: bool,
    ignore_case: bool,
    unicode: bool,
) -> Result<Regexp2Wrapper, regexp2::Error> {
    let mut opts: regexp2::RegexOptions = regexp2::ECMASCRIPT;
    if multiline {
        opts |= regexp2::MULTILINE;
    }
    if dot_all {
        opts |= regexp2::SINGLELINE;
    }
    if ignore_case {
        opts |= regexp2::IGNORE_CASE;
    }
    if unicode {
        opts |= regexp2::UNICODE;
    }
    let rx = regexp2::compile(src, opts)?;
    Ok(Regexp2Wrapper {
        rx: Arc::new(rx),
        cache: None,
    })
}
