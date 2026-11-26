use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Once;
use std::thread::JoinHandle;
use std::time::Duration;

use elysiandb::acl;
use elysiandb::api;
use elysiandb::boot;
use elysiandb::cache;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json;
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::hook;
use elysiandb::httpd::{Handler, Server, ServerConfig, bind_tcp4};
use elysiandb::httprouter::Router;
use elysiandb::routing;
use elysiandb::security::{self, Role};
use elysiandb::storage;
use elysiandb::transport::tcp::tcp_routing::{self, Conn};

use super::fixture::{
    FixtureRequest, fixture_load_requests, hook_requests, kv_requests, ownership_requests,
    read_requests, transaction_requests, write_requests,
};
use super::golden::{
    Corpus, GOLDEN_PATH, GoldenEntry, differs, expected_body, intentional_change, read_golden,
    testdata, truncate,
};
use super::tempdir::TempDir;

static PACKAGES: Once = Once::new();

pub struct CorpusServer {
    pub corpus: Corpus,
    server: Server,
    thread: Option<JoinHandle<io::Result<()>>>,
    _folder: Option<TempDir>,
}

impl CorpusServer {
    pub fn run(&mut self, r: &FixtureRequest) -> GoldenEntry {
        self.corpus.run(r)
    }
}

impl Drop for CorpusServer {
    fn drop(&mut self) {
        let _ = self.server.shutdown_with_timeout(Duration::from_secs(5));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct Engine;

impl Drop for Engine {
    fn drop(&mut self) {
        shutdown_engine();
    }
}

pub fn shutdown_engine() {
    let _ = storage::close();
}

pub fn register_packages() {
    PACKAGES.call_once(|| {
        api::index::register();
        security::register();
    });
}

pub fn corpus_config(folder: &str) -> Config {
    let mut cfg = Config::default();
    cfg.store.folder = folder.to_string();
    cfg.store.shards = 8;
    cfg.store.flush_interval_seconds = 0;
    cfg.server.http.enabled = true;
    cfg.server.http.host = "127.0.0.1".to_string();
    cfg.server.http.port = 1;
    cfg.server.tcp.enabled = true;
    cfg.server.tcp.host = "127.0.0.1".to_string();
    cfg.server.tcp.port = 1;
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "user".to_string();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = true;
    cfg.api.cache.enabled = true;
    cfg.api.cache.cleanup_interval_seconds = 10;
    cfg.api.hooks.enabled = true;
    cfg.api.index.workers = 1;
    cfg.log.level = "error".to_string();
    cfg.log.format = "text".to_string();
    cfg
}

struct LoopbackConn;

impl Conn for LoopbackConn {
    fn close(&self) {}

    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(SocketAddr::from(([127, 0, 0, 1], 40000)))
    }
}

fn serve(handler: Handler) -> (Server, SocketAddr, JoinHandle<io::Result<()>>) {
    let ln = bind_tcp4("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    let addr = ln.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let server = Server::new(ServerConfig::new(handler));
    let s = server.clone();
    let thread = std::thread::spawn(move || s.serve(ln));

    (server, addr, thread)
}

pub fn serve_routes(folder: Option<TempDir>) -> (CorpusServer, bool) {
    let mut r = Router::new();
    routing::register_routes(&mut r);
    let (server, addr, thread) = serve(boot::new_http_handler(r));

    let mut session = tcp_routing::new_session();
    let auth = tcp_routing::route_line(
        format!(
            "AUTH {} {}\n",
            security::DEFAULT_ADMIN_USERNAME,
            security::DEFAULT_ADMIN_PASSWORD
        )
        .as_bytes(),
        &LoopbackConn,
        &mut session,
    );

    let mut corpus = Corpus::new(
        addr,
        Box::new(|r: &FixtureRequest| {
            match security::create_session(
                r.path.as_bytes(),
                Role::user(),
                Duration::from_secs(3600),
            ) {
                Ok(session) => session.id,
                Err(e) => panic!("cannot open a session for {}: {e}", r.path),
            }
        }),
        Box::new(move |line: &str| {
            tcp_routing::route_line(format!("{line}\n").as_bytes(), &LoopbackConn, &mut session)
        }),
    );
    corpus.login(
        "admin",
        security::DEFAULT_ADMIN_USERNAME,
        security::DEFAULT_ADMIN_PASSWORD,
    );

    (
        CorpusServer {
            corpus,
            server,
            thread: Some(thread),
            _folder: folder,
        },
        auth == b"OK",
    )
}

pub fn start_corpus_server() -> CorpusServer {
    register_packages();

    let folder = TempDir::new();
    let cfg = corpus_config(&folder.path().to_string_lossy());
    globals::set_config(Some(Arc::new(cfg)));
    security::reset_login_attempts();
    storage::load_db();
    storage::load_json_db();
    cache::init_cache(GoDuration(10 * GoDuration::SECOND.0));
    hook::init_hooks();

    if let Err(e) = security::init_admin_user_if_not_exists() {
        panic!("cannot create the default admin: {e}");
    }

    let _ = acl::init_acl();

    let (s, authenticated) = serve_routes(Some(folder));
    if !authenticated {
        panic!("tcp auth failed");
    }

    s
}

pub fn corpus_requests() -> Vec<FixtureRequest> {
    let mut reqs = Vec::new();
    reqs.extend(fixture_load_requests());
    reqs.extend(hook_requests());
    reqs.extend(read_requests());
    reqs.extend(write_requests());
    reqs.extend(transaction_requests());
    reqs.extend(kv_requests());
    reqs.extend(ownership_requests());

    reqs
}

fn run_corpus() -> Vec<GoldenEntry> {
    let mut s = start_corpus_server();
    let mut entries = Vec::new();
    for r in corpus_requests() {
        entries.push(s.run(&r));
    }
    shutdown_engine();

    entries
}

#[test]
fn golden_corpus() {
    let _engine = Engine;
    let entries = run_corpus();

    if std::env::var("ELYSIAN_RECORD_GOLDEN").as_deref() == Ok("1") {
        let mut data = json::marshal_indent(&entries, "", "  ").unwrap_or_else(|e| panic!("{e}"));
        data.push(b'\n');

        let path = testdata(GOLDEN_PATH);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(&path, data).unwrap_or_else(|e| panic!("{e}"));

        println!("recorded {} entries", entries.len());
        return;
    }

    let golden = read_golden();

    if golden.len() != entries.len() {
        panic!(
            "corpus has {} requests, golden file has {}",
            entries.len(),
            golden.len()
        );
    }

    let mut errors = Vec::new();
    let mut failures = 0;
    for (i, want) in golden.iter().enumerate() {
        let got = &entries[i];
        if intentional_change(&want.request) {
            continue;
        }

        if differs(got, want, false) {
            failures += 1;
            if failures <= 25 {
                errors.push(format!(
                    "#{i} {} {}\nwant {} {:?} {}\ngot  {} {:?} {}",
                    want.request.method,
                    want.request.path,
                    want.status,
                    want.content_type,
                    truncate(expected_body(want, false)),
                    got.status,
                    got.content_type,
                    truncate(&got.body)
                ));
            }
        }
    }

    if failures > 0 {
        panic!(
            "{}\n{failures} of {} responses differ from the golden corpus",
            errors.join("\n"),
            golden.len()
        );
    }
}
