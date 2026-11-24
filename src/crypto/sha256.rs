pub const SIZE: usize = 32;
pub const BLOCK_SIZE: usize = 64;

const INIT: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[derive(Clone)]
pub struct Digest {
    h: [u32; 8],
    x: [u8; BLOCK_SIZE],
    nx: usize,
    len: u64,
}

impl Default for Digest {
    fn default() -> Digest {
        Digest::new()
    }
}

impl Digest {
    pub fn new() -> Digest {
        Digest {
            h: INIT,
            x: [0; BLOCK_SIZE],
            nx: 0,
            len: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Digest::new();
    }

    pub fn size(&self) -> usize {
        SIZE
    }

    pub fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    pub fn write(&mut self, mut p: &[u8]) {
        self.len = self.len.wrapping_add(p.len() as u64);
        if self.nx > 0 {
            let n = (BLOCK_SIZE - self.nx).min(p.len());
            self.x[self.nx..self.nx + n].copy_from_slice(&p[..n]);
            self.nx += n;
            if self.nx == BLOCK_SIZE {
                let x = self.x;
                block(&mut self.h, &x);
                self.nx = 0;
            }
            p = &p[n..];
        }
        if p.len() >= BLOCK_SIZE {
            let n = p.len() & !(BLOCK_SIZE - 1);
            block(&mut self.h, &p[..n]);
            p = &p[n..];
        }
        if !p.is_empty() {
            self.x[..p.len()].copy_from_slice(p);
            self.nx = p.len();
        }
    }

    pub fn sum(&self) -> [u8; SIZE] {
        let mut d = self.clone();
        d.check_sum()
    }

    fn check_sum(&mut self) -> [u8; SIZE] {
        let len = self.len;
        let mut tmp = [0u8; BLOCK_SIZE + 8];
        tmp[0] = 0x80;
        let t = if len % 64 < 56 {
            56 - len % 64
        } else {
            64 + 56 - len % 64
        } as usize;
        tmp[t..t + 8].copy_from_slice(&(len << 3).to_be_bytes());
        self.write(&tmp[..t + 8]);

        let mut digest = [0u8; SIZE];
        for (i, h) in self.h.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
        }
        digest
    }
}

pub fn sum256(data: &[u8]) -> [u8; SIZE] {
    let mut d = Digest::new();
    d.write(data);
    d.check_sum()
}

fn block(dig: &mut [u32; 8], mut p: &[u8]) {
    let mut w = [0u32; 64];
    while p.len() >= BLOCK_SIZE {
        for (wi, chunk) in w.iter_mut().zip(p[..BLOCK_SIZE].chunks_exact(4)) {
            *wi = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let v1 = w[i - 2];
            let t1 = v1.rotate_right(17) ^ v1.rotate_right(19) ^ (v1 >> 10);
            let v2 = w[i - 15];
            let t2 = v2.rotate_right(7) ^ v2.rotate_right(18) ^ (v2 >> 3);
            w[i] = t1
                .wrapping_add(w[i - 7])
                .wrapping_add(t2)
                .wrapping_add(w[i - 16]);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *dig;

        for i in 0..64 {
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (x, y) in dig.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }

        p = &p[BLOCK_SIZE..];
    }
}
