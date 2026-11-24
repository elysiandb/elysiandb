#![allow(dead_code)]

use gocompat::time::{Location, load_location, set_local};
use std::process::Command;

pub const ZONE_VAR: &str = "GOJA_TZ_CHILD";

pub enum Zone {
    Env(&'static str),
    Load(&'static str),
    Fixed(&'static str, i32),
}

impl Zone {
    fn key(&self) -> String {
        match self {
            Zone::Env(n) => format!("env:{n}"),
            Zone::Load(n) => format!("load:{n}"),
            Zone::Fixed(n, o) => format!("fixed:{n}:{o}"),
        }
    }
}

pub fn is_child() -> bool {
    std::env::var(ZONE_VAR).is_ok()
}

pub fn run_in_zone(test: &str, zone: Zone, body: impl FnOnce()) {
    let key = zone.key();
    if let Ok(k) = std::env::var(ZONE_VAR) {
        if k != key {
            return;
        }
        match zone {
            Zone::Env(n) => assert_eq!(std::env::var("TZ").as_deref(), Ok(n)),
            Zone::Load(n) => {
                let loc = load_location(n).unwrap_or_else(|e| panic!("{n}: {e}"));
                assert!(set_local(&loc), "the local zone was already initialised");
            }
            Zone::Fixed(n, o) => {
                assert!(
                    set_local(&Location::fixed(n, o)),
                    "the local zone was already initialised"
                );
            }
        }
        body();
        return;
    }
    let exe = std::env::current_exe().expect("current exe");
    let mut cmd = Command::new(exe);
    cmd.args([
        "--exact",
        test,
        "--nocapture",
        "--include-ignored",
        "--test-threads=1",
    ])
    .env(ZONE_VAR, &key);
    if let Zone::Env(n) = zone {
        cmd.env("TZ", n);
    }
    let out = cmd.output().expect("spawn the test binary");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{test} in {key}:\n{text}");
    assert!(
        text.contains("1 passed"),
        "{test} did not run in {key}:\n{text}"
    );
}
