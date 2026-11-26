#[path = "../tempdir.rs"]
mod tempdir;

mod authentication_test;
mod basic_user_test;
mod cors_test;
mod credentials_test;
mod login_test;
mod session_cache_test;
mod session_test;
mod token_test;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use elysiandb::configuration::{Config, StatsConfig, StoreConfig};
use elysiandb::globals;
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security::{self, BasicHashedUser, Check, Middleware, Role};
use elysiandb::storage;
use elysiandb::yaml::StringList;

use tempdir::TempDir;

pub struct Store {
    dir: TempDir,
}

impl Store {
    pub fn path(&self) -> String {
        self.dir.path().to_string_lossy().into_owned()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn open_store(shards: i64) -> Store {
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: dir.path().to_string_lossy().into_owned(),
            shards,
            ..StoreConfig::default()
        },
        stats: StatsConfig { enabled: false },
        ..Config::default()
    })));
    storage::load_db();
    storage::load_json_db();

    Store { dir }
}

pub fn setup() -> Store {
    open_store(8)
}

pub fn setup_temp_store() -> Store {
    open_store(4)
}

pub fn config() -> Config {
    globals::config().map(|c| (*c).clone()).unwrap_or_default()
}

pub fn set_config(cfg: Config) {
    globals::set_config(Some(Arc::new(cfg)));
}

pub fn update_config(f: impl FnOnce(&mut Config)) {
    let mut cfg = config();
    f(&mut cfg);
    set_config(cfg);
}

pub fn set_authentication(enabled: bool, mode: &str, token: &str) {
    update_config(|cfg| {
        cfg.security.authentication.enabled = enabled;
        cfg.security.authentication.mode = mode.to_string();
        cfg.security.authentication.token = token.to_string();
    });
}

pub fn string_list(items: &[&str]) -> StringList {
    StringList(Some(items.iter().map(|s| s.to_string()).collect()))
}

pub fn seed_user(username: &str, role: Role) {
    security::init_basic_users_storage().expect("init users storage");

    let user = BasicHashedUser {
        username: username.as_bytes().to_vec(),
        password: format!("hash-of-{username}"),
        role,
    };
    user.save().expect("save user");
}

pub fn basic_user(username: &str, password: &str, role: Role) -> security::BasicUser {
    security::BasicUser {
        username: username.as_bytes().to_vec(),
        password: password.as_bytes().to_vec(),
        role,
    }
}

pub fn ctx_from(req: &Request, peer: Option<IpAddr>) -> RequestCtx {
    let mut ctx = RequestCtx::new();
    ctx.init(req, peer.map(|ip| SocketAddr::new(ip, 0)));
    ctx
}

pub fn ctx_with_cookie(id: &str) -> RequestCtx {
    let mut req = Request::new();
    req.header.set_cookie(security::SESSION_COOKIE_NAME, id);
    ctx_from(&req, None)
}

pub fn ctx_with_header(name: &str, value: &str) -> RequestCtx {
    let mut req = Request::new();
    if !value.is_empty() {
        req.header.set(name, value);
    }
    ctx_from(&req, None)
}

pub fn ip(s: &str) -> IpAddr {
    s.parse().expect("ip")
}

pub struct BasicCheckRestore(Option<Check>);

impl Drop for BasicCheckRestore {
    fn drop(&mut self) {
        security::set_check_basic_authentication(self.0.take());
    }
}

pub fn replace_basic_check(f: Check) -> BasicCheckRestore {
    BasicCheckRestore(security::set_check_basic_authentication(Some(f)))
}

pub struct UserAuthRestore(Option<Middleware>);

impl Drop for UserAuthRestore {
    fn drop(&mut self) {
        security::set_user_auth(self.0.take());
    }
}

pub fn replace_user_auth(f: Middleware) -> UserAuthRestore {
    UserAuthRestore(security::set_user_auth(Some(f)))
}

pub struct ClockRestore;

impl Drop for ClockRestore {
    fn drop(&mut self) {
        security::set_login_clock(None);
    }
}

pub struct AttemptsRestore;

impl Drop for AttemptsRestore {
    fn drop(&mut self) {
        security::reset_login_attempts();
    }
}
