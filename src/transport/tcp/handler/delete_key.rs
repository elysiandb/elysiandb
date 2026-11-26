use crate::storage;
use crate::wildcard;

pub fn handle_delete(query: &[u8]) -> Vec<u8> {
    let count = if wildcard::key_contains_wildcard(query) {
        handle_wildcard_key_delete(query)
    } else {
        handle_single_key_delete(query)
    };

    format!("Deleted {count}").into_bytes()
}

fn handle_single_key_delete(key: &[u8]) -> usize {
    storage::delete_by_key(key);
    1
}

fn handle_wildcard_key_delete(pattern: &[u8]) -> usize {
    storage::delete_by_wildcard_key(pattern)
}
