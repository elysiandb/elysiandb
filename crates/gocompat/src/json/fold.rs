use crate::unicode;
use crate::utf8::{self, RUNE_SELF};

pub fn append_folded_name(out: &mut Vec<u8>, input: &[u8]) {
    let mut i = 0;
    while i < input.len() {
        let c = input[i];
        if c < RUNE_SELF {
            out.push(if c.is_ascii_lowercase() { c - 32 } else { c });
            i += 1;
            continue;
        }
        let (r, n) = utf8::decode_rune(&input[i..]);
        utf8::append_rune(out, unicode::to_upper(unicode::to_lower(r)));
        i += n;
    }
}

pub fn fold_name(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    append_folded_name(&mut out, input);
    out
}
