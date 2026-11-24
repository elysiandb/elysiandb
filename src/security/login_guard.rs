use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::sync::Mutex;

use super::login_source::LoginSource;

const LOGIN_FAILURES_PER_CLIENT_AND_USER: i64 = 5;
const LOGIN_FAILURES_PER_CLIENT: i64 = 20;
const LOGIN_LOCKOUT_MAX: Duration = Duration::from_secs(15 * 60);
const LOGIN_FAILURE_MEMORY: Duration = Duration::from_secs(15 * 60);
const LOGIN_GUARD_MAX_ENTRIES: usize = 100_000;

pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

struct LoginAttempts {
    failures: i64,
    last_failure: Instant,
    locked_until: Option<Instant>,
}

struct LoginGuard {
    attempts: BTreeMap<Vec<u8>, LoginAttempts>,
    now: Option<Clock>,
}

static DEFAULT_LOGIN_GUARD: Mutex<LoginGuard> = Mutex::new(LoginGuard {
    attempts: BTreeMap::new(),
    now: None,
});

pub fn reset_login_attempts() {
    DEFAULT_LOGIN_GUARD.lock().attempts = BTreeMap::new();
}

pub fn set_login_clock(now: Option<Clock>) {
    DEFAULT_LOGIN_GUARD.lock().now = now;
}

fn client_login_key(source: LoginSource) -> Vec<u8> {
    let mut key = b"client|".to_vec();
    key.extend_from_slice(source.client_key().as_bytes());
    key
}

fn client_user_login_key(source: LoginSource, username: &[u8]) -> Vec<u8> {
    let mut key = b"user|".to_vec();
    key.extend_from_slice(source.client_key().as_bytes());
    key.push(b'|');
    key.extend_from_slice(username);
    key
}

fn locked_at(entry: &LoginAttempts, now: Instant) -> bool {
    entry.locked_until.is_some_and(|until| now < until)
}

fn forgotten(entry: &LoginAttempts, now: Instant) -> bool {
    !locked_at(entry, now)
        && now
            .checked_duration_since(entry.last_failure)
            .is_some_and(|elapsed| elapsed > LOGIN_FAILURE_MEMORY)
}

fn lockout_duration(excess: i64) -> Duration {
    if excess >= 10 {
        return LOGIN_LOCKOUT_MAX;
    }

    let duration = Duration::from_secs(1u64 << excess.max(0));
    if duration > LOGIN_LOCKOUT_MAX {
        return LOGIN_LOCKOUT_MAX;
    }

    duration
}

impl LoginGuard {
    fn now(&self) -> Instant {
        match &self.now {
            Some(clock) => clock(),
            None => Instant::now(),
        }
    }

    fn lock_remaining(&mut self, key: &[u8], now: Instant) -> Duration {
        let Some(entry) = self.attempts.get(key) else {
            return Duration::ZERO;
        };

        if forgotten(entry, now) {
            self.attempts.remove(key);
            return Duration::ZERO;
        }

        match entry.locked_until {
            Some(until) if now < until => until - now,
            _ => Duration::ZERO,
        }
    }

    fn fail(&mut self, key: Vec<u8>, threshold: i64, now: Instant) {
        let known = self
            .attempts
            .get(&key)
            .is_some_and(|entry| !forgotten(entry, now));

        if !known {
            self.make_room(now);
            self.attempts.insert(
                key.clone(),
                LoginAttempts {
                    failures: 0,
                    last_failure: now,
                    locked_until: None,
                },
            );
        }

        let Some(entry) = self.attempts.get_mut(&key) else {
            return;
        };

        entry.failures += 1;
        entry.last_failure = now;

        if entry.failures >= threshold {
            entry.locked_until = Some(now + lockout_duration(entry.failures - threshold));
        }
    }

    fn make_room(&mut self, now: Instant) {
        if self.attempts.len() < LOGIN_GUARD_MAX_ENTRIES {
            return;
        }

        self.attempts.retain(|_, entry| !forgotten(entry, now));

        if self.attempts.len() < LOGIN_GUARD_MAX_ENTRIES {
            return;
        }

        let mut oldest: Option<(&Vec<u8>, Instant)> = None;
        for (key, entry) in &self.attempts {
            if locked_at(entry, now) {
                continue;
            }

            if oldest.is_none_or(|(_, last)| entry.last_failure < last) {
                oldest = Some((key, entry.last_failure));
            }
        }

        if let Some(key) = oldest.map(|(key, _)| key.clone()) {
            self.attempts.remove(&key);
        }
    }
}

pub(crate) fn retry_after(source: LoginSource, username: &[u8]) -> Duration {
    let mut guard = DEFAULT_LOGIN_GUARD.lock();

    let now = guard.now();
    let wait = guard.lock_remaining(&client_login_key(source), now);
    let user_wait = guard.lock_remaining(&client_user_login_key(source, username), now);

    wait.max(user_wait)
}

pub(crate) fn record_failure(source: LoginSource, username: &[u8]) {
    let mut guard = DEFAULT_LOGIN_GUARD.lock();

    let now = guard.now();
    guard.fail(client_login_key(source), LOGIN_FAILURES_PER_CLIENT, now);
    guard.fail(
        client_user_login_key(source, username),
        LOGIN_FAILURES_PER_CLIENT_AND_USER,
        now,
    );
}

pub(crate) fn record_success(source: LoginSource, username: &[u8]) {
    DEFAULT_LOGIN_GUARD
        .lock()
        .attempts
        .remove(&client_user_login_key(source, username));
}
