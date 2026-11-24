use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::strconv::append_float;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Number(pub f64);

impl Encode for Number {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut b = Vec::with_capacity(24);
        append_float(&mut b, self.0, b'f', -1, 64);
        e.write_marshaled(&b)
    }
}

pub fn num(f: f64) -> Option<Number> {
    if f.is_nan() || f.is_infinite() {
        return None;
    }

    Some(Number(round(f)))
}

pub fn round(f: f64) -> f64 {
    if f.is_nan() || f.is_infinite() {
        return f;
    }

    (f * 1000.0).round() / 1000.0
}
