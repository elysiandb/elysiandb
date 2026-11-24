use crate::configuration::Config;
use arc_swap::ArcSwapOption;
use std::sync::Arc;

static CONFIG: ArcSwapOption<Config> = ArcSwapOption::const_empty();

pub fn set_config(c: Option<Arc<Config>>) {
    CONFIG.store(c);
}

pub fn config() -> Option<Arc<Config>> {
    CONFIG.load_full()
}

pub fn engine() -> String {
    let Some(c) = config() else {
        panic!("the configuration is not set");
    };
    if c.engine.name.is_empty() {
        return "internal".to_string();
    }

    c.engine.name.clone()
}
