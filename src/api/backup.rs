use std::sync::mpsc;

use crate::datastore::{Doc, Snapshot, decode_map};
use crate::gocompat::errors::BoxError;
use crate::gocompat::json::{self, Map, Value};
use crate::sync::{Condvar, Mutex};

use super::storage::{add_entity_type, db, update};

const RESTORE_BATCH: usize = 1000;
const RAW_BATCH_DOCUMENTS: usize = 2048;
const RAW_BATCH_BYTES: usize = 8 << 20;
const RAW_BATCH_WORKERS: usize = 3;

pub type RawNext<'a> = &'a mut (dyn FnMut() -> Result<Option<(Vec<u8>, Vec<u8>)>, BoxError> + Send);

pub fn iterate_entities(
    entity: &[u8],
    f: &mut dyn FnMut(Map) -> Result<(), BoxError>,
) -> Result<(), BoxError> {
    let Some(e) = db() else {
        return Ok(());
    };

    iterate_snapshot(&e.snapshot(entity), f)
}

pub fn iterate_snapshot(
    snap: &Snapshot,
    f: &mut dyn FnMut(Map) -> Result<(), BoxError>,
) -> Result<(), BoxError> {
    let mut result = Ok(());
    snap.ascend(0, |d: Doc| {
        let Ok(data) = d.decode() else {
            return true;
        };

        result = f(data);

        result.is_ok()
    });

    result
}

fn id_of(data: &Map) -> &[u8] {
    match data.get("id") {
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"",
    }
}

pub fn restore_entities(
    entity: &[u8],
    next: &mut dyn FnMut() -> Result<Option<Map>, BoxError>,
) -> (usize, Result<(), BoxError>) {
    add_entity_type(entity);

    let mut count = 0;
    let mut batch: Vec<Map> = Vec::new();
    let flush = |batch: &mut Vec<Map>, count: &mut usize| -> Result<(), BoxError> {
        if batch.is_empty() {
            return Ok(());
        }

        let err = update(|tx| {
            for data in batch.iter() {
                tx.put(entity, id_of(data), data)?;
            }
            Ok(())
        });

        *count += batch.len();
        batch.clear();

        err
    };

    loop {
        match next() {
            Ok(None) => break,
            Err(err) => {
                if let Err(flush_err) = flush(&mut batch, &mut count) {
                    return (count, Err(flush_err));
                }

                return (count, Err(err));
            }
            Ok(Some(data)) => {
                batch.push(data);
                if batch.len() >= RESTORE_BATCH
                    && let Err(err) = flush(&mut batch, &mut count)
                {
                    return (count, Err(err));
                }
            }
        }
    }

    let err = flush(&mut batch, &mut count);
    (count, err)
}

struct RawDocument {
    id: Vec<u8>,
    raw: Vec<u8>,
    value: Vec<u8>,
    doc: Option<Map>,
}

struct Slots {
    free: Mutex<usize>,
    released: Condvar,
}

impl Slots {
    fn take(&self) {
        let mut free = self.free.lock();
        while *free == 0 {
            free = self.released.wait(free);
        }
        *free -= 1;
    }

    fn give(&self) {
        *self.free.lock() += 1;
        self.released.notify_one();
    }
}

type Work = mpsc::Receiver<Result<Vec<RawDocument>, BoxError>>;

fn decode_batch(mut docs: Vec<RawDocument>) -> Result<Vec<RawDocument>, BoxError> {
    for d in docs.iter_mut() {
        let doc = decode_map(&d.raw).map_err(|e| Box::new(e) as BoxError)?;
        d.value = json::marshal(&doc)?;
        d.doc = doc;
        d.raw = Vec::new();
    }

    Ok(docs)
}

pub fn restore_raw_entities(entity: &[u8], next: RawNext<'_>) -> (usize, Result<(), BoxError>) {
    add_entity_type(entity);

    let slots = Slots {
        free: Mutex::new(RAW_BATCH_WORKERS),
        released: Condvar::new(),
    };
    let (queue_tx, queue_rx) = mpsc::sync_channel::<Work>(RAW_BATCH_WORKERS);

    std::thread::scope(|s| {
        let slots = &slots;
        let producer = std::thread::Builder::new()
            .name("restore-reader".to_string())
            .spawn_scoped(s, move || -> Option<BoxError> {
                let mut batch: Vec<RawDocument> = Vec::with_capacity(RAW_BATCH_DOCUMENTS);
                let mut size = 0usize;
                let send = |batch: &mut Vec<RawDocument>, size: &mut usize| {
                    if batch.is_empty() {
                        return;
                    }

                    let docs = std::mem::replace(batch, Vec::with_capacity(RAW_BATCH_DOCUMENTS));
                    *size = 0;
                    let (done_tx, done_rx) = mpsc::sync_channel(1);
                    slots.take();
                    if queue_tx.send(done_rx).is_err() {
                        slots.give();
                        return;
                    }
                    let spawned = std::thread::Builder::new()
                        .name("restore-decoder".to_string())
                        .spawn_scoped(s, move || {
                            let res = decode_batch(docs);
                            slots.give();
                            let _ = done_tx.send(res);
                        });
                    if spawned.is_err() {
                        slots.give();
                    }
                };

                loop {
                    match next() {
                        Ok(Some((id, raw))) => {
                            size += raw.len();
                            batch.push(RawDocument {
                                id,
                                raw,
                                value: Vec::new(),
                                doc: None,
                            });
                            if batch.len() >= RAW_BATCH_DOCUMENTS || size >= RAW_BATCH_BYTES {
                                send(&mut batch, &mut size);
                            }
                        }
                        Ok(None) => {
                            send(&mut batch, &mut size);
                            return None;
                        }
                        Err(err) => {
                            send(&mut batch, &mut size);
                            return Some(err);
                        }
                    }
                }
            });

        let mut count = 0usize;
        let mut write_err: Option<BoxError> = None;
        for done in queue_rx.iter() {
            let res = done
                .recv()
                .unwrap_or_else(|_| Err(crate::gocompat::errors::new("the decoder stopped")));
            let err = match res {
                Ok(docs) if write_err.is_none() => {
                    let written = update(|tx| {
                        for d in &docs {
                            tx.put_bytes(entity, &d.id, d.value.clone(), d.doc.clone())?;
                        }
                        Ok(())
                    });
                    if written.is_ok() {
                        count += docs.len();
                    }
                    written.err()
                }
                Ok(_) => None,
                Err(err) => Some(err),
            };
            if let Some(err) = err
                && write_err.is_none()
            {
                write_err = Some(err);
            }
        }

        let read_err = match producer {
            Ok(handle) => handle.join().unwrap_or(None),
            Err(err) => Some(Box::new(err) as BoxError),
        };

        if let Some(err) = write_err {
            return (count, Err(err));
        }

        match read_err {
            Some(err) => (count, Err(err)),
            None => (count, Ok(())),
        }
    })
}
