use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs as stdfs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::durable::fs::fsync_fd;
use crate::durable::{Fs, FsError, OS, filepath};
use crate::gocompat::base64::STD_ENCODING;
use crate::gocompat::json::{
    self, Base64Bytes, Decode, DecodeState, Encode, EncodeState, Expected, Map, Subject,
};
use crate::gocompat::{strconv, strings};
use crate::json_struct;

pub const DATA_FILE: &str = "elysiandb.json";
pub const JSON_DATA_FILE: &str = "elysiandbjson.json";
pub const EXPIRATION_FILE: &str = "elysiandb.expiration.json";
pub const JSON_RECOVERY_FILE: &str = "elysiandb.json.recovery.log";
pub const STORE_RECOVERY_FILE: &str = "elysiandb.store.recovery.log";
pub const FAILED_SUFFIX: &str = ".failed";
pub const API_KEY_PREFIX: &str = "api:entity:";
pub const TYPES_LIST_KEY: &str = "api:entity:types:list";

pub const FILES: [&str; 7] = [
    DATA_FILE,
    JSON_DATA_FILE,
    EXPIRATION_FILE,
    JSON_RECOVERY_FILE,
    STORE_RECOVERY_FILE,
    "elysiandb.json.recovery.log.failed",
    "elysiandb.store.recovery.log.failed",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub id: Vec<u8>,
    pub doc: Option<Map>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub kv: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    pub expirations: BTreeMap<Vec<u8>, i64>,
    pub types: Vec<Vec<u8>>,
    pub entities: BTreeMap<Vec<u8>, Vec<Document>>,
    pub replayed: usize,
}

#[derive(Clone, Debug)]
pub enum Error {
    Fs(FsError),
    Json(json::Error),
    File(String, Box<Error>),
}

impl Error {
    pub fn json(&self) -> Option<&json::Error> {
        match self {
            Error::Json(e) => Some(e),
            Error::File(_, inner) => inner.json(),
            Error::Fs(_) => None,
        }
    }

    pub fn fs_error(&self) -> Option<&FsError> {
        match self {
            Error::Fs(e) => Some(e),
            Error::File(_, inner) => inner.fs_error(),
            Error::Json(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Fs(e) => fmt::Display::fmt(e, f),
            Error::Json(e) => f.write_str(&e.message(Subject::File)),
            Error::File(name, inner) => write!(f, "{name}: {inner}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Fs(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::File(_, inner) => Some(&**inner),
        }
    }
}

impl From<FsError> for Error {
    fn from(e: FsError) -> Error {
        Error::Fs(e)
    }
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub fn id_index_key(entity: &[u8]) -> Vec<u8> {
    concat(&[API_KEY_PREFIX.as_bytes(), entity, b":internal:index:id"])
}

pub fn document_key(entity: &[u8], id: &[u8]) -> Vec<u8> {
    concat(&[API_KEY_PREFIX.as_bytes(), entity, b":id:", id])
}

pub fn present(folder: &Path) -> bool {
    FILES
        .iter()
        .any(|name| stdfs::metadata(filepath::join(folder, name)).is_ok())
}

pub fn files_in(folder: &Path) -> Vec<&'static str> {
    FILES
        .iter()
        .copied()
        .filter(|name| stdfs::metadata(filepath::join(folder, name)).is_ok())
        .collect()
}

fn read_os_file(path: &Path) -> Result<Vec<u8>, FsError> {
    let mut f = stdfs::File::open(path).map_err(|e| FsError::new("open", path, e))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| FsError::new("read", path, e))?;
    Ok(data)
}

fn read_json_file<T: Decode + ?Sized>(path: &Path, out: &mut T) -> Result<bool, Error> {
    let data = read_os_file(path)?;

    let trimmed = strings::trim_space(&data);
    if trimmed.is_empty() {
        return Ok(false);
    }

    json::unmarshal(&data, out).map_err(Error::Json)?;

    Ok(trimmed == b"null")
}

#[derive(Default)]
struct KvValue(Option<Vec<u8>>);

impl Decode for KvValue {
    const EXPECTED: Expected = Expected::String;
    const MAP_EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), json::Error> {
        let mut v: Option<Base64Bytes> = None;
        let res = v.decode(d);
        self.0 = v.map(|b| b.0);
        res
    }
}

type KvMap = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

type DocMap = BTreeMap<Vec<u8>, Option<Map>>;

fn read_kv(path: &Path) -> Result<(KvMap, bool), Error> {
    let mut kv: BTreeMap<Vec<u8>, KvValue> = BTreeMap::new();
    let null = read_json_file(path, &mut kv)?;

    Ok((kv.into_iter().map(|(k, v)| (k, v.0)).collect(), null))
}

pub fn read_kv_file(path: &Path) -> Result<KvMap, Error> {
    read_kv(path).map(|(kv, _)| kv)
}

pub fn read_expirations_file(path: &Path) -> Result<BTreeMap<i64, Vec<Vec<u8>>>, Error> {
    let mut raw: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();
    read_json_file(path, &mut raw)?;

    let mut out = BTreeMap::new();
    for (k, list) in raw {
        let Ok(ts) = strconv::parse_int(&k, 10, 64) else {
            continue;
        };

        out.insert(ts, list);
    }

    Ok(out)
}

fn read_documents(path: &Path) -> Result<(DocMap, bool), Error> {
    let mut docs: DocMap = BTreeMap::new();
    let null = read_json_file(path, &mut docs)?;

    Ok((docs, null))
}

pub fn read_documents_file(path: &Path) -> Result<DocMap, Error> {
    read_documents(path).map(|(docs, _)| docs)
}

fn read_if_present<T>(
    path: &Path,
    read: fn(&Path) -> Result<T, Error>,
    empty: T,
) -> Result<T, Error> {
    if let Err(e) = stdfs::metadata(path)
        && e.kind() == io::ErrorKind::NotFound
    {
        return Ok(empty);
    }

    read(path).map_err(|err| {
        let name = filepath::base(path);
        Error::File(
            String::from_utf8_lossy(name.as_bytes()).into_owned(),
            Box::new(err),
        )
    })
}

fn unix_seconds(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => {
            let d = e.duration();
            let whole = -(d.as_secs() as i64);
            if d.subsec_nanos() > 0 {
                whole - 1
            } else {
                whole
            }
        }
    }
}

pub fn read(folder: &Path, replay_recovery: bool, now: SystemTime) -> Result<Data, Error> {
    let (mut kv, kv_null) = read_if_present(
        &filepath::join(folder, DATA_FILE),
        read_kv,
        (BTreeMap::new(), false),
    )?;

    let raw_exp = read_if_present(
        &filepath::join(folder, EXPIRATION_FILE),
        read_expirations_file,
        BTreeMap::new(),
    )?;

    let (mut docs, docs_null) = read_if_present(
        &filepath::join(folder, JSON_DATA_FILE),
        read_documents,
        (BTreeMap::new(), false),
    )?;

    let mut expirations: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    for (at, keys) in raw_exp {
        for k in keys {
            expirations.insert(k, at);
        }
    }

    let now_unix = unix_seconds(now);
    expirations.retain(|k, at| {
        if *at < now_unix {
            kv.remove(k);
            return false;
        }
        true
    });

    let mut replayed = 0;

    if replay_recovery {
        replayed += replay_json_log(
            &filepath::join(folder, JSON_RECOVERY_FILE),
            &mut docs,
            docs_null,
        )?;

        replayed += replay_store_log(
            &filepath::join(folder, STORE_RECOVERY_FILE),
            &mut kv,
            kv_null,
            &mut expirations,
            now_unix,
        )?;
    }

    let (types, entities) = repair(&kv, docs);

    Ok(Data {
        kv,
        expirations,
        types,
        entities,
        replayed,
    })
}

const LINE_BUFFER: usize = 4096;

fn each_line(path: &Path, mut f: impl FnMut(&[u8])) -> Result<(), Error> {
    let file = match stdfs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(FsError::new("open", path, e).into()),
    };

    let mut r = BufReader::with_capacity(LINE_BUFFER, file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let res = r.read_until(b'\n', &mut line);

        let trimmed = strings::trim_space(&line);
        if !trimmed.is_empty() {
            f(trimmed);
        }

        match res {
            Ok(0) => return Ok(()),
            Ok(_) if line.last() != Some(&b'\n') => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(FsError::new("read", path, e).into()),
        }
    }
}

json_struct! {
    #[derive(Default)]
    struct JsonOp {
        op: String => "op",
        key: Vec<u8> => "key",
        value: Option<Map> => "value" [omitempty],
    }
}

json_struct! {
    #[derive(Default)]
    struct StoreOp {
        op: String => "op",
        key: Vec<u8> => "key",
        value: Option<Base64Bytes> => "value" [omitempty],
        ttl: i64 => "ttl" [omitempty],
    }
}

fn replay_json_log(path: &Path, docs: &mut DocMap, null: bool) -> Result<usize, Error> {
    let mut n = 0;
    each_line(path, |line| {
        let mut o = JsonOp::default();
        if json::unmarshal(line, &mut o).is_err() {
            return;
        }

        match o.op.as_str() {
            "put" => {
                if null {
                    panic!("{JSON_DATA_FILE} holds null, a logged document cannot be stored in it");
                }
                docs.insert(o.key, o.value);
                n += 1;
            }
            "del" => {
                docs.remove(&o.key);
                n += 1;
            }
            _ => {}
        }
    })?;

    Ok(n)
}

fn replay_store_log(
    path: &Path,
    kv: &mut KvMap,
    null: bool,
    expirations: &mut BTreeMap<Vec<u8>, i64>,
    now: i64,
) -> Result<usize, Error> {
    let mut n = 0;
    each_line(path, |line| {
        let mut o = StoreOp::default();
        if json::unmarshal(line, &mut o).is_err() {
            return;
        }

        match o.op.as_str() {
            "put" => {
                if o.ttl > 0 && now >= o.ttl {
                    return;
                }

                if null {
                    panic!("{DATA_FILE} holds null, a logged key cannot be stored in it");
                }
                kv.insert(o.key.clone(), o.value.map(|v| v.0));
                if o.ttl > 0 {
                    expirations.insert(o.key, o.ttl);
                }
                n += 1;
            }
            "del" => {
                kv.remove(&o.key);
                expirations.remove(&o.key);
                n += 1;
            }
            _ => {}
        }
    })?;

    Ok(n)
}

fn decode_ids(b: Option<&Option<Vec<u8>>>) -> Vec<Vec<u8>> {
    let Some(Some(b)) = b else {
        return Vec::new();
    };

    b.split(|&c| c == b'\n')
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn cut<'a>(s: &'a [u8], sep: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    let i = s.windows(sep.len()).position(|w| w == sep)?;
    Some((&s[..i], &s[i + sep.len()..]))
}

fn parse_document_key(key: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = key.strip_prefix(API_KEY_PREFIX.as_bytes())?;

    let (entity, id) = cut(rest, b":id:")?;
    if entity.is_empty() || id.is_empty() {
        return None;
    }

    Some((entity, id))
}

type Repaired = (Vec<Vec<u8>>, BTreeMap<Vec<u8>, Vec<Document>>);

fn repair(kv: &KvMap, docs: DocMap) -> Repaired {
    let mut by_entity: BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Option<Map>>> = BTreeMap::new();
    for (key, doc) in docs {
        let Some((entity, id)) = parse_document_key(&key) else {
            continue;
        };

        by_entity
            .entry(entity.to_vec())
            .or_default()
            .insert(id.to_vec(), doc);
    }

    let mut types = decode_ids(kv.get(TYPES_LIST_KEY.as_bytes()));
    let mut known: BTreeSet<Vec<u8>> = types.iter().cloned().collect();

    for entity in by_entity.keys() {
        if !known.contains(entity) {
            types.push(entity.clone());
            known.insert(entity.clone());
        }
    }

    let mut out: BTreeMap<Vec<u8>, Vec<Document>> = BTreeMap::new();
    for entity in &types {
        if out.contains_key(entity) {
            continue;
        }

        let mut present = by_entity.remove(entity).unwrap_or_default();
        let mut list = Vec::new();
        for id in decode_ids(kv.get(&id_index_key(entity))) {
            let Some(doc) = present.remove(&id) else {
                continue;
            };

            list.push(Document { id, doc });
        }

        for (id, doc) in present {
            list.push(Document { id, doc });
        }

        out.insert(entity.clone(), list);
    }

    (dedupe(types), out)
}

fn dedupe(list: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        if seen.insert(v.clone()) {
            out.push(v);
        }
    }

    out
}

struct KvOut<'a>(Option<Cow<'a, [u8]>>);

impl Encode for KvOut<'_> {
    fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
        match &self.0 {
            None => e.write_null(),
            Some(b) => {
                let mut s = Vec::with_capacity(STD_ENCODING.encoded_len(b.len()) + 2);
                s.push(b'"');
                STD_ENCODING.append_encode(&mut s, b);
                s.push(b'"');
                e.write_raw(&s);
            }
        }
        Ok(())
    }
}

fn join_lines<'a>(parts: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, p) in parts.enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(p);
    }
    out
}

pub fn write(folder: &Path, d: &Data) -> Result<(), Error> {
    OS.mkdir_all(folder, 0o755)?;

    let mut kv: BTreeMap<Vec<u8>, KvOut<'_>> = BTreeMap::new();
    for (k, v) in &d.kv {
        if !k.starts_with(API_KEY_PREFIX.as_bytes()) {
            kv.insert(k.clone(), KvOut(v.as_deref().map(Cow::Borrowed)));
        }
    }

    if !d.types.is_empty() {
        let joined = join_lines(d.types.iter().map(Vec::as_slice));
        kv.insert(
            TYPES_LIST_KEY.as_bytes().to_vec(),
            KvOut(Some(Cow::Owned(joined))),
        );
    }

    let mut docs: BTreeMap<Vec<u8>, &Option<Map>> = BTreeMap::new();
    for entity in &d.types {
        let list = d.entities.get(entity).map(Vec::as_slice).unwrap_or(&[]);
        for doc in list {
            docs.insert(document_key(entity, &doc.id), &doc.doc);
        }

        if !list.is_empty() {
            let joined = join_lines(list.iter().map(|doc| doc.id.as_slice()));
            kv.insert(id_index_key(entity), KvOut(Some(Cow::Owned(joined))));
        }
    }

    let mut exp: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for (k, at) in &d.expirations {
        if kv.contains_key(k) {
            exp.entry(strconv::format_int(*at, 10))
                .or_default()
                .push(k.clone());
        }
    }

    for keys in exp.values_mut() {
        keys.sort();
    }

    write_json(&filepath::join(folder, EXPIRATION_FILE), &exp)?;
    write_json(&filepath::join(folder, DATA_FILE), &kv)?;
    write_json(&filepath::join(folder, JSON_DATA_FILE), &docs)?;

    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn write_json<T: Encode + ?Sized>(path: &Path, value: &T) -> Result<(), Error> {
    let tmp = with_suffix(path, ".tmp");
    let f = stdfs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| FsError::new("open", &tmp, e))?;

    let mut err: Option<Error> = match json::marshal(value) {
        Ok(mut b) => {
            b.push(b'\n');
            (&f).write_all(&b)
                .err()
                .map(|e| FsError::new("write", &tmp, e).into())
        }
        Err(e) => Some(Error::Json(e)),
    };

    if err.is_none() {
        err = fsync_fd(f.as_raw_fd())
            .err()
            .map(|e| FsError::new("sync", &tmp, e).into());
    }

    let fd = f.into_raw_fd();
    if unsafe { libc::close(fd) } != 0 && err.is_none() {
        err = Some(FsError::new("close", &tmp, io::Error::last_os_error()).into());
    }

    if let Some(err) = err {
        let _ = OS.remove(&tmp);
        return Err(err);
    }

    OS.rename(&tmp, path)?;

    Ok(())
}
