use super::types::{StatsContainer, new_stats_container};

pub static STATS: StatsContainer = StatsContainer::empty();

pub fn stats() -> &'static StatsContainer {
    &STATS
}

pub fn init() {
    STATS.replace(new_stats_container());
}
