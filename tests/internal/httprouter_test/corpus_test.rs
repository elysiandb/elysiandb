use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use elysiandb::httpd::{RequestCtx, Server, ServerConfig, bind_tcp4, handler};
use elysiandb::httprouter::Router;

const OFF_ROUTES: [(&str, &str); 49] = [
    ("GET", "/health"),
    ("GET", "/kv/mget"),
    ("GET", "/kv/{key}"),
    ("PUT", "/kv/{key}"),
    ("DELETE", "/kv/{key}"),
    ("POST", "/save"),
    ("POST", "/reset"),
    ("GET", "/stats"),
    ("GET", "/api/export"),
    ("POST", "/api/import"),
    ("GET", "/api/backup"),
    ("POST", "/api/restore"),
    ("GET", "/api/{entity}"),
    ("POST", "/api/query"),
    ("POST", "/api/{entity}"),
    ("GET", "/api/{entity}/{id}"),
    ("PUT", "/api/{entity}/{id}"),
    ("PUT", "/api/{entity}"),
    ("DELETE", "/api/{entity}/{id}"),
    ("DELETE", "/api/{entity}"),
    ("GET", "/api/{entity}/count"),
    ("GET", "/api/{entity}/{id}/exists"),
    ("POST", "/api/{entity}/migrate"),
    ("GET", "/api/entity/types"),
    ("GET", "/api/entity/types/name"),
    ("POST", "/api/{entity}/create"),
    ("GET", "/api/{entity}/schema"),
    ("PUT", "/api/{entity}/schema"),
    ("POST", "/api/tx/begin"),
    ("POST", "/api/tx/{txId}/rollback"),
    ("POST", "/api/tx/{txId}/entity/{entity}"),
    ("PUT", "/api/tx/{txId}/entity/{entity}/{id}"),
    ("DELETE", "/api/tx/{txId}/entity/{entity}/{id}"),
    ("POST", "/api/tx/{txId}/commit"),
    ("GET", "/config"),
    ("GET", "/api/hook/{entity}"),
    ("GET", "/api/hook/id/{id}"),
    ("POST", "/api/hook/{entity}"),
    ("DELETE", "/api/hook/{entity}/{id}"),
    ("PUT", "/api/hook/id/{id}"),
    ("GET", "/monitoring/overview"),
    ("GET", "/monitoring/series"),
    ("GET", "/monitoring/details"),
    ("GET", "/monitoring/logs"),
    ("GET", "/monitoring/logs/download"),
    ("GET", "/monitoring/logs/level"),
    ("PUT", "/monitoring/logs/level"),
    ("GET", "/metrics"),
    ("GET", "/admin/{filepath:*}"),
];

const USER_ROUTES: [(&str, &str); 13] = [
    ("POST", "/api/security/user"),
    ("GET", "/api/security/user"),
    ("GET", "/api/security/user/{user_name}"),
    ("DELETE", "/api/security/user/{user_name}"),
    ("PUT", "/api/security/user/{user_name}/password"),
    ("PUT", "/api/security/user/{user_name}/role"),
    ("POST", "/api/security/login"),
    ("POST", "/api/security/logout"),
    ("GET", "/api/security/me"),
    ("GET", "/api/acl/{user_name}/{entity}"),
    ("GET", "/api/acl/{user_name}"),
    ("PUT", "/api/acl/{user_name}/{entity}"),
    ("PUT", "/api/acl/{user_name}/{entity}/default"),
];

fn route_handler(ctx: &mut RequestCtx) {
    let route = ctx.matched_route_path().unwrap_or("").to_string();
    let mut keys: Vec<String> = ctx
        .user_value_keys()
        .into_iter()
        .filter(|k| !k.starts_with("__"))
        .map(str::to_string)
        .collect();
    keys.sort();
    let mut out = format!("route={route}");
    for k in keys {
        out.push_str(&format!(" {k}={}", String::from_utf8_lossy(ctx.param(&k))));
    }
    ctx.set_body_string(out);
}

fn build_router(user_mode: bool) -> Router {
    let mut r = Router::new();
    r.save_matched_route_path = true;
    r.panic_handler = Some(Arc::new(|ctx: &mut RequestCtx, _payload| {
        ctx.reset_body();
        ctx.set_status_code(500);
    }));
    let h = handler(route_handler);
    for (i, (m, p)) in OFF_ROUTES.iter().enumerate() {
        if user_mode && i == 35 {
            for (um, up) in USER_ROUTES {
                r.handle(um, up, h.clone());
            }
        }
        r.handle(m, p, h.clone());
    }
    r
}

struct Running {
    server: Server,
    addr: SocketAddr,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl Running {
    fn start(user_mode: bool) -> Running {
        let mut cfg = ServerConfig::new(build_router(user_mode).into_handler());
        cfg.name = "ElysianDB".to_string();
        cfg.read_buffer_size = 64 << 10;
        cfg.read_timeout = Duration::from_secs(3);
        cfg.write_timeout = Duration::from_secs(3);
        cfg.idle_timeout = Duration::from_secs(30);
        let ln = match bind_tcp4("127.0.0.1:0") {
            Ok(ln) => ln,
            Err(e) => panic!("{e}"),
        };
        let addr = match ln.local_addr() {
            Ok(a) => a,
            Err(e) => panic!("{e}"),
        };
        let server = Server::new(cfg);
        let s = server.clone();
        let thread = Some(std::thread::spawn(move || s.serve(ln)));
        Running {
            server,
            addr,
            thread,
        }
    }

    fn exchange(&self, req: &[u8]) -> String {
        let mut c = match TcpStream::connect(self.addr) {
            Ok(c) => c,
            Err(e) => panic!("{e}"),
        };
        let _ = c.write_all(req);
        let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
        let mut out = Vec::new();
        let _ = c.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.server.shutdown_with_timeout(Duration::from_secs(5));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn golden(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/http")
        .join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

fn head_fields(raw: &str) -> (String, Vec<String>, String) {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("").to_string();
    let fields = lines
        .filter(|l| {
            l.starts_with("Location:")
                || l.starts_with("Allow:")
                || l.starts_with("Content-Length:")
        })
        .map(str::to_string)
        .collect();
    (status, fields, body.to_string())
}

#[test]
fn test_router_answers_match_the_corpus() {
    let srv = Running::start(false);
    let cases = [
        "002-health-head",
        "072-not-found",
        "073-method-not-allowed",
        "074-patch",
        "075-trace",
        "076-connect",
        "077-unknown-method",
        "080-trailing-slash-get",
        "081-trailing-slash-post",
        "082-case-redirect",
        "083-double-slash",
        "084-dot-segment",
        "086-http10-no-host-redirect",
        "090-admin-redirect",
    ];
    for case in cases {
        let req = golden(&format!("off/{case}.req"));
        let want = String::from_utf8_lossy(&golden(&format!("off/{case}.resp"))).into_owned();
        let got = srv.exchange(&req);
        let (ws, wf, wb) = head_fields(&want);
        let (gs, gf, gb) = head_fields(&got);
        assert_eq!(gs, ws, "{case}");
        assert_eq!(gf, wf, "{case}");
        assert_eq!(gb, wb, "{case}");
    }
}

#[test]
fn test_route_matching_matches_the_corpus_and_the_spec() {
    let srv = Running::start(false);
    let cases: [(&str, &str); 8] = [
        (
            "085-dot-dot",
            "route=/api/{entity}/{id} entity=.. id=health",
        ),
        ("087-static-vs-param-types", "route=/api/entity/types"),
        (
            "088-static-vs-param-count",
            "route=/api/{entity}/count entity=book",
        ),
        (
            "089-param-entity-id",
            "route=/api/{entity}/{id} entity=entity id=x",
        ),
        ("091-admin-root", "route=/admin/{filepath:*} filepath="),
        (
            "095-admin-dotdot-panic",
            "route=/admin/{filepath:*} filepath=../kv",
        ),
        ("096-encoded-slash", "route=/api/{entity} entity=book%2Fb1"),
        ("097-percent-invalid", "route=/api/{entity} entity=book"),
    ];
    for (case, want) in cases {
        let got = srv.exchange(&golden(&format!("off/{case}.req")));
        let (status, _, body) = head_fields(&got);
        assert_eq!(status, "HTTP/1.1 200 OK", "{case}");
        assert_eq!(body, want, "{case}");
    }

    let spec: [(&str, &str, &str); 17] = [
        (
            "GET",
            "/api/a%2Fb/?q=1",
            "308|301 Location: http://h/api/a/b?q=1",
        ),
        ("GET", "/api/a b/", "301 Location: http://h/api/a%20b"),
        ("GET", "/Api/x%41y", "301 Location: http://h/api/xAy"),
        (
            "GET",
            "/api//book",
            "200 route=/api/{entity}/{id} entity= id=book",
        ),
        ("GET", "/kv/", "404"),
        ("GET", "/kv/mget", "200 route=/kv/mget"),
        ("PUT", "/kv/mget", "200 route=/kv/{key} key=mget"),
        (
            "POST",
            "/api/export",
            "200 route=/api/{entity} entity=export",
        ),
        ("GET", "/api/query", "200 route=/api/{entity} entity=query"),
        (
            "GET",
            "/api/tx/begin",
            "200 route=/api/{entity}/{id} entity=tx id=begin",
        ),
        ("POST", "/api/tx", "200 route=/api/{entity} entity=tx"),
        (
            "POST",
            "/api/tx/migrate",
            "200 route=/api/{entity}/migrate entity=tx",
        ),
        (
            "GET",
            "/api/security/user",
            "200 route=/api/{entity}/{id} entity=security id=user",
        ),
        ("GET", "/api/acl", "200 route=/api/{entity} entity=acl"),
        (
            "GET",
            "/api/hook/id",
            "200 route=/api/hook/{entity} entity=id",
        ),
        ("POST", "/admin/", "405 Allow: GET, OPTIONS"),
        ("GET", "/Admin/", "404"),
    ];
    for (method, target, want) in spec {
        let req = format!("{method} {target} HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
        let got = srv.exchange(req.as_bytes());
        let (status, fields, body) = head_fields(&got);
        let code = &status[9..12];
        let mut summary = code.to_string();
        for f in fields.iter().filter(|f| !f.starts_with("Content-Length")) {
            summary.push(' ');
            summary.push_str(f);
        }
        if code == "200" {
            summary.push(' ');
            summary.push_str(&body);
        }
        let want = want.replace("308|301", "301");
        assert_eq!(summary, want, "{method} {target}");
    }
}

#[test]
fn test_user_mode_routes() {
    let srv = Running::start(true);
    for (method, target, want) in [
        ("GET", "/api/security/user", "route=/api/security/user"),
        (
            "GET",
            "/api/acl/bob",
            "route=/api/acl/{user_name} user_name=bob",
        ),
        (
            "PUT",
            "/api/acl/bob/book/default",
            "route=/api/acl/{user_name}/{entity}/default entity=book user_name=bob",
        ),
        (
            "GET",
            "/api/book/b1",
            "route=/api/{entity}/{id} entity=book id=b1",
        ),
    ] {
        let req = format!("{method} {target} HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
        let (_, _, body) = head_fields(&srv.exchange(req.as_bytes()));
        assert_eq!(body, want, "{method} {target}");
    }
}
