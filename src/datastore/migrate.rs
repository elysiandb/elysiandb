use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::durable::{self, Fs, Manifests, OS, filepath};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json;
use crate::gocompat::time::Time;
use crate::journal;
use crate::legacy::{self, Data, Document};
use crate::log;

use super::engine::{ENGINE_LOG, Engine, LOCK_FILE, LOG_FOLDER, MANIFEST_FILE, Options, open};

pub const LEGACY_FOLDER: &str = "legacy-0.2.4";
pub const MIGRATION_FOLDER: &str = ".migration";
pub const MIGRATION_BATCH: usize = 1000;
pub const MIGRATION_BYTES_PER_SECOND: i64 = 32 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub entities: usize,
    pub documents: usize,
    pub kv_keys: usize,
    pub replayed: usize,
    pub duration: Duration,
}

fn json_string(b: &[u8]) -> String {
    match json::marshal(&b.to_vec()) {
        Ok(v) => String::from_utf8_lossy(&v).into_owned(),
        Err(_) => String::new(),
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn migrate_if_needed(
    opts: &Options,
    manifests: &Manifests,
) -> Result<Option<MigrationReport>, BoxError> {
    let dir = &opts.folder;
    let stored = manifests.stored()?;

    if !stored.is_empty() {
        move_legacy_files(dir);
        return Ok(None);
    }

    if !legacy::present(dir) {
        return Ok(None);
    }

    let files = durable::or(opts.fs.clone());
    let start = Instant::now();
    remove_tree(&*files, &filepath::join(dir, LOG_FOLDER))?;
    remove_tree(&*files, &filepath::join(dir, durable::PAGES_FOLDER))?;
    let tmp = filepath::join(dir, MIGRATION_FOLDER);
    remove_tree(&*files, &tmp)?;

    check_free_space(dir)?;

    let bytes = legacy_bytes(dir);
    ENGINE_LOG.info(
        "migrating a 0.2.4 data folder, the first start takes longer",
        crate::kv![
            "bytes" => bytes,
            "estimated_seconds" => legacy_bytes(dir) / MIGRATION_BYTES_PER_SECOND + 1,
        ],
    );

    let data = match legacy::read(dir, opts.legacy_recovery, SystemTime::now()) {
        Ok(d) => d,
        Err(err) => {
            return Err(errors::wrap(
                "cannot read the 0.2.4 data files: ",
                Box::new(err),
            ));
        }
    };

    let mut report = match write_migration(&tmp, &data, opts.shards, opts.fs.clone()) {
        Ok(r) => r,
        Err(err) => {
            let _ = remove_tree(&*files, &tmp);
            return Err(errors::wrap(
                "migration failed, the 0.2.4 files are untouched: ",
                err,
            ));
        }
    };

    if let Err(err) = verify_migration(&tmp, &data, opts.shards, opts.fs.clone()) {
        let _ = remove_tree(&*files, &tmp);
        return Err(errors::wrap(
            "migration check failed, the 0.2.4 files are untouched: ",
            err,
        ));
    }

    let entries = files.read_dir(&tmp)?;

    for entry in entries {
        let name = entry.name();
        if name == MANIFEST_FILE || name == LOCK_FILE {
            continue;
        }

        files.rename(&filepath::join(&tmp, name), &filepath::join(dir, name))?;
    }

    files.sync_dir(dir)?;

    files.rename(
        &filepath::join(&tmp, MANIFEST_FILE),
        &filepath::join(dir, MANIFEST_FILE),
    )?;

    files.sync_dir(dir)?;

    if let Err(err) = remove_tree(&*files, &tmp) {
        ENGINE_LOG.warn(
            "cannot remove the migration folder",
            crate::kv!["err" => log::display(&err)],
        );
    }
    move_legacy_files(dir);

    report.replayed = data.replayed;
    report.duration = start.elapsed();
    let legacy_files = filepath::join(dir, LEGACY_FOLDER);
    ENGINE_LOG.info(
        "storage migrated",
        crate::kv![
            "from" => "0.2.4",
            "entity_types" => report.entities,
            "documents" => report.documents,
            "kv_keys" => report.kv_keys,
            "recovery_entries_replayed" => report.replayed,
            "legacy_files" => legacy_files.as_os_str().as_encoded_bytes(),
            "duration_ms" => log::since(start),
        ],
    );

    Ok(Some(report))
}

pub(crate) fn remove_tree(files: &dyn Fs, path: &Path) -> Result<(), BoxError> {
    let entries = match files.read_dir(path) {
        Ok(e) => e,
        Err(err) if err.is_not_exist() => return Ok(()),
        Err(err) => return Err(Box::new(err)),
    };

    for entry in entries {
        let child = filepath::join(path, entry.name());
        if entry.is_dir() {
            remove_tree(files, &child)?;
            continue;
        }

        if let Err(err) = files.remove(&child)
            && !err.is_not_exist()
        {
            return Err(Box::new(err));
        }
    }

    if let Err(err) = files.remove(path)
        && !err.is_not_exist()
    {
        return Err(Box::new(err));
    }

    Ok(())
}

fn legacy_bytes(dir: &Path) -> i64 {
    let mut used = 0i64;
    for name in legacy::files_in(dir) {
        if let Ok(info) = std::fs::metadata(filepath::join(dir, name)) {
            used += info.len() as i64;
        }
    }

    used
}

pub(crate) fn move_legacy_files(dir: &Path) {
    let files = legacy::files_in(dir);
    if files.is_empty() {
        return;
    }

    let target = filepath::join(dir, LEGACY_FOLDER);
    if let Err(err) = OS.mkdir_all(&target, 0o700) {
        ENGINE_LOG.warn(
            "cannot keep the 0.2.4 files aside",
            crate::kv!["err" => log::display(&err)],
        );
        return;
    }

    for name in files {
        let mut dst = filepath::join(&target, name);
        if std::fs::metadata(&dst).is_ok() {
            let stamp = Time::now().utc().format("20060102T150405");
            dst = filepath::join(&target, format!("{name}.{stamp}"));
        }

        if let Err(err) = OS.rename(&filepath::join(dir, name), &dst) {
            ENGINE_LOG.warn(
                "cannot keep a 0.2.4 file aside",
                crate::kv!["file" => name, "err" => log::display(&err)],
            );
            continue;
        }

        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o600));
    }
}

fn check_free_space(dir: &Path) -> Result<(), BoxError> {
    let used = legacy_bytes(dir);

    let Ok(free) = durable::free_bytes(dir) else {
        return Ok(());
    };

    if free < 2 * used {
        return Err(errors::new(format!(
            "not enough free space to migrate the data folder: {free} bytes free, {} needed",
            2 * used
        )));
    }

    Ok(())
}

fn nested_options(dir: &Path, shards: i64, files: Option<Arc<dyn Fs>>) -> Options {
    Options {
        folder: dir.to_path_buf(),
        shards,
        durability: journal::Mode::Write,
        fs: files,
        ..Options::default()
    }
}

fn write_migration(
    dir: &Path,
    data: &Data,
    shards: i64,
    files: Option<Arc<dyn Fs>>,
) -> Result<MigrationReport, BoxError> {
    let e = open(nested_options(dir, shards, files))?;

    let mut report = MigrationReport::default();
    let result = (|| -> Result<(), BoxError> {
        e.update(|tx| {
            for entity in &data.types {
                tx.ensure_type(entity);
            }
            Ok(())
        })?;

        let empty: Vec<Document> = Vec::new();
        for entity in &data.types {
            let docs = data.entities.get(entity).unwrap_or(&empty);
            report.entities += 1;
            let mut start = 0usize;
            while start < docs.len() {
                let end = (start + MIGRATION_BATCH).min(docs.len());
                e.update(|tx| {
                    for doc in &docs[start..end] {
                        tx.put_opt(entity, &doc.id, doc.doc.as_ref())?;
                    }
                    Ok(())
                })?;
                report.documents += end - start;
                start += MIGRATION_BATCH;
            }
        }

        for (key, value) in &data.kv {
            if key.starts_with(legacy::API_KEY_PREFIX.as_bytes()) {
                continue;
            }

            let exp = data.expirations.get(key).copied().unwrap_or(0);
            e.kv_put(key, value.as_deref().unwrap_or_default(), exp)?;
            report.kv_keys += 1;
        }

        Ok(())
    })();

    if let Err(err) = result {
        e.abandon();
        return Err(err);
    }

    e.close()?;

    Ok(report)
}

fn verify_migration(
    dir: &Path,
    data: &Data,
    shards: i64,
    files: Option<Arc<dyn Fs>>,
) -> Result<(), BoxError> {
    let e = open(nested_options(dir, shards, files))?;
    struct Abandon(Arc<Engine>);
    impl Drop for Abandon {
        fn drop(&mut self) {
            self.0.abandon();
        }
    }
    let e = Abandon(e);
    let e = &e.0;

    let types = e.types();
    if types.len() != data.types.len() {
        return Err(errors::new(format!(
            "{} entity types migrated, {} expected",
            types.len(),
            data.types.len()
        )));
    }

    let empty: Vec<Document> = Vec::new();
    for (i, t) in data.types.iter().enumerate() {
        if types[i] != *t {
            return Err(errors::new(format!(
                "entity type #{i} is {}, {} expected",
                json_string(&types[i]),
                json_string(t)
            )));
        }

        let snap = e.snapshot(t);
        let docs = data.entities.get(t).unwrap_or(&empty);
        if snap.len() != docs.len() {
            return Err(errors::new(format!(
                "{} holds {} documents, {} expected",
                lossy(t),
                snap.len(),
                docs.len()
            )));
        }

        let mut i = 0usize;
        let mut mismatch: Option<BoxError> = None;
        snap.ascend(0, |d| {
            let want = json::marshal(&docs[i].doc).unwrap_or_default();
            let ok = match d.bytes() {
                Ok(got) => d.id().as_ref() == docs[i].id.as_slice() && *got == *want,
                Err(_) => false,
            };
            if !ok {
                mismatch = Some(errors::new(format!(
                    "{} document #{i} ({}) differs after migration",
                    lossy(t),
                    lossy(&docs[i].id)
                )));
                return false;
            }
            i += 1;
            true
        });

        if let Some(err) = mismatch {
            return Err(err);
        }
    }

    for (key, value) in &data.kv {
        if key.starts_with(legacy::API_KEY_PREFIX.as_bytes()) {
            continue;
        }

        let want_exp = data.expirations.get(key).copied().unwrap_or(0);
        let same = match e.kv_get(key) {
            None => false,
            Some((got, exp)) => {
                got.as_deref().unwrap_or_default() == value.as_deref().unwrap_or_default()
                    && exp == want_exp
            }
        };
        if !same {
            return Err(errors::new(format!(
                "key {} differs after migration",
                json_string(key)
            )));
        }
    }

    Ok(())
}

impl Engine {
    pub fn export_legacy(&self, folder: &Path) -> Result<Data, BoxError> {
        let snaps = self.snapshot_all();
        let mut data = Data {
            types: self.types(),
            ..Data::default()
        };

        for t in data.types.clone() {
            let Some(snap) = snaps.get(&t) else {
                continue;
            };

            let mut decode_err: Option<BoxError> = None;
            let mut list: Vec<Document> = Vec::new();
            snap.ascend(0, |d| match d.decode() {
                Ok(doc) => {
                    list.push(Document {
                        id: d.id().into_owned(),
                        doc: Some(doc),
                    });
                    true
                }
                Err(err) => {
                    decode_err = Some(err);
                    false
                }
            });

            if let Some(err) = decode_err {
                return Err(err);
            }
            if !list.is_empty() {
                data.entities.insert(t.clone(), list);
            }
        }

        for key in self.kv_keys() {
            let Some((value, exp)) = self.kv_get(&key) else {
                continue;
            };

            data.kv.insert(key.clone(), value);
            if exp > 0 {
                data.expirations.insert(key, exp);
            }
        }

        if let Err(err) = legacy::write(folder, &data) {
            return Err(Box::new(err));
        }

        Ok(data)
    }
}
