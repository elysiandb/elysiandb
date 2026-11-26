use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use elysiandb::configuration::{CrashRecoveryConfig, StoreConfig};
use elysiandb::durable::filepath;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::storage;

use super::tempdir::TempDir;
use super::{CloseStorage, serial, set_store};

fn expect_owner_only(dir: &Path) {
    let mut files = 0usize;
    let mut pending: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let entries = std::fs::read_dir(&folder).unwrap_or_else(|e| panic!("{e}"));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("{e}"));
            let path = entry.path();
            let info = std::fs::symlink_metadata(&path).unwrap_or_else(|e| panic!("{e}"));
            if info.is_dir() {
                pending.push(path);
                continue;
            }

            files += 1;
            let perm = info.permissions().mode() & 0o777;
            if info.file_type().is_file() && perm != 0o600 {
                panic!("{}: expected mode 0600, got {perm:o}", path.display());
            }
        }
    }

    if files == 0 {
        panic!("expected data files in {}", dir.display());
    }
}

#[test]
fn data_files_are_owner_only() {
    let _serial = serial();
    let dir = TempDir::new();
    set_store(StoreConfig {
        folder: dir.path().to_string_lossy().into_owned(),
        shards: 4,
        crash_recovery: CrashRecoveryConfig {
            enabled: true,
            max_log_mb: 1,
        },
        ..StoreConfig::default()
    });

    storage::load_db();
    let _close = CloseStorage;
    expect_owner_only(dir.path());

    let _ = storage::put_key_value(b"k", b"v");
    let doc = Map::from([("id".to_string(), Value::String("doc".to_string()))]);
    if let Err(err) = storage::put_json_value(b"api:entity:docs:id:doc", &doc) {
        panic!("{err}");
    }
    storage::write_to_db();
    expect_owner_only(dir.path());
}

#[test]
fn existing_data_files_become_owner_only_at_startup() {
    let _serial = serial();
    let dir = TempDir::new();
    for name in [
        storage::DATA_FILE,
        storage::EXPIRATION_DATA_FILE,
        storage::JSON_DATA_FILE,
    ] {
        let path = filepath::join(dir.path(), name);
        if let Err(err) = std::fs::write(&path, b"{}") {
            panic!("{err}");
        }
        if let Err(err) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)) {
            panic!("{err}");
        }
    }

    set_store(StoreConfig {
        folder: dir.path().to_string_lossy().into_owned(),
        shards: 4,
        ..StoreConfig::default()
    });
    storage::load_db();
    let _close = CloseStorage;

    expect_owner_only(dir.path());
}
