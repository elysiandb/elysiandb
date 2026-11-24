mod advisor;
mod backlog;
mod bitmap;
mod build;
mod cache;
mod changes;
mod checkpoint;
mod compact;
mod composite;
mod dirty;
#[allow(clippy::module_inception)]
mod engine;
mod entity;
mod indexkey;
mod indexmeta;
mod kv;
mod migrate;
mod pointintime;
mod release;
mod trigram;
mod tx;

pub use advisor::{
    ADVISOR_FILL_SHARE, ADVISOR_MIN_DOCS, BITMAP_MAX_DISTINCT, ORDERED_MIN_DISTINCT,
    PRESSURE_SWEEPS,
};
pub use backlog::{BULK_BATCH_RECORDS, IndexOp, OpKind, apply_ops, change_ops};
pub use bitmap::{bitmap_key, posting_count};
pub use build::{INDEX_IDLE_AFTER, SMALL_ENTITY_DOCS};
pub use changes::stamp_slot;
pub use checkpoint::{
    CheckpointCorrupt, CheckpointData, CkEntity, CkExtras, CkIndex, CkKv, LEGACY_CHECKPOINTS,
    MANIFEST_FORMAT, MANIFEST_FORMAT_PAGES, Manifest, ManifestEntity, ManifestIndex, PageRoot,
    UnsupportedCheckpoint, decode_checkpoint, parse_manifest, read_checkpoint,
};
pub use compact::EngineStats;
pub use composite::{
    IRREGULAR_KEY, add_irregular, composite_keys, composite_prefix, composite_spec, irregular_count,
};
pub use engine::{
    Doc, ERR_CLOSED, ERR_CONFLICT, ERR_DISK_FULL, ERR_LOCKED, ERR_WRITE_FAILED, Engine, Error,
    FreeSpaceProbe, IndexView, KvExpiredHook, LOCK_FILE, LOG_FOLDER, MANIFEST_FILE, NotAnObject,
    Options, Snapshot, StopToken, is_error, open, prefix_end,
};
pub use entity::{Slot, decode_doc, decode_map, decode_slot_value, encode_slot};
pub use indexkey::{
    INDEX_BITMAP, INDEX_COMPOSITE, INDEX_FILTER, INDEX_REFS, INDEX_SEARCH, INDEX_SORT, IndexKind,
    IndexSpec, KEY_BOOL, KEY_DATE, KEY_ELEMENT, KEY_LOWER, KEY_NUMBER, KEY_PRESENT, KEY_RAW,
    RefPath, SORT_RANK_BOOL, SORT_RANK_MISSING, SORT_RANK_NUMBER, SORT_RANK_OTHER,
    SORT_RANK_STRING, SORT_RANK_TIME, SortInput, append_escaped, append_number, append_terminated,
    append_time, document_refs, element_string, entry_key, extract_keys, filter_keys, is_reference,
    key_payload, key_seq, parse_index_name, ref_key, ref_keys, ref_prefix, seq_key, sort_input,
    sort_key, sort_value_key, sort_value_key_of, unique_keys,
};
pub use indexmeta::{IndexState, ORIGIN_ADVISOR, ORIGIN_HOOK, ORIGIN_QUERY, index_kind_name};
pub use kv::lower_key;
pub use migrate::{LEGACY_FOLDER, MigrationReport};
pub use pointintime::{KvItem, PointInTime};
pub use trigram::{
    Bitset, POSTING_ARRAY_MAX, POSTING_CHUNK, POSTING_CHUNK_BITS, Posting, document_trigrams,
    pattern_trigrams, posting_add, posting_each, posting_key, posting_remove, search_grams,
    search_spec,
};
pub use tx::{Tx, TxBytes};
