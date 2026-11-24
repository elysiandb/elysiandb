use std::collections::{BTreeSet, HashMap};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::listener::WakePipe;
use super::server::{ConnState, ServerInner};
use super::sys::Selector;
use crate::sync::Mutex;

const WAKE_TOKEN: u64 = u64::MAX;
const STOP_TICK: Duration = Duration::from_millis(100);

struct Entry {
    st: ConnState,
    deadline: Option<Instant>,
    closable_at: Instant,
}

struct Registry {
    entries: HashMap<u64, Entry>,
    deadlines: BTreeSet<(Instant, u64)>,
    next_token: u64,
    exited: bool,
}

pub(crate) struct Poller {
    selector: Selector,
    wake: WakePipe,
    registry: Mutex<Registry>,
}

impl Poller {
    pub(crate) fn new() -> io::Result<Poller> {
        let selector = Selector::new()?;
        let wake = WakePipe::new()?;
        selector.add(wake.read_fd(), WAKE_TOKEN, false)?;
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
        st: ConnState,
        deadline: Option<Instant>,
        closable_at: Instant,
    ) -> Result<(), ConnState> {
        let mut reg = self.registry.lock();
        if reg.exited {
            return Err(st);
        }
        let token = reg.next_token;
        reg.next_token += 1;
        let fd = st.inner.fd();
        if let Some(d) = deadline {
            reg.deadlines.insert((d, token));
        }
        reg.entries.insert(
            token,
            Entry {
                st,
                deadline,
                closable_at,
            },
        );
        if self.selector.add(fd, token, true).is_err() {
            let entry = reg.entries.remove(&token);
            if let Some(d) = deadline {
                reg.deadlines.remove(&(d, token));
            }
            return match entry {
                Some(e) => Err(e.st),
                None => Ok(()),
            };
        }
        drop(reg);
        if deadline.is_some() {
            self.wake();
        }
        Ok(())
    }

    fn take(&self, reg: &mut Registry, token: u64) -> Option<ConnState> {
        let entry = reg.entries.remove(&token)?;
        if let Some(d) = entry.deadline {
            reg.deadlines.remove(&(d, token));
        }
        let _ = self.selector.delete(entry.st.inner.fd());
        Some(entry.st)
    }

    pub(crate) fn run(self: &Arc<Self>, server: &Arc<ServerInner>) {
        let mut ready = Vec::new();
        loop {
            let stopping = server.is_stopping();
            let timeout = {
                let mut reg = self.registry.lock();
                if stopping && reg.entries.is_empty() && server.accept_done() {
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
            let mut close = Vec::new();
            {
                let mut reg = self.registry.lock();
                for &token in &ready {
                    if token == WAKE_TOKEN {
                        self.wake.drain();
                        continue;
                    }
                    if let Some(st) = self.take(&mut reg, token) {
                        dispatch.push(st);
                    }
                }
                let now = Instant::now();
                let expired: Vec<u64> = reg
                    .deadlines
                    .iter()
                    .take_while(|(d, _)| *d <= now)
                    .map(|(_, t)| *t)
                    .collect();
                for token in expired {
                    if let Some(st) = self.take(&mut reg, token) {
                        close.push(st);
                    }
                }
                if server.is_stopping() {
                    let closable: Vec<u64> = reg
                        .entries
                        .iter()
                        .filter(|(_, e)| e.closable_at <= now)
                        .map(|(t, _)| *t)
                        .collect();
                    for token in closable {
                        if let Some(st) = self.take(&mut reg, token) {
                            close.push(st);
                        }
                    }
                }
            }
            for st in close {
                server.close_conn(st);
            }
            for st in dispatch {
                server.dispatch(st);
            }
        }
    }
}
