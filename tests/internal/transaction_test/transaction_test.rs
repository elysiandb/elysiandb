use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::time::Time;
use elysiandb::schema::ValidationError;
use elysiandb::transaction::{self, Storage, TxOperation};

use super::serial;

#[derive(Default)]
struct FakeStorage {
    write_err: bool,
    update_fail: bool,
    delete_hit: AtomicBool,
    write_count: AtomicUsize,
    update_count: AtomicUsize,
}

impl Storage for FakeStorage {
    fn write_entity(&self, _entity: &[u8], _data: &mut Map) -> Vec<ValidationError> {
        if self.write_err {
            return vec![ValidationError::new("", "x")];
        }
        self.write_count.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    }

    fn update_entity_by_id(&self, _entity: &[u8], _id: &[u8], _data: &Map) -> Option<Map> {
        if self.update_fail {
            return None;
        }
        self.update_count.fetch_add(1, Ordering::SeqCst);
        let mut m = Map::new();
        m.insert("ok".to_string(), Value::Bool(true));
        Some(m)
    }

    fn delete_entity_by_id(&self, _entity: &[u8], _id: &[u8]) {
        self.delete_hit.store(true, Ordering::SeqCst);
    }
}

struct Restore(Arc<dyn Storage>);

impl Drop for Restore {
    fn drop(&mut self) {
        transaction::set_storage_impl(self.0.clone());
    }
}

fn use_fake(f: Arc<FakeStorage>) -> Restore {
    let orig = transaction::storage_impl();
    transaction::set_storage_impl(f);
    Restore(orig)
}

fn op(kind: &str, entity: &str, id: &str, data: &[(&str, i64)]) -> TxOperation {
    TxOperation {
        kind: kind.to_string(),
        entity: entity.as_bytes().to_vec(),
        id: id.as_bytes().to_vec(),
        data: data
            .iter()
            .map(|(k, v)| (k.to_string(), Value::Int(*v)))
            .collect(),
        ..TxOperation::default()
    }
}

#[test]
fn test_begin_transaction() {
    let _s = serial();
    let tx = transaction::begin_transaction();
    if tx.id.is_empty() {
        panic!("empty id");
    }
}

#[test]
fn test_get_transaction_ok() {
    let _s = serial();
    let tx = transaction::begin_transaction();
    match transaction::get_transaction(&tx.id) {
        Ok(got) if got.id == tx.id => {}
        Ok(_) => panic!("wrong tx"),
        Err(err) => panic!("unexpected err {err}"),
    }
}

#[test]
fn test_get_transaction_not_found() {
    let _s = serial();
    match transaction::get_transaction("missing") {
        Err(err) if err.to_string() == "transaction not found" => {}
        other => panic!("expected error, got {:?}", other.map(|t| t.id)),
    }
}

#[test]
fn test_add_operation_ok() {
    let _s = serial();
    let tx = transaction::begin_transaction();
    if let Err(err) = transaction::add_operation(&tx.id, op("write", "x", "", &[("a", 1)])) {
        panic!("unexpected err {err}");
    }
    let got = transaction::get_transaction(&tx.id).map(|t| t.ops.len());
    if !matches!(got, Ok(1)) {
        panic!("op not added");
    }
}

#[test]
fn test_add_operation_not_found() {
    let _s = serial();
    if transaction::add_operation("missing", TxOperation::default()).is_ok() {
        panic!("expected error");
    }
}

#[test]
fn test_rollback_transaction_ok() {
    let _s = serial();
    let tx = transaction::begin_transaction();
    if let Err(err) = transaction::rollback_transaction(&tx.id) {
        panic!("unexpected err {err}");
    }
    if transaction::get_transaction(&tx.id).is_ok() {
        panic!("should be removed");
    }
}

#[test]
fn test_rollback_transaction_not_found() {
    let _s = serial();
    if transaction::rollback_transaction("missing").is_err() {
        panic!("unexpected err");
    }
}

#[test]
fn test_commit_transaction_write_validation_fails() {
    let _s = serial();
    let f = Arc::new(FakeStorage {
        write_err: true,
        ..FakeStorage::default()
    });
    let _restore = use_fake(f);

    let tx = transaction::begin_transaction();
    let _ = transaction::add_operation(&tx.id, op("write", "x", "", &[("a", 1)]));

    match transaction::commit_transaction(&tx.id) {
        Err(err) if err.to_string() == "validation error" => {}
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn test_commit_transaction_update_fails() {
    let _s = serial();
    let f = Arc::new(FakeStorage {
        update_fail: true,
        ..FakeStorage::default()
    });
    let _restore = use_fake(f);

    let tx = transaction::begin_transaction();
    let _ = transaction::add_operation(&tx.id, op("update", "x", "1", &[("a", 1)]));

    match transaction::commit_transaction(&tx.id) {
        Err(err) if err.to_string() == "update failed" => {}
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn test_commit_transaction_delete_ok() {
    let _s = serial();
    let f = Arc::new(FakeStorage::default());
    let _restore = use_fake(f.clone());

    let tx = transaction::begin_transaction();
    let _ = transaction::add_operation(&tx.id, op("delete", "x", "1", &[]));

    if let Err(err) = transaction::commit_transaction(&tx.id) {
        panic!("unexpected err {err}");
    }
    if !f.delete_hit.load(Ordering::SeqCst) {
        panic!("delete not called");
    }
}

#[test]
fn test_commit_transaction_not_found() {
    let _s = serial();
    if transaction::commit_transaction("missing").is_ok() {
        panic!("expected error");
    }
}

#[test]
fn test_commit_transaction_write_and_update_success() {
    let _s = serial();
    let f = Arc::new(FakeStorage::default());
    let _restore = use_fake(f.clone());

    let tx = transaction::begin_transaction();
    let _ = transaction::add_operation(&tx.id, op("write", "a", "", &[("x", 1)]));
    let _ = transaction::add_operation(&tx.id, op("update", "a", "1", &[("y", 2)]));

    if let Err(err) = transaction::commit_transaction(&tx.id) {
        panic!("unexpected err {err}");
    }

    if f.write_count.load(Ordering::SeqCst) != 1 || f.update_count.load(Ordering::SeqCst) != 1 {
        panic!("ops not called correctly");
    }
}

#[test]
fn test_generate_tx_id() {
    let _s = serial();
    let _id = Time::now().format("20060102150405.000000000");
    let got = transaction::begin_transaction();
    if got.id.is_empty() {
        panic!("missing id");
    }
}
