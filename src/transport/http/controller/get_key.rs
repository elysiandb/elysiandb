use std::io::Write;

use crate::gocompat::url;
use crate::httpd::{RequestCtx, STATUS_NOT_FOUND};
use crate::metrics;
use crate::stat;
use crate::storage;
use crate::wildcard;

use super::multi_get::{MultiGetEntry, marshal_entries, stats_enabled};

pub type GetEntry = MultiGetEntry;

fn marshal_entry(entry: &GetEntry) -> Vec<u8> {
    let mut out = Vec::new();
    entry.append_json(&mut out);
    out
}

pub fn get_key_controller(ctx: &mut RequestCtx) {
    ctx.set_content_type("text/plain; charset=utf-8");

    let mut key = ctx.param("key").to_vec();

    if let Ok(dec) = url::path_unescape(&key) {
        key = dec;
    }

    if wildcard::key_contains_wildcard(&key) {
        handle_wildcard_key(key, ctx);
    } else {
        handle_single_key(key, ctx);
    }

    ctx.response.header.set("Content-Type", "application/json");
}

fn handle_single_key(key: Vec<u8>, ctx: &mut RequestCtx) {
    if storage::key_has_expired(&key) {
        storage::delete_by_key(&key);

        metrics::kv_miss();
        if stats_enabled() {
            stat::stats().increment_misses();
        }

        let json_data = marshal_entry(&GetEntry { key, val: None });
        let _ = ctx.write_all(&json_data);
        ctx.set_status_code(STATUS_NOT_FOUND);

        return;
    }

    let Ok(data) = storage::get_by_key(&key) else {
        metrics::kv_miss();
        if stats_enabled() {
            stat::stats().increment_misses();
        }

        let json_data = marshal_entry(&GetEntry { key, val: None });
        let _ = ctx.write_all(&json_data);
        ctx.set_status_code(STATUS_NOT_FOUND);

        return;
    };

    metrics::kv_hit();
    if stats_enabled() {
        stat::stats().increment_hits();
    }

    let json_data = marshal_entry(&GetEntry {
        key,
        val: Some(data),
    });

    let _ = ctx.write_all(&json_data);
}

fn handle_wildcard_key(key: Vec<u8>, ctx: &mut RequestCtx) {
    let mut results: Vec<MultiGetEntry> = Vec::new();
    let data = storage::get_by_wildcard_key(&key);

    if data.is_empty() {
        metrics::kv_miss();
        if stats_enabled() {
            stat::stats().increment_misses();
        }

        results.push(MultiGetEntry { key, val: None });

        let json_data = marshal_entries(&results);
        let _ = ctx.write_all(&json_data);

        return;
    }

    for (k, v) in data {
        results.push(MultiGetEntry {
            key: k,
            val: Some(v),
        });

        metrics::kv_hit();
        if stats_enabled() {
            stat::stats().increment_hits();
        }
    }

    let json_data = marshal_entries(&results);
    let _ = ctx.write_all(&json_data);
}
