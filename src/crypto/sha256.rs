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
    software: bool,
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
            software: false,
        }
    }

    pub fn new_software() -> Digest {
        Digest {
            software: true,
            ..Digest::new()
        }
    }

    pub fn reset(&mut self) {
        let software = self.software;
        *self = Digest::new();
        self.software = software;
    }

    fn compress(&mut self, p: &[u8]) {
        if self.software {
            block_generic(&mut self.h, p);
        } else {
            block(&mut self.h, p);
        }
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
                self.compress(&x);
                self.nx = 0;
            }
            p = &p[n..];
        }
        if p.len() >= BLOCK_SIZE {
            let n = p.len() & !(BLOCK_SIZE - 1);
            self.compress(&p[..n]);
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

pub fn sum256_software(data: &[u8]) -> [u8; SIZE] {
    let mut d = Digest::new_software();
    d.write(data);
    d.check_sum()
}

pub fn hardware() -> bool {
    hardware_detected()
}

#[cfg(target_arch = "aarch64")]
fn hardware_detected() -> bool {
    std::arch::is_aarch64_feature_detected!("sha2")
}

#[cfg(target_arch = "x86_64")]
fn hardware_detected() -> bool {
    std::arch::is_x86_feature_detected!("sha")
        && std::arch::is_x86_feature_detected!("sse4.1")
        && std::arch::is_x86_feature_detected!("ssse3")
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
fn hardware_detected() -> bool {
    false
}

fn block(dig: &mut [u32; 8], p: &[u8]) {
    if hardware_detected() {
        block_hardware(dig, p);
        return;
    }

    block_generic(dig, p);
}

#[cfg(target_arch = "aarch64")]
fn block_hardware(dig: &mut [u32; 8], p: &[u8]) {
    unsafe { block_arm64(dig, p) }
}

#[cfg(target_arch = "x86_64")]
fn block_hardware(dig: &mut [u32; 8], p: &[u8]) {
    unsafe { block_shani(dig, p) }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
fn block_hardware(dig: &mut [u32; 8], p: &[u8]) {
    block_generic(dig, p);
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "sha2")]
fn block_arm64(dig: &mut [u32; 8], p: &[u8]) {
    use core::arch::aarch64::{
        uint32x4_t, vaddq_u32, vld1q_u8, vld1q_u32, vreinterpretq_u32_u8, vrev32q_u8,
        vsha256h2q_u32, vsha256hq_u32, vsha256su0q_u32, vsha256su1q_u32, vst1q_u32,
    };

    let k = |i: usize| -> uint32x4_t { unsafe { vld1q_u32(K[i..i + 4].as_ptr()) } };
    let mut abcd = unsafe { vld1q_u32(dig[0..4].as_ptr()) };
    let mut efgh = unsafe { vld1q_u32(dig[4..8].as_ptr()) };

    for chunk in p.chunks_exact(BLOCK_SIZE) {
        let abcd_save = abcd;
        let efgh_save = efgh;

        let load = |i: usize| -> uint32x4_t {
            let bytes = unsafe { vld1q_u8(chunk[i..i + 16].as_ptr()) };
            vreinterpretq_u32_u8(vrev32q_u8(bytes))
        };
        let mut s0 = load(0);
        let mut s1 = load(16);
        let mut s2 = load(32);
        let mut s3 = load(48);

        let rounds = |abcd: &mut uint32x4_t, efgh: &mut uint32x4_t, w: uint32x4_t, i: usize| {
            let t = vaddq_u32(w, k(i));
            let prev = *abcd;
            *abcd = vsha256hq_u32(prev, *efgh, t);
            *efgh = vsha256h2q_u32(*efgh, prev, t);
        };

        rounds(&mut abcd, &mut efgh, s0, 0);
        rounds(&mut abcd, &mut efgh, s1, 4);
        rounds(&mut abcd, &mut efgh, s2, 8);
        rounds(&mut abcd, &mut efgh, s3, 12);

        let mut t = 16;
        while t < 64 {
            s0 = vsha256su1q_u32(vsha256su0q_u32(s0, s1), s2, s3);
            rounds(&mut abcd, &mut efgh, s0, t);
            s1 = vsha256su1q_u32(vsha256su0q_u32(s1, s2), s3, s0);
            rounds(&mut abcd, &mut efgh, s1, t + 4);
            s2 = vsha256su1q_u32(vsha256su0q_u32(s2, s3), s0, s1);
            rounds(&mut abcd, &mut efgh, s2, t + 8);
            s3 = vsha256su1q_u32(vsha256su0q_u32(s3, s0), s1, s2);
            rounds(&mut abcd, &mut efgh, s3, t + 12);
            t += 16;
        }

        abcd = vaddq_u32(abcd, abcd_save);
        efgh = vaddq_u32(efgh, efgh_save);
    }

    unsafe {
        vst1q_u32(dig[0..4].as_mut_ptr(), abcd);
        vst1q_u32(dig[4..8].as_mut_ptr(), efgh);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
fn block_shani(dig: &mut [u32; 8], p: &[u8]) {
    use core::arch::x86_64::{
        __m128i, _mm_add_epi32, _mm_alignr_epi8, _mm_blend_epi16, _mm_loadu_si128, _mm_set_epi32,
        _mm_set_epi64x, _mm_sha256msg1_epu32, _mm_sha256msg2_epu32, _mm_sha256rnds2_epu32,
        _mm_shuffle_epi8, _mm_shuffle_epi32, _mm_storeu_si128,
    };

    let mask = _mm_set_epi64x(
        0x0C0D_0E0F_0809_0A0Bu64 as i64,
        0x0405_0607_0001_0203u64 as i64,
    );
    let k = |i: usize| -> __m128i {
        _mm_set_epi32(
            K[4 * i + 3] as i32,
            K[4 * i + 2] as i32,
            K[4 * i + 1] as i32,
            K[4 * i] as i32,
        )
    };
    let schedule = |v0: __m128i, v1: __m128i, v2: __m128i, v3: __m128i| -> __m128i {
        let t1 = _mm_sha256msg1_epu32(v0, v1);
        let t2 = _mm_alignr_epi8(v3, v2, 4);
        let t3 = _mm_add_epi32(t1, t2);
        _mm_sha256msg2_epu32(t3, v3)
    };

    let state = dig.as_ptr() as *const __m128i;
    let dcba = unsafe { _mm_loadu_si128(state) };
    let efgh = unsafe { _mm_loadu_si128(state.add(1)) };

    let cdab = _mm_shuffle_epi32(dcba, 0xB1);
    let efgh = _mm_shuffle_epi32(efgh, 0x1B);
    let mut abef = _mm_alignr_epi8(cdab, efgh, 8);
    let mut cdgh = _mm_blend_epi16(efgh, cdab, 0xF0);

    for chunk in p.chunks_exact(BLOCK_SIZE) {
        let abef_save = abef;
        let cdgh_save = cdgh;

        let data = chunk.as_ptr() as *const __m128i;
        let mut w0 = _mm_shuffle_epi8(unsafe { _mm_loadu_si128(data) }, mask);
        let mut w1 = _mm_shuffle_epi8(unsafe { _mm_loadu_si128(data.add(1)) }, mask);
        let mut w2 = _mm_shuffle_epi8(unsafe { _mm_loadu_si128(data.add(2)) }, mask);
        let mut w3 = _mm_shuffle_epi8(unsafe { _mm_loadu_si128(data.add(3)) }, mask);
        let mut w4;

        let rounds = |abef: &mut __m128i, cdgh: &mut __m128i, w: __m128i, i: usize| {
            let t1 = _mm_add_epi32(w, k(i));
            *cdgh = _mm_sha256rnds2_epu32(*cdgh, *abef, t1);
            let t2 = _mm_shuffle_epi32(t1, 0x0E);
            *abef = _mm_sha256rnds2_epu32(*abef, *cdgh, t2);
        };

        rounds(&mut abef, &mut cdgh, w0, 0);
        rounds(&mut abef, &mut cdgh, w1, 1);
        rounds(&mut abef, &mut cdgh, w2, 2);
        rounds(&mut abef, &mut cdgh, w3, 3);
        w4 = schedule(w0, w1, w2, w3);
        rounds(&mut abef, &mut cdgh, w4, 4);
        w0 = schedule(w1, w2, w3, w4);
        rounds(&mut abef, &mut cdgh, w0, 5);
        w1 = schedule(w2, w3, w4, w0);
        rounds(&mut abef, &mut cdgh, w1, 6);
        w2 = schedule(w3, w4, w0, w1);
        rounds(&mut abef, &mut cdgh, w2, 7);
        w3 = schedule(w4, w0, w1, w2);
        rounds(&mut abef, &mut cdgh, w3, 8);
        w4 = schedule(w0, w1, w2, w3);
        rounds(&mut abef, &mut cdgh, w4, 9);
        w0 = schedule(w1, w2, w3, w4);
        rounds(&mut abef, &mut cdgh, w0, 10);
        w1 = schedule(w2, w3, w4, w0);
        rounds(&mut abef, &mut cdgh, w1, 11);
        w2 = schedule(w3, w4, w0, w1);
        rounds(&mut abef, &mut cdgh, w2, 12);
        w3 = schedule(w4, w0, w1, w2);
        rounds(&mut abef, &mut cdgh, w3, 13);
        w4 = schedule(w0, w1, w2, w3);
        rounds(&mut abef, &mut cdgh, w4, 14);
        w0 = schedule(w1, w2, w3, w4);
        rounds(&mut abef, &mut cdgh, w0, 15);

        abef = _mm_add_epi32(abef, abef_save);
        cdgh = _mm_add_epi32(cdgh, cdgh_save);
    }

    let feba = _mm_shuffle_epi32(abef, 0x1B);
    let dchg = _mm_shuffle_epi32(cdgh, 0xB1);
    let dcba = _mm_blend_epi16(feba, dchg, 0xF0);
    let hgef = _mm_alignr_epi8(dchg, feba, 8);

    let out = dig.as_mut_ptr() as *mut __m128i;
    unsafe {
        _mm_storeu_si128(out, dcba);
        _mm_storeu_si128(out.add(1), hgef);
    }
}

fn block_generic(dig: &mut [u32; 8], mut p: &[u8]) {
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
