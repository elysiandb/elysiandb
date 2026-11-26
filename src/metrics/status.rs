use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::Ordering;

use crate::globals;
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::sort;
use crate::gocompat::strings;
use crate::gocompat::time::{Duration, Time};
use crate::sync::Mutex;

use super::counters::{
    AUTH_FAILURES, C_HTTP, C_HTTP5XX, HOOK_FAILURE, RECOVERY_STATE, REPLAYED, flush_failing_since,
    started_at,
};
use super::sampler::{self, Moment, Snapshot};
use super::series::{Series, round};

pub const SEVERITY_CRITICAL: &str = "critical";
pub const SEVERITY_WARNING: &str = "warning";
pub const SEVERITY_INFO: &str = "info";

pub const STATUS_HEALTHY: &str = "healthy";
pub const STATUS_WARNING: &str = "warning";
pub const STATUS_CRITICAL: &str = "critical";

const RECENT_FAILURE_WINDOW: Duration = Duration(5 * 60 * 1_000_000_000);
const MINIMUM_WINDOW_REQUESTS: f64 = 20.0;
const ERROR_RATE_THRESHOLD: f64 = 0.01;
const AUTH_FAILURE_THRESHOLD: i64 = 20;
const INDEX_BACKLOG_DELAY: Duration = Duration::MINUTE;
const OLD_TRANSACTION_AGE: Duration = Duration(10 * 60 * 1_000_000_000);
const UNCLEAN_SHUTDOWN_NOTICE: Duration = Duration::HOUR;

#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    Float(f64),
    Int(i64),
    Str(Vec<u8>),
}

impl Encode for Detail {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        match self {
            Detail::Float(f) => e.write_f64(*f)?,
            Detail::Int(n) => e.write_i64(*n),
            Detail::Str(s) => e.write_string(s),
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub code: &'static str,
    pub severity: &'static str,
    pub since: Option<Time>,
    pub details: BTreeMap<&'static str, Detail>,
}

impl Problem {
    fn new(code: &'static str, severity: &'static str) -> Problem {
        Problem {
            code,
            severity,
            since: None,
            details: BTreeMap::new(),
        }
    }

    fn since(mut self, since: Time) -> Problem {
        self.since = Some(since);
        self
    }

    fn detail(mut self, key: &'static str, value: Detail) -> Problem {
        self.details.insert(key, value);
        self
    }
}

impl Encode for Problem {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "code");
        e.write_string(self.code.as_bytes());
        e.write_field_name(&mut first, "severity");
        e.write_string(self.severity.as_bytes());
        e.write_field_name(&mut first, "since");
        let zero = Time::default();
        super::counters::encode_time(self.since.as_ref().unwrap_or(&zero), e)?;
        if !self.details.is_empty() {
            e.write_field_name(&mut first, "details");
            let mut inner = true;
            for (k, v) in &self.details {
                e.write_field_name(&mut inner, k);
                v.encode(e)?;
            }
            e.end_struct(inner);
        }
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    pub level: &'static str,
    pub problems: Vec<Problem>,
}

impl Encode for Status {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "level");
        e.write_string(self.level.as_bytes());
        e.write_field_name(&mut first, "problems");
        self.problems.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

static ACTIVE: Mutex<BTreeMap<&'static str, Problem>> = Mutex::new(BTreeMap::new());

pub(super) fn reset_problems() {
    *ACTIVE.lock() = BTreeMap::new();
}

pub(super) fn update(now: &Moment, found: Vec<Problem>) {
    let mut active = ACTIVE.lock();

    let mut next = BTreeMap::new();
    for mut p in found {
        if let Some(prev) = active.get(p.code) {
            p.since = prev.since.clone();
        } else if p.since.is_none() {
            p.since = Some(now.wall.clone());
        }
        next.insert(p.code, p);
    }

    *active = next;
}

fn severity_order(severity: &str) -> i32 {
    match severity {
        SEVERITY_CRITICAL => 0,
        SEVERITY_WARNING => 1,
        SEVERITY_INFO => 2,
        _ => 0,
    }
}

pub fn current_status() -> Status {
    let active = ACTIVE.lock();

    let mut status = Status {
        level: STATUS_HEALTHY,
        problems: Vec::with_capacity(active.len()),
    };
    for p in active.values() {
        status.problems.push(p.clone());
        match p.severity {
            SEVERITY_CRITICAL => status.level = STATUS_CRITICAL,
            SEVERITY_WARNING => {
                if status.level == STATUS_HEALTHY {
                    status.level = STATUS_WARNING;
                }
            }
            _ => {}
        }
    }

    sort::slice(&mut status.problems, |a, b| {
        if severity_order(a.severity) != severity_order(b.severity) {
            return severity_order(a.severity) < severity_order(b.severity);
        }
        a.code < b.code
    });

    status
}

pub(super) fn evaluate(now: &Moment) -> Vec<Problem> {
    let cfg = globals::config();
    let mut problems = Vec::new();

    if let Some(since) = flush_failing_since() {
        problems.push(Problem::new("flush_failing", SEVERITY_CRITICAL).since(since));
    }

    if let (Some(at), _) = RECOVERY_STATE.lock().get()
        && now.sub(&at) < RECENT_FAILURE_WINDOW
    {
        problems.push(Problem::new("recovery_write_failing", SEVERITY_CRITICAL));
    }

    let state = sampler::evaluation(RECENT_FAILURE_WINDOW);
    let cur = state.window.as_ref().map(|(_, cur)| cur.clone());

    if let Some((prev, cur)) = &state.window {
        let requests = counter_delta(prev, cur, C_HTTP);
        let failures = counter_delta(prev, cur, C_HTTP5XX);
        if requests >= MINIMUM_WINDOW_REQUESTS && failures / requests > ERROR_RATE_THRESHOLD {
            problems.push(
                Problem::new("error_rate_high", SEVERITY_WARNING)
                    .detail("error_rate", Detail::Float(round(failures / requests))),
            );
        }

        if let Some(cfg) = &cfg
            && cfg.log.slow_request_ms > 0
        {
            let latency = (cur.http - prev.http) + (cur.tcp - prev.tcp);
            if latency.total() as f64 >= MINIMUM_WINDOW_REQUESTS {
                let p95 = latency.quantile(0.95);
                if p95 > cfg.log.slow_request_ms as f64 {
                    problems.push(
                        Problem::new("latency_high", SEVERITY_WARNING)
                            .detail("p95_ms", Detail::Float(round(p95))),
                    );
                }
            }
        }
    }

    if let (Some(at), hook_id) = HOOK_FAILURE.lock().get()
        && now.sub(&at) < RECENT_FAILURE_WINDOW
    {
        problems.push(
            Problem::new("hook_failing", SEVERITY_WARNING).detail("hook_id", Detail::Str(hook_id)),
        );
    }

    let (addr, count) = AUTH_FAILURES.lock().worst();
    if count > AUTH_FAILURE_THRESHOLD {
        problems.push(
            Problem::new("auth_failures", SEVERITY_WARNING)
                .detail("remote_addr", Detail::Str(addr))
                .detail("per_minute", Detail::Int(count)),
        );
    }

    if let Some(from) = &state.backlog_from
        && now.sub(from) > INDEX_BACKLOG_DELAY
    {
        problems.push(Problem::new("index_backlog", SEVERITY_WARNING).since(from.wall.clone()));
    }

    if let Some(cur) = &cur {
        let oldest = cur.gauge_or_zero(Series::OldestTransaction);
        if oldest > OLD_TRANSACTION_AGE.seconds() {
            problems.push(
                Problem::new("old_transaction", SEVERITY_WARNING)
                    .detail("seconds", Detail::Int(oldest as i64)),
            );
        }
    }

    if state.slow.default_admin_pw {
        problems.push(Problem::new("default_admin_password", SEVERITY_WARNING));
    }

    if let Some(cfg) = &cfg
        && authentication_off_on_public_address(
            cfg.security.authentication.enabled,
            &public_listeners(),
        )
    {
        problems.push(Problem::new("auth_disabled_public", SEVERITY_WARNING));
    }

    let entries = REPLAYED.load(Ordering::SeqCst);
    let started = started_at();
    if entries > 0 && now.sub(&started) < UNCLEAN_SHUTDOWN_NOTICE {
        problems.push(
            Problem::new("unclean_shutdown", SEVERITY_INFO)
                .since(started.wall.clone())
                .detail("entries", Detail::Int(entries)),
        );
    }

    problems
}

pub(super) fn counter_delta(prev: &Snapshot, cur: &Snapshot, c: usize) -> f64 {
    if cur.counters[c] < prev.counters[c] {
        return 0.0;
    }

    (cur.counters[c] - prev.counters[c]) as f64
}

fn public_listeners() -> Vec<String> {
    let Some(cfg) = globals::config() else {
        return Vec::new();
    };

    let mut hosts = Vec::new();
    if cfg.server.http.enabled {
        hosts.push(cfg.server.http.host.clone());
    }
    if cfg.server.tcp.enabled {
        hosts.push(cfg.server.tcp.host.clone());
    }

    hosts
}

pub fn authentication_off_on_public_address<S: AsRef<[u8]>>(
    auth_enabled: bool,
    hosts: &[S],
) -> bool {
    if auth_enabled {
        return false;
    }

    hosts.iter().any(|h| !is_loopback_host(h))
}

pub fn is_loopback_host(host: impl AsRef<[u8]>) -> bool {
    let mut host = strings::trim_space(host.as_ref());
    while let [b'[' | b']', rest @ ..] = host {
        host = rest;
    }
    while let [rest @ .., b'[' | b']'] = host {
        host = rest;
    }

    if strings::equal_fold(host, b"localhost") {
        return true;
    }

    let Ok(text) = std::str::from_utf8(host) else {
        return false;
    };

    text.parse::<IpAddr>()
        .is_ok_and(|ip| ip.to_canonical().is_loopback())
}
