mod apic;
mod decode;
mod node;
mod parserc;
mod readerc;
mod resolve;
mod scannerc;
mod value;
#[allow(clippy::module_inception)]
mod yaml;
mod yamlh;
mod yamlprivateh;

pub use decode::{Decoder, Fields, Outcome, Scalar, Target};
pub use node::{Document, Node, NodeKind, parse};
pub use resolve::Resolved;
pub use value::Value;
pub use yaml::{
    Error, Expected, Got, Issue, IssueKind, PREFIX, PathSegment, SyntaxError, SyntaxKind, dotted,
    unmarshal, unmarshal_strict,
};
pub use yamlh::Mark;

#[macro_export]
macro_rules! config_struct {
    ($(
        pub struct $name:ident {
            $($field:ident : $ty:ty = $key:literal,)*
        }
    )*) => {
        $(
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct $name {
                $(pub $field: $ty,)*
            }

            impl $crate::yaml::Target for $name {
                const EXPECTED: $crate::yaml::Expected = $crate::yaml::Expected::Mapping;

                fn decode_scalar(&mut self, _scalar: &$crate::yaml::Scalar<'_>) -> $crate::yaml::Outcome {
                    $crate::yaml::Outcome::Mismatch
                }

                fn decode_mapping(
                    &mut self,
                    d: &mut $crate::yaml::Decoder<'_>,
                    n: usize,
                    _null: bool,
                ) -> ::std::result::Result<bool, $crate::yaml::Error> {
                    d.mapping_struct(n, self)
                }

                fn set_zero(&mut self) {
                    *self = Self::default();
                }

                fn to_value(&self) -> $crate::yaml::Value {
                    $crate::yaml::Value::Map(::std::vec![$((
                        $crate::yaml::Value::Str($key.as_bytes().to_vec()),
                        $crate::yaml::Target::to_value(&self.$field),
                    ),)*])
                }
            }

            impl $crate::yaml::Fields for $name {
                const KEYS: &'static [&'static str] = &[$($key),*];

                fn decode_field(
                    &mut self,
                    d: &mut $crate::yaml::Decoder<'_>,
                    index: usize,
                    n: usize,
                ) -> ::std::result::Result<bool, $crate::yaml::Error> {
                    match <Self as $crate::yaml::Fields>::KEYS.get(index).copied() {
                        $(Some($key) => d.unmarshal(n, &mut self.$field),)*
                        _ => Ok(false),
                    }
                }
            }
        )*
    };
}
