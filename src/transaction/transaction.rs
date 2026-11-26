use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crate::api::{self, BatchOperation, ByteFields, WriteSimulation};
use crate::engine;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::Map;
use crate::gocompat::time::Time;
use crate::schema::ValidationError;
use crate::sync::{Mutex, RwLock};

#[derive(Clone, Debug, Default)]
pub struct TxOperation {
    pub kind: String,
    pub entity: Vec<u8>,
    pub id: Vec<u8>,
    pub data: Map,
    pub fields: ByteFields,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: String,
    pub ops: Vec<TxOperation>,
    pub started_at: Instant,
}

pub trait Storage: Send + Sync {
    fn write_entity(&self, entity: &[u8], data: &mut Map) -> Vec<ValidationError>;
    fn update_entity_by_id(&self, entity: &[u8], id: &[u8], data: &Map) -> Option<Map>;
    fn delete_entity_by_id(&self, entity: &[u8], id: &[u8]);

    fn as_preflighter(&self) -> Option<&dyn Preflighter> {
        None
    }

    fn as_batch_committer(&self) -> Option<&dyn BatchCommitter> {
        None
    }
}

pub trait Preflighter {
    fn preflight(&self, ops: &[TxOperation]) -> Result<(), BoxError>;
}

pub trait BatchCommitter {
    fn commit_batch(&self, ops: &[TxOperation]) -> (bool, Result<(), BoxError>);
}

pub struct RealStorage;

impl BatchCommitter for RealStorage {
    fn commit_batch(&self, ops: &[TxOperation]) -> (bool, Result<(), BoxError>) {
        let batch: Vec<BatchOperation> = ops
            .iter()
            .map(|op| BatchOperation {
                kind: op.kind.clone(),
                entity: op.entity.clone(),
                id: op.id.clone(),
                data: op.data.clone(),
                fields: op.fields.clone(),
            })
            .collect();

        (true, api::commit_batch(&batch))
    }
}

impl Preflighter for RealStorage {
    fn preflight(&self, ops: &[TxOperation]) -> Result<(), BoxError> {
        let mut simulation = WriteSimulation::new(
            Box::new(engine::get_entity_schema),
            Box::new(engine::read_entity_by_id),
        );

        for op in ops {
            match op.kind.as_str() {
                "write" => {
                    if !simulation.write(&op.entity, &op.data).is_empty() {
                        return Err(errors::new("validation error"));
                    }
                }
                "update" => {
                    if !simulation.update(&op.entity, &op.id, &op.data) {
                        return Err(errors::new("update failed"));
                    }
                }
                "delete" => simulation.delete(&op.entity, &op.id),
                _ => {}
            }
        }

        Ok(())
    }
}

impl Storage for RealStorage {
    fn write_entity(&self, entity: &[u8], data: &mut Map) -> Vec<ValidationError> {
        engine::write_entity(entity, data)
    }

    fn update_entity_by_id(&self, entity: &[u8], id: &[u8], data: &Map) -> Option<Map> {
        engine::update_entity_by_id(entity, id, data)
    }

    fn delete_entity_by_id(&self, entity: &[u8], id: &[u8]) {
        engine::delete_entity_by_id(entity, id);
    }

    fn as_preflighter(&self) -> Option<&dyn Preflighter> {
        Some(self)
    }

    fn as_batch_committer(&self) -> Option<&dyn BatchCommitter> {
        Some(self)
    }
}

static STORAGE_IMPL: RwLock<Option<Arc<dyn Storage>>> = RwLock::new(None);

pub fn storage_impl() -> Arc<dyn Storage> {
    match STORAGE_IMPL.read().as_ref() {
        Some(s) => s.clone(),
        None => Arc::new(RealStorage),
    }
}

pub fn set_storage_impl(s: Arc<dyn Storage>) {
    *STORAGE_IMPL.write() = Some(s);
}

static COMMIT_MU: Mutex<()> = Mutex::new(());

static TX_MANAGER: LazyLock<Mutex<BTreeMap<String, Transaction>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn not_found() -> BoxError {
    errors::new("transaction not found")
}

pub fn begin_transaction() -> Transaction {
    let mut txs = TX_MANAGER.lock();

    let tx_id = generate_tx_id();
    let tx = Transaction {
        id: tx_id.clone(),
        ops: Vec::new(),
        started_at: Instant::now(),
    };

    txs.insert(tx_id, tx.clone());

    tx
}

pub fn open_transactions() -> (usize, Duration) {
    let txs = TX_MANAGER.lock();

    let mut oldest = Duration::ZERO;
    let now = Instant::now();
    for tx in txs.values() {
        let age = now.saturating_duration_since(tx.started_at);
        if age > oldest {
            oldest = age;
        }
    }

    (txs.len(), oldest)
}

fn generate_tx_id() -> String {
    Time::now().format("20060102150405.000000000")
}

pub fn get_transaction(tx_id: &str) -> Result<Transaction, BoxError> {
    TX_MANAGER.lock().get(tx_id).cloned().ok_or_else(not_found)
}

pub fn add_operation(tx_id: &str, op: TxOperation) -> Result<(), BoxError> {
    let mut txs = TX_MANAGER.lock();
    let tx = txs.get_mut(tx_id).ok_or_else(not_found)?;

    tx.ops.push(op);

    Ok(())
}

pub fn commit_transaction(tx_id: &str) -> Result<(), BoxError> {
    let tx = TX_MANAGER.lock().remove(tx_id).ok_or_else(not_found)?;

    let storage = storage_impl();
    if let Some(preflighter) = storage.as_preflighter() {
        preflighter.preflight(&tx.ops)?;
    }

    if let Some(committer) = storage.as_batch_committer() {
        let (handled, err) = committer.commit_batch(&tx.ops);
        if handled {
            return err;
        }
    }

    let _commit = COMMIT_MU.lock();

    for op in &tx.ops {
        match op.kind.as_str() {
            "write" => {
                let mut data = op.data.clone();
                if !storage.write_entity(&op.entity, &mut data).is_empty() {
                    return Err(errors::new("validation error"));
                }
            }
            "update" => {
                if storage
                    .update_entity_by_id(&op.entity, &op.id, &op.data)
                    .is_none()
                {
                    return Err(errors::new("update failed"));
                }
            }
            "delete" => storage.delete_entity_by_id(&op.entity, &op.id),
            _ => {}
        }
    }

    Ok(())
}

pub fn rollback_transaction(tx_id: &str) -> Result<(), BoxError> {
    TX_MANAGER.lock().remove(tx_id);

    Ok(())
}
