mod format;
mod print;

pub use print::{append, appendf, appendln, sprint, sprintf, sprintln};

#[derive(Clone, Debug, PartialEq)]
pub enum Arg<'a> {
    Nil,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Float32(f32),
    Str(&'a [u8]),
    Bytes(&'a [u8]),
    Slice(Vec<Arg<'a>>),
    Map(Vec<(Arg<'a>, Arg<'a>)>),
    Struct(Vec<(&'a str, Arg<'a>)>),
    Stringer(&'a [u8], Box<Arg<'a>>),
    Error(&'a [u8]),
}
