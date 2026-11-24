use crate::transport::tcp::parsing;
use crate::wildcard;

use super::get_key::{not_found, single_value, wildcard_lines};

pub fn handle_multi_get(query: &[u8]) -> Vec<u8> {
    let keys: Vec<&[u8]> = query.split(|&b| b == b' ').collect();
    if keys.is_empty() {
        return b"ERR".to_vec();
    }

    let mut results = vec![Vec::new(); keys.len()];
    for (i, key) in keys.iter().enumerate() {
        if wildcard::key_contains_wildcard(key) {
            handle_mget_wildcard_key(key, i, &mut results);
        } else {
            handle_mget_single_key(key, i, &mut results);
        }
    }

    parsing::join_byte_slices(&results, b"\n")
}

pub fn handle_mget_single_key(key: &[u8], i: usize, results: &mut [Vec<u8>]) {
    results[i] = match single_value(key) {
        Some(data) => data,
        None => not_found(key),
    };
}

pub fn handle_mget_wildcard_key(key: &[u8], i: usize, results: &mut [Vec<u8>]) {
    results[i] = match wildcard_lines(key) {
        Some(result) => result,
        None => not_found(key),
    };
}
