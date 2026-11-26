use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Duration;

use super::sys::MAX_WORKERS;
use crate::globals::procs;
use crate::sync::{Condvar, Mutex};

pub(crate) type Job = Box<dyn FnOnce() + Send>;

const WORKER_STACK_SIZE: usize = 8 << 20;
const MAX_IDLE: Duration = Duration::from_secs(10);
const SPAWN_RETRY_MIN: Duration = Duration::from_millis(5);
const SPAWN_RETRY_MAX: Duration = Duration::from_secs(1);

struct State {
    queue: VecDeque<Job>,
    idle: usize,
    total: usize,
}

pub(crate) struct Pool {
    name: &'static str,
    stack_size: usize,
    state: Mutex<State>,
    cv: Condvar,
    max_workers: usize,
}

static POOL: OnceLock<Pool> = OnceLock::new();

pub(crate) fn pool() -> &'static Pool {
    POOL.get_or_init(|| Pool::new("http-worker", WORKER_STACK_SIZE))
}

struct Handoff {
    pool: &'static Pool,
    job: Option<Job>,
}

impl Drop for Handoff {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            self.pool.state.lock().queue.push_back(job);
            self.pool.cv.notify_one();
        }
    }
}

struct WorkerExit(&'static Pool);

impl Drop for WorkerExit {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let mut st = self.0.state.lock();
            st.total = st.total.saturating_sub(1);
        }
    }
}

impl Pool {
    pub(crate) fn new(name: &'static str, stack_size: usize) -> Pool {
        Pool {
            name,
            stack_size,
            state: Mutex::new(State {
                queue: VecDeque::new(),
                idle: 0,
                total: 0,
            }),
            cv: Condvar::new(),
            max_workers: MAX_WORKERS,
        }
    }

    pub(crate) fn submit(&'static self, job: Job) {
        let mut st = self.state.lock();
        if st.idle > st.queue.len() {
            st.queue.push_back(job);
            drop(st);
            self.cv.notify_one();
            return;
        }
        if st.total < self.max_workers {
            st.total += 1;
            drop(st);
            let mut first = Some(job);
            let mut delay = SPAWN_RETRY_MIN;
            loop {
                if self.spawn(first.take()) {
                    return;
                }
                let mut st = self.state.lock();
                if st.total > 1 {
                    st.total -= 1;
                    return;
                }
                drop(st);
                std::thread::sleep(delay);
                delay = (delay * 2).min(SPAWN_RETRY_MAX);
            }
        }
        st.queue.push_back(job);
    }

    fn spawn(&'static self, first: Option<Job>) -> bool {
        let mut handoff = Handoff {
            pool: self,
            job: first,
        };
        std::thread::Builder::new()
            .name(self.name.to_string())
            .stack_size(self.stack_size)
            .spawn(move || {
                let first = handoff.job.take();
                drop(handoff);
                crate::sync::run_fatal(|| self.run(first));
            })
            .is_ok()
    }

    fn run(&'static self, first: Option<Job>) {
        let guard = WorkerExit(self);
        if let Some(job) = first {
            job();
        }
        loop {
            let mut st = self.state.lock();
            let job = loop {
                if let Some(job) = st.queue.pop_front() {
                    break Some(job);
                }
                st.idle += 1;
                let (g, timed_out) = self.cv.wait_timeout(st, MAX_IDLE);
                st = g;
                st.idle -= 1;
                if timed_out && st.queue.is_empty() && st.total > procs() {
                    st.total -= 1;
                    break None;
                }
            };
            drop(st);
            match job {
                Some(job) => job(),
                None => {
                    std::mem::forget(guard);
                    return;
                }
            }
        }
    }
}
