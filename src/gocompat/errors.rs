use std::error::Error as StdError;
use std::fmt;

pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    msg: String,
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Error {
        Error { msg: msg.into() }
    }

    pub fn message(&self) -> &str {
        &self.msg
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl StdError for Error {}

pub fn new(msg: impl Into<String>) -> BoxError {
    Box::new(Error::new(msg))
}

#[derive(Debug)]
pub struct Wrapped {
    msg: String,
    source: BoxError,
}

impl Wrapped {
    pub fn new(msg: impl Into<String>, source: BoxError) -> Wrapped {
        Wrapped {
            msg: msg.into(),
            source,
        }
    }

    pub fn inner(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self.source.as_ref()
    }

    pub fn into_inner(self) -> BoxError {
        self.source
    }
}

impl fmt::Display for Wrapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl StdError for Wrapped {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

pub fn wrap(prefix: &str, err: BoxError) -> BoxError {
    let msg = format!("{prefix}{err}");
    Box::new(Wrapped::new(msg, err))
}

#[derive(Debug)]
pub struct Joined {
    errs: Vec<BoxError>,
}

impl Joined {
    pub fn errors(&self) -> &[BoxError] {
        &self.errs
    }

    pub fn into_errors(self) -> Vec<BoxError> {
        self.errs
    }
}

impl fmt::Display for Joined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, err) in self.errs.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{err}")?;
        }
        Ok(())
    }
}

impl StdError for Joined {}

pub fn join<I>(errs: I) -> Option<Joined>
where
    I: IntoIterator<Item = Option<BoxError>>,
{
    let errs: Vec<BoxError> = errs.into_iter().flatten().collect();
    if errs.is_empty() {
        None
    } else {
        Some(Joined { errs })
    }
}

pub fn unwrap<'a>(err: &'a (dyn StdError + 'static)) -> Option<&'a (dyn StdError + 'static)> {
    if err.is::<Joined>() {
        return None;
    }
    err.source()
}

pub fn any<'a, F>(err: &'a (dyn StdError + 'static), pred: &mut F) -> bool
where
    F: FnMut(&'a (dyn StdError + 'static)) -> bool,
{
    let mut cur = err;
    loop {
        if pred(cur) {
            return true;
        }
        if let Some(joined) = cur.downcast_ref::<Joined>() {
            for child in &joined.errs {
                let child: &(dyn StdError + 'static) = child.as_ref();
                if any(child, pred) {
                    return true;
                }
            }
            return false;
        }
        match cur.source() {
            Some(next) => cur = next,
            None => return false,
        }
    }
}

pub fn find<'a, T>(err: &'a (dyn StdError + 'static)) -> Option<&'a T>
where
    T: StdError + 'static,
{
    let mut found = None;
    any(err, &mut |e| match e.downcast_ref::<T>() {
        Some(t) => {
            found = Some(t);
            true
        }
        None => false,
    });
    found
}

pub fn is<T>(err: &(dyn StdError + 'static), target: &T) -> bool
where
    T: StdError + PartialEq + 'static,
{
    any(err, &mut |e| e.downcast_ref::<T>() == Some(target))
}
