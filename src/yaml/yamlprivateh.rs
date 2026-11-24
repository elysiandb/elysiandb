pub(super) const INPUT_RAW_BUFFER_SIZE: usize = 512;

pub(super) fn is_alpha(b: &[u8], i: usize) -> bool {
    b[i].is_ascii_digit()
        || b[i].is_ascii_uppercase()
        || b[i].is_ascii_lowercase()
        || b[i] == b'_'
        || b[i] == b'-'
}

pub(super) fn is_digit(b: &[u8], i: usize) -> bool {
    b[i].is_ascii_digit()
}

pub(super) fn as_digit(b: &[u8], i: usize) -> isize {
    isize::from(b[i]) - isize::from(b'0')
}

pub(super) fn is_hex(b: &[u8], i: usize) -> bool {
    b[i].is_ascii_digit() || (b'A'..=b'F').contains(&b[i]) || (b'a'..=b'f').contains(&b[i])
}

pub(super) fn as_hex(b: &[u8], i: usize) -> u32 {
    let bi = b[i];
    if (b'A'..=b'F').contains(&bi) {
        return u32::from(bi) - u32::from(b'A') + 10;
    }
    if (b'a'..=b'f').contains(&bi) {
        return u32::from(bi) - u32::from(b'a') + 10;
    }
    u32::from(bi).wrapping_sub(u32::from(b'0'))
}

pub(super) fn is_z(b: &[u8], i: usize) -> bool {
    b[i] == 0x00
}

pub(super) fn is_bom(b: &[u8]) -> bool {
    b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF
}

pub(super) fn is_space(b: &[u8], i: usize) -> bool {
    b[i] == b' '
}

pub(super) fn is_tab(b: &[u8], i: usize) -> bool {
    b[i] == b'\t'
}

pub(super) fn is_blank(b: &[u8], i: usize) -> bool {
    b[i] == b' ' || b[i] == b'\t'
}

pub(super) fn is_break(b: &[u8], i: usize) -> bool {
    b[i] == b'\r'
        || b[i] == b'\n'
        || b[i] == 0xC2 && b[i + 1] == 0x85
        || b[i] == 0xE2 && b[i + 1] == 0x80 && b[i + 2] == 0xA8
        || b[i] == 0xE2 && b[i + 1] == 0x80 && b[i + 2] == 0xA9
}

pub(super) fn is_crlf(b: &[u8], i: usize) -> bool {
    b[i] == b'\r' && b[i + 1] == b'\n'
}

pub(super) fn is_breakz(b: &[u8], i: usize) -> bool {
    is_break(b, i) || b[i] == 0
}

pub(super) fn is_blankz(b: &[u8], i: usize) -> bool {
    is_blank(b, i) || is_breakz(b, i)
}

pub(super) fn width(b: u8) -> usize {
    if b & 0x80 == 0x00 {
        return 1;
    }
    if b & 0xE0 == 0xC0 {
        return 2;
    }
    if b & 0xF0 == 0xE0 {
        return 3;
    }
    if b & 0xF8 == 0xF0 {
        return 4;
    }
    0
}
