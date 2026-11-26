use crate::globals;
use crate::metrics;
use crate::stat;
use crate::storage;
use crate::wildcard;

pub(super) fn stats_enabled() -> bool {
    match globals::config() {
        Some(cfg) => cfg.stats.enabled,
        None => panic!("the configuration is not set"),
    }
}

pub(super) fn record_hit() {
    metrics::kv_hit();
    if stats_enabled() {
        stat::stats().increment_hits();
    }
}

pub(super) fn record_miss() {
    metrics::kv_miss();
    if stats_enabled() {
        stat::stats().increment_misses();
    }
}

pub(super) fn not_found(key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(key.len() + 10);
    out.extend_from_slice(key);
    out.extend_from_slice(b"=not found");
    out
}

pub(super) fn wildcard_lines(pattern: &[u8]) -> Option<Vec<u8>> {
    let data = storage::get_by_wildcard_key(pattern);
    if data.is_empty() {
        record_miss();
        return None;
    }

    let mut result = Vec::new();
    for (k, v) in &data {
        result.extend_from_slice(k);
        result.push(b'=');
        result.extend_from_slice(v);
        result.push(b'\n');

        record_hit();
    }

    if !result.is_empty() {
        result.pop();
    }

    Some(result)
}

pub(super) fn single_value(key: &[u8]) -> Option<Vec<u8>> {
    if storage::key_has_expired(key) {
        storage::delete_by_key(key);
        record_miss();
        return None;
    }

    match storage::get_by_key(key) {
        Ok(data) => {
            record_hit();
            Some(data)
        }
        Err(_) => {
            record_miss();
            None
        }
    }
}

pub fn handle_get(query: &[u8]) -> Vec<u8> {
    if wildcard::key_contains_wildcard(query) {
        return handle_wildcard_key(query);
    }

    handle_single_key(query)
}

fn handle_wildcard_key(query: &[u8]) -> Vec<u8> {
    match wildcard_lines(query) {
        Some(result) => result,
        None => not_found(query),
    }
}

fn handle_single_key(key: &[u8]) -> Vec<u8> {
    match single_value(key) {
        Some(data) => {
            let mut out = Vec::with_capacity(key.len() + 1 + data.len());
            out.extend_from_slice(key);
            out.push(b'=');
            out.extend_from_slice(&data);
            out
        }
        None => not_found(key),
    }
}
