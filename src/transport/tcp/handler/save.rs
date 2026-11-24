use crate::storage;

pub fn handle_save() -> Vec<u8> {
    storage::write_to_db();
    b"OK".to_vec()
}
