use super::regexp::Op;
use std::fmt;

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Op::NoMatch => "NoMatch",
            Op::EmptyMatch => "EmptyMatch",
            Op::Literal => "Literal",
            Op::CharClass => "CharClass",
            Op::AnyCharNotNL => "AnyCharNotNL",
            Op::AnyChar => "AnyChar",
            Op::BeginLine => "BeginLine",
            Op::EndLine => "EndLine",
            Op::BeginText => "BeginText",
            Op::EndText => "EndText",
            Op::WordBoundary => "WordBoundary",
            Op::NoWordBoundary => "NoWordBoundary",
            Op::Capture => "Capture",
            Op::Star => "Star",
            Op::Plus => "Plus",
            Op::Quest => "Quest",
            Op::Repeat => "Repeat",
            Op::Concat => "Concat",
            Op::Alternate => "Alternate",
            Op::LeftParen => "opPseudo",
            Op::VerticalBar => return write!(f, "Op({})", *self as u8),
        };
        f.write_str(name)
    }
}
