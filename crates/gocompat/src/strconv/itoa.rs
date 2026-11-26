pub fn format_uint(i: u64, base: u32) -> String {
    let mut dst = Vec::with_capacity(20);
    format_bits(&mut dst, i, base, false);
    ascii_string(dst)
}

pub fn format_int(i: i64, base: u32) -> String {
    let mut dst = Vec::with_capacity(20);
    format_bits(&mut dst, i as u64, base, i < 0);
    ascii_string(dst)
}

pub fn itoa(i: i64) -> String {
    format_int(i, 10)
}

pub fn append_int(dst: &mut Vec<u8>, i: i64, base: u32) {
    format_bits(dst, i as u64, base, i < 0);
}

pub fn append_uint(dst: &mut Vec<u8>, i: u64, base: u32) {
    format_bits(dst, i, base, false);
}

fn ascii_string(b: Vec<u8>) -> String {
    b.into_iter().map(char::from).collect()
}

pub(super) static SMALLS_STRING: &[u8; 200] = b"00010203040506070809\
10111213141516171819\
20212223242526272829\
30313233343536373839\
40414243444546474849\
50515253545556575859\
60616263646566676869\
70717273747576777879\
80818283848586878889\
90919293949596979899";

static DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub(super) fn format_bits(dst: &mut Vec<u8>, u: u64, base: u32, neg: bool) {
    if !(2..=36).contains(&base) {
        panic!("illegal integer base {base}");
    }

    let mut a = [0u8; 65];
    let mut i = a.len();

    let mut u = u;
    if neg {
        u = u.wrapping_neg();
    }

    if base == 10 {
        let mut us = u;
        while us >= 100 {
            let is = (us % 100 * 2) as usize;
            us /= 100;
            i -= 2;
            a[i + 1] = SMALLS_STRING[is + 1];
            a[i] = SMALLS_STRING[is];
        }

        let is = (us * 2) as usize;
        i -= 1;
        a[i] = SMALLS_STRING[is + 1];
        if us >= 10 {
            i -= 1;
            a[i] = SMALLS_STRING[is];
        }
    } else if base.is_power_of_two() {
        let shift = base.trailing_zeros();
        let b = u64::from(base);
        let m = u64::from(base) - 1;
        while u >= b {
            i -= 1;
            a[i] = DIGITS[(u & m) as usize];
            u >>= shift;
        }
        i -= 1;
        a[i] = DIGITS[u as usize];
    } else {
        let b = u64::from(base);
        while u >= b {
            i -= 1;
            let q = u / b;
            a[i] = DIGITS[(u - q * b) as usize];
            u = q;
        }
        i -= 1;
        a[i] = DIGITS[u as usize];
    }

    if neg {
        i -= 1;
        a[i] = b'-';
    }

    dst.extend_from_slice(&a[i..]);
}
