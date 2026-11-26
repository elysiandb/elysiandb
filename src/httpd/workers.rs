use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Duration;

use super::sys::MAX_WORKERS;
use crate::globals::procs;
use crate::sync::{Condvar, Mutex};

pub(crate) type Job = Box<dyn FnOnce() + Send>;

const WORKER_STACK_SIZE: usize = 8 << 20;
const MAX_IDLE: Duration = Duration::from_secs(10);

struct State {
    queue: VecDeque<Job>,
    idle: usize,
    total: usize,
}

pub(crate) struct Pool {
    state: Mutex<State>,
    cv: Condvar,
    max_workers: usize,
}

static POOL: OnceLock<Pool> = OnceLock::new();

pub(crate) fn pool() -> &'static Pool {
    POOL.get_or_init(|| Pool {
        state: Mutex::new(State {
            queue: VecDeque::new(),
            idle: 0,
            total: 0,
        }),
        cv: Condvar::new(),
        max_workers: MAX_WORKERS,
    })
}

struct WorkerExit;

impl Drop for WorkerExit {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let p = pool();
            let mut st = p.state.lock();
            st.total = st.total.saturating_sub(1);
        }
    }
}

impl Pool {
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
            let spawned = std::thread::Builder::new()
                .name("http-worker".to_string())
                .stack_size(WORKER_STACK_SIZE)
                .spawn(move || crate::sync::run_fatal(|| self.run(job)));
            if spawned.is_err() {
                let mut st = self.state.lock();
                st.total -= 1;
            }
            return;
        }
        st.queue.push_back(job);
    }

    fn run(&'static self, first: Job) {
        let guard = WorkerExit;
        first();
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
