pub const MAX_VARINT_LEN16: usize = 3;
pub const MAX_VARINT_LEN32: usize = 5;
pub const MAX_VARINT_LEN64: usize = 10;

pub fn append_uvarint(buf: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        buf.push((x as u8) | 0x80);
        x >>= 7;
    }
    buf.push(x as u8);
}

pub fn append_varint(buf: &mut Vec<u8>, x: i64) {
    let mut ux = (x as u64) << 1;
    if x < 0 {
        ux = !ux;
    }
    append_uvarint(buf, ux);
}

pub fn uvarint(buf: &[u8]) -> (u64, isize) {
    let mut x: u64 = 0;
    let mut s: u32 = 0;
    for (i, &b) in buf.iter().enumerate() {
        if i == MAX_VARINT_LEN64 {
            return (0, -(i as isize + 1));
        }
        if b < 0x80 {
            if i == MAX_VARINT_LEN64 - 1 && b > 1 {
                return (0, -(i as isize + 1));
            }
            return (x | (u64::from(b) << s), i as isize + 1);
        }
        x |= u64::from(b & 0x7f) << s;
        s += 7;
    }
    (0, 0)
}

pub fn varint(buf: &[u8]) -> (i64, isize) {
    let (ux, n) = uvarint(buf);
    let mut x = (ux >> 1) as i64;
    if ux & 1 != 0 {
        x = !x;
    }
    (x, n)
}
