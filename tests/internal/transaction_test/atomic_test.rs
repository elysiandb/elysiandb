use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use elysiandb::api;
use elysiandb::configuration::{Config, CrashRecoveryConfig, StoreConfig};
use elysiandb::datastore::LOG_FOLDER;
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::journal::{self, KIND_DOC_DELETE, KIND_DOC_PUT, Kind, Position};
use elysiandb::storage;
use elysiandb::transaction::{self, TxOperation};

use super::serial;
use super::tempdir::TempDir;

fn doc(pairs: &[(&str, Value)]) -> Map {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn commit_three_operations() {
    let tx = transaction::begin_transaction();
    for op in [
        TxOperation {
            kind: "write".to_string(),
            entity: b"acct".to_vec(),
            id: Vec::new(),
            data: doc(&[("id", s("a")), ("balance", Value::Number(10.0))]),
        },
        TxOperation {
            kind: "update".to_string(),
            entity: b"acct".to_vec(),
            id: b"seed".to_vec(),
            data: doc(&[("balance", Value::Number(90.0))]),
        },
        TxOperation {
            kind: "delete".to_string(),
            entity: b"acct".to_vec(),
            id: b"old".to_vec(),
            data: Map::new(),
        },
    ] {
        if let Err(err) = transaction::add_operation(&tx.id, op) {
            panic!("{err}");
        }
    }
    if let Err(err) = transaction::commit_transaction(&tx.id) {
        panic!("{err}");
    }
}

fn last_segment(folder: &Path) -> PathBuf {
    let dir = folder.join(LOG_FOLDER);
    let mut names: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(err) => panic!("{err}"),
    };
    names.sort();
    names
        .pop()
        .unwrap_or_else(|| panic!("no segment in {}", dir.display()))
}

fn configure(folder: &Path) {
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: folder.to_string_lossy().into_owned(),
            shards: 4,
            crash_recovery: CrashRecoveryConfig {
                enabled: true,
                max_log_mb: 100,
            },
            ..StoreConfig::default()
        },
        ..Config::default()
    })));
}

fn seed() {
    storage::load_db();
    api::write_entity(
        b"acct",
        &mut doc(&[("id", s("seed")), ("balance", Value::Number(100.0))]),
    );
    api::write_entity(
        b"acct",
        &mut doc(&[("id", s("old")), ("balance", Value::Number(0.0))]),
    );
    if let Err(err) = storage::checkpoint() {
        panic!("{err}");
    }
}

fn abandon() {
    match storage::engine() {
        Some(e) => e.abandon(),
        None => panic!("no engine"),
    }
}

#[test]
fn test_commit_is_atomic_across_a_crash() {
    let _s = serial();
    for torn in [false, true] {
        let dir = TempDir::new();
        let folder = dir.path();
        configure(folder);
        seed();
        let path = last_segment(folder);
        let before = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        commit_three_operations();
        abandon();
        let after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if after <= before {
            panic!("the commit should be in the log");
        }
        if torn {
            let file = match std::fs::OpenOptions::new().write(true).open(&path) {
                Ok(f) => f,
                Err(err) => panic!("{err}"),
            };
            if let Err(err) = file.set_len(after - 3) {
                panic!("{err}");
            }
        }
        storage::load_db();
        let a = api::read_entity_by_id(b"acct", b"a");
        let seed_balance =
            api::read_entity_by_id(b"acct", b"seed").and_then(|m| m.get("balance").cloned());
        let old = api::read_entity_by_id(b"acct", b"old");
        if torn {
            if a.is_some() || seed_balance != Some(Value::Number(100.0)) || old.is_none() {
                panic!(
                    "a torn commit is dropped as a whole, got a={a:?} seed={seed_balance:?} old={old:?}"
                );
            }
        } else if a.is_none() || seed_balance != Some(Value::Number(90.0)) || old.is_some() {
            panic!(
                "a complete commit is replayed as a whole, got a={a:?} seed={seed_balance:?} old={old:?}"
            );
        }
        let _ = storage::close();
    }
}

#[test]
fn test_commit_is_one_batch() {
    let _s = serial();
    let dir = TempDir::new();
    let folder = dir.path();
    configure(folder);
    seed();
    commit_three_operations();
    abandon();

    let j = match journal::open(&folder.join(LOG_FOLDER), journal::Options::default()) {
        Ok(j) => j,
        Err(err) => panic!("{err}"),
    };
    let mut batches: Vec<Vec<Kind>> = Vec::new();
    let replayed = j.replay(
        Position {
            segment: 1,
            offset: 0,
        },
        |recs| {
            batches.push(recs.iter().map(|r| r.kind).collect());
            Ok(())
        },
    );
    j.abandon();
    if let Err(err) = replayed {
        panic!("{err}");
    }
    let Some(last) = batches.last() else {
        panic!("no batch");
    };
    let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
    for k in last {
        *kinds.entry(k.0).or_insert(0) += 1;
    }
    let puts = kinds.get(&KIND_DOC_PUT.0).copied().unwrap_or(0);
    let deletes = kinds.get(&KIND_DOC_DELETE.0).copied().unwrap_or(0);
    if puts < 2 || deletes != 1 {
        panic!("the three operations should be in the last batch, got {kinds:?}");
    }
    let _ = storage::close();
}
