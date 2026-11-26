pub fn key_contains_wildcard(key: &[u8]) -> bool {
    key.iter().any(|&c| c == b'*' || c == b'?')
}
