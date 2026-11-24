use crate::docpath;

pub fn match_glob(pattern: &[u8], s: &[u8]) -> bool {
    docpath::match_glob(pattern, s)
}
