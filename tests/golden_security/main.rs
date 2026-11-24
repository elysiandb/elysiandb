#[path = "../common/mod.rs"]
mod common;
#[path = "../internal/tempdir.rs"]
mod tempdir;

mod acl;
mod basic_header;
mod bcrypt;
mod bcrypt_parse;
mod cli;
mod cookies;
mod dto_decode;
mod guard;
mod http;
mod ip;
mod keyfile;
mod origin;
mod scan;
mod session;
mod sessions_decode;
mod validation;

use std::sync::Arc;

use common::{hex, read_json, str_field};
use elysiandb::configuration::Config;
use elysiandb::crypto::sha256;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::storage;
use elysiandb::yaml::StringList;

use tempdir::TempDir;

pub(crate) struct Store {
    pub(crate) dir: TempDir,
}

impl Store {
    pub(crate) fn path(&self) -> String {
        self.dir.path().to_string_lossy().into_owned()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub(crate) fn security_config(folder: &str, mode: &str) -> Config {
    let mut cfg = Config::default();
    cfg.store.folder = folder.to_string();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = !mode.is_empty();
    cfg.security.authentication.mode = mode.to_string();
    cfg.server.http.cors.allowed_origins = StringList(Some(vec![
        " http://allowed.test/ ".to_string(),
        "HTTPS://Upper.Test".to_string(),
        "http://\u{212a}elvin.test".to_string(),
    ]));
    cfg.server.http.trusted_proxies = StringList(Some(vec![
        "10.0.0.0/8".to_string(),
        "192.168.1.1".to_string(),
        "::1/128".to_string(),
    ]));
    cfg.log.level = "error".to_string();
    cfg
}

pub(crate) fn security_store(mode: &str) -> Store {
    let dir = TempDir::new();
    let folder = dir.path().to_string_lossy().into_owned();
    globals::set_config(Some(Arc::new(security_config(&folder, mode))));
    storage::load_db();
    storage::load_json_db();
    elysiandb::security::reset_login_attempts();
    Store { dir }
}

pub(crate) fn bool_of(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

pub(crate) fn flag(row: &Value, key: &str) -> bool {
    row.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn int_of(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

pub(crate) fn assert_manifest_entry(file: &str) {
    let manifest = read_json("security/MANIFEST.json");
    assert_eq!(str_field(&manifest, "go_version"), "go1.24.7");
    assert_eq!(str_field(&manifest, "baseline"), "1a11e5e");
    let files = manifest
        .get("files")
        .and_then(Value::as_array)
        .expect("files");
    let entry = files
        .iter()
        .find(|f| str_field(f, "path") == file)
        .unwrap_or_else(|| panic!("{file} is not in the manifest"));
    let data = common::read_golden(&format!("security/{file}"));
    assert_eq!(
        entry.get("size").and_then(Value::as_u64),
        Some(data.len() as u64),
        "{file}"
    );
    assert_eq!(
        str_field(entry, "sha256"),
        hex(&sha256::sum256(&data)),
        "{file}"
    );
}
