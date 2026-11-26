use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::io::Write;

use crate::gocompat::errors::BoxError;
use crate::gocompat::json::{self, Encode, Encoder, Value};
use crate::gocompat::time::Time;
use crate::json_struct;
use crate::security::ByteString;

pub const FORMAT: &str = "elysiandb-backup";
pub const FORMAT_VERSION: i64 = 1;

pub const MANIFEST_PATH: &str = "manifest.json";
pub const KEY_PATH: &str = "security/users.key";
pub const KV_PATH: &str = "kv/keys.jsonl";

pub const KV_ENCODING_BASE64: &str = "base64";

const SCHEMAS_FOLDER: &str = "schemas/";
const ENTITIES_FOLDER: &str = "entities/";

pub(super) const FILE_NAME_PREFIX: &str = "elysiandb-backup-";
const FILE_NAME_LAYOUT: &str = "20060102T150405Z";
pub const FILE_NAME_EXTENSION: &str = ".tar.gz";

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Manifest {
        pub format: String => "format",
        pub format_version: i64 => "format_version",
        pub elysiandb_version: String => "elysiandb_version",
        pub created_at: String => "created_at",
        pub engine: String => "engine",
        pub entities: Vec<EntityManifest> => "entities",
        pub schemas: Vec<ByteString> => "schemas",
        pub kv_keys: i64 => "kv_keys",
        pub files: BTreeMap<String, String> => "files",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct EntityManifest {
        pub name: ByteString => "name",
        pub documents: i64 => "documents",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct KvEntry {
        pub key: ByteString => "key",
        pub value: ByteString => "value",
        pub encoding: String => "encoding" [omitempty],
        pub expires_at: i64 => "expires_at" [omitempty],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveError {
    pub reason: String,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid backup archive: {}", self.reason)
    }
}

impl StdError for ArchiveError {}

#[derive(Debug)]
pub struct IncompleteRestoreError {
    pub err: BoxError,
}

impl fmt::Display for IncompleteRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the restore failed after the data was replaced, run it again: {}",
            self.err
        )
    }
}

impl StdError for IncompleteRestoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.err.as_ref())
    }
}

pub(super) fn invalid_archive(reason: String) -> BoxError {
    Box::new(ArchiveError { reason })
}

pub fn file_name(t: &Time) -> String {
    format!(
        "{FILE_NAME_PREFIX}{}{FILE_NAME_EXTENSION}",
        t.utc().format(FILE_NAME_LAYOUT)
    )
}

pub fn schema_path(entity: &[u8]) -> String {
    format!("{SCHEMAS_FOLDER}{}.json", escape_name(entity))
}

pub fn entity_path(entity: &[u8]) -> String {
    format!("{ENTITIES_FOLDER}{}.jsonl", escape_name(entity))
}

impl Manifest {
    pub(super) fn paths(&self) -> Vec<String> {
        let mut paths = Vec::with_capacity(self.schemas.len() + self.entities.len() + 2);
        paths.push(KEY_PATH.to_string());

        for entity in &self.schemas {
            paths.push(schema_path(entity.as_bytes()));
        }

        for entity in &self.entities {
            paths.push(entity_path(entity.name.as_bytes()));
        }

        paths.push(KV_PATH.to_string());
        paths
    }
}

fn escape_name(name: &[u8]) -> String {
    let mut b = String::with_capacity(name.len());
    for &c in name {
        if is_safe_name_byte(c) {
            b.push(c as char);
            continue;
        }

        b.push_str(&format!("%{c:02X}"));
    }

    b
}

fn is_safe_name_byte(c: u8) -> bool {
    c.is_ascii_lowercase()
        || c.is_ascii_uppercase()
        || c.is_ascii_digit()
        || c == b'_'
        || c == b'-'
        || c == b'.'
}

pub(super) fn encode_line<T: Encode + ?Sized>(w: &mut dyn Write, v: &T) -> Result<(), BoxError> {
    let mut encoder = Encoder::new(w);
    encoder.set_escape_html(false);
    encoder.encode(v)?;

    Ok(())
}

pub(super) fn quote(s: &[u8]) -> String {
    json_text(&ByteString(s.to_vec()))
}

pub(super) fn json_text<T: Encode + ?Sized>(v: &T) -> String {
    match json::marshal(v) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(err) => err.to_string(),
    }
}

pub(super) fn value_text(v: Option<&Value>) -> String {
    match v {
        Some(v) => json_text(v),
        None => json_text(&Value::Null),
    }
}

pub(super) fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(super) fn hex(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(b.len() * 2);
    for &x in b {
        out.push(DIGITS[usize::from(x >> 4)] as char);
        out.push(DIGITS[usize::from(x & 0x0f)] as char);
    }
    out
}
