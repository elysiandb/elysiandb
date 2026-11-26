#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use elysiandb::gocompat::json::{self, RawMessage};
use elysiandb::json_struct;
use elysiandb::security;

use super::fixture::FixtureRequest;
use super::http::Client;

pub const GOLDEN_PATH: &str = "golden.json";
pub const LEGACY_GOLDEN_PATH: &str = "golden_legacy.json";
pub const LEGACY_FIXTURES_DIR: &str = "v0.2.4";
pub const ARCHIVES_DIR: &str = "archives";

const GO_QUERY_SYNTAX: &str =
    r#"{"error":"invalid character 'o' in literal null (expecting 'u')"}"#;
const QUERY_SYNTAX: &str = r#"{"error":"invalid JSON at byte 2: unexpected character 'o'"}"#;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct GoldenEntry {
        pub request: FixtureRequest => "request",
        pub status: i64 => "status",
        pub content_type: String => "content_type" [omitempty],
        pub body: String => "body",
    }
}

pub fn testdata(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/compat/testdata")
        .join(rel)
}

pub fn skip(reason: &str) {
    use std::io::Write;

    let _ = std::io::stderr().write_all(format!("\nskipped: {reason}\n").as_bytes());
}

pub fn matching(dir: &Path, prefix: &str, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| {
                    name.len() >= prefix.len() + suffix.len()
                        && name.starts_with(prefix)
                        && name.ends_with(suffix)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();

    names
}

pub fn read_golden() -> Vec<GoldenEntry> {
    let path = testdata(GOLDEN_PATH);
    let raw =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut golden: Vec<GoldenEntry> = Vec::new();
    json::unmarshal(&raw, &mut golden)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

    golden
}

pub fn read_legacy_golden() -> BTreeMap<String, Vec<GoldenEntry>> {
    let path = testdata(LEGACY_GOLDEN_PATH);
    let raw =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut golden: BTreeMap<String, Vec<GoldenEntry>> = BTreeMap::new();
    json::unmarshal(&raw, &mut golden)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

    golden
}

pub fn golden_full() -> Vec<GoldenEntry> {
    read_legacy_golden().remove("full").unwrap_or_default()
}

pub fn expected_body(want: &GoldenEntry, go_wording: bool) -> &str {
    let r = &want.request;
    if !go_wording
        && r.method == "POST"
        && r.path == "/api/query"
        && want.status == 400
        && want.body == GO_QUERY_SYNTAX
    {
        return QUERY_SYNTAX;
    }

    &want.body
}

pub fn differs(got: &GoldenEntry, want: &GoldenEntry, go_wording: bool) -> bool {
    got.status != want.status
        || got.content_type != want.content_type
        || got.body != expected_body(want, go_wording)
}

fn hex_run(b: &[u8], at: usize, n: usize) -> bool {
    b.len() >= at + n
        && b[at..at + n]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

fn digit_run(b: &[u8], at: usize, n: usize) -> bool {
    b.len() >= at + n && b[at..at + n].iter().all(u8::is_ascii_digit)
}

fn uuid_at(b: &[u8], at: usize) -> bool {
    let mut i = at;
    for (k, n) in [8usize, 4, 4, 4, 12].into_iter().enumerate() {
        if k > 0 {
            if b.get(i) != Some(&b'-') {
                return false;
            }
            i += 1;
        }
        if !hex_run(b, i, n) {
            return false;
        }
        i += n;
    }

    true
}

fn tx_at(b: &[u8], at: usize) -> bool {
    digit_run(b, at, 14) && b.get(at + 14) == Some(&b'.') && digit_run(b, at + 15, 9)
}

fn replace_matches(
    b: &[u8],
    len: usize,
    with: &[u8],
    matches: fn(&[u8], usize) -> bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if matches(b, i) {
            out.extend_from_slice(with);
            i += len;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }

    out
}

pub fn normalize_body(r: &FixtureRequest, body: &[u8]) -> String {
    let sorted;
    let mut body = body;
    if r.unordered {
        sorted = sort_json_array(body);
        body = &sorted;
    }

    let out = replace_matches(body, 36, b"<uuid>", uuid_at);
    let out = replace_matches(&out, 24, b"<tx>", tx_at);

    String::from_utf8_lossy(&out).into_owned()
}

pub fn sort_json_array(body: &[u8]) -> Vec<u8> {
    let mut items: Vec<RawMessage> = Vec::new();
    if json::unmarshal(body, &mut items).is_err() {
        let mut lines: Vec<&[u8]> = body.split(|&c| c == b'\n').collect();
        lines.sort();
        return lines.join(&b'\n');
    }

    items.sort_by(|a, b| a.0.cmp(&b.0));

    json::marshal(&items).unwrap_or_default()
}

pub fn truncate(s: &str) -> String {
    if s.len() > 600 {
        return format!(
            "{}... ({} bytes)",
            String::from_utf8_lossy(&s.as_bytes()[..600]),
            s.len()
        );
    }

    s.to_string()
}

pub fn intentional_change(_r: &FixtureRequest) -> bool {
    false
}

pub type Login = Box<dyn FnMut(&FixtureRequest) -> String>;
pub type Tcp = Box<dyn FnMut(&str) -> Vec<u8>>;

pub struct Answer {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub set_cookie: String,
}

pub struct Corpus {
    pub client: Client,
    pub cookies: BTreeMap<String, String>,
    pub tx_id: String,
    pub open_session: Login,
    pub tcp: Tcp,
}

impl Corpus {
    pub fn new(addr: SocketAddr, open_session: Login, tcp: Tcp) -> Corpus {
        Corpus {
            client: Client::without_cookies(addr, Duration::from_secs(30)),
            cookies: BTreeMap::new(),
            tx_id: String::new(),
            open_session,
            tcp,
        }
    }

    pub fn login(&mut self, user: &str, username: &str, password: &str) {
        let mut credentials = BTreeMap::new();
        credentials.insert("username".to_string(), username.to_string());
        credentials.insert("password".to_string(), password.to_string());
        let body = json::marshal(&credentials).unwrap_or_default();
        let answer = self.http(
            "POST",
            "/api/security/login",
            &String::from_utf8_lossy(&body),
            "",
        );
        if answer.status != 200 {
            panic!("login as {username} failed: {}", answer.status);
        }

        let prefix = format!("{}=", security::SESSION_COOKIE_NAME);
        for part in answer.set_cookie.split(';') {
            if let Some(value) = part.trim().strip_prefix(&prefix) {
                self.cookies.insert(user.to_string(), value.to_string());
            }
        }
    }

    pub fn http(&mut self, method: &str, path: &str, body: &str, user: &str) -> Answer {
        let user = if user.is_empty() { "admin" } else { user };

        let mut headers: Vec<(&str, &str)> = Vec::new();
        if method != "GET" && method != "HEAD" {
            headers.push(("Content-Type", "application/x-www-form-urlencoded"));
        }
        let cookie = self
            .cookies
            .get(user)
            .filter(|c| !c.is_empty())
            .map(|c| format!("{}={c}", security::SESSION_COOKIE_NAME));
        if let Some(c) = &cookie {
            headers.push(("Cookie", c));
        }

        let resp = self
            .client
            .request(method, path, &headers, body.as_bytes(), usize::MAX)
            .unwrap_or_else(|e| panic!("{method} {path} failed: {e}"));

        Answer {
            status: resp.status,
            content_type: resp
                .header("Content-Type")
                .unwrap_or("text/plain; charset=utf-8")
                .to_string(),
            set_cookie: resp.header("Set-Cookie").unwrap_or("").to_string(),
            body: resp.body,
        }
    }

    pub fn run(&mut self, r: &FixtureRequest) -> GoldenEntry {
        match r.method.as_str() {
            "LOGIN" => {
                let id = (self.open_session)(r);
                self.cookies.insert(r.as_user.clone(), id);
                return GoldenEntry {
                    request: r.clone(),
                    status: 200,
                    ..GoldenEntry::default()
                };
            }
            "TCP" => {
                let out = (self.tcp)(&r.path);
                return GoldenEntry {
                    request: r.clone(),
                    body: normalize_body(r, &out),
                    ..GoldenEntry::default()
                };
            }
            _ => {}
        }

        let path = r.path.replace("{tx}", &self.tx_id);
        let answer = self.http(&r.method, &path, &r.body, &r.as_user);

        if r.path == "/api/tx/begin" {
            self.tx_id = json::unmarshal_value(&answer.body)
                .ok()
                .and_then(|v| {
                    v.get("transaction_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
        }

        GoldenEntry {
            request: r.clone(),
            status: i64::from(answer.status),
            content_type: answer.content_type,
            body: normalize_body(r, &answer.body),
        }
    }
}
