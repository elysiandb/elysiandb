use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tag(());

pub const UND: Tag = Tag(());

impl Tag {
    pub fn type_for_key(&self, _key: &str) -> &'static str {
        ""
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("und")
    }
}
