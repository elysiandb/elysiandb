use super::cipher::Cipher;

fn get_next_word(b: &[u8], pos: &mut usize) -> u32 {
    let mut w: u32 = 0;
    let mut j = *pos;
    for _ in 0..4 {
        w = (w << 8) | u32::from(b[j]);
        j += 1;
        if j >= b.len() {
            j = 0;
        }
    }
    *pos = j;
    w
}

pub fn expand_key(key: &[u8], c: &mut Cipher) {
    let mut j = 0;
    for i in 0..18 {
        let mut d: u32 = 0;
        for _ in 0..4 {
            d = (d << 8) | u32::from(key[j]);
            j += 1;
            if j >= key.len() {
                j = 0;
            }
        }
        c.p[i] ^= d;
    }

    let (mut l, mut r) = (0u32, 0u32);
    for i in (0..18).step_by(2) {
        (l, r) = encrypt_block(l, r, c);
        c.p[i] = l;
        c.p[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        (l, r) = encrypt_block(l, r, c);
        c.s0[i] = l;
        c.s0[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        (l, r) = encrypt_block(l, r, c);
        c.s1[i] = l;
        c.s1[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        (l, r) = encrypt_block(l, r, c);
        c.s2[i] = l;
        c.s2[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        (l, r) = encrypt_block(l, r, c);
        c.s3[i] = l;
        c.s3[i + 1] = r;
    }
}

pub(super) fn expand_key_with_salt(key: &[u8], salt: &[u8], c: &mut Cipher) {
    let mut j = 0;
    for i in 0..18 {
        c.p[i] ^= get_next_word(key, &mut j);
    }

    j = 0;
    let (mut l, mut r) = (0u32, 0u32);
    for i in (0..18).step_by(2) {
        l ^= get_next_word(salt, &mut j);
        r ^= get_next_word(salt, &mut j);
        (l, r) = encrypt_block(l, r, c);
        c.p[i] = l;
        c.p[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        l ^= get_next_word(salt, &mut j);
        r ^= get_next_word(salt, &mut j);
        (l, r) = encrypt_block(l, r, c);
        c.s0[i] = l;
        c.s0[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        l ^= get_next_word(salt, &mut j);
        r ^= get_next_word(salt, &mut j);
        (l, r) = encrypt_block(l, r, c);
        c.s1[i] = l;
        c.s1[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        l ^= get_next_word(salt, &mut j);
        r ^= get_next_word(salt, &mut j);
        (l, r) = encrypt_block(l, r, c);
        c.s2[i] = l;
        c.s2[i + 1] = r;
    }
    for i in (0..256).step_by(2) {
        l ^= get_next_word(salt, &mut j);
        r ^= get_next_word(salt, &mut j);
        (l, r) = encrypt_block(l, r, c);
        c.s3[i] = l;
        c.s3[i + 1] = r;
    }
}

#[inline(always)]
fn f(c: &Cipher, x: u32) -> u32 {
    (c.s0[(x >> 24) as u8 as usize].wrapping_add(c.s1[(x >> 16) as u8 as usize])
        ^ c.s2[(x >> 8) as u8 as usize])
        .wrapping_add(c.s3[x as u8 as usize])
}

#[inline(always)]
pub(super) fn encrypt_block(l: u32, r: u32, c: &Cipher) -> (u32, u32) {
    let (mut xl, mut xr) = (l, r);
    xl ^= c.p[0];
    xr ^= f(c, xl) ^ c.p[1];
    xl ^= f(c, xr) ^ c.p[2];
    xr ^= f(c, xl) ^ c.p[3];
    xl ^= f(c, xr) ^ c.p[4];
    xr ^= f(c, xl) ^ c.p[5];
    xl ^= f(c, xr) ^ c.p[6];
    xr ^= f(c, xl) ^ c.p[7];
    xl ^= f(c, xr) ^ c.p[8];
    xr ^= f(c, xl) ^ c.p[9];
    xl ^= f(c, xr) ^ c.p[10];
    xr ^= f(c, xl) ^ c.p[11];
    xl ^= f(c, xr) ^ c.p[12];
    xr ^= f(c, xl) ^ c.p[13];
    xl ^= f(c, xr) ^ c.p[14];
    xr ^= f(c, xl) ^ c.p[15];
    xl ^= f(c, xr) ^ c.p[16];
    xr ^= c.p[17];
    (xr, xl)
}
