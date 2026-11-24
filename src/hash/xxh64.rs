pub const PRIME1: u64 = 0x9e37_79b1_85eb_ca87;
pub const PRIME2: u64 = 0xc2b2_ae3d_27d4_eb4f;
pub const PRIME3: u64 = 0x1656_67b1_9e37_79f9;
pub const PRIME4: u64 = 0x85eb_ca77_c2b2_ae63;
pub const PRIME5: u64 = 0x27d4_eb2f_1656_67c5;

fn read_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn read_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(PRIME2))
        .rotate_left(31)
        .wrapping_mul(PRIME1)
}

fn merge_round(acc: u64, val: u64) -> u64 {
    (acc ^ round(0, val))
        .wrapping_mul(PRIME1)
        .wrapping_add(PRIME4)
}

pub fn sum64(b: &[u8]) -> u64 {
    let n = b.len();
    let mut p = b;
    let mut h: u64;
    if n >= 32 {
        let mut v1 = PRIME1.wrapping_add(PRIME2);
        let mut v2 = PRIME2;
        let mut v3 = 0u64;
        let mut v4 = 0u64.wrapping_sub(PRIME1);
        while p.len() >= 32 {
            v1 = round(v1, read_u64(&p[0..]));
            v2 = round(v2, read_u64(&p[8..]));
            v3 = round(v3, read_u64(&p[16..]));
            v4 = round(v4, read_u64(&p[24..]));
            p = &p[32..];
        }
        h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        h = merge_round(h, v1);
        h = merge_round(h, v2);
        h = merge_round(h, v3);
        h = merge_round(h, v4);
    } else {
        h = PRIME5;
    }

    h = h.wrapping_add(n as u64);

    while p.len() >= 8 {
        let k1 = round(0, read_u64(p));
        h ^= k1;
        h = h.rotate_left(27).wrapping_mul(PRIME1).wrapping_add(PRIME4);
        p = &p[8..];
    }
    if p.len() >= 4 {
        h ^= u64::from(read_u32(p)).wrapping_mul(PRIME1);
        h = h.rotate_left(23).wrapping_mul(PRIME2).wrapping_add(PRIME3);
        p = &p[4..];
    }
    for &c in p {
        h ^= u64::from(c).wrapping_mul(PRIME5);
        h = h.rotate_left(11).wrapping_mul(PRIME1);
    }

    h ^= h >> 33;
    h = h.wrapping_mul(PRIME2);
    h ^= h >> 29;
    h = h.wrapping_mul(PRIME3);
    h ^= h >> 32;
    h
}
