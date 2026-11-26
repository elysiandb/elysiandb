use std::time::{Duration as StdDuration, Instant};

use crate::backup;
use crate::boot;
use crate::globals;
use crate::gocompat::time::Duration;
use crate::hook;
use crate::kv;
use crate::log::{self, Logger};
use crate::metrics::{self, Moment};
use crate::security;
use crate::storage;
use crate::sync::{block_shutdown_signals, spawn};

const SERVER_LOG: Logger = log::for_component("boot");

const SHUTDOWN_DRAIN_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const SHUTDOWN_CLOSE_TIMEOUT: StdDuration = StdDuration::from_secs(3);
const READY_WITHIN: StdDuration = StdDuration::from_secs(5 * 60);
const READY_POLL: StdDuration = StdDuration::from_millis(100);

fn since_started() -> Duration {
    Moment::now().sub(&metrics::started_at())
}

fn announce_ready() {
    let deadline = Instant::now() + READY_WITHIN;
    while !hook::views_ready() && Instant::now() < deadline {
        std::thread::sleep(READY_POLL);
    }

    let took = since_started();
    metrics::set_startup(StdDuration::from_nanos(took.0.max(0) as u64));
    SERVER_LOG.info("ready to serve", kv!["startup_ms" => took.milliseconds()]);
    globals::set_starting(false);
}

fn shutdown_signals() -> libc::sigset_t {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        set
    }
}

fn wait_for_shutdown_signal() -> &'static str {
    block_shutdown_signals();
    let set = shutdown_signals();
    let mut sig: libc::c_int = 0;
    loop {
        if unsafe { libc::sigwait(&set, &mut sig) } == 0 {
            break;
        }
    }

    unsafe {
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }

    if sig == libc::SIGTERM {
        "terminated"
    } else {
        "interrupt"
    }
}

pub fn start_server() {
    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };

    if cfg.stats.enabled {
        boot::boot_stats();
    }

    boot::warn_if_authentication_is_off_on_public_address();
    security::warn_if_default_admin_password();

    globals::set_starting(true);
    drop(spawn("announce-ready", announce_ready));

    if cfg.server.tcp.enabled {
        drop(spawn("tcp-listener", boot::init_tcp));
    }

    boot::boot_monitoring();
    let stop_backups = backup::start_periodic();

    if cfg.admin_ui.enabled && cfg.server.http.enabled {
        let url = format!(
            "http://{}:{}/admin",
            cfg.server.http.host, cfg.server.http.port
        );
        SERVER_LOG.info("admin ui available", kv!["url" => url]);
    }

    let received = wait_for_shutdown_signal();

    SERVER_LOG.info("shutdown requested", kv!["signal" => received]);

    stop_backups();

    let start = Instant::now();
    boot::stop_http(SHUTDOWN_DRAIN_TIMEOUT);
    boot::stop_tcp(SHUTDOWN_DRAIN_TIMEOUT.saturating_sub(start.elapsed()));
    SERVER_LOG.info("requests drained", kv!["duration_ms" => log::since(start)]);

    let start = Instant::now();
    match storage::close_within(SHUTDOWN_CLOSE_TIMEOUT) {
        Err(err) => SERVER_LOG.error(
            "cannot close the data folder cleanly",
            kv!["err" => log::display(&err)],
        ),
        Ok(()) => SERVER_LOG.info(
            "data flushed on shutdown",
            kv!["duration_ms" => log::since(start)],
        ),
    }

    metrics::stop();

    let uptime = since_started().round(Duration::SECOND).to_string();
    SERVER_LOG.info("shutdown complete", kv!["uptime" => uptime]);
}
