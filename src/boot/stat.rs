use std::time::Duration;

use crate::stat;
use crate::sync::spawn;

pub fn boot_stats() {
    stat::init();
    drop(spawn("stats-uptime", collect));
}

fn collect() {
    loop {
        stat::stats().increment_uptime_seconds();
        std::thread::sleep(Duration::from_secs(1));
    }
}
