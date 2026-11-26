use crate::storage;

pub fn handle_reset() -> Vec<u8> {
    storage::reset_store();
    b"OK".to_vec()
}
