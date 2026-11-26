use super::value::{Attr, Value};
use crate::sync::Mutex;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const THROTTLE_MAX_KEYS: usize = 10_000;

type Clock = Box<dyn Fn() -> Instant + Send>;

pub struct Throttle {
    inner: Mutex<Inner>,
}

struct Inner {
    interval: Duration,
    entries: BTreeMap<Vec<u8>, ThrottleEntry>,
    now: Option<Clock>,
}

struct ThrottleEntry {
    last: Instant,
    suppressed: usize,
}

fn within(now: Instant, last: Instant, interval: Duration) -> bool {
    match now.checked_duration_since(last) {
        Some(elapsed) => elapsed < interval,
        None => true,
    }
}

impl Throttle {
    pub const fn new(interval: Duration) -> Throttle {
        Throttle {
            inner: Mutex::new(Inner {
                interval,
                entries: BTreeMap::new(),
                now: None,
            }),
        }
    }

    pub fn set_clock(&self, now: impl Fn() -> Instant + Send + 'static) {
        self.inner.lock().now = Some(Box::new(now));
    }

    pub fn allow(&self, key: impl AsRef<[u8]>) -> (usize, bool) {
        let key = key.as_ref();
        let mut inner = self.inner.lock();

        let now = inner.now();
        let interval = inner.interval;
        let Some(e) = inner.entries.get_mut(key) else {
            inner.make_room(now);
            inner.entries.insert(
                key.to_vec(),
                ThrottleEntry {
                    last: now,
                    suppressed: 0,
                },
            );
            return (0, true);
        };

        if within(now, e.last, interval) {
            e.suppressed += 1;
            return (0, false);
        }

        let suppressed = e.suppressed;
        e.last = now;
        e.suppressed = 0;

        (suppressed, true)
    }

    pub fn reset(&self, key: impl AsRef<[u8]>) -> usize {
        let mut inner = self.inner.lock();

        match inner.entries.remove(key.as_ref()) {
            Some(e) => e.suppressed,
            None => 0,
        }
    }
}

impl Inner {
    fn now(&self) -> Instant {
        match &self.now {
            Some(clock) => clock(),
            None => Instant::now(),
        }
    }

    fn make_room(&mut self, now: Instant) {
        if self.entries.len() < THROTTLE_MAX_KEYS {
            return;
        }

        let interval = self.interval;
        self.entries.retain(|_, e| within(now, e.last, interval));

        if self.entries.len() >= THROTTLE_MAX_KEYS {
            self.entries = BTreeMap::new();
        }
    }
}

pub fn with_suppressed<'a>(attrs: &[Attr<'a>], suppressed: usize) -> Vec<Attr<'a>> {
    let mut out = attrs.to_vec();
    if suppressed > 0 {
        out.push((
            "suppressed",
            Value::Int(i64::try_from(suppressed).unwrap_or(i64::MAX)),
        ));
    }

    out
}
