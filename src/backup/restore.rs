use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::acl;
use crate::api;
use crate::archive::gzip;
use crate::archive::tar::{self, TYPE_REG};
use crate::cache;
use crate::configuration::Config;
use crate::crypto::sha256::{self, Digest};
use crate::durable::{self, Fs, filepath};
use crate::engine;
use crate::globals;
use crate::gocompat::base64::STD_ENCODING;
use crate::gocompat::errors::BoxError;
use crate::gocompat::filepath as gofilepath;
use crate::gocompat::json::{self, Decoder, Map, RawMessage, Subject, Value};
use crate::gocompat::strings;
use crate::gocompat::time::{RFC3339, Time};
use crate::hook;
use crate::json_struct;
use crate::schema::SCHEMA_ENTITY;
use crate::security;
use crate::storage;

use super::archive::{
    FORMAT, FORMAT_VERSION, IncompleteRestoreError, KEY_PATH, KV_ENCODING_BASE64, KV_PATH, KvEntry,
    MANIFEST_PATH, Manifest, entity_path, hex, invalid_archive, json_text, quote, schema_path,
    text, value_text,
};
use super::create::{create_temp, simple_document_id};

const MAX_MANIFEST_SIZE: u64 = 64 << 20;
const MAX_KEY_SIZE: u64 = 64 << 10;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Report {
        pub entities: BTreeMap<Vec<u8>, i64> => "entities",
        pub schemas: i64 => "schemas",
        pub kv_keys: i64 => "kv_keys",
        pub expired_kv_keys: i64 => "kv_keys_expired",
    }
}

type TarStream<R> = tar::Reader<gzip::Reader<BufReader<R>>>;

struct ArchiveReader<R: Read> {
    tr: TarStream<R>,
}

pub fn restore<R: Read + Seek + Send>(mut src: R) -> Result<Report, BoxError> {
    let manifest = verify(&mut src)?;

    src.seek(SeekFrom::Start(0))?;

    apply(src, &manifest)
}

pub fn verify<R: Read>(src: R) -> Result<Manifest, BoxError> {
    let mut archive = open_archive(src)?;

    let manifest = archive.read_manifest()?;

    for path in manifest.paths() {
        archive.next(&path)?;

        let expected = manifest.files.get(&path).map(String::as_str).unwrap_or("");
        verify_entry(
            &mut archive.tr,
            &path,
            expected,
            &manifest.checker_for(&path),
        )?;
    }

    archive.end()?;

    Ok(manifest)
}

enum Check<'m> {
    Key,
    Keys(i64),
    Schema(&'m [u8]),
    Documents(i64),
    Unexpected,
}

impl Manifest {
    fn checker_for(&self, path: &str) -> Check<'_> {
        if path == KEY_PATH {
            return Check::Key;
        }

        if path == KV_PATH {
            return Check::Keys(self.kv_keys);
        }

        for entity in &self.schemas {
            if schema_path(entity.as_bytes()) == path {
                return Check::Schema(entity.as_bytes());
            }
        }

        for entity in &self.entities {
            if entity_path(entity.name.as_bytes()) == path {
                return Check::Documents(entity.documents);
            }
        }

        Check::Unexpected
    }

    fn validate(&self) -> Result<(), BoxError> {
        if self.format != FORMAT {
            return Err(invalid_archive(format!(
                "unknown format {}",
                quote(self.format.as_bytes())
            )));
        }

        if self.format_version < 1 {
            return Err(invalid_archive(format!(
                "invalid format version {}",
                self.format_version
            )));
        }

        if self.format_version > FORMAT_VERSION {
            return Err(invalid_archive(format!(
                "format version {} is newer than the version {} supported by ElysianDB {}",
                self.format_version,
                FORMAT_VERSION,
                globals::version()
            )));
        }

        if self.kv_keys < 0 {
            return Err(invalid_archive(format!(
                "invalid KV key count {}",
                self.kv_keys
            )));
        }

        for entity in &self.entities {
            if entity.name.as_bytes().is_empty() || entity.documents < 0 {
                return Err(invalid_archive(format!(
                    "invalid entity entry {}",
                    json_text(entity)
                )));
            }
        }

        let paths = self.paths();
        let mut unique: HashSet<&str> = HashSet::new();
        for path in &paths {
            if !unique.insert(path.as_str()) {
                return Err(invalid_archive(format!("{path} is listed twice")));
            }

            if !self.files.contains_key(path) {
                return Err(invalid_archive(format!("no checksum for {path}")));
            }
        }

        if self.files.len() != paths.len() {
            for path in self.files.keys() {
                if !unique.contains(path.as_str()) {
                    return Err(invalid_archive(format!(
                        "checksum for an unknown file {path}"
                    )));
                }
            }
        }

        Ok(())
    }
}

fn run_check<R: Read>(check: &Check<'_>, r: R, path: &str) -> Result<(), BoxError> {
    match check {
        Check::Key => read_key(r).map(|_| ()),
        Check::Keys(expected) => expect_count(path, *expected, KvReader::new(r, path).count()),
        Check::Schema(entity) => read_schema(r, path, entity).map(|_| ()),
        Check::Documents(expected) => {
            expect_count(path, *expected, DocumentReader::new(r, path, true).count())
        }
        Check::Unexpected => Err(invalid_archive(format!("unexpected file {path}"))),
    }
}

fn expect_count(path: &str, expected: i64, count: Result<i64, BoxError>) -> Result<(), BoxError> {
    let n = count?;

    if n != expected {
        return Err(invalid_archive(format!(
            "{path} holds {n} entries, the manifest announces {expected}"
        )));
    }

    Ok(())
}

struct Tee<'r, R> {
    r: &'r mut R,
    hash: Digest,
}

impl<R: Read> Read for Tee<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.r.read(buf)?;
        self.hash.write(&buf[..n]);

        Ok(n)
    }
}

fn verify_entry<R: Read>(
    r: &mut R,
    path: &str,
    expected_sum: &str,
    check: &Check<'_>,
) -> Result<(), BoxError> {
    let mut tee = Tee {
        r,
        hash: Digest::new(),
    };

    let check_err = run_check(check, &mut tee, path);
    if let Err(err) = io::copy(&mut tee, &mut io::sink()) {
        return Err(invalid_archive(format!("{path} cannot be read: {err}")));
    }

    if hex(&tee.hash.sum()) != expected_sum {
        return Err(invalid_archive(format!("checksum mismatch for {path}")));
    }

    check_err
}

pub fn apply<R: Read + Send>(src: R, manifest: &Manifest) -> Result<Report, BoxError> {
    let mut archive = open_archive(src)?;

    archive.next(MANIFEST_PATH)?;

    archive.next(KEY_PATH)?;

    let key = read_key(&mut archive.tr)?;

    mark_restoring()?;

    security::write_key(&key)?;

    match replace_data(&mut archive, manifest) {
        Ok(report) => {
            clear_restoring();
            Ok(report)
        }
        Err(err) => Err(Box::new(IncompleteRestoreError { err })),
    }
}

struct WakeIndexes;

impl Drop for WakeIndexes {
    fn drop(&mut self) {
        storage::wake_indexes();
    }
}

fn replace_data<R: Read + Send>(
    archive: &mut ArchiveReader<R>,
    manifest: &Manifest,
) -> Result<Report, BoxError> {
    engine::wipe_all()?;

    storage::release_indexes();
    let _wake = WakeIndexes;

    let mut report = Report {
        schemas: restore_schemas(archive, manifest)?,
        ..Report::default()
    };

    for entity in &manifest.entities {
        let path = entity_path(entity.name.as_bytes());
        archive.next(&path)?;

        let count = restore_documents(
            entity.name.as_bytes(),
            DocumentReader::new(&mut archive.tr, &path, false),
        )?;

        report
            .entities
            .insert(entity.name.as_bytes().to_vec(), count);
    }

    restore_keys(archive, &mut report)?;

    finish_restore()?;

    Ok(report)
}

fn restore_documents<R: Read + Send>(
    entity: &[u8],
    mut documents: DocumentReader<R>,
) -> Result<i64, BoxError> {
    let mut next = || documents.next_raw();
    let (count, result) = api::restore_raw_entities(entity, &mut next);
    result?;

    Ok(count as i64)
}

fn restore_schemas<R: Read>(
    archive: &mut ArchiveReader<R>,
    manifest: &Manifest,
) -> Result<i64, BoxError> {
    let mut schemas: VecDeque<Map> = VecDeque::with_capacity(manifest.schemas.len());
    for entity in &manifest.schemas {
        let path = schema_path(entity.as_bytes());
        archive.next(&path)?;

        let data = read_schema(&mut archive.tr, &path, entity.as_bytes())?;

        schemas.push_back(data);
    }

    let (count, result) =
        engine::restore_entities(SCHEMA_ENTITY.as_bytes(), &mut || Ok(schemas.pop_front()));
    result?;

    Ok(count as i64)
}

fn restore_keys<R: Read>(
    archive: &mut ArchiveReader<R>,
    report: &mut Report,
) -> Result<(), BoxError> {
    archive.next(KV_PATH)?;

    let mut keys = KvReader::new(&mut archive.tr, KV_PATH);
    let now = Time::now().unix();
    loop {
        let Some((entry, value)) = keys.next()? else {
            return Ok(());
        };

        if entry.expires_at > 0 && entry.expires_at <= now {
            report.expired_kv_keys += 1;
            continue;
        }

        storage::put_key_value_with_expiration(entry.key.as_bytes(), &value, entry.expires_at)?;

        report.kv_keys += 1;
    }
}

const RESTORING_MARKER: &str = "RESTORING";
const UPLOAD_PREFIX: &str = ".restore-upload-";

fn config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

fn store_folder() -> PathBuf {
    PathBuf::from(config().store.folder.clone())
}

fn restoring_path() -> PathBuf {
    filepath::join(&store_folder(), RESTORING_MARKER)
}

fn mark_restoring() -> Result<(), BoxError> {
    durable::OS.mkdir_all(&store_folder(), 0o755)?;

    let stamp = format!("{}\n", Time::now().utc().format(RFC3339));
    durable::write_file_atomic(&durable::OS, &restoring_path(), |w| {
        w.write(stamp.as_bytes())
    })?;

    Ok(())
}

fn clear_restoring() {
    let _ = fs::remove_file(restoring_path());
    let _ = durable::OS.sync_dir(&store_folder());
}

pub fn restore_was_interrupted() -> bool {
    fs::metadata(restoring_path()).is_ok()
}

pub fn new_upload() -> Result<(File, PathBuf), BoxError> {
    let folder = store_folder();
    durable::OS.mkdir_all(&folder, 0o755)?;

    Ok(create_temp(&folder, UPLOAD_PREFIX)?)
}

pub fn remove_abandoned_uploads() {
    let pattern = filepath::join(&store_folder(), format!("{UPLOAD_PREFIX}*"));
    let found = gofilepath::glob(pattern.as_os_str().as_bytes()).unwrap_or_default();
    for path in found {
        let _ = fs::remove_file(Path::new(std::ffi::OsStr::from_bytes(&path)));
    }
}

fn finish_restore() -> Result<(), BoxError> {
    security::delete_all_sessions()?;

    if config().api.cache.enabled
        && let Some(store) = cache::cache_store()
    {
        store.purge_all();
    }

    security::init_admin_user_if_not_exists()?;

    acl::init_acl()?;

    hook::init_hooks();

    storage::write_to_db();

    Ok(())
}

fn open_archive<R: Read>(src: R) -> Result<ArchiveReader<R>, BoxError> {
    let gz =
        gzip::new_reader(src).map_err(|err| invalid_archive(format!("not a gzip file: {err}")))?;

    Ok(ArchiveReader {
        tr: tar::Reader::new(gz),
    })
}

impl<R: Read> ArchiveReader<R> {
    fn next(&mut self, expected: &str) -> Result<(), BoxError> {
        let header = match self.tr.next() {
            Ok(Some(header)) => header,
            Ok(None) => return Err(invalid_archive(format!("{expected} is missing"))),
            Err(err) => return Err(invalid_archive(format!("unreadable tar stream: {err}"))),
        };

        if header.name != expected.as_bytes() {
            return Err(invalid_archive(format!(
                "expected {expected}, found {}",
                text(&header.name)
            )));
        }

        if header.typeflag != TYPE_REG {
            return Err(invalid_archive(format!("{expected} is not a regular file")));
        }

        Ok(())
    }

    fn end(&mut self) -> Result<(), BoxError> {
        match self.tr.next() {
            Ok(None) => Ok(()),
            Err(err) => Err(invalid_archive(format!("unreadable tar stream: {err}"))),
            Ok(Some(header)) => Err(invalid_archive(format!(
                "unexpected file {}",
                text(&header.name)
            ))),
        }
    }

    fn read_manifest(&mut self) -> Result<Manifest, BoxError> {
        self.next(MANIFEST_PATH)?;

        let mut data = Vec::new();
        if let Err(err) = (&mut self.tr)
            .take(MAX_MANIFEST_SIZE + 1)
            .read_to_end(&mut data)
        {
            return Err(invalid_archive(format!(
                "{MANIFEST_PATH} cannot be read: {err}"
            )));
        }

        if data.len() as u64 > MAX_MANIFEST_SIZE {
            return Err(invalid_archive(format!("{MANIFEST_PATH} is too large")));
        }

        let mut manifest = Manifest::default();
        if let Err(err) = json::unmarshal(&data, &mut manifest) {
            return Err(invalid_archive(format!(
                "{MANIFEST_PATH} is not valid JSON: {}",
                err.reason(Subject::Value)
            )));
        }

        manifest.validate()?;

        Ok(manifest)
    }
}

fn read_key<R: Read>(r: R) -> Result<Vec<u8>, BoxError> {
    let mut data = Vec::new();
    if let Err(err) = r.take(MAX_KEY_SIZE + 1).read_to_end(&mut data) {
        return Err(invalid_archive(format!("{KEY_PATH} cannot be read: {err}")));
    }

    if data.len() as u64 > MAX_KEY_SIZE {
        return Err(invalid_archive(format!("{KEY_PATH} is too large")));
    }

    let key = strings::trim_space(&data);
    if key.is_empty() || key.iter().any(|c| b" \t\r\n".contains(c)) {
        return Err(invalid_archive(format!(
            "{KEY_PATH} does not hold a valid key"
        )));
    }

    Ok(key.to_vec())
}

fn read_schema<R: Read>(r: R, path: &str, entity: &[u8]) -> Result<Map, BoxError> {
    let mut decoder = Decoder::new(r);

    let mut data: Option<Map> = None;
    let decoded = decoder.decode(&mut data);
    let data = match (decoded, data) {
        (Ok(()), Some(data)) => data,
        _ => return Err(invalid_archive(format!("{path} is not a JSON object"))),
    };

    let describes = matches!(data.get("id"), Some(Value::String(id)) if id.as_bytes() == entity);
    if !describes {
        return Err(invalid_archive(format!(
            "{path} describes {} instead of {}",
            value_text(data.get("id")),
            quote(entity)
        )));
    }

    if let Some(fields) = data.get("fields")
        && !matches!(fields, Value::Object(_))
    {
        return Err(invalid_archive(format!("{path} has invalid fields")));
    }

    let mut extra = Value::Null;
    match decoder.decode(&mut extra) {
        Err(err) if err.is_eof() => {}
        _ => {
            return Err(invalid_archive(format!(
                "{path} holds more than one schema"
            )));
        }
    }

    Ok(data)
}

const ID_HASH_SIZE: usize = 16;

type RawDocument = (Vec<u8>, Vec<u8>);

struct DocumentReader<R> {
    decoder: Decoder<R>,
    path: String,
    line: i64,
    ids: Option<HashSet<[u8; ID_HASH_SIZE]>>,
}

impl<R: Read> DocumentReader<R> {
    fn new(r: R, path: &str, checked: bool) -> DocumentReader<R> {
        DocumentReader {
            decoder: Decoder::new(r),
            path: path.to_string(),
            line: 0,
            ids: checked.then(HashSet::new),
        }
    }

    fn next_raw(&mut self) -> Result<Option<RawDocument>, BoxError> {
        let mut raw = RawMessage::default();
        if let Err(err) = self.decoder.decode(&mut raw) {
            if err.is_eof() {
                return Ok(None);
            }

            return Err(invalid_archive(format!(
                "{}, document {}: {err}",
                self.path,
                self.line + 1
            )));
        }

        let raw = raw.0;
        let id = match simple_document_id(&raw) {
            Some(id) => id.to_vec(),
            None => {
                let mut data: Option<Map> = None;
                if let Err(err) = json::unmarshal(&raw, &mut data) {
                    return Err(invalid_archive(format!(
                        "{}, document {}: {err}",
                        self.path,
                        self.line + 1
                    )));
                }

                match data.as_ref().and_then(|d| d.get("id")) {
                    Some(Value::String(id)) => id.as_bytes().to_vec(),
                    _ => Vec::new(),
                }
            }
        };

        self.line += 1;

        if id.is_empty() {
            return Err(invalid_archive(format!(
                "{}, document {}: a document must be an object with a string id",
                self.path, self.line
            )));
        }

        if let Some(ids) = &mut self.ids {
            let sum = sha256::sum256(&id);
            let mut key = [0u8; ID_HASH_SIZE];
            key.copy_from_slice(&sum[..ID_HASH_SIZE]);
            if !ids.insert(key) {
                return Err(invalid_archive(format!(
                    "{}, document {}: duplicate id {}",
                    self.path,
                    self.line,
                    quote(&id)
                )));
            }
        }

        Ok(Some((id, raw)))
    }

    fn count(&mut self) -> Result<i64, BoxError> {
        while self.next_raw()?.is_some() {}

        Ok(self.line)
    }
}

struct KvReader<R> {
    decoder: Decoder<R>,
    path: String,
    line: i64,
    keys: HashSet<Vec<u8>>,
}

impl<R: Read> KvReader<R> {
    fn new(r: R, path: &str) -> KvReader<R> {
        let mut decoder = Decoder::new(r);
        decoder.disallow_unknown_fields();

        KvReader {
            decoder,
            path: path.to_string(),
            line: 0,
            keys: HashSet::new(),
        }
    }

    fn next(&mut self) -> Result<Option<(KvEntry, Vec<u8>)>, BoxError> {
        let mut entry = KvEntry::default();
        if let Err(err) = self.decoder.decode(&mut entry) {
            if err.is_eof() {
                return Ok(None);
            }

            return Err(invalid_archive(format!(
                "{}, entry {}: {err}",
                self.path,
                self.line + 1
            )));
        }

        self.line += 1;

        let key = entry.key.as_bytes();
        if key.is_empty() || globals::is_api_key(key) {
            return Err(invalid_archive(format!(
                "{}, entry {}: invalid key {}",
                self.path,
                self.line,
                quote(key)
            )));
        }

        if !self.keys.insert(key.to_vec()) {
            return Err(invalid_archive(format!(
                "{}, entry {}: duplicate key {}",
                self.path,
                self.line,
                quote(key)
            )));
        }

        if entry.expires_at < 0 {
            return Err(invalid_archive(format!(
                "{}, entry {}: invalid expiration {}",
                self.path, self.line, entry.expires_at
            )));
        }

        match entry.encoding.as_str() {
            "" => {
                let value = entry.value.as_bytes().to_vec();
                Ok(Some((entry, value)))
            }
            KV_ENCODING_BASE64 => match STD_ENCODING.decode_string(entry.value.as_bytes()) {
                Ok(value) => Ok(Some((entry, value))),
                Err(_) => Err(invalid_archive(format!(
                    "{}, entry {}: invalid base64 value",
                    self.path, self.line
                ))),
            },
            other => Err(invalid_archive(format!(
                "{}, entry {}: unknown encoding {}",
                self.path,
                self.line,
                quote(other.as_bytes())
            ))),
        }
    }

    fn count(&mut self) -> Result<i64, BoxError> {
        while self.next()?.is_some() {}

        Ok(self.line)
    }
}
