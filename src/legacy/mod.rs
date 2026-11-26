#[allow(clippy::module_inception)]
pub mod legacy;

pub use legacy::{
    API_KEY_PREFIX, DATA_FILE, Data, Document, EXPIRATION_FILE, Error, FAILED_SUFFIX, FILES,
    JSON_DATA_FILE, JSON_RECOVERY_FILE, STORE_RECOVERY_FILE, TYPES_LIST_KEY, document_key,
    files_in, id_index_key, present, read, read_documents_file, read_expirations_file,
    read_kv_file, write,
};
