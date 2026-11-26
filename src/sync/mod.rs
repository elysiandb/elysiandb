use std::any::Any;
use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync as stdsync;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

pub use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

#[derive(Default)]
pub struct Mutex<T: ?Sized>(stdsync::Mutex<T>);

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Mutex<T> {
        Mutex(stdsync::Mutex::new(value))
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.0.try_lock() {
            Ok(g) => Some(g),
            Err(stdsync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
            Err(stdsync::TryLockError::WouldBlock) => None,
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Default)]
pub struct RwLock<T: ?Sized>(stdsync::RwLock<T>);

impl<T> RwLock<T> {
    pub const fn new(value: T) -> RwLock<T> {
        RwLock(stdsync::RwLock::new(value))
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.0.try_read() {
            Ok(g) => Some(g),
            Err(stdsync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
            Err(stdsync::TryLockError::WouldBlock) => None,
        }
    }

    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.0.try_write() {
            Ok(g) => Some(g),
            Err(stdsync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
            Err(stdsync::TryLockError::WouldBlock) => None,
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Default)]
pub struct Condvar(stdsync::Condvar);

impl Condvar {
    pub const fn new() -> Condvar {
        Condvar(stdsync::Condvar::new())
    }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.0.wait(guard).unwrap_or_else(PoisonError::into_inner)
    }

    pub fn wait_while<'a, T, F>(&self, guard: MutexGuard<'a, T>, condition: F) -> MutexGuard<'a, T>
    where
        F: FnMut(&mut T) -> bool,
    {
        self.0
            .wait_while(guard, condition)
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn wait_timeout<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        dur: Duration,
    ) -> (MutexGuard<'a, T>, bool) {
        let (g, r) = self
            .0
            .wait_timeout(guard, dur)
            .unwrap_or_else(PoisonError::into_inner);
        (g, r.timed_out())
    }

    pub fn wait_until<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        deadline: Instant,
    ) -> (MutexGuard<'a, T>, bool) {
        let now = Instant::now();
        if now >= deadline {
            return (guard, true);
        }
        self.wait_timeout(guard, deadline - now)
    }

    pub fn notify_one(&self) {
        self.0.notify_one();
    }

    pub fn notify_all(&self) {
        self.0.notify_all();
    }
}

#[derive(Debug, Default)]
pub struct Signal {
    pending: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    pub const fn new() -> Signal {
        Signal {
            pending: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    pub fn kick(&self) {
        let mut p = self.pending.lock();
        if !*p {
            *p = true;
            self.cv.notify_one();
        }
    }

    pub fn try_take(&self) -> bool {
        std::mem::replace(&mut *self.pending.lock(), false)
    }

    pub fn wait(&self) {
        let mut p = self.pending.lock();
        while !*p {
            p = self.cv.wait(p);
        }
        *p = false;
    }

    pub fn wait_timeout(&self, dur: Duration) -> bool {
        self.wait_until(Instant::now() + dur)
    }

    pub fn wait_until(&self, deadline: Instant) -> bool {
        let mut p = self.pending.lock();
        while !*p {
            let (g, timed_out) = self.cv.wait_until(p, deadline);
            p = g;
            if timed_out && !*p {
                return false;
            }
        }
        *p = false;
        true
    }
}

#[derive(Debug, Default)]
pub struct Stop {
    stopped: Mutex<bool>,
    cv: Condvar,
}

impl Stop {
    pub const fn new() -> Stop {
        Stop {
            stopped: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    pub fn stop(&self) {
        let mut s = self.stopped.lock();
        *s = true;
        self.cv.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        *self.stopped.lock()
    }

    pub fn wait(&self) {
        let mut s = self.stopped.lock();
        while !*s {
            s = self.cv.wait(s);
        }
    }

    pub fn wait_timeout(&self, dur: Duration) -> bool {
        self.wait_until(Instant::now() + dur)
    }

    pub fn wait_until(&self, deadline: Instant) -> bool {
        let mut s = self.stopped.lock();
        while !*s {
            let (g, timed_out) = self.cv.wait_until(s, deadline);
            s = g;
            if timed_out {
                return *s;
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct Ticker {
    period: Duration,
    next: Instant,
}

impl Ticker {
    pub fn new(period: Duration) -> Ticker {
        assert!(!period.is_zero(), "ticker period must be positive");
        Ticker {
            period,
            next: Instant::now() + period,
        }
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    pub fn reset(&mut self, period: Duration) {
        assert!(!period.is_zero(), "ticker period must be positive");
        self.period = period;
        self.next = Instant::now() + period;
    }

    pub fn deadline(&self) -> Instant {
        self.next
    }

    pub fn take(&mut self, now: Instant) -> Option<Instant> {
        if now < self.next {
            return None;
        }
        let fired = self.next;
        let behind = now.duration_since(self.next);
        let periods = behind.as_nanos() / self.period.as_nanos() + 1;
        self.next = u32::try_from(periods)
            .ok()
            .and_then(|skip| self.period.checked_mul(skip))
            .and_then(|step| self.next.checked_add(step))
            .unwrap_or(now + self.period);
        Some(fired)
    }

    pub fn wait(&mut self, stop: &Stop) -> Option<Instant> {
        loop {
            if let Some(t) = self.take(Instant::now()) {
                return Some(t);
            }
            if stop.wait_until(self.next) {
                return None;
            }
        }
    }

    pub fn wait_signal(&mut self, signal: &Signal) -> TickOrSignal {
        loop {
            if let Some(t) = self.take(Instant::now()) {
                return TickOrSignal::Tick(t);
            }
            if signal.wait_until(self.next) {
                return TickOrSignal::Signal;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOrSignal {
    Tick(Instant),
    Signal,
}

#[derive(Debug, Default)]
pub struct WaitGroup {
    count: Mutex<usize>,
    cv: Condvar,
}

impl WaitGroup {
    pub const fn new() -> WaitGroup {
        WaitGroup {
            count: Mutex::new(0),
            cv: Condvar::new(),
        }
    }

    pub fn add(&self, n: usize) {
        *self.count.lock() += n;
    }

    pub fn done(&self) {
        let mut c = self.count.lock();
        assert!(*c > 0, "negative wait group counter");
        *c -= 1;
        if *c == 0 {
            self.cv.notify_all();
        }
    }

    pub fn count(&self) -> usize {
        *self.count.lock()
    }

    pub fn wait(&self) {
        let mut c = self.count.lock();
        while *c > 0 {
            c = self.cv.wait(c);
        }
    }

    pub fn wait_timeout(&self, dur: Duration) -> bool {
        let deadline = Instant::now() + dur;
        let mut c = self.count.lock();
        while *c > 0 {
            let (g, timed_out) = self.cv.wait_until(c, deadline);
            c = g;
            if timed_out {
                return *c == 0;
            }
        }
        true
    }
}

pub const UNKNOWN_PANIC: &str = "unknown panic";

thread_local! {
    static PANIC_TRACE: RefCell<Option<String>> = const { RefCell::new(None) };
    static PANIC_TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn backtrace_requested() -> bool {
    std::env::var_os("RUST_BACKTRACE").is_some_and(|v| v != "0")
}

pub fn install_base_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = panic_message(info.payload());
        let trace = backtrace_requested().then(|| Backtrace::force_capture().to_string());
        PANIC_TEXT.with(|t| *t.borrow_mut() = Some(text));
        PANIC_TRACE.with(|t| *t.borrow_mut() = trace);
    }));
}

pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }

    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }

    UNKNOWN_PANIC.to_string()
}

pub fn die(payload: &(dyn Any + Send)) -> ! {
    die_with(&panic_message(payload))
}

fn die_with(message: &str) -> ! {
    let mut out = format!("fatal error: {message}\n");
    if let Some(trace) = PANIC_TRACE.with(|t| t.borrow_mut().take()) {
        out.push_str(&trace);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }

    let mut stderr = ManuallyDrop::new(unsafe { File::from_raw_fd(libc::STDERR_FILENO) });
    let _ = stderr.write_all(out.as_bytes());
    std::process::exit(2)
}

pub fn run_fatal<T>(f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(payload) => die(payload.as_ref()),
    }
}

pub struct FatalOnPanic;

impl Drop for FatalOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let text = PANIC_TEXT.with(|t| t.borrow_mut().take());
            die_with(text.as_deref().unwrap_or(UNKNOWN_PANIC));
        }
    }
}

pub fn block_shutdown_signals() {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

fn thread_body<F, T>(f: F) -> impl FnOnce() -> T
where
    F: FnOnce() -> T,
{
    move || {
        block_shutdown_signals();
        run_fatal(f)
    }
}

pub fn spawn<F, T>(name: &str, f: F) -> std::thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(thread_body(f))
        .unwrap_or_else(|e| panic!("cannot start thread {name}: {e}"))
}

pub fn spawn_with_stack<F, T>(
    name: &str,
    stack: usize,
    f: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(stack)
        .spawn(thread_body(f))
}
