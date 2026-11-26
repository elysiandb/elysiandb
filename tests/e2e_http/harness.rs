use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use elysiandb::acl;
use elysiandb::api as api_storage;
use elysiandb::cache;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::hook;
use elysiandb::httpd::{Handler, Server, ServerConfig, bind_tcp4};
use elysiandb::httprouter::Router;
use elysiandb::routing;
use elysiandb::security;
use elysiandb::storage;
use elysiandb::sync::Mutex;

use crate::client::{self, Head};

pub const RACE_DETECTOR: bool = cfg!(debug_assertions);

static COUNTER: AtomicU64 = AtomicU64::new(0);
static REGISTER: Once = Once::new();

pub fn register_packages() {
    REGISTER.call_once(api_storage::index::register);
}

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> TempDir {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("e2e_http");
        let _ = std::fs::create_dir_all(&base);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{}-{nanos}-{n}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return TempDir { path },
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("cannot create a temporary folder: {e}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn folder(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    pub fn set(name: &'static str, value: &str) -> EnvGuard {
        let previous = std::env::var_os(name);
        unsafe { std::env::set_var(name, value) };
        EnvGuard { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => unsafe { std::env::set_var(self.name, v) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

struct Pairs {
    listener: TcpListener,
    lock: Mutex<()>,
}

impl Pairs {
    fn new() -> Pairs {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap_or_else(|e| panic!("cannot open the loopback pair listener: {e}"));
        Pairs {
            listener,
            lock: Mutex::new(()),
        }
    }

    fn pair(&self) -> io::Result<(TcpStream, TcpStream)> {
        let _serial = self.lock.lock();
        let addr = self.listener.local_addr()?;
        let client = TcpStream::connect(addr)?;
        let local = client.local_addr()?;
        loop {
            let (server, from) = self.listener.accept()?;
            if from == local {
                return Ok((client, server));
            }
        }
    }
}

pub struct ServerHandle {
    server: Server,
    pairs: Option<Pairs>,
    addr: Option<SocketAddr>,
    thread: Mutex<Option<JoinHandle<io::Result<()>>>>,
    _dir: Option<TempDir>,
}

impl ServerHandle {
    pub fn in_memory(server: Server, dir: Option<TempDir>) -> Arc<ServerHandle> {
        Arc::new(ServerHandle {
            server,
            pairs: Some(Pairs::new()),
            addr: None,
            thread: Mutex::new(None),
            _dir: dir,
        })
    }

    pub fn listening(server: Server, dir: Option<TempDir>) -> Arc<ServerHandle> {
        let ln = bind_tcp4("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
        let addr = ln.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let s = server.clone();
        let thread = std::thread::spawn(move || s.serve(ln));
        Arc::new(ServerHandle {
            server,
            pairs: None,
            addr: Some(addr),
            thread: Mutex::new(Some(thread)),
            _dir: dir,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
            .unwrap_or_else(|| panic!("the server has no listener"))
    }

    pub fn dial(&self, ip: Option<IpAddr>) -> io::Result<TcpStream> {
        if let Some(pairs) = &self.pairs {
            let (client, server) = pairs.pair()?;
            let peer = SocketAddr::new(ip.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), 0);
            self.server.serve_connection(server, peer)?;
            return Ok(client);
        }

        TcpStream::connect(self.addr())
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.server.shutdown();
        if let Some(t) = self.thread.lock().take() {
            let _ = t.join();
        }
    }
}

pub struct ApiResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: BTreeMap<String, String>,
}

impl ApiResponse {
    pub fn header(&self, name: &str) -> String {
        self.headers
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub fn json(&self) -> Value {
        json::unmarshal_value(&self.body).unwrap_or_else(|e| {
            panic!(
                "invalid JSON body {:?}: {e}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn object(&self) -> Map {
        let mut v = self.json();
        match &mut v {
            Value::Object(m) => std::mem::take(m),
            _ => panic!(
                "expected a JSON object, got {}",
                String::from_utf8_lossy(&self.body)
            ),
        }
    }

    pub fn list(&self) -> Vec<Map> {
        let mut v = self.json();
        let Value::Array(raw) = &mut v else {
            panic!(
                "expected a JSON array, got {}",
                String::from_utf8_lossy(&self.body)
            );
        };

        let mut out = Vec::with_capacity(raw.len());
        for item in raw.iter_mut() {
            match item {
                Value::Object(m) => out.push(std::mem::take(m)),
                _ => panic!(
                    "expected an array of objects, got {}",
                    String::from_utf8_lossy(&self.body)
                ),
            }
        }

        out
    }

    pub fn ids(&self) -> Vec<String> {
        self.list()
            .iter()
            .map(|item| {
                item.get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Clone)]
pub struct ApiServer {
    handle: Arc<ServerHandle>,
    ip: Option<IpAddr>,
    pub cookie: String,
    pub headers: Head,
}

pub fn default_api_config(folder: &str) -> Config {
    let mut cfg = Config::default();
    cfg.store.folder = folder.to_string();
    cfg.store.shards = 8;
    cfg.stats.enabled = true;
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "user".to_string();
    cfg.security.authentication.token = "test-token".to_string();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = true;
    cfg.api.cache.enabled = true;
    cfg.api.cache.cleanup_interval_seconds = 10;
    cfg.api.hooks.enabled = true;
    cfg
}

pub type Configure<'a> = Option<&'a dyn Fn(&mut Config)>;

pub fn start_api_server(configure: Configure<'_>) -> ApiServer {
    let (srv, dir) = new_api_server(configure);
    let handle = ServerHandle::in_memory(srv, Some(dir));

    ApiServer {
        handle,
        ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        cookie: String::new(),
        headers: Head::new(),
    }
}

pub fn start_tcp_admin_session(configure: Configure<'_>) -> (ApiServer, String) {
    let (srv, dir) = new_api_server(configure);
    let handle = ServerHandle::listening(srv, Some(dir));
    let addr = handle.addr().to_string();
    let mut s = ApiServer {
        handle,
        ip: None,
        cookie: String::new(),
        headers: Head::new(),
    };
    s.login(
        security::DEFAULT_ADMIN_USERNAME,
        security::DEFAULT_ADMIN_PASSWORD,
    );

    (s, addr)
}

fn new_api_server(configure: Configure<'_>) -> (Server, TempDir) {
    register_packages();
    let dir = TempDir::new();
    let mut cfg = default_api_config(&dir.folder());
    if let Some(f) = configure {
        f(&mut cfg);
    }

    globals::set_config(Some(Arc::new(cfg)));
    security::reset_login_attempts();
    storage::load_db();
    storage::load_json_db();
    cache::init_cache(GoDuration(10 * GoDuration::SECOND.0));
    hook::init_hooks();

    if let Err(err) = security::init_admin_user_if_not_exists() {
        panic!("cannot create the default admin: {err}");
    }

    let _ = acl::init_acl();

    let mut r = Router::new();
    routing::register_routes(&mut r);

    let handler: Handler = security::protect(r.into_handler());
    (Server::new(ServerConfig::new(handler)), dir)
}

pub fn start_admin_session(configure: Configure<'_>) -> ApiServer {
    let mut s = start_api_server(configure);
    s.login(
        security::DEFAULT_ADMIN_USERNAME,
        security::DEFAULT_ADMIN_PASSWORD,
    );

    s
}

impl ApiServer {
    pub fn from(&self, ip: &str) -> ApiServer {
        let ip: IpAddr = ip
            .parse()
            .unwrap_or_else(|e| panic!("invalid IP {ip}: {e}"));
        ApiServer {
            handle: self.handle.clone(),
            ip: Some(ip),
            cookie: String::new(),
            headers: Head::new(),
        }
    }

    pub fn anonymous(&self) -> ApiServer {
        ApiServer {
            handle: self.handle.clone(),
            ip: self.ip,
            cookie: String::new(),
            headers: Head::new(),
        }
    }

    pub fn login(&mut self, username: &str, password: &str) {
        let mut m = Map::new();
        m.insert("password".to_string(), Value::String(password.to_string()));
        m.insert("username".to_string(), Value::String(username.to_string()));
        let body = json::marshal(&m).unwrap_or_default();
        let resp = self.do_("POST", "/api/security/login", body, &[]);
        if resp.status != 200 {
            panic!(
                "login as {username} failed: {} {}",
                resp.status,
                resp.text()
            );
        }

        let prefix = format!("{}=", security::SESSION_COOKIE_NAME);
        for part in resp.header("Set-Cookie").split(';') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix(&prefix) {
                self.cookie = value.to_string();
            }
        }

        if self.cookie.is_empty() {
            panic!("login as {username} did not return a session cookie");
        }
    }

    pub fn try_do(
        &self,
        method: &str,
        path: &str,
        body: impl AsRef<[u8]>,
        headers: &[&str],
    ) -> io::Result<ApiResponse> {
        let mut head = Head::new();
        for (k, v) in self.headers.lines() {
            head.set(k, v);
        }
        for pair in headers.chunks(2) {
            if let [k, v] = pair {
                head.set(k, v);
            }
        }

        let mut cookies = Vec::new();
        if !self.cookie.is_empty() {
            cookies.push((
                security::SESSION_COOKIE_NAME.to_string(),
                self.cookie.clone(),
            ));
        }

        let request =
            client::fasthttp_request(method, "test", path, body.as_ref(), &head, &cookies);
        let stream = self.handle.dial(self.ip)?;
        let resp = client::round_trip(stream, &request, method)?;

        let mut out = ApiResponse {
            status: resp.status,
            body: resp.body,
            headers: BTreeMap::new(),
        };
        for (key, value) in resp.headers {
            let name = key.to_ascii_lowercase();
            let value = String::from_utf8_lossy(&value).into_owned();
            match out.headers.get_mut(&name) {
                Some(existing) => {
                    existing.push_str("; ");
                    existing.push_str(&value);
                }
                None => {
                    out.headers.insert(name, value);
                }
            }
        }

        Ok(out)
    }

    pub fn do_(
        &self,
        method: &str,
        path: &str,
        body: impl AsRef<[u8]>,
        headers: &[&str],
    ) -> ApiResponse {
        self.try_do(method, path, body, headers)
            .unwrap_or_else(|e| panic!("{method} {path} failed: {e}"))
    }

    pub fn must_do(
        &self,
        status: u16,
        method: &str,
        path: &str,
        body: impl AsRef<[u8]>,
        headers: &[&str],
    ) -> ApiResponse {
        let resp = self.do_(method, path, body, headers);
        if resp.status != status {
            panic!(
                "{method} {path}: expected {status}, got {} {}",
                resp.status,
                resp.text()
            );
        }

        resp
    }
}

pub struct TestClient {
    handle: Arc<ServerHandle>,
}

pub fn start_test_server() -> TestClient {
    register_packages();
    let tmp = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = tmp.folder();
    cfg.store.shards = 8;
    cfg.stats.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();

    let mut r = Router::new();
    routing::register_routes(&mut r);
    let srv = Server::new(ServerConfig::new(r.into_handler()));

    TestClient {
        handle: ServerHandle::in_memory(srv, Some(tmp)),
    }
}

impl TestClient {
    pub fn do_(&self, method: &str, url: &str, body: impl AsRef<[u8]>) -> client::Response {
        let path = url.strip_prefix("http://test").unwrap_or(url);
        let request =
            client::fasthttp_request(method, "test", path, body.as_ref(), &Head::new(), &[]);
        let stream = self
            .handle
            .dial(None)
            .unwrap_or_else(|e| panic!("{method} {url} failed: {e}"));
        client::round_trip(stream, &request, method)
            .unwrap_or_else(|e| panic!("{method} {url} failed: {e}"))
    }
}

pub fn map(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(map(pairs))
}

pub fn vs(v: &str) -> Value {
    Value::String(v.to_string())
}

pub fn vn(v: f64) -> Value {
    Value::Number(v)
}

pub fn marshal(v: &Value) -> Vec<u8> {
    json::marshal(v).unwrap_or_else(|e| panic!("cannot encode JSON: {e}"))
}

pub fn as_object(v: Option<&Value>) -> Map {
    match v {
        Some(Value::Object(m)) => m.clone(),
        other => panic!("expected an object, got {other:?}"),
    }
}
