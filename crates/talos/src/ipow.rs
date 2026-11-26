const OVERFLOWS: [i64; 64] = [
    9223372036854775807,
    9223372036854775807,
    3037000499,
    2097151,
    55108,
    6208,
    1448,
    511,
    234,
    127,
    78,
    52,
    38,
    28,
    22,
    18,
    15,
    13,
    11,
    9,
    8,
    7,
    7,
    6,
    6,
    5,
    5,
    5,
    4,
    4,
    4,
    4,
    3,
    3,
    3,
    3,
    3,
    3,
    3,
    3,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
    2,
];

const HIGHEST_BIT_SET: [u8; 63] = [
    0, 1, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
];

pub fn ipow(mut base: i64, mut exp: i64) -> i64 {
    if exp >= 63 {
        if base == 1 {
            return 1;
        }
        if base == -1 {
            return 1 - 2 * (exp & 1);
        }
        return 0;
    }
    if base > OVERFLOWS[exp as usize] || base.wrapping_neg() > OVERFLOWS[exp as usize] {
        return 0;
    }
    let mut result: i64 = 1;
    let mut level = HIGHEST_BIT_SET[exp as u8 as usize];
    while level > 0 {
        if exp & 1 != 0 {
            result = result.wrapping_mul(base);
        }
        if level > 1 {
            exp >>= 1;
            base = base.wrapping_mul(base);
        }
        level -= 1;
    }
    result
}
