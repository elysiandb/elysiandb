use std::collections::{BTreeSet, HashMap};
use std::io;
use std::os::fd::RawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::listener::WakePipe;
use super::sys::Selector;
use crate::sync::Mutex;

const WAKE_TOKEN: u64 = u64::MAX;
const STOP_TICK: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interest {
    Read,
    Write,
}

pub(crate) trait Parked: Send {
    fn fd(&self) -> RawFd;

    fn interest(&self) -> Interest {
        Interest::Read
    }
}

pub(crate) trait Host<T>: Send + Sync {
    fn stopping(&self) -> bool;
    fn accept_finished(&self) -> bool;
    fn expired(&self, conn: T);
    fn drained(&self, conn: T);
    fn ready(self: &Arc<Self>, conn: T);
}

struct Entry<T> {
    conn: T,
    interest: Interest,
    deadline: Option<Instant>,
    closable_at: Option<Instant>,
}

struct Registry<T> {
    entries: HashMap<u64, Entry<T>>,
    deadlines: BTreeSet<(Instant, u64)>,
    next_token: u64,
    exited: bool,
}

pub(crate) struct Poller<T> {
    selector: Selector,
    wake: WakePipe,
    registry: Mutex<Registry<T>>,
}

impl<T: Parked> Poller<T> {
    pub(crate) fn new() -> io::Result<Poller<T>> {
        let selector = Selector::new()?;
        let wake = WakePipe::new()?;
        selector.add(wake.read_fd(), WAKE_TOKEN, false, Interest::Read)?;
        Ok(Poller {
            selector,
            wake,
            registry: Mutex::new(Registry {
                entries: HashMap::new(),
                deadlines: BTreeSet::new(),
                next_token: 1,
                exited: false,
            }),
        })
    }

    pub(crate) fn wake(&self) {
        self.wake.wake();
    }

    pub(crate) fn register(
        &self,
        conn: T,
        deadline: Option<Instant>,
        closable_at: Option<Instant>,
    ) -> Result<(), T> {
        let mut reg = self.registry.lock();
        if reg.exited {
            return Err(conn);
        }
        let token = reg.next_token;
        reg.next_token += 1;
        let fd = conn.fd();
        let interest = conn.interest();
        if let Some(d) = deadline {
            reg.deadlines.insert((d, token));
        }
        reg.entries.insert(
            token,
            Entry {
                conn,
                interest,
                deadline,
                closable_at,
            },
        );
        if self.selector.add(fd, token, true, interest).is_err() {
            let entry = reg.entries.remove(&token);
            if let Some(d) = deadline {
                reg.deadlines.remove(&(d, token));
            }
            return match entry {
                Some(e) => Err(e.conn),
                None => Ok(()),
            };
        }
        drop(reg);
        if deadline.is_some() {
            self.wake();
        }
        Ok(())
    }

    fn take(&self, reg: &mut Registry<T>, token: u64) -> Option<T> {
        let entry = reg.entries.remove(&token)?;
        if let Some(d) = entry.deadline {
            reg.deadlines.remove(&(d, token));
        }
        let _ = self.selector.delete(entry.conn.fd(), entry.interest);
        Some(entry.conn)
    }

    pub(crate) fn run<H: Host<T>>(self: &Arc<Self>, host: &Arc<H>) {
        let mut ready = Vec::new();
        loop {
            let stopping = host.stopping();
            let timeout = {
                let mut reg = self.registry.lock();
                if stopping && reg.entries.is_empty() && host.accept_finished() {
                    reg.exited = true;
                    return;
                }
                let next = reg.deadlines.iter().next().map(|(d, _)| *d);
                let now = Instant::now();
                let mut t = next.map(|d| d.saturating_duration_since(now));
                if stopping {
                    t = Some(t.map_or(STOP_TICK, |t| t.min(STOP_TICK)));
                }
                t
            };
            if self.selector.wait(&mut ready, timeout).is_err() {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            let mut dispatch = Vec::new();
            let mut expired = Vec::new();
            let mut drained = Vec::new();
            {
                let mut reg = self.registry.lock();
                for &token in &ready {
                    if token == WAKE_TOKEN {
                        self.wake.drain();
                        continue;
                    }
                    if let Some(conn) = self.take(&mut reg, token) {
                        dispatch.push(conn);
                    }
                }
                let now = Instant::now();
                let due: Vec<u64> = reg
                    .deadlines
                    .iter()
                    .take_while(|(d, _)| *d <= now)
                    .map(|(_, t)| *t)
                    .collect();
                for token in due {
                    if let Some(conn) = self.take(&mut reg, token) {
                        expired.push(conn);
                    }
                }
                if host.stopping() {
                    let closable: Vec<u64> = reg
                        .entries
                        .iter()
                        .filter(|(_, e)| e.closable_at.is_some_and(|at| at <= now))
                        .map(|(t, _)| *t)
                        .collect();
                    for token in closable {
                        if let Some(conn) = self.take(&mut reg, token) {
                            drained.push(conn);
                        }
                    }
                }
            }
            for conn in expired {
                host.expired(conn);
            }
            for conn in drained {
                host.drained(conn);
            }
            for conn in dispatch {
                host.ready(conn);
            }
        }
    }
}
