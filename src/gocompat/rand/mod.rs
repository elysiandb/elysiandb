mod cooked;
mod rng;

pub use rng::RngSource;

#[derive(Clone)]
pub struct Rand {
    src: RngSource,
}

pub fn new_source(seed: i64) -> RngSource {
    RngSource::new(seed)
}

impl Rand {
    pub fn new(src: RngSource) -> Rand {
        Rand { src }
    }

    pub fn seed(&mut self, seed: i64) {
        self.src.seed(seed);
    }

    pub fn int63(&mut self) -> i64 {
        self.src.int63()
    }

    pub fn uint32(&mut self) -> u32 {
        (self.int63() >> 31) as u32
    }

    pub fn uint64(&mut self) -> u64 {
        self.src.uint64()
    }

    pub fn int31(&mut self) -> i32 {
        (self.int63() >> 32) as i32
    }

    pub fn int(&mut self) -> i64 {
        let u = self.int63() as u64;
        ((u << 1) >> 1) as i64
    }

    pub fn int63n(&mut self, n: i64) -> i64 {
        assert!(n > 0, "invalid argument to Int63n");
        if n & (n - 1) == 0 {
            return self.int63() & (n - 1);
        }
        let max = ((1u64 << 63) - 1 - (1u64 << 63) % n as u64) as i64;
        let mut v = self.int63();
        while v > max {
            v = self.int63();
        }
        v % n
    }

    pub fn int31n(&mut self, n: i32) -> i32 {
        assert!(n > 0, "invalid argument to Int31n");
        if n & (n - 1) == 0 {
            return self.int31() & (n - 1);
        }
        let max = ((1u32 << 31) - 1 - (1u32 << 31) % n as u32) as i32;
        let mut v = self.int31();
        while v > max {
            v = self.int31();
        }
        v % n
    }

    pub fn intn(&mut self, n: i64) -> i64 {
        assert!(n > 0, "invalid argument to Intn");
        if n <= i64::from(i32::MAX) {
            return i64::from(self.int31n(n as i32));
        }
        self.int63n(n)
    }

    pub fn float64(&mut self) -> f64 {
        loop {
            let f = self.int63() as f64 / (1u64 << 63) as f64;
            if f != 1.0 {
                return f;
            }
        }
    }

    pub fn float32(&mut self) -> f32 {
        loop {
            let f = self.float64() as f32;
            if f != 1.0 {
                return f;
            }
        }
    }

    pub fn perm(&mut self, n: usize) -> Vec<i64> {
        let mut m = vec![0i64; n];
        for i in 0..n {
            let j = self.intn(i as i64 + 1) as usize;
            m[i] = m[j];
            m[j] = i as i64;
        }
        m
    }
}
