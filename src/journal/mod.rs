pub mod fs;
#[allow(clippy::module_inception)]
pub mod journal;
pub mod record;

pub use fs::{File, Fs, OSFS};
pub use journal::{
    DEFAULT_SEGMENT_SIZE, Journal, Mode, Options, ReadWindow, ReplayStats, SegmentInfo, Ticket,
    WriteErrorHook, new_read_window, open, segment_name,
};
pub use record::{
    ERR_BAD_SEGMENT, ERR_CLOSED, ERR_CORRUPT, ERR_TORN_BATCH, Error, KIND_CLEAR_ALL,
    KIND_DOC_DELETE, KIND_DOC_MOVE, KIND_DOC_PUT, KIND_ENTITY_CREATE, KIND_ENTITY_DROP,
    KIND_ENTITY_RESET, KIND_KV_DELETE, KIND_KV_EXPIRE, KIND_KV_MOVE, KIND_KV_PUT, KIND_KV_RESET,
    Kind, Location, Position, Record,
};
