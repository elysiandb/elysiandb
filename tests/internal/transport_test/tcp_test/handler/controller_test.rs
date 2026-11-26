use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};
use elysiandb::transport::tcp::handler;

use super::tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

struct Setup {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn setup() -> Setup {
    let serial = SERIAL.lock();
    let dir = TempDir::new();

    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.stats.enabled = false;
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::reset_store();

    Setup {
        _dir: dir,
        _serial: serial,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn handle_set_and_get() {
    let _s = setup();

    if handler::handle_set(b"a 1", 0) != b"OK" {
        panic!("set failed");
    }

    let out = handler::handle_get(b"a");
    if out != b"a=1" {
        panic!("get failed");
    }
}

#[test]
fn handle_set_with_ttl() {
    let _s = setup();

    if handler::handle_set(b"ttl 123", 1) != b"OK" {
        panic!("set ttl failed");
    }

    let out = handler::handle_get(b"ttl");
    if out != b"ttl=123" {
        panic!("get ttl failed");
    }
}

#[test]
fn handle_get_not_found() {
    let _s = setup();

    let out = handler::handle_get(b"missing");
    if out != b"missing=not found" {
        panic!("should not exist");
    }
}

#[test]
fn handle_delete_single() {
    let _s = setup();

    handler::handle_set(b"x 9", 0);

    let out = handler::handle_delete(b"x");
    if out != b"Deleted 1" {
        panic!("delete failed");
    }

    let out = handler::handle_get(b"x");
    if out != b"x=not found" {
        panic!("delete ineffective");
    }
}

#[test]
fn handle_reset() {
    let _s = setup();

    handler::handle_set(b"a 1", 0);
    handler::handle_reset();

    let out = handler::handle_get(b"a");
    if out != b"a=not found" {
        panic!("reset failed");
    }
}

#[test]
fn handle_save() {
    let _s = setup();

    handler::handle_set(b"a 1", 0);

    let out = handler::handle_save();
    if out != b"OK" {
        panic!("save failed");
    }
}

#[test]
fn handle_multi_get() {
    let _s = setup();

    handler::handle_set(b"a 1", 0);
    handler::handle_set(b"b 2", 0);

    let out = handler::handle_multi_get(b"a b c");
    if !contains(&out, b"1") || !contains(&out, b"2") || !contains(&out, b"c=not found") {
        panic!("mget failed");
    }
}

#[test]
fn handle_wildcard_get() {
    let _s = setup();

    handler::handle_set(b"user:1 a", 0);
    handler::handle_set(b"user:2 b", 0);

    let out = handler::handle_get(b"user:*");
    if !contains(&out, b"user:1") || !contains(&out, b"user:2") {
        panic!("wildcard get failed");
    }
}

#[test]
fn handle_wildcard_delete() {
    let _s = setup();

    handler::handle_set(b"wild:1 a", 0);
    handler::handle_set(b"wild:2 b", 0);

    let out = handler::handle_delete(b"wild:*");
    if !out.starts_with(b"Deleted ") {
        panic!("wildcard delete wrong prefix");
    }

    let out2 = handler::handle_get(b"wild:*");
    if !contains(&out2, b"not found") {
        panic!("wildcard delete ineffective");
    }
}

#[test]
fn handle_multi_get_wildcard() {
    let _s = setup();

    handler::handle_set(b"a:1 x", 0);
    handler::handle_set(b"a:2 y", 0);

    let out = handler::handle_multi_get(b"a:*");
    if !contains(&out, b"a:1") || !contains(&out, b"a:2") {
        panic!("mget wildcard failed");
    }
}
