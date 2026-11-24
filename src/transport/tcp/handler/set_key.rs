use crate::kv;
use crate::log;
use crate::storage;
use crate::transport::tcp::parsing;

pub fn handle_set(query: &[u8], ttl: i64) -> Vec<u8> {
    let (key, val) = parsing::first_word_bytes(query);

    let result = if ttl > 0 {
        storage::put_key_value_with_ttl(key, val, ttl)
    } else {
        storage::put_key_value(key, val)
    };

    if let Err(err) = result {
        log::error(
            "set failed",
            kv!["component" => "tcp", "err" => log::display(&err)],
        );
        return b"ERR".to_vec();
    }

    b"OK".to_vec()
}
