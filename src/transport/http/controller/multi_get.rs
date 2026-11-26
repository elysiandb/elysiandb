use std::collections::BTreeSet;

use crate::globals;
use crate::gocompat::json;
use crate::gocompat::strings;
use crate::httpd::RequestCtx;
use crate::metrics;
use crate::stat;
use crate::storage;
use crate::wildcard;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiGetEntry {
    pub key: Vec<u8>,
    pub val: Option<Vec<u8>>,
}

impl MultiGetEntry {
    pub(super) fn append_json(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"{\"key\":");
        json::append_string(out, &self.key, true);
        out.extend_from_slice(b",\"value\":");
        match &self.val {
            Some(v) => json::append_string(out, v, true),
            None => out.extend_from_slice(b"null"),
        }
        out.push(b'}');
    }
}

pub(super) fn marshal_entries(entries: &[MultiGetEntry]) -> Vec<u8> {
    let mut out = vec![b'['];
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        e.append_json(&mut out);
    }
    out.push(b']');
    out
}

pub(super) fn stats_enabled() -> bool {
    globals::config().is_some_and(|cfg| cfg.stats.enabled)
}

pub fn multi_get_controller(ctx: &mut RequestCtx) {
    ctx.set_content_type("application/json; charset=utf-8");

    let raw = strings::trim_space(ctx.query_args().peek("keys")).to_vec();
    if raw.is_empty() {
        ctx.write_string("[]");
        return;
    }

    let keys: Vec<Vec<u8>> = raw
        .split(|&b| b == b',')
        .map(strings::trim_space)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect();

    if keys.is_empty() {
        ctx.write_string("[]");
        return;
    }

    let mut results: Vec<MultiGetEntry> = Vec::with_capacity(keys.len());
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();

    for key in keys {
        if wildcard::key_contains_wildcard(&key) {
            handle_mget_wildcard_key(&key, &mut results, &mut seen);
        } else {
            handle_mget_single_key(key, &mut results, &mut seen);
        }
    }

    let json_data = marshal_entries(&results);
    let _ = std::io::Write::write_all(ctx, &json_data);
}

fn handle_mget_single_key(
    key: Vec<u8>,
    results: &mut Vec<MultiGetEntry>,
    seen: &mut BTreeSet<Vec<u8>>,
) {
    if seen.contains(&key) {
        return;
    }

    seen.insert(key.clone());

    if storage::key_has_expired(&key) {
        storage::delete_by_key(&key);
        results.push(MultiGetEntry { key, val: None });

        metrics::kv_miss();
        if stats_enabled() {
            stat::stats().increment_misses();
        }

        return;
    }

    let Ok(data) = storage::get_by_key(&key) else {
        results.push(MultiGetEntry { key, val: None });

        metrics::kv_miss();
        if stats_enabled() {
            stat::stats().increment_misses();
        }

        return;
    };

    results.push(MultiGetEntry {
        key,
        val: Some(data),
    });

    metrics::kv_hit();
    if stats_enabled() {
        stat::stats().increment_hits();
    }
}

fn handle_mget_wildcard_key(
    pattern: &[u8],
    results: &mut Vec<MultiGetEntry>,
    seen: &mut BTreeSet<Vec<u8>>,
) {
    let data = storage::get_by_wildcard_key(pattern);
    for (k, v) in data {
        if seen.contains(&k) {
            continue;
        }

        seen.insert(k.clone());

        results.push(MultiGetEntry {
            key: k,
            val: Some(v),
        });

        metrics::kv_hit();
        if stats_enabled() {
            stat::stats().increment_hits();
        }
    }
}
