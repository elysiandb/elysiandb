#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use elysiandb::gocompat::json::Value;

pub fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("path under the root")
                .to_string_lossy()
                .into_owned();
            let data = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            out.insert(rel, data);
        }
    }

    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

pub fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap_or_else(|e| panic!("cannot create {}: {e}", dst.display()));
    for (rel, data) in read_tree(src) {
        let target = dst.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", parent.display()));
        }
        std::fs::write(&target, data)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", target.display()));
    }
}

pub fn assert_same_tree(ours: &Path, theirs: &Path, label: &str) {
    let a = read_tree(ours);
    let b = read_tree(theirs);
    let names_a: Vec<&String> = a.keys().collect();
    let names_b: Vec<&String> = b.keys().collect();
    assert_eq!(names_a, names_b, "{label}: file names differ");
    for (name, data) in &a {
        assert!(
            data == &b[name],
            "{label}: {name} differs ({} bytes, {} expected)",
            data.len(),
            b[name].len()
        );
    }
}

pub fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing field {key} in {row:?}"))
}

pub fn array<'a>(row: &'a Value, key: &str) -> &'a Vec<Value> {
    field(row, key)
        .as_array()
        .unwrap_or_else(|| panic!("field {key} is not an array in {row:?}"))
}

pub fn string<'a>(row: &'a Value, key: &str) -> &'a str {
    field(row, key)
        .as_str()
        .unwrap_or_else(|| panic!("field {key} is not a string in {row:?}"))
}

pub fn int(row: &Value, key: &str) -> i64 {
    field(row, key)
        .as_i64()
        .unwrap_or_else(|| panic!("field {key} is not an integer in {row:?}"))
}

pub fn uint(row: &Value, key: &str) -> u64 {
    field(row, key)
        .as_u64()
        .unwrap_or_else(|| panic!("field {key} is not an unsigned integer in {row:?}"))
}

pub fn dec_u64(row: &Value, key: &str) -> u64 {
    string(row, key)
        .parse()
        .unwrap_or_else(|e| panic!("field {key} is not a decimal u64 in {row:?}: {e}"))
}

pub fn dec_i64(row: &Value, key: &str) -> i64 {
    string(row, key)
        .parse()
        .unwrap_or_else(|e| panic!("field {key} is not a decimal i64 in {row:?}: {e}"))
}

pub fn hex_bytes(row: &Value, key: &str) -> Vec<u8> {
    super::common::unhex(string(row, key))
}

pub fn formula_bytes(n: usize, shift: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (((i + shift) * 31 + 7) & 0xff) as u8)
        .collect()
}
