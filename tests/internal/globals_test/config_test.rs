use std::sync::Arc;
use std::thread;
use std::time::Duration;

use elysiandb::configuration::{
    Config, LogConfig, ServerConfig, ServersConfig, StatsConfig, StoreConfig,
};
use elysiandb::globals;

use super::serial;

fn store(folder: &str, shards: i64) -> Arc<Config> {
    Arc::new(Config {
        store: StoreConfig {
            folder: folder.to_string(),
            shards,
            ..StoreConfig::default()
        },
        ..Config::default()
    })
}

#[test]
fn test_get_config_when_unset_is_nil() {
    let _serial = serial();
    globals::set_config(None);

    if let Some(got) = globals::config() {
        panic!("GetConfig() = {got:?}, want <nil>");
    }
}

#[test]
fn test_set_then_get_config_same_pointer_and_values() {
    let _serial = serial();
    globals::set_config(None);

    let want = Arc::new(Config {
        store: StoreConfig {
            folder: "tmp-folder".to_string(),
            shards: 16,
            ..StoreConfig::default()
        },
        server: ServersConfig {
            http: ServerConfig {
                enabled: true,
                host: "127.0.0.1".to_string(),
                port: 9090,
                ..ServerConfig::default()
            },
            tcp: ServerConfig {
                enabled: false,
                host: "0.0.0.0".to_string(),
                port: 8088,
                ..ServerConfig::default()
            },
        },
        log: LogConfig {
            flush_interval_seconds: 5,
            ..LogConfig::default()
        },
        stats: StatsConfig { enabled: true },
        ..Config::default()
    });

    globals::set_config(Some(want.clone()));
    let got = globals::config().expect("GetConfig() returned nil");

    assert!(
        Arc::ptr_eq(&got, &want),
        "GetConfig() returned different pointer: got={:p} want={:p}",
        Arc::as_ptr(&got),
        Arc::as_ptr(&want)
    );
    assert!(
        got.store.shards == 16 && got.server.http.port == 9090 && got.stats.enabled,
        "unexpected values: {got:?}"
    );
}

#[test]
fn test_concurrent_set_and_get_no_panics_final_is_known_value() {
    let _serial = serial();
    globals::set_config(None);

    let c1 = store("a", 4);
    let c2 = store("b", 8);
    let c3 = store("c", 12);
    let known = [c1.clone(), c2.clone(), c3.clone()];

    let mut handles = Vec::new();

    handles.push(thread::spawn(move || {
        for _ in 0..1000 {
            globals::set_config(Some(c1.clone()));
            globals::set_config(Some(c2.clone()));
            globals::set_config(Some(c3.clone()));
        }
    }));

    let readers = 4;
    for _ in 0..readers {
        handles.push(thread::spawn(|| {
            for _ in 0..2000 {
                let _ = globals::config();
                thread::sleep(Duration::from_micros(1));
            }
        }));
    }

    for h in handles {
        h.join().expect("no panic");
    }

    let Some(fin) = globals::config() else {
        panic!("final GetConfig() returned nil, want one of known configs");
    };
    let is_known = known.iter().any(|k| Arc::ptr_eq(&fin, k));
    assert!(
        is_known,
        "final config pointer {:p} is not one of known values {:p}/{:p}/{:p}",
        Arc::as_ptr(&fin),
        Arc::as_ptr(&known[0]),
        Arc::as_ptr(&known[1]),
        Arc::as_ptr(&known[2])
    );
}
