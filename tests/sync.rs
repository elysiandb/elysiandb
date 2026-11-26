use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use elysiandb::sync::{
    Condvar, Mutex, RwLock, Signal, Stop, TickOrSignal, Ticker, WaitGroup, spawn,
};

#[test]
fn poisoned_locks_stay_usable() {
    let m = Arc::new(Mutex::new(1));
    let rw = Arc::new(RwLock::new(2));
    let (m2, rw2) = (m.clone(), rw.clone());
    let r = std::thread::Builder::new()
        .name("poison".to_string())
        .spawn(move || {
            let _g = m2.lock();
            let _w = rw2.write();
            panic!("poison both locks");
        })
        .unwrap_or_else(|e| panic!("cannot start thread poison: {e}"))
        .join();
    assert!(r.is_err());
    *m.lock() += 1;
    *rw.write() += 1;
    assert_eq!(*m.lock(), 2);
    assert_eq!(*rw.read(), 3);
    assert!(m.try_lock().is_some());
    assert!(rw.try_read().is_some());
    assert!(rw.try_write().is_some());
    let m = Arc::try_unwrap(m).unwrap();
    assert_eq!(m.into_inner(), 2);
}

#[test]
fn try_lock_reports_a_held_lock() {
    let m = Mutex::new(0);
    let g = m.lock();
    assert!(m.try_lock().is_none());
    drop(g);
    let rw = RwLock::new(0);
    let r = rw.read();
    assert!(rw.try_write().is_none());
    assert!(rw.try_read().is_some());
    drop(r);
}

#[test]
fn condvar_wait_until_a_past_deadline_returns_at_once() {
    let m = Mutex::new(0);
    let cv = Condvar::new();
    let g = m.lock();
    let (_g, timed_out) = cv.wait_until(g, Instant::now() - Duration::from_millis(1));
    assert!(timed_out);
}

#[test]
fn signals_coalesce() {
    let s = Signal::new();
    s.kick();
    s.kick();
    assert!(s.try_take());
    assert!(!s.try_take());
    assert!(!s.wait_timeout(Duration::from_millis(5)));
    s.kick();
    assert!(s.wait_timeout(Duration::from_millis(5)));
}

#[test]
fn a_signal_wakes_a_waiting_thread() {
    let s = Arc::new(Signal::new());
    let s2 = s.clone();
    let h = spawn("waiter", move || {
        s2.wait();
        true
    });
    std::thread::sleep(Duration::from_millis(10));
    s.kick();
    assert!(h.join().unwrap());
}

#[test]
fn stop_wakes_every_waiter_and_stays_set() {
    let stop = Arc::new(Stop::new());
    let woken = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let (stop, woken) = (stop.clone(), woken.clone());
            spawn(&format!("stop-{i}"), move || {
                assert!(stop.wait_timeout(Duration::from_secs(10)));
                woken.fetch_add(1, Ordering::SeqCst);
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(10));
    assert!(!stop.is_stopped());
    stop.stop();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(woken.load(Ordering::SeqCst), 3);
    assert!(stop.is_stopped());
    assert!(stop.wait_timeout(Duration::ZERO));
    stop.wait();
}

#[test]
fn a_stop_wait_times_out_when_nothing_stops() {
    let stop = Stop::new();
    let start = Instant::now();
    assert!(!stop.wait_timeout(Duration::from_millis(20)));
    assert!(start.elapsed() >= Duration::from_millis(20));
}

#[test]
fn ticker_drops_missed_ticks_and_keeps_its_grid() {
    let period = Duration::from_millis(100);
    let mut t = Ticker::new(period);
    let first = t.deadline();
    assert_eq!(t.take(first - Duration::from_millis(1)), None);
    assert_eq!(t.take(first), Some(first));
    assert_eq!(t.deadline(), first + period);
    let late = first + period * 3 + Duration::from_millis(40);
    assert_eq!(t.take(late), Some(first + period));
    assert_eq!(t.deadline(), first + period * 4);
    assert_eq!(t.take(late), None);
    let exact = first + period * 4;
    assert_eq!(t.take(exact), Some(exact));
    assert_eq!(t.deadline(), first + period * 5);
    t.reset(Duration::from_millis(7));
    assert_eq!(t.period(), Duration::from_millis(7));
}

#[test]
fn ticker_wait_returns_ticks_until_stopped() {
    let stop = Arc::new(Stop::new());
    let mut t = Ticker::new(Duration::from_millis(5));
    assert!(t.wait(&stop).is_some());
    assert!(t.wait(&stop).is_some());
    stop.stop();
    std::thread::sleep(Duration::from_millis(6));
    assert!(t.wait(&stop).is_some());
    assert!(t.wait(&stop).is_none());
}

#[test]
fn ticker_wait_signal_returns_the_first_event() {
    let s = Signal::new();
    let mut t = Ticker::new(Duration::from_secs(60));
    s.kick();
    assert_eq!(t.wait_signal(&s), TickOrSignal::Signal);
    let mut fast = Ticker::new(Duration::from_millis(5));
    assert!(matches!(fast.wait_signal(&s), TickOrSignal::Tick(_)));
}

#[test]
#[should_panic(expected = "ticker period must be positive")]
fn a_zero_ticker_period_panics() {
    let _ = Ticker::new(Duration::ZERO);
}

#[test]
fn wait_group_waits_for_every_task() {
    let wg = Arc::new(WaitGroup::new());
    let n = Arc::new(AtomicUsize::new(0));
    wg.add(4);
    for i in 0..4 {
        let (wg, n) = (wg.clone(), n.clone());
        spawn(&format!("task-{i}"), move || {
            std::thread::sleep(Duration::from_millis(5));
            n.fetch_add(1, Ordering::SeqCst);
            wg.done();
        });
    }
    wg.wait();
    assert_eq!(n.load(Ordering::SeqCst), 4);
    assert_eq!(wg.count(), 0);
    assert!(wg.wait_timeout(Duration::ZERO));
    wg.add(1);
    assert!(!wg.wait_timeout(Duration::from_millis(5)));
    wg.done();
}

#[test]
fn spawned_threads_carry_their_name() {
    let h = spawn("named-thread", || {
        std::thread::current().name().map(str::to_string)
    });
    assert_eq!(h.join().unwrap().as_deref(), Some("named-thread"));
}

const FATAL_TEST: &str = "a_panic_in_a_spawned_thread_ends_the_process";

#[test]
fn a_panic_in_a_spawned_thread_ends_the_process() {
    if std::env::var_os("RUN_FATAL_HELPER").is_some_and(|v| v == "spawn") {
        elysiandb::sync::install_base_panic_hook();
        let _ = spawn("doomed", || panic!("boom in a thread")).join();
        std::thread::sleep(Duration::from_secs(10));
        return;
    }

    let run = |backtrace: bool| {
        let mut cmd = std::process::Command::new(std::env::current_exe().expect("test binary"));
        cmd.args([FATAL_TEST, "--exact", "--test-threads=1"])
            .env("RUN_FATAL_HELPER", "spawn")
            .env_remove("RUST_BACKTRACE")
            .env_remove("RUST_LIB_BACKTRACE");
        if backtrace {
            cmd.env("RUST_BACKTRACE", "1");
        }
        cmd.output().expect("child test run")
    };

    let out = run(false);
    assert_eq!(out.status.code(), Some(2), "{:?}", out.status);
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fatal error: boom in a thread\n"
    );

    let out = run(true);
    assert_eq!(out.status.code(), Some(2), "{:?}", out.status);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("fatal error: boom in a thread\n"), "{err}");
    assert!(err.lines().count() > 1, "no backtrace in {err}");
}
