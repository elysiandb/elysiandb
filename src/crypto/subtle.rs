use std::hint::black_box;

pub fn constant_time_compare(x: &[u8], y: &[u8]) -> i32 {
    if x.len() != y.len() {
        return 0;
    }

    let mut v: u8 = 0;
    for i in 0..x.len() {
        v = black_box(v | (x[i] ^ y[i]));
    }

    constant_time_byte_eq(v, 0)
}

pub fn constant_time_byte_eq(x: u8, y: u8) -> i32 {
    ((u32::from(x ^ y).wrapping_sub(1)) >> 31) as i32
}
