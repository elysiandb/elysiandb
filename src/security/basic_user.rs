use std::fmt;
use std::fs::{OpenOptions, Permissions};
use std::io::{self, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::crypto::{bcrypt, rand, sha256};
use crate::durable::{self, Fs, FsError};
use crate::engine::{self, ByteFields};
use crate::gocompat::base64::STD_ENCODING;
use crate::gocompat::fmt::{ScanError, fscanf_word};
use crate::gocompat::json::{
    self, Decode, DecodeState, Encode, EncodeState, Expected, Kind, Map, Value,
};
use crate::gocompat::{unicode, utf8};
use crate::httpd::{HEADER_AUTHORIZATION, RequestCtx};
use crate::sync::RwLock;

use super::login::authenticate_user_from;
use super::login_source::login_source_from_request;
use super::session::{credentials_fingerprint, delete_user_sessions, delete_user_sessions_except};
use super::{config, text};

pub const KEY_FILENAME: &str = "users.key";
pub const SESSIONS_FILENAME: &str = "sessions.json";
pub const USER_ENTITY: &str = "_elysiandb_core_user";
pub const DEFAULT_ADMIN_USERNAME: &str = "admin";
pub const DEFAULT_ADMIN_PASSWORD: &str = "admin";
pub const ADMIN_PASSWORD_ENV_VAR: &str = "ELYSIANDB_ADMIN_PASSWORD";
pub const MIN_PASSWORD_LENGTH: usize = 8;
pub const MAX_USERNAME_LENGTH: usize = 128;
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";

const DUMMY_PASSWORD: &[u8] = b"elysiandb-dummy-password";

#[derive(Debug)]
pub enum SecurityError {
    UserNotFoundWrapped(Vec<u8>),
    UserNotFound(Vec<u8>),
    InvalidRole,
    DefaultAdminProtected,
    LastAdmin,
    UserAlreadyExists,
    InvalidUsername,
    PasswordTooShort,
    InvalidUserData(&'static str),
    Validation(String),
    KeyGeneration(io::Error),
    NoKeyFound(String),
    Random(io::Error),
    Hash(bcrypt::Error),
    Fs(FsError),
    Json(json::Error),
    Storage(String),
}

impl SecurityError {
    pub fn is_user_not_found(&self) -> bool {
        matches!(self, SecurityError::UserNotFoundWrapped(_))
    }

    pub fn message(&self) -> Vec<u8> {
        match self {
            SecurityError::UserNotFoundWrapped(name) | SecurityError::UserNotFound(name) => {
                let mut out = b"user '".to_vec();
                out.extend_from_slice(name);
                out.extend_from_slice(b"' not found");
                out
            }
            other => other.to_string().into_bytes(),
        }
    }
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityError::UserNotFoundWrapped(name) | SecurityError::UserNotFound(name) => {
                write!(f, "user '{}' not found", text(name))
            }
            SecurityError::InvalidRole => f.write_str("invalid role: expected 'admin' or 'user'"),
            SecurityError::DefaultAdminProtected => {
                f.write_str("the default admin user cannot be deleted")
            }
            SecurityError::LastAdmin => f.write_str("at least one admin user must remain"),
            SecurityError::UserAlreadyExists => f.write_str("user already exists"),
            SecurityError::InvalidUsername => f.write_str(
                "invalid username: 1 to 128 characters, without spaces, control characters or '/'",
            ),
            SecurityError::PasswordTooShort => write!(
                f,
                "password must be at least {MIN_PASSWORD_LENGTH} characters long"
            ),
            SecurityError::InvalidUserData(field) => write!(f, "invalid {field} in user data"),
            SecurityError::Validation(text) => f.write_str(text),
            SecurityError::KeyGeneration(e) => write!(f, "failed to generate key: {e}"),
            SecurityError::NoKeyFound(path) => write!(f, "read {path}: no key found"),
            SecurityError::Random(e) => write!(f, "cannot read random bytes: {e}"),
            SecurityError::Hash(e) => e.fmt(f),
            SecurityError::Fs(e) => e.fmt(f),
            SecurityError::Json(e) => e.fmt(f),
            SecurityError::Storage(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for SecurityError {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Role(pub String);

impl Role {
    pub fn new(role: impl Into<String>) -> Role {
        Role(role.into())
    }

    pub fn admin() -> Role {
        Role(ROLE_ADMIN.to_string())
    }

    pub fn user() -> Role {
        Role(ROLE_USER.to_string())
    }

    pub fn is_admin(&self) -> bool {
        self.0 == ROLE_ADMIN
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Decode for Role {
    const EXPECTED: Expected = Expected::String;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), json::Error> {
        self.0.decode(d)
    }
}

impl Encode for Role {
    fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
        self.0.encode(e)
    }

    fn is_empty_value(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteString(pub Vec<u8>);

impl ByteString {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl From<&[u8]> for ByteString {
    fn from(b: &[u8]) -> ByteString {
        ByteString(b.to_vec())
    }
}

impl From<&str> for ByteString {
    fn from(s: &str) -> ByteString {
        ByteString(s.as_bytes().to_vec())
    }
}

impl Decode for ByteString {
    const EXPECTED: Expected = Expected::String;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), json::Error> {
        if d.peek() == Kind::Null {
            d.skip();
            return Ok(());
        }
        let mut text = String::new();
        text.decode(d)?;
        self.0 = text.into_bytes();
        Ok(())
    }
}

impl Encode for ByteString {
    fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
        e.write_string(&self.0);
        Ok(())
    }

    fn is_empty_value(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BasicUser {
    pub username: Vec<u8>,
    pub password: Vec<u8>,
    pub role: Role,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BasicHashedUser {
    pub username: Vec<u8>,
    pub password: String,
    pub role: Role,
}

fn digest(password: &[u8], key: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(password.len() + key.len());
    input.extend_from_slice(password);
    input.extend_from_slice(key);
    sha256::sum256(&input)
}

fn hash_password(password: &[u8], key: &[u8]) -> Result<String, SecurityError> {
    let hashed = bcrypt::generate_from_password(&digest(password, key), bcrypt::DEFAULT_COST)
        .map_err(SecurityError::Hash)?;
    Ok(String::from_utf8_lossy(&hashed).into_owned())
}

impl BasicUser {
    pub fn to_hased_user(&self) -> Result<BasicHashedUser, SecurityError> {
        let key = create_key_file_or_get_key()?;
        let password = hash_password(&self.password, &key)?;

        let role = if self.role.0.is_empty() {
            Role::user()
        } else {
            self.role.clone()
        };

        Ok(BasicHashedUser {
            username: self.username.clone(),
            password,
            role,
        })
    }
}

fn string_of<'a>(data: &'a Map, key: &str) -> Option<&'a str> {
    match data.get(key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

impl BasicHashedUser {
    pub fn to_data_map(&self) -> Map {
        let mut m = Map::new();
        m.insert("id".to_string(), Value::String(text(&self.username)));
        m.insert("username".to_string(), Value::String(text(&self.username)));
        m.insert("password".to_string(), Value::String(self.password.clone()));
        m.insert("role".to_string(), Value::String(self.role.0.clone()));
        m
    }

    pub fn from_data_map(data: &Map) -> Result<BasicHashedUser, SecurityError> {
        let Some(username) = string_of(data, "username") else {
            return Err(SecurityError::InvalidUserData("username"));
        };
        let Some(password) = string_of(data, "password") else {
            return Err(SecurityError::InvalidUserData("password"));
        };
        let Some(role) = string_of(data, "role") else {
            return Err(SecurityError::InvalidUserData("role"));
        };

        Ok(BasicHashedUser {
            username: username.as_bytes().to_vec(),
            password: password.to_string(),
            role: Role::new(role),
        })
    }

    pub fn save(&self) -> Result<(), SecurityError> {
        let fields = ByteFields::id(&self.username).with("username", &self.username);
        let errors =
            engine::write_entity_bytes(USER_ENTITY.as_bytes(), &mut self.to_data_map(), &fields);
        if errors.is_empty() {
            return Ok(());
        }

        let mut result = String::new();
        for (i, e) in errors.iter().enumerate() {
            result.push_str(&format!("Error {i}: {}\n", e.to_error()));
        }

        Err(SecurityError::Validation(result))
    }
}

pub fn init_admin_user_if_not_exists() -> Result<(), SecurityError> {
    if engine::read_entity_by_id(USER_ENTITY.as_bytes(), DEFAULT_ADMIN_USERNAME.as_bytes())
        .is_some()
    {
        return Ok(());
    }

    let mut password = std::env::var_os(ADMIN_PASSWORD_ENV_VAR)
        .map(|v| v.as_bytes().to_vec())
        .unwrap_or_default();
    if password.is_empty() {
        password = DEFAULT_ADMIN_PASSWORD.as_bytes().to_vec();
    }

    create_basic_user(&BasicUser {
        username: DEFAULT_ADMIN_USERNAME.as_bytes().to_vec(),
        password,
        role: Role::admin(),
    })
}

fn required_string() -> Value {
    let mut m = Map::new();
    m.insert("type".to_string(), Value::String("string".to_string()));
    m.insert("required".to_string(), Value::Bool(true));
    Value::Object(m)
}

pub fn user_entity_schema() -> Map {
    let mut m = Map::new();
    for field in ["id", "username", "password", "role"] {
        m.insert(field.to_string(), required_string());
    }
    m
}

pub fn init_basic_users_storage() -> Result<(), SecurityError> {
    if engine::entity_type_exists(USER_ENTITY.as_bytes()) {
        return Ok(());
    }

    engine::create_entity_type(USER_ENTITY.as_bytes())
        .map_err(|e| SecurityError::Storage(e.to_string()))?;

    engine::update_entity_schema(USER_ENTITY.as_bytes(), &user_entity_schema());

    Ok(())
}

pub fn get_basic_user_by_username(username: &[u8]) -> Result<Map, SecurityError> {
    init_basic_users_storage()?;

    let Some(mut u) = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username) else {
        return Err(SecurityError::UserNotFound(username.to_vec()));
    };

    u.remove("password");

    Ok(u)
}

pub fn list_basic_users() -> Result<Vec<Map>, SecurityError> {
    init_basic_users_storage()?;

    let users = engine::list_entities(
        USER_ENTITY.as_bytes(),
        0,
        0,
        b"username",
        true,
        None,
        b"",
        b"",
    );

    let mut result = Vec::with_capacity(users.len());
    for mut u in users {
        u.remove("password");
        result.push(u);
    }

    Ok(result)
}

pub fn validate_username(username: &[u8]) -> Result<(), SecurityError> {
    if username.is_empty()
        || username.len() > MAX_USERNAME_LENGTH
        || username == b"."
        || username == b".."
    {
        return Err(SecurityError::InvalidUsername);
    }

    let mut i = 0;
    while i < username.len() {
        let (r, width) = utf8::decode_rune(&username[i..]);
        i += width;
        if r == '/' as utf8::Rune || unicode::is_space(r) || unicode::is_control(r) {
            return Err(SecurityError::InvalidUsername);
        }
    }

    Ok(())
}

pub fn validate_password(password: &[u8]) -> Result<(), SecurityError> {
    if utf8::rune_count(password) < MIN_PASSWORD_LENGTH {
        return Err(SecurityError::PasswordTooShort);
    }

    Ok(())
}

pub fn basic_user_exists(username: &[u8]) -> bool {
    engine::read_entity_by_id(USER_ENTITY.as_bytes(), username).is_some()
}

pub fn create_basic_user(user: &BasicUser) -> Result<(), SecurityError> {
    init_basic_users_storage()?;

    let hashed_user = user.to_hased_user()?;

    hashed_user.save()
}

pub fn change_user_password(username: &[u8], new_password: &[u8]) -> Result<(), SecurityError> {
    change_password(username, new_password, b"")
}

pub fn change_user_password_keeping_session(
    username: &[u8],
    new_password: &[u8],
    session_id: &[u8],
) -> Result<(), SecurityError> {
    change_password(username, new_password, session_id)
}

fn change_password(
    username: &[u8],
    new_password: &[u8],
    kept_session_id: &[u8],
) -> Result<(), SecurityError> {
    let Some(u) = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username) else {
        return Err(SecurityError::UserNotFound(username.to_vec()));
    };

    let mut user = BasicHashedUser::from_data_map(&u)?;

    let key = create_key_file_or_get_key()?;

    user.password = hash_password(new_password, &key)?;

    user.save()?;

    delete_user_sessions_except(
        username,
        kept_session_id,
        &credentials_fingerprint(user.password.as_bytes()),
    )
}

pub fn change_user_role(username: &[u8], new_role: &[u8]) -> Result<(), SecurityError> {
    let Some(u) = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username) else {
        return Err(SecurityError::UserNotFoundWrapped(username.to_vec()));
    };

    if new_role != ROLE_ADMIN.as_bytes() && new_role != ROLE_USER.as_bytes() {
        return Err(SecurityError::InvalidRole);
    }

    let mut user = BasicHashedUser::from_data_map(&u)?;

    if new_role != ROLE_ADMIN.as_bytes() && is_last_admin(username) {
        return Err(SecurityError::LastAdmin);
    }

    let role_changed = user.role.0.as_bytes() != new_role;
    user.role = Role::new(text(new_role));

    user.save()?;

    if role_changed {
        return delete_user_sessions(username);
    }

    Ok(())
}

pub fn delete_basic_user(username: &[u8]) -> Result<(), SecurityError> {
    if username == DEFAULT_ADMIN_USERNAME.as_bytes() {
        return Err(SecurityError::DefaultAdminProtected);
    }

    if is_last_admin(username) {
        return Err(SecurityError::LastAdmin);
    }

    engine::delete_entity_by_id(USER_ENTITY.as_bytes(), username);

    delete_user_sessions(username)
}

fn is_admin_value(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::String(s)) if s == ROLE_ADMIN)
}

fn is_last_admin(username: &[u8]) -> bool {
    let Some(u) = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username) else {
        return false;
    };
    if !is_admin_value(u.get("role")) {
        return false;
    }

    let users = list_basic_users().unwrap_or_default();

    let admins = users
        .iter()
        .filter(|user| is_admin_value(user.get("role")))
        .count();

    admins <= 1
}

pub(crate) fn hex_string(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(b.len() * 2);
    for &x in b {
        out.push(DIGITS[usize::from(x >> 4)] as char);
        out.push(DIGITS[usize::from(x & 0x0f)] as char);
    }
    out
}

pub fn generate_key() -> Result<String, SecurityError> {
    let mut b = vec![0u8; 512];
    rand::os_random(&mut b).map_err(SecurityError::KeyGeneration)?;

    Ok(hex_string(&b))
}

fn key_path() -> String {
    format!("{}/{}", config().store.folder, KEY_FILENAME)
}

pub fn create_key_file_or_get_key() -> Result<Vec<u8>, SecurityError> {
    let cfg = config();
    let path = key_path();
    let fs_error = |op: &'static str, e: io::Error| SecurityError::Fs(FsError::new(op, &path, e));

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(|e| fs_error("open", e))?;

    let _ = file.set_permissions(Permissions::from_mode(0o600));

    let size = file.metadata().map_err(|e| fs_error("stat", e))?.len();

    if size == 0 {
        let key = generate_key()?;

        file.write_all_at(key.as_bytes(), 0)
            .map_err(|e| fs_error("write", e))?;

        file.sync_all().map_err(|e| fs_error("sync", e))?;

        let _ = durable::OS.sync_dir(Path::new(&cfg.store.folder));

        return Ok(key.into_bytes());
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|e| fs_error("read", e))?;

    let (key, result) = fscanf_word(&mut file);
    match result {
        Ok(()) => Ok(key),
        Err(ScanError::Io(e)) => Err(fs_error("read", e)),
        Err(_) => Err(SecurityError::NoKeyFound(path.clone())),
    }
}

pub fn write_key(key: &[u8]) -> Result<(), SecurityError> {
    let cfg = config();
    let path = key_path();

    durable::OS
        .mkdir_all(Path::new(&cfg.store.folder), 0o755)
        .map_err(SecurityError::Fs)?;

    durable::write_file_atomic(&durable::OS, Path::new(&path), |w| w.write(key))
        .map_err(SecurityError::Fs)
}

static DUMMY_PASSWORD_HASH: OnceLock<Vec<u8>> = OnceLock::new();

fn spend_password_check(password: &[u8]) {
    let dummy = DUMMY_PASSWORD_HASH.get_or_init(|| {
        bcrypt::generate_from_password(DUMMY_PASSWORD, bcrypt::DEFAULT_COST).unwrap_or_default()
    });

    let sum = sha256::sum256(password);
    let _ = bcrypt::compare_hash_and_password(dummy, &sum);
}

pub fn authenticate_user(username: &[u8], password: &[u8]) -> Option<BasicHashedUser> {
    let Some(u) = engine::read_entity_by_id(USER_ENTITY.as_bytes(), username) else {
        spend_password_check(password);
        return None;
    };

    let Ok(user) = BasicHashedUser::from_data_map(&u) else {
        spend_password_check(password);
        return None;
    };

    let key = create_key_file_or_get_key().ok()?;

    let sum = digest(password, &key);

    if bcrypt::compare_hash_and_password(user.password.as_bytes(), &sum).is_ok() {
        return Some(user);
    }

    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BasicHeader {
    NotBasic,
    Undecodable,
    Decoded {
        payload: Vec<u8>,
        credentials: Option<(Vec<u8>, Vec<u8>)>,
    },
}

impl BasicHeader {
    pub fn credentials(&self) -> Option<(&[u8], &[u8])> {
        match self {
            BasicHeader::Decoded {
                credentials: Some((username, password)),
                ..
            } => Some((username, password)),
            _ => None,
        }
    }
}

pub fn parse_basic_header(header: &[u8]) -> BasicHeader {
    let Some(encoded) = header.strip_prefix(b"Basic ") else {
        return BasicHeader::NotBasic;
    };

    let Ok(payload) = STD_ENCODING.decode_string(encoded) else {
        return BasicHeader::Undecodable;
    };

    let credentials = payload
        .iter()
        .position(|&b| b == b':')
        .map(|i| (payload[..i].to_vec(), payload[i + 1..].to_vec()));

    BasicHeader::Decoded {
        payload,
        credentials,
    }
}

pub type Check = Arc<dyn Fn(&RequestCtx) -> bool + Send + Sync>;

static CHECK_BASIC_AUTHENTICATION: RwLock<Option<Check>> = RwLock::new(None);

pub fn check_basic_authentication(ctx: &RequestCtx) -> bool {
    let replaced = CHECK_BASIC_AUTHENTICATION.read().clone();
    match replaced {
        Some(f) => f(ctx),
        None => default_check_basic_authentication(ctx),
    }
}

pub fn set_check_basic_authentication(f: Option<Check>) -> Option<Check> {
    std::mem::replace(&mut *CHECK_BASIC_AUTHENTICATION.write(), f)
}

fn default_check_basic_authentication(ctx: &RequestCtx) -> bool {
    let header = parse_basic_header(ctx.request.header.peek(HEADER_AUTHORIZATION));
    let Some((username, password)) = header.credentials() else {
        return false;
    };

    let (_, _, ok) = authenticate_user_from(username, password, login_source_from_request(ctx));

    ok
}
