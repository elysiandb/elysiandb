#[path = "../common/mod.rs"]
mod common;

mod bcrypt;
mod bcrypt_parse;

use common::{hex, read_json, str_field};
use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::Value;

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
