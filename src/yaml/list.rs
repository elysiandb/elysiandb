use super::decode::{Decoder, Outcome, Scalar, Target};
use super::value::Value;
use super::yaml::{Error, Expected};
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use std::ops::Deref;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringList(pub Option<Vec<String>>);

impl StringList {
    pub const fn nil() -> StringList {
        StringList(None)
    }

    pub fn is_nil(&self) -> bool {
        self.0.is_none()
    }

    pub fn as_slice(&self) -> &[String] {
        self.0.as_deref().unwrap_or(&[])
    }
}

impl Deref for StringList {
    type Target = [String];

    fn deref(&self) -> &[String] {
        self.as_slice()
    }
}

impl From<Vec<String>> for StringList {
    fn from(items: Vec<String>) -> StringList {
        StringList(Some(items))
    }
}

impl Target for StringList {
    const EXPECTED: Expected = Expected::Strings;

    fn decode_scalar(&mut self, _scalar: &Scalar<'_>) -> Outcome {
        Outcome::Mismatch
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        null: bool,
    ) -> Result<bool, Error> {
        let mut items: Vec<String> = Vec::new();
        let good = items.decode_sequence(d, n, null);
        self.0 = Some(items);
        good
    }

    fn scalar_null(&mut self, _null: bool) {
        self.0 = None;
    }

    fn set_zero(&mut self) {
        self.0 = None;
    }

    fn to_value(&self) -> Value {
        Value::Seq(self.as_slice().iter().map(Target::to_value).collect())
    }
}

impl Encode for StringList {
    const NILABLE: bool = true;

    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        match &self.0 {
            None => {
                e.write_null();
                Ok(())
            }
            Some(items) => items.encode(e),
        }
    }

    fn is_empty_value(&self) -> bool {
        self.as_slice().is_empty()
    }
}
