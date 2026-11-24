use std::collections::HashMap;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::crypto::{rand, sha256, subtle};
use crate::durable::FsError;
use crate::engine;
use crate::gocompat::json::{self, Decoder};
use crate::httpd::RequestCtx;
use crate::json_struct;
use crate::sync::{Mutex, RwLock};

use super::authentication::{
    authentication_is_enabled, basic_authentication_is_enabled, token_authentication_is_enabled,
};
use super::basic_user::{
    BasicHashedUser, Role, SESSIONS_FILENAME, SecurityError, USER_ENTITY,
    check_basic_authentication, hex_string,
};
use super::token::check_token_authentication;
use super::user_auth::{SESSION_COOKIE_NAME, USERNAME_KEY};
use super::{config, text};

pub const SESSION_TTL: Duration = Duration::from_secs(24 * 3600);

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Session {
        pub id: String => -,
        pub token_hash: String => "token_hash",
        pub username: String => "username",
        pub role: Role => "role",
        pub credentials: String => "credentials",
        pub expires_at: i64 => "expires_at",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct SessionsFile {
        pub sessions: Option<Vec<Session>> => "sessions",
    }
}

pub fn get_current_username(ctx: &RequestCtx) -> String {
    ctx.user_value::<String>(USERNAME_KEY)
        .cloned()
        .unwrap_or_default()
}

pub fn current_user_can_manage_user(
    ctx: &RequestCtx,
    username: &[u8],
) -> Result<bool, SecurityError> {
    let Some(current) = current_session(ctx)? else {
        return Ok(false);
    };

    Ok(current_user_is_admin(ctx) || current.username.as_bytes() != username)
}

pub fn current_session(ctx: &RequestCtx) -> Result<Option<Session>, SecurityError> {
    get_session(ctx.request.header.cookie(SESSION_COOKIE_NAME))
}

pub fn current_user_is_admin(ctx: &RequestCtx) -> bool {
    if !authentication_is_enabled() {
        return true;
    }

    if token_authentication_is_enabled() {
        return check_token_authentication(ctx);
    }

    if basic_authentication_is_enabled() {
        return check_basic_authentication(ctx);
    }

    match current_session(ctx) {
        Ok(Some(session)) => session.role.is_admin(),
        _ => false,
    }
}

fn generate_session_id() -> Result<String, SecurityError> {
    let mut b = [0u8; 32];
    rand::os_random(&mut b).map_err(SecurityError::Random)?;

    Ok(hex_string(&b))
}

pub fn hash_session_id(id: &[u8]) -> String {
    hex_string(&sha256::sum256(id))
}

pub fn credentials_fingerprint(password_hash: &[u8]) -> String {
    hex_string(&sha256::sum256(password_hash)[..16])
}

fn stored_user(username: &[u8]) -> Option<BasicHashedUser> {
    let data = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username)?;

    BasicHashedUser::from_data_map(&data).ok()
}

struct SessionsCacheEntry {
    path: String,
    file: SessionsFile,
    by_hash: HashMap<String, usize>,
}

static SESSIONS_CACHE: RwLock<Option<Arc<SessionsCacheEntry>>> = RwLock::new(None);

static SESSIONS_MU: Mutex<()> = Mutex::new(());

fn sessions_path() -> String {
    format!("{}/{}", config().store.folder, SESSIONS_FILENAME)
}

fn copy_sessions(sf: &SessionsFile) -> SessionsFile {
    let sessions = match &sf.sessions {
        Some(list) if !list.is_empty() => Some(list.clone()),
        _ => None,
    };

    SessionsFile { sessions }
}

fn new_sessions_cache_entry(path: String, sf: &SessionsFile) -> SessionsCacheEntry {
    let file = copy_sessions(sf);
    let mut by_hash = HashMap::new();
    for (i, s) in file.sessions.iter().flatten().enumerate() {
        if !s.token_hash.is_empty() {
            by_hash.insert(s.token_hash.clone(), i);
        }
    }

    SessionsCacheEntry {
        path,
        file,
        by_hash,
    }
}

fn cached_sessions() -> Result<Arc<SessionsCacheEntry>, SecurityError> {
    let path = sessions_path();

    let cached = SESSIONS_CACHE.read().clone();
    if let Some(cached) = cached
        && cached.path == path
    {
        return Ok(cached);
    }

    let sf = read_sessions_file(&path)?;

    let entry = Arc::new(new_sessions_cache_entry(path, &sf));
    cache_sessions(Some(entry.clone()));

    Ok(entry)
}

pub fn read_sessions_file(path: &str) -> Result<SessionsFile, SecurityError> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(SessionsFile {
                sessions: Some(Vec::new()),
            });
        }
        Err(e) => return Err(SecurityError::Fs(FsError::new("open", path, e))),
    };

    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|e| SecurityError::Fs(FsError::new("read", path, e)))?;

    let mut sf = SessionsFile::default();
    Decoder::new(data.as_slice())
        .decode(&mut sf)
        .map_err(SecurityError::Json)?;

    Ok(sf)
}

fn load_sessions() -> Result<SessionsFile, SecurityError> {
    let entry = cached_sessions()?;

    Ok(copy_sessions(&entry.file))
}

fn cache_sessions(entry: Option<Arc<SessionsCacheEntry>>) {
    *SESSIONS_CACHE.write() = entry;
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

fn is_active(s: &Session, now: i64) -> bool {
    s.expires_at > now && !s.token_hash.is_empty()
}

pub fn active_session_count() -> usize {
    let Ok(entry) = cached_sessions() else {
        return 0;
    };

    let now = unix_now();

    entry
        .file
        .sessions
        .iter()
        .flatten()
        .filter(|s| is_active(s, now))
        .count()
}

fn save_sessions(sf: &SessionsFile) -> Result<(), SecurityError> {
    let path = sessions_path();
    let tmp_path = format!("{path}.tmp");

    let mut data = json::marshal(sf).unwrap_or_default();
    data.push(b'\n');

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp_path)
        .map_err(|e| SecurityError::Fs(FsError::new("open", &tmp_path, e)))?;
    file.write_all(&data)
        .map_err(|e| SecurityError::Fs(FsError::new("write", &tmp_path, e)))?;
    drop(file);

    let _ = fs::set_permissions(&tmp_path, Permissions::from_mode(0o600));

    fs::rename(&tmp_path, &path)
        .map_err(|e| SecurityError::Fs(FsError::new("rename", &tmp_path, e)))?;

    cache_sessions(Some(Arc::new(new_sessions_cache_entry(path, sf))));

    Ok(())
}

pub fn create_session(
    username: &[u8],
    role: Role,
    ttl: Duration,
) -> Result<Session, SecurityError> {
    let Some(user) = stored_user(username) else {
        return Err(SecurityError::UserNotFoundWrapped(username.to_vec()));
    };

    let _guard = SESSIONS_MU.lock();

    let mut sf = load_sessions()?;

    let now = unix_now();
    let mut active: Vec<Session> = sf
        .sessions
        .take()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| is_active(s, now))
        .collect();

    let id = generate_session_id()?;

    let session = Session {
        token_hash: hash_session_id(id.as_bytes()),
        id,
        username: text(username),
        role,
        credentials: credentials_fingerprint(user.password.as_bytes()),
        expires_at: now + i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX),
    };

    active.push(session.clone());
    sf.sessions = Some(active);

    save_sessions(&sf)?;

    Ok(session)
}

pub fn get_session(id: &[u8]) -> Result<Option<Session>, SecurityError> {
    if id.is_empty() {
        return Ok(None);
    }

    let entry = cached_sessions()?;

    let hash = hash_session_id(id);
    let Some(&i) = entry.by_hash.get(&hash) else {
        return Ok(None);
    };

    let Some(s) = entry.file.sessions.as_ref().and_then(|list| list.get(i)) else {
        return Ok(None);
    };
    if s.expires_at <= unix_now()
        || subtle::constant_time_compare(s.token_hash.as_bytes(), hash.as_bytes()) != 1
    {
        return Ok(None);
    }

    let mut found = s.clone();
    let Some(user) = stored_user(found.username.as_bytes()) else {
        return Ok(None);
    };
    if subtle::constant_time_compare(
        found.credentials.as_bytes(),
        credentials_fingerprint(user.password.as_bytes()).as_bytes(),
    ) != 1
    {
        return Ok(None);
    }

    found.id = text(id);
    found.role = user.role;

    Ok(Some(found))
}

pub fn protect_sessions_file() {
    let _ = fs::set_permissions(sessions_path(), Permissions::from_mode(0o600));
}

pub fn delete_all_sessions() -> Result<(), SecurityError> {
    let _guard = SESSIONS_MU.lock();

    cache_sessions(None);

    let path = sessions_path();
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(SecurityError::Fs(FsError::new("remove", &path, e)))
        }
        _ => Ok(()),
    }
}

pub fn delete_session(id: &[u8]) -> Result<(), SecurityError> {
    let _guard = SESSIONS_MU.lock();

    let mut sf = load_sessions()?;

    let hash = hash_session_id(id);
    let now = unix_now();
    let updated: Vec<Session> = sf
        .sessions
        .take()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.token_hash != hash && is_active(s, now))
        .collect();

    sf.sessions = if updated.is_empty() {
        None
    } else {
        Some(updated)
    };

    save_sessions(&sf)
}

pub fn delete_user_sessions(username: &[u8]) -> Result<(), SecurityError> {
    delete_user_sessions_except(username, b"", "")
}

pub(crate) fn delete_user_sessions_except(
    username: &[u8],
    kept_session_id: &[u8],
    credentials: &str,
) -> Result<(), SecurityError> {
    let _guard = SESSIONS_MU.lock();

    let mut sf = load_sessions()?;

    let kept_hash = if kept_session_id.is_empty() {
        String::new()
    } else {
        hash_session_id(kept_session_id)
    };

    let mut changed = false;
    let mut updated = Vec::new();
    for mut s in sf.sessions.take().unwrap_or_default() {
        if s.username.as_bytes() != username {
            updated.push(s);
            continue;
        }

        if !kept_hash.is_empty() && s.token_hash == kept_hash {
            s.credentials = credentials.to_string();
            updated.push(s);
            changed = true;
            continue;
        }

        changed = true;
    }

    if !changed {
        return Ok(());
    }

    sf.sessions = Some(updated);

    save_sessions(&sf)
}
