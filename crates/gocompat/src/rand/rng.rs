use super::cooked::RNG_COOKED;

const RNG_LEN: usize = 607;
const RNG_TAP: usize = 273;
const RNG_MASK: u64 = (1 << 63) - 1;
const INT32_MAX: i64 = (1 << 31) - 1;

#[derive(Clone)]
pub struct RngSource {
    tap: usize,
    feed: usize,
    vec: [i64; RNG_LEN],
}

fn seedrand(x: i32) -> i32 {
    const A: i32 = 48271;
    const Q: i32 = 44488;
    const R: i32 = 3399;
    let hi = x / Q;
    let lo = x % Q;
    let mut x = A.wrapping_mul(lo).wrapping_sub(R.wrapping_mul(hi));
    if x < 0 {
        x = x.wrapping_add(INT32_MAX as i32);
    }
    x
}

impl RngSource {
    pub fn new(seed: i64) -> RngSource {
        let mut rng = RngSource {
            tap: 0,
            feed: RNG_LEN - RNG_TAP,
            vec: [0; RNG_LEN],
        };
        rng.seed(seed);
        rng
    }

    pub fn seed(&mut self, seed: i64) {
        self.tap = 0;
        self.feed = RNG_LEN - RNG_TAP;
        let mut seed = seed % INT32_MAX;
        if seed < 0 {
            seed += INT32_MAX;
        }
        if seed == 0 {
            seed = 89482311;
        }
        let mut x = seed as i32;
        for i in -20..RNG_LEN as i64 {
            x = seedrand(x);
            if i >= 0 {
                let mut u = i64::from(x) << 40;
                x = seedrand(x);
                u ^= i64::from(x) << 20;
                x = seedrand(x);
                u ^= i64::from(x);
                u ^= RNG_COOKED[i as usize];
                self.vec[i as usize] = u;
            }
        }
    }

    pub fn int63(&mut self) -> i64 {
        (self.uint64() & RNG_MASK) as i64
    }

    pub fn uint64(&mut self) -> u64 {
        self.tap = if self.tap == 0 {
            RNG_LEN - 1
        } else {
            self.tap - 1
        };
        self.feed = if self.feed == 0 {
            RNG_LEN - 1
        } else {
            self.feed - 1
        };
        let x = self.vec[self.feed].wrapping_add(self.vec[self.tap]);
        self.vec[self.feed] = x;
        x as u64
    }
}
