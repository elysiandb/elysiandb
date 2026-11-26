use std::cmp::Ordering;

pub type Word = u64;

pub const W: u32 = 64;

pub fn norm(mut z: Vec<Word>) -> Vec<Word> {
    while z.last() == Some(&0) {
        z.pop();
    }
    z
}

pub fn from_u64(x: u64) -> Vec<Word> {
    if x == 0 { Vec::new() } else { vec![x] }
}

pub fn cmp(x: &[Word], y: &[Word]) -> Ordering {
    if x.len() != y.len() {
        return x.len().cmp(&y.len());
    }
    for i in (0..x.len()).rev() {
        match x[i].cmp(&y[i]) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    Ordering::Equal
}

pub fn add(x: &[Word], y: &[Word]) -> Vec<Word> {
    let (x, y) = if x.len() < y.len() { (y, x) } else { (x, y) };
    let mut z = Vec::with_capacity(x.len() + 1);
    let mut carry = 0u64;
    for i in 0..x.len() {
        let yi = if i < y.len() { y[i] } else { 0 };
        let (s1, c1) = x[i].overflowing_add(yi);
        let (s2, c2) = s1.overflowing_add(carry);
        z.push(s2);
        carry = u64::from(c1) + u64::from(c2);
    }
    if carry != 0 {
        z.push(carry);
    }
    norm(z)
}

pub fn sub(x: &[Word], y: &[Word]) -> Vec<Word> {
    let mut z = Vec::with_capacity(x.len());
    let mut borrow = 0u64;
    for i in 0..x.len() {
        let yi = if i < y.len() { y[i] } else { 0 };
        let (d1, b1) = x[i].overflowing_sub(yi);
        let (d2, b2) = d1.overflowing_sub(borrow);
        z.push(d2);
        borrow = u64::from(b1) + u64::from(b2);
    }
    if borrow != 0 || y.len() > x.len() {
        panic!("underflow");
    }
    norm(z)
}

pub fn mul(x: &[Word], y: &[Word]) -> Vec<Word> {
    if x.is_empty() || y.is_empty() {
        return Vec::new();
    }
    let mut z = vec![0u64; x.len() + y.len()];
    for (i, &xi) in x.iter().enumerate() {
        if xi == 0 {
            continue;
        }
        let mut carry: u128 = 0;
        for (j, &yj) in y.iter().enumerate() {
            let t = u128::from(xi) * u128::from(yj) + u128::from(z[i + j]) + carry;
            z[i + j] = t as u64;
            carry = t >> 64;
        }
        let mut k = i + y.len();
        while carry != 0 {
            let t = u128::from(z[k]) + carry;
            z[k] = t as u64;
            carry = t >> 64;
            k += 1;
        }
    }
    norm(z)
}

pub fn mul_add_ww(x: &[Word], y: Word, r: Word) -> Vec<Word> {
    let mut z = Vec::with_capacity(x.len() + 1);
    let mut carry = u128::from(r);
    for &xi in x {
        let t = u128::from(xi) * u128::from(y) + carry;
        z.push(t as u64);
        carry = t >> 64;
    }
    if carry != 0 {
        z.push(carry as u64);
    }
    norm(z)
}

pub fn div_w(x: &[Word], y: Word) -> (Vec<Word>, Word) {
    let mut q = vec![0u64; x.len()];
    let mut r: u128 = 0;
    for i in (0..x.len()).rev() {
        let cur = (r << 64) | u128::from(x[i]);
        q[i] = (cur / u128::from(y)) as u64;
        r = cur % u128::from(y);
    }
    (norm(q), r as u64)
}

pub fn shl(x: &[Word], s: u64) -> Vec<Word> {
    if x.is_empty() {
        return Vec::new();
    }
    let words = (s / u64::from(W)) as usize;
    let bits = (s % u64::from(W)) as u32;
    let mut z = vec![0u64; words];
    z.reserve(x.len() + 1);
    if bits == 0 {
        z.extend_from_slice(x);
    } else {
        let mut carry = 0u64;
        for &xi in x {
            z.push((xi << bits) | carry);
            carry = xi >> (W - bits);
        }
        z.push(carry);
    }
    norm(z)
}

pub fn shr(x: &[Word], s: u64) -> Vec<Word> {
    let words = s / u64::from(W);
    if words >= x.len() as u64 {
        return Vec::new();
    }
    let words = words as usize;
    let bits = (s % u64::from(W)) as u32;
    let src = &x[words..];
    let mut z = Vec::with_capacity(src.len());
    if bits == 0 {
        z.extend_from_slice(src);
    } else {
        for i in 0..src.len() {
            let hi = if i + 1 < src.len() {
                src[i + 1] << (W - bits)
            } else {
                0
            };
            z.push((src[i] >> bits) | hi);
        }
    }
    norm(z)
}

pub fn bit_len(x: &[Word]) -> u64 {
    match x.last() {
        None => 0,
        Some(&top) => (x.len() as u64 - 1) * u64::from(W) + u64::from(W - top.leading_zeros()),
    }
}

pub fn trailing_zero_bits(x: &[Word]) -> u64 {
    for (i, &w) in x.iter().enumerate() {
        if w != 0 {
            return i as u64 * u64::from(W) + u64::from(w.trailing_zeros());
        }
    }
    0
}

pub fn low64(x: &[Word]) -> u64 {
    x.first().copied().unwrap_or(0)
}

pub fn div(u: &[Word], v: &[Word]) -> (Vec<Word>, Vec<Word>) {
    if v.is_empty() {
        panic!("division by zero");
    }
    if cmp(u, v) == Ordering::Less {
        return (Vec::new(), u.to_vec());
    }
    if v.len() == 1 {
        let (q, r) = div_w(u, v[0]);
        return (q, from_u64(r));
    }
    let s = v[v.len() - 1].leading_zeros();
    let vn = shl_bits(v, s, v.len());
    let mut un = shl_bits(u, s, u.len() + 1);
    let n = vn.len();
    let m = u.len() - n;
    let mut q = vec![0u64; m + 1];
    let b: u128 = 1 << 64;
    for j in (0..=m).rev() {
        let num = (u128::from(un[j + n]) << 64) | u128::from(un[j + n - 1]);
        let mut qhat = num / u128::from(vn[n - 1]);
        let mut rhat = num % u128::from(vn[n - 1]);
        while qhat >= b || qhat * u128::from(vn[n - 2]) > ((rhat << 64) | u128::from(un[j + n - 2]))
        {
            qhat -= 1;
            rhat += u128::from(vn[n - 1]);
            if rhat >= b {
                break;
            }
        }
        let mut borrow: i128 = 0;
        let mut carry: u128 = 0;
        for i in 0..n {
            let p = qhat * u128::from(vn[i]) + carry;
            carry = p >> 64;
            let t = i128::from(un[i + j]) - borrow - i128::from(p as u64);
            un[i + j] = t as u64;
            borrow = if t < 0 { 1 } else { 0 };
        }
        let t = i128::from(un[j + n]) - borrow - carry as i128;
        un[j + n] = t as u64;
        if t < 0 {
            qhat -= 1;
            let mut c: u128 = 0;
            for i in 0..n {
                let s2 = u128::from(un[i + j]) + u128::from(vn[i]) + c;
                un[i + j] = s2 as u64;
                c = s2 >> 64;
            }
            un[j + n] = un[j + n].wrapping_add(c as u64);
        }
        q[j] = qhat as u64;
    }
    let r = shr_bits(&un[..n], s);
    (norm(q), norm(r))
}

fn shl_bits(x: &[Word], s: u32, len: usize) -> Vec<Word> {
    let mut z = vec![0u64; len];
    if s == 0 {
        z[..x.len()].copy_from_slice(x);
        return z;
    }
    let mut carry = 0u64;
    for i in 0..x.len() {
        z[i] = (x[i] << s) | carry;
        carry = x[i] >> (W - s);
    }
    if x.len() < len {
        z[x.len()] = carry;
    }
    z
}

fn shr_bits(x: &[Word], s: u32) -> Vec<Word> {
    if s == 0 {
        return x.to_vec();
    }
    let mut z = vec![0u64; x.len()];
    for i in 0..x.len() {
        let hi = if i + 1 < x.len() {
            x[i + 1] << (W - s)
        } else {
            0
        };
        z[i] = (x[i] >> s) | hi;
    }
    z
}

pub fn exp(x: &[Word], y: &[Word], m: &[Word]) -> Vec<Word> {
    if m.len() == 1 && m[0] == 1 {
        return Vec::new();
    }
    if y.is_empty() {
        return vec![1];
    }
    if x.is_empty() {
        return Vec::new();
    }
    if x.len() == 1 && x[0] == 1 {
        return vec![1];
    }
    let reduce = |v: Vec<Word>| -> Vec<Word> { if m.is_empty() { v } else { div(&v, m).1 } };
    let base = reduce(x.to_vec());
    let mut result: Vec<Word> = vec![1];
    let bits = bit_len(y);
    for i in (0..bits).rev() {
        result = reduce(mul(&result, &result));
        if (y[(i / u64::from(W)) as usize] >> (i % u64::from(W))) & 1 == 1 {
            result = reduce(mul(&result, &base));
        }
    }
    reduce(result)
}

pub fn twos(x: &[Word], neg: bool, len: usize) -> Vec<Word> {
    let mut z = vec![0u64; len];
    z[..x.len()].copy_from_slice(x);
    if neg {
        let mut carry = 1u64;
        for w in z.iter_mut() {
            let (s, c) = (!*w).overflowing_add(carry);
            *w = s;
            carry = u64::from(c);
        }
    }
    z
}

pub fn from_twos(mut z: Vec<Word>) -> (Vec<Word>, bool) {
    let neg = z.last().is_some_and(|&w| w >> 63 == 1);
    if neg {
        let mut carry = 1u64;
        for w in z.iter_mut() {
            let (s, c) = (!*w).overflowing_add(carry);
            *w = s;
            carry = u64::from(c);
        }
    }
    (norm(z), neg)
}
