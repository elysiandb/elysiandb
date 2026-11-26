use std::ops::{Add, Sub};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const BUCKET_BOUNDS_MS: [f64; 17] = [
    0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0,
    5000.0, 10000.0,
];

pub const NUM_BUCKETS: usize = BUCKET_BOUNDS_MS.len() + 1;

pub struct Histogram {
    pub(super) counts: [AtomicU64; NUM_BUCKETS],
    pub(super) sum: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hist(pub [u64; NUM_BUCKETS]);

pub(super) fn nanos(d: Duration) -> i64 {
    i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)
}

fn bucket_for(ms: f64) -> usize {
    for (i, &bound) in BUCKET_BOUNDS_MS.iter().enumerate() {
        if ms <= bound {
            return i;
        }
    }

    NUM_BUCKETS - 1
}

impl Histogram {
    pub const fn new() -> Histogram {
        Histogram {
            counts: [const { AtomicU64::new(0) }; NUM_BUCKETS],
            sum: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, d: Duration) {
        let n = nanos(d);
        self.counts[bucket_for(n as f64 / 1_000_000.0)].fetch_add(1, Ordering::SeqCst);
        if n > 0 {
            self.sum.fetch_add(n as u64, Ordering::SeqCst);
        }
    }

    pub fn sum_seconds(&self) -> f64 {
        self.sum.load(Ordering::SeqCst) as f64 / 1_000_000_000.0
    }

    pub fn snapshot(&self) -> Hist {
        let mut out = Hist::default();
        for (i, c) in self.counts.iter().enumerate() {
            out.0[i] = c.load(Ordering::SeqCst);
        }

        out
    }
}

impl Default for Histogram {
    fn default() -> Histogram {
        Histogram::new()
    }
}

impl Sub for Hist {
    type Output = Hist;

    fn sub(self, o: Hist) -> Hist {
        let mut out = Hist::default();
        for i in 0..NUM_BUCKETS {
            if self.0[i] >= o.0[i] {
                out.0[i] = self.0[i] - o.0[i];
            }
        }

        out
    }
}

impl Add for Hist {
    type Output = Hist;

    fn add(self, o: Hist) -> Hist {
        let mut out = Hist::default();
        for i in 0..NUM_BUCKETS {
            out.0[i] = self.0[i].wrapping_add(o.0[i]);
        }

        out
    }
}

impl Hist {
    pub fn total(&self) -> u64 {
        let mut t = 0u64;
        for &c in &self.0 {
            t = t.wrapping_add(c);
        }

        t
    }

    pub fn quantile(&self, q: f64) -> f64 {
        let total = self.total();
        if total == 0 {
            return f64::NAN;
        }

        let rank = q * total as f64;
        let mut seen = 0.0f64;
        for (i, &c) in self.0.iter().enumerate() {
            if c == 0 {
                continue;
            }

            if seen + c as f64 >= rank {
                let lower = if i > 0 { BUCKET_BOUNDS_MS[i - 1] } else { 0.0 };

                if i == NUM_BUCKETS - 1 {
                    return lower;
                }

                let upper = BUCKET_BOUNDS_MS[i];
                return lower + (upper - lower) * (rank - seen) / c as f64;
            }

            seen += c as f64;
        }

        BUCKET_BOUNDS_MS[BUCKET_BOUNDS_MS.len() - 1]
    }
}
