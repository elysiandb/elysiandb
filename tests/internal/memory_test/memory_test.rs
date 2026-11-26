use std::error::Error as StdError;
use std::fmt;
use std::sync::mpsc;
use std::time::{Duration as StdDuration, Instant};

use elysiandb::gocompat::errors::{self, BoxError};
use elysiandb::gocompat::time::Duration;
use elysiandb::memory::{self, ERR_BUSY, Level};
use elysiandb::sync::spawn;

struct Cleanup<F: FnMut()>(F);

impl<F: FnMut()> Drop for Cleanup<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[derive(Debug, PartialEq)]
struct ClientLeft;

impl fmt::Display for ClientLeft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("client left")
    }
}

impl StdError for ClientLeft {}

#[test]
fn shares_follow_the_budget() {
    memory::configure(1 << 40);
    let _cleanup = Cleanup(|| memory::configure(0));

    let mut total = 0i64;
    for p in memory::pools() {
        assert!(
            p.limit() > 0,
            "{} has no limit under an explicit budget",
            p.name()
        );
        total += p.limit();
    }

    assert!(
        total <= (1i64 << 40) * 65 / 100,
        "the shares should leave room for the runtime, they take {total} of {}",
        1i64 << 40
    );

    assert_eq!(
        memory::current(),
        Level::Normal,
        "a small heap under a huge budget is not under pressure, got {}",
        memory::current()
    );
}

#[test]
fn without_a_budget_nothing_is_limited() {
    memory::configure(0);

    assert!(
        memory::REQUESTS.try_acquire(1 << 50),
        "without a budget a request is never refused"
    );
    memory::REQUESTS.release(1 << 50);

    assert!(
        memory::HOOK_RESULTS.limit() > 0,
        "hook results keep a default limit without a budget"
    );
}

#[test]
fn acquire_waits_then_gives_up() {
    memory::configure(1 << 40);
    let _cleanup = Cleanup(|| memory::configure(0));

    let p = &memory::REQUESTS;
    let limit = p.limit();
    assert!(p.try_acquire(limit), "the first reservation always fits");
    let _release = Cleanup(move || memory::REQUESTS.release(limit));

    assert!(!p.try_acquire(1), "a full pool refuses");

    let before = p.refused();
    let start = Instant::now();
    match p.acquire(1, Duration::MILLISECOND * 30, None) {
        Err(err) if errors::is(err.as_ref(), &ERR_BUSY) => {}
        other => panic!("a full pool ends in a refusal, got {other:?}"),
    }
    assert!(
        start.elapsed() >= StdDuration::from_millis(20),
        "a request should wait before being refused"
    );
    assert_eq!(p.refused(), before + 1, "a refusal is counted");

    let stop = || -> Result<(), BoxError> { Err(Box::new(ClientLeft)) };
    match p.acquire(1, Duration::MINUTE, Some(&stop)) {
        Err(err) if errors::is(err.as_ref(), &ClientLeft) => {}
        other => panic!("a waiter stops when its request stops, got {other:?}"),
    }

    let (tx, rx) = mpsc::channel();
    drop(spawn("memory-test-waiter", move || {
        let _ = tx.send(memory::REQUESTS.acquire(1 << 20, Duration::MINUTE, None));
    }));
    std::thread::sleep(StdDuration::from_millis(10));
    p.release(limit / 2);
    let _readd = Cleanup(move || memory::REQUESTS.add(limit / 2));
    match rx.recv_timeout(StdDuration::from_secs(5)) {
        Ok(Ok(())) => p.release(1 << 20),
        Ok(Err(err)) => panic!("a waiter gets memory once it is released: {err}"),
        Err(_) => panic!("a waiter was not woken by a release"),
    }
}

#[test]
fn pressure_follows_the_process() {
    memory::configure(1 << 20);
    let _cleanup = Cleanup(|| memory::configure(0));

    memory::sample();
    assert_eq!(
        memory::current(),
        Level::Critical,
        "a heap larger than the budget is critical, got {}",
        memory::current()
    );

    assert_eq!(
        memory::VALUES.effective(),
        memory::VALUES.limit() / 4,
        "critical pressure quarters the share of values"
    );

    assert_eq!(
        memory::HOOK_RESULTS.effective(),
        memory::HOOK_RESULTS.limit() / 4,
        "critical pressure quarters the share of hook results"
    );
}
