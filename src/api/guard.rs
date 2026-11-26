use std::fmt;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::gocompat::errors::BoxError;
use crate::memory;

use super::plan::heavy_queue_timeout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardError {
    Deadline,
    Abandoned,
    Busy,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GuardError::Deadline => "request deadline exceeded",
            GuardError::Abandoned => "request abandoned by the client",
            GuardError::Busy => "server busy",
        })
    }
}

impl std::error::Error for GuardError {}

pub const ERR_DEADLINE: GuardError = GuardError::Deadline;
pub const ERR_ABANDONED: GuardError = GuardError::Abandoned;
pub const ERR_BUSY: GuardError = GuardError::Busy;

pub type AbandonedProbe = Box<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
pub struct Guard {
    pub deadline: Option<Instant>,
    pub abandoned: Option<AbandonedProbe>,
    pub admitted: bool,
    pub laned: bool,
    parent: Option<Arc<Guard>>,
    err: OnceLock<GuardError>,
    reserved: AtomicI64,
    charged: AtomicI64,
    lanes: AtomicU32,
}

const MEMORY_CHUNK: i64 = 1 << 20;

fn guard_error_of(r: &Guard, err: BoxError) -> GuardError {
    if let Some(g) = err.downcast_ref::<GuardError>() {
        return *g;
    }

    if err.downcast_ref::<memory::Busy>().is_some() {
        return r.fail(GuardError::Busy);
    }

    r.fail(GuardError::Busy)
}

impl Guard {
    pub fn new(deadline: Option<Instant>, abandoned: Option<AbandonedProbe>) -> Guard {
        Guard {
            deadline,
            abandoned,
            ..Guard::default()
        }
    }

    fn root(&self) -> &Guard {
        let mut g = self;
        while let Some(p) = &g.parent {
            g = p;
        }

        g
    }

    pub(crate) fn enter_lane(&self, bit: u32) -> bool {
        self.root().lanes.fetch_or(bit, Ordering::SeqCst) & bit == 0
    }

    pub fn charge(&self, n: i64) -> Result<(), GuardError> {
        let r = self.root();
        if n <= 0 || memory::REQUESTS.limit() <= 0 {
            return Ok(());
        }

        let used = r.charged.fetch_add(n, Ordering::SeqCst) + n;
        let limit = memory::request_cap();
        if limit > 0 && used > limit {
            memory::REQUESTS.refuse();
            return Err(r.fail(GuardError::Busy));
        }

        loop {
            let reserved = r.reserved.load(Ordering::SeqCst);
            if used <= reserved {
                return Ok(());
            }

            let chunk = MEMORY_CHUNK.max(used - reserved);
            let stop = || r.check().map_err(|e| Box::new(e) as BoxError);
            if let Err(err) = memory::REQUESTS.acquire(chunk, heavy_queue_timeout(), Some(&stop)) {
                return Err(guard_error_of(r, err));
            }

            r.reserved.fetch_add(chunk, Ordering::SeqCst);
        }
    }

    pub fn reserve(&self, n: i64) -> Result<(), GuardError> {
        let r = self.root();
        if memory::REQUESTS.limit() <= 0 {
            return Ok(());
        }

        loop {
            let reserved = r.reserved.load(Ordering::SeqCst);
            if reserved >= n {
                return Ok(());
            }

            let stop = || r.check().map_err(|e| Box::new(e) as BoxError);
            if let Err(err) =
                memory::REQUESTS.acquire(n - reserved, heavy_queue_timeout(), Some(&stop))
            {
                return Err(guard_error_of(r, err));
            }

            r.reserved.fetch_add(n - reserved, Ordering::SeqCst);
        }
    }

    pub fn reject(&self) -> GuardError {
        self.root().fail(GuardError::Busy)
    }

    pub fn release_memory(&self) {
        let r = self.root();
        r.charged.store(0, Ordering::SeqCst);
        let n = r.reserved.swap(0, Ordering::SeqCst);
        if n > 0 {
            memory::REQUESTS.release(n);
        }
    }

    pub fn check(&self) -> Result<(), GuardError> {
        if let Some(err) = self.err.get() {
            return Err(*err);
        }

        if let Some(deadline) = self.deadline
            && Instant::now() > deadline
        {
            return Err(self.fail(GuardError::Deadline));
        }

        if let Some(abandoned) = &self.abandoned
            && abandoned()
        {
            return Err(self.fail(GuardError::Abandoned));
        }

        if let Some(parent) = &self.parent
            && let Err(err) = parent.check()
        {
            return Err(self.fail(err));
        }

        Ok(())
    }

    pub(crate) fn fail(&self, err: GuardError) -> GuardError {
        let _ = self.err.set(err);

        *self.err.get().unwrap_or(&err)
    }

    pub fn err(&self) -> Option<GuardError> {
        self.err.get().copied()
    }

    pub fn until(parent: Option<Arc<Guard>>, deadline: Option<Instant>) -> Guard {
        Guard {
            deadline,
            admitted: true,
            parent,
            ..Guard::default()
        }
    }

    pub fn stopped(&self) -> bool {
        self.check().is_err()
    }
}

pub fn check(guard: Option<&Guard>) -> Result<(), GuardError> {
    guard.map_or(Ok(()), Guard::check)
}

pub fn err(guard: Option<&Guard>) -> Option<GuardError> {
    guard.and_then(Guard::err)
}

pub fn charge(guard: Option<&Guard>, n: i64) -> Result<(), GuardError> {
    guard.map_or(Ok(()), |g| g.charge(n))
}

pub fn reserve(guard: Option<&Guard>, n: i64) -> Result<(), GuardError> {
    guard.map_or(Ok(()), |g| g.reserve(n))
}

pub fn reject(guard: Option<&Guard>) -> GuardError {
    guard.map_or(GuardError::Busy, Guard::reject)
}

pub fn release_memory(guard: Option<&Guard>) {
    if let Some(g) = guard {
        g.release_memory();
    }
}

pub fn stopped(guard: Option<&Guard>) -> bool {
    check(guard).is_err()
}
