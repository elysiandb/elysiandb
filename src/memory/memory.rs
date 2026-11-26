use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::time::{Duration as StdDuration, Instant};

use super::counting;
use crate::gocompat::errors::BoxError;
use crate::gocompat::time::Duration;
use crate::sync::{Condvar, Mutex, Stop, Ticker, spawn};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Normal = 0,
    High = 1,
    Critical = 2,
}

impl Level {
    fn from_i32(v: i32) -> Level {
        match v {
            1 => Level::High,
            2 => Level::Critical,
            _ => Level::Normal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::High => "high",
            Level::Critical => "critical",
            Level::Normal => "normal",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("memory budget exhausted")
    }
}

impl StdError for Busy {}

pub const ERR_BUSY: Busy = Busy;

const VALUES_SHARE: f64 = 0.10;
const PAGES_SHARE: f64 = 0.15;
const DIRTY_SHARE: f64 = 0.08;
const HOOKS_SHARE: f64 = 0.12;
const BUILDS_SHARE: f64 = 0.06;
const REQUESTS_SHARE: f64 = 0.12;

const HIGH_SHARE: f64 = 0.65;
const CRITICAL_SHARE: f64 = 0.80;
const TOTAL_HIGH_SHARE: f64 = 0.95;
const TOTAL_CRITICAL_SHARE: f64 = 1.05;
const REQUEST_SHARE_CAP: f64 = 0.5;

const ELASTIC_CEILING: f64 = 0.30;
const ELASTIC_GROW_BELOW: f64 = 0.48;
const ELASTIC_SHRINK_ABOVE: f64 = 0.56;
const ELASTIC_STEP: i64 = 64;
const DEFAULT_HOOK_RESULTS: i64 = 256 << 20;
const MIN_BUILD_BYTES: i64 = 64 << 20;
const SAMPLE_INTERVAL: StdDuration = StdDuration::from_millis(100);

const ACQUIRE_POLL: StdDuration = StdDuration::from_millis(20);

pub struct Pool {
    name: &'static str,
    high_percent: i64,
    limit: AtomicI64,
    used: AtomicI64,
    peak: AtomicI64,
    refused: AtomicI64,

    changed: Mutex<u64>,
    cv: Condvar,
}

impl Pool {
    const fn new(name: &'static str, high_percent: i64) -> Pool {
        Pool {
            name,
            high_percent,
            limit: AtomicI64::new(0),
            used: AtomicI64::new(0),
            peak: AtomicI64::new(0),
            refused: AtomicI64::new(0),
            changed: Mutex::new(0),
            cv: Condvar::new(),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn limit(&self) -> i64 {
        self.limit.load(Ordering::SeqCst)
    }

    pub fn used(&self) -> i64 {
        self.used.load(Ordering::SeqCst)
    }

    pub fn peak(&self) -> i64 {
        self.peak.load(Ordering::SeqCst)
    }

    pub fn refused(&self) -> i64 {
        self.refused.load(Ordering::SeqCst)
    }

    pub fn refuse(&self) {
        self.refused.fetch_add(1, Ordering::SeqCst);
    }

    fn set_limit(&self, n: i64) {
        self.limit.store(n, Ordering::SeqCst);
        self.signal();
    }

    pub fn add(&self, n: i64) {
        let v = self.used.fetch_add(n, Ordering::SeqCst).wrapping_add(n);
        if v > self.peak.load(Ordering::SeqCst) {
            self.peak.store(v, Ordering::SeqCst);
        }

        if n < 0 {
            self.signal();
        }
    }

    pub fn available(&self) -> i64 {
        let limit = self.effective();
        if limit <= 0 {
            return 1 << 62;
        }

        limit - self.used.load(Ordering::SeqCst)
    }

    pub fn effective(&self) -> i64 {
        let limit = self.limit.load(Ordering::SeqCst);
        if limit <= 0 {
            return 0;
        }

        match current() {
            Level::High => limit * self.high_percent / 100,
            Level::Critical => limit / 4,
            Level::Normal => limit,
        }
    }

    pub fn set(&self, n: i64) {
        let old = self.used.swap(n, Ordering::SeqCst);
        if n > self.peak.load(Ordering::SeqCst) {
            self.peak.store(n, Ordering::SeqCst);
        }

        if n < old {
            self.signal();
        }
    }

    pub fn try_acquire(&self, n: i64) -> bool {
        let limit = self.effective();
        loop {
            let used = self.used.load(Ordering::SeqCst);
            let next = used.wrapping_add(n);
            if limit > 0 && used > 0 && next > limit {
                return false;
            }

            if self
                .used
                .compare_exchange(used, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if next > self.peak.load(Ordering::SeqCst) {
                    self.peak.store(next, Ordering::SeqCst);
                }
                return true;
            }
        }
    }

    pub fn acquire(
        &self,
        n: i64,
        timeout: Duration,
        stop: Option<&dyn Fn() -> Result<(), BoxError>>,
    ) -> Result<(), BoxError> {
        if self.try_acquire(n) {
            return Ok(());
        }

        let start = Instant::now();
        loop {
            let seen = *self.changed.lock();

            if self.try_acquire(n) {
                return Ok(());
            }

            let mut wait = ACQUIRE_POLL;
            if timeout.0 >= 0 {
                let budget = StdDuration::from_nanos(timeout.0 as u64);
                let elapsed = start.elapsed();
                if elapsed >= budget {
                    self.refused.fetch_add(1, Ordering::SeqCst);
                    return Err(Box::new(ERR_BUSY));
                }
                wait = wait.min(budget - elapsed);
            }

            let until = Instant::now() + wait;
            let mut g = self.changed.lock();
            while *g == seen {
                let (next, timed_out) = self.cv.wait_until(g, until);
                g = next;
                if timed_out {
                    break;
                }
            }
            drop(g);

            if let Some(stop) = stop {
                stop()?;
            }
        }
    }

    pub fn release(&self, n: i64) {
        self.add(-n);
    }

    fn signal(&self) {
        let mut g = self.changed.lock();
        *g = g.wrapping_add(1);
        self.cv.notify_all();
    }
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("name", &self.name)
            .field("limit", &self.limit())
            .field("used", &self.used())
            .finish()
    }
}

pub static VALUES: Pool = Pool::new("values", 50);
pub static PAGES: Pool = Pool::new("pages", 50);
pub static DIRTY: Pool = Pool::new("dirty_pages", 100);
pub static HOOK_RESULTS: Pool = Pool::new("hook_results", 100);
pub static BUILDS: Pool = Pool::new("index_builds", 50);
pub static REQUESTS: Pool = Pool::new("requests", 50);

static POOLS: [&Pool; 6] = [&VALUES, &PAGES, &DIRTY, &HOOK_RESULTS, &BUILDS, &REQUESTS];

pub fn pools() -> &'static [&'static Pool] {
    &POOLS
}

type Listener = Arc<dyn Fn(Level) + Send + Sync>;

static BUDGET: AtomicI64 = AtomicI64::new(0);
static LEVEL: AtomicI32 = AtomicI32::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static TOTAL: AtomicI64 = AtomicI64::new(0);
static ELASTIC: AtomicI64 = AtomicI64::new(0);

static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

static SAMPLER: Mutex<Option<Arc<Stop>>> = Mutex::new(None);

pub fn configure(bytes: i64) {
    BUDGET.store(bytes, Ordering::SeqCst);

    if bytes <= 0 {
        for p in pools() {
            p.set_limit(0);
        }
        HOOK_RESULTS.set_limit(DEFAULT_HOOK_RESULTS);
        sample();
        return;
    }

    VALUES.set_limit(share(bytes, VALUES_SHARE));
    PAGES.set_limit(share(bytes, PAGES_SHARE));
    DIRTY.set_limit(share(bytes, DIRTY_SHARE));
    HOOK_RESULTS.set_limit(share(bytes, HOOKS_SHARE));
    BUILDS.set_limit(share(bytes, BUILDS_SHARE).max(MIN_BUILD_BYTES));
    REQUESTS.set_limit(share(bytes, REQUESTS_SHARE));
    sample();
}

fn share(b: i64, s: f64) -> i64 {
    (b as f64 * s) as i64
}

pub fn budget() -> i64 {
    BUDGET.load(Ordering::SeqCst)
}

pub fn current() -> Level {
    Level::from_i32(LEVEL.load(Ordering::SeqCst))
}

pub fn live() -> i64 {
    LIVE.load(Ordering::SeqCst)
}

pub fn total() -> i64 {
    TOTAL.load(Ordering::SeqCst)
}

pub fn on_level(f: impl Fn(Level) + Send + Sync + 'static) {
    LISTENERS.lock().push(Arc::new(f));
}

pub fn sample() {
    let (l, t) = {
        let _g = SAMPLER.lock();
        (counting::allocated(), counting::resident())
    };

    LIVE.store(l, Ordering::SeqCst);
    TOTAL.store(t, Ordering::SeqCst);

    let b = BUDGET.load(Ordering::SeqCst);
    let next = level_for(b, l, t);
    follow_spare(b, l, next);
    let prev = Level::from_i32(LEVEL.swap(next as i32, Ordering::SeqCst));
    if next == prev {
        return;
    }

    for p in pools() {
        p.signal();
    }

    if next > prev {
        let fns = LISTENERS.lock().clone();
        for f in fns {
            f(next);
        }
    }
}

pub fn follow_spare(b: i64, l: i64, lv: Level) {
    if b <= 0 || lv != Level::Normal {
        ELASTIC.store(0, Ordering::SeqCst);
        return;
    }

    let mut cur = ELASTIC.load(Ordering::SeqCst);
    if l as f64 >= ELASTIC_SHRINK_ABOVE * b as f64 {
        cur -= 4 * b / ELASTIC_STEP;
    } else if (l as f64) < ELASTIC_GROW_BELOW * b as f64 {
        cur += b / ELASTIC_STEP;
    }

    ELASTIC.store(cur.min(share(b, ELASTIC_CEILING)).max(0), Ordering::SeqCst);
}

pub fn elastic() -> i64 {
    ELASTIC.load(Ordering::SeqCst)
}

pub fn level_for(b: i64, l: i64, t: i64) -> Level {
    if b <= 0 {
        return Level::Normal;
    }

    let (b, l, t) = (b as f64, l as f64, t as f64);
    if l >= CRITICAL_SHARE * b || t >= TOTAL_CRITICAL_SHARE * b {
        return Level::Critical;
    }
    if l >= HIGH_SHARE * b || t >= TOTAL_HIGH_SHARE * b {
        return Level::High;
    }

    Level::Normal
}

pub fn request_cap() -> i64 {
    let limit = REQUESTS.limit();
    if limit > 0 {
        return (limit as f64 * REQUEST_SHARE_CAP) as i64;
    }

    0
}

pub fn start() {
    let mut g = SAMPLER.lock();
    if g.is_some() {
        return;
    }

    let stop = Arc::new(Stop::new());
    *g = Some(stop.clone());
    drop(spawn("memory-sampler", move || {
        let mut ticker = Ticker::new(SAMPLE_INTERVAL);
        while ticker.wait(&stop).is_some() {
            sample();
        }
    }));
}

pub fn stop() {
    if let Some(s) = SAMPLER.lock().take() {
        s.stop();
    }
}

#[doc(hidden)]
pub fn set_level(lv: Level) {
    LEVEL.store(lv as i32, Ordering::SeqCst);
}

#[doc(hidden)]
pub fn set_elastic(n: i64) {
    ELASTIC.store(n, Ordering::SeqCst);
}
