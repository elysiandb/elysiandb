use elysiandb::config_struct;
use elysiandb::yaml::{
    self, Error, Expected, Got, Issue, IssueKind, NodeKind, PathSegment, SyntaxKind, Target, Value,
};
use std::path::PathBuf;

#[derive(Clone, Debug)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn field(&self, key: &str) -> &J {
        self.get(key)
            .unwrap_or_else(|| panic!("missing field {key} in {self:?}"))
    }

    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }

    fn arr(&self) -> &[J] {
        match self {
            J::Arr(items) => items,
            _ => panic!("not an array: {self:?}"),
        }
    }

    fn obj(&self) -> &[(String, J)] {
        match self {
            J::Obj(entries) => entries,
            _ => panic!("not an object: {self:?}"),
        }
    }

    fn int(&self) -> i64 {
        match self {
            J::Num(n) => n.parse().unwrap_or_else(|e| panic!("bad number {n}: {e}")),
            _ => panic!("not a number: {self:?}"),
        }
    }

    fn boolean(&self) -> bool {
        match self {
            J::Bool(b) => *b,
            _ => panic!("not a boolean: {self:?}"),
        }
    }
}

struct JsonReader<'a> {
    b: &'a [u8],
    i: usize,
}

impl JsonReader<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.b[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }

    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'n' => {
                self.i += 4;
                J::Null
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'"' => J::Str(self.string()),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(items);
                }
                loop {
                    items.push(self.value());
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    if c == b']' {
                        return J::Arr(items);
                    }
                    assert_eq!(c, b',');
                }
            }
            b'{' => {
                self.i += 1;
                let mut entries = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(entries);
                }
                loop {
                    self.ws();
                    let key = self.string();
                    self.eat(b':');
                    entries.push((key, self.value()));
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    if c == b'}' {
                        return J::Obj(entries);
                    }
                    assert_eq!(c, b',');
                }
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                J::Num(String::from_utf8(self.b[start..self.i].to_vec()).unwrap())
            }
        }
    }

    fn hex4(&mut self) -> u32 {
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
        self.i += 4;
        u32::from_str_radix(s, 16).unwrap()
    }

    fn string(&mut self) -> String {
        assert_eq!(self.b[self.i], b'"');
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).unwrap(),
                b'\\' => {
                    let e = self.b[self.i];
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4();
                            if (0xD800..0xDC00).contains(&cp) && self.b[self.i] == b'\\' {
                                self.i += 2;
                                let low = self.hex4();
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                            }
                            char::from_u32(cp).unwrap()
                        }
                        _ => panic!("bad escape {}", e as char),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/yaml")
        .join(rel)
}

fn read_rows(rel: &str) -> Vec<J> {
    let data = std::fs::read(golden_path(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
    data.split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|line| JsonReader { b: line, i: 0 }.value())
        .collect()
}

fn line_count(rel: &str) -> usize {
    let data = std::fs::read(golden_path(rel)).unwrap();
    data.iter().filter(|&&b| b == b'\n').count()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn input_of(row: &J) -> Vec<u8> {
    match row.get("input") {
        Some(J::Str(s)) => s.as_bytes().to_vec(),
        _ => unhex(row.field("input_hex").str()),
    }
}

config_struct! {
pub struct ProbeInner {
    x: i64 = "x",
    y: Vec<String> = "y",
}
pub struct Probe {
    a: i64 = "a",
    b: String = "b",
    c: bool = "c",
    d: Vec<String> = "d",
    e: Option<bool> = "e",
    f: ProbeInner = "f",
    g: Vec<ProbeInner> = "g",
    h: Value = "h",
}
}

config_struct! {
pub struct SecurityConfig {
    authentication: AuthenticationConfig = "authentication",
}
pub struct AuthenticationConfig {
    enabled: bool = "enabled",
    mode: String = "mode",
    token: String = "token",
}
pub struct AdminUiConfig {
    enabled: bool = "enabled",
}
pub struct ServersConfig {
    http: ServerConfig = "http",
    tcp: ServerConfig = "tcp",
}
pub struct LogConfig {
    level: String = "level",
    format: String = "format",
    access_log: bool = "accessLog",
    slow_request_ms: i64 = "slowRequestMs",
    buffer_lines: i64 = "bufferLines",
    flush_interval_seconds: i64 = "flushIntervalSeconds",
}
pub struct PrometheusConfig {
    enabled: bool = "enabled",
    token: String = "token",
}
pub struct MonitoringConfig {
    prometheus: PrometheusConfig = "prometheus",
    pprof: PprofConfig = "pprof",
}
pub struct PprofConfig {
    enabled: bool = "enabled",
}
pub struct ServerConfig {
    enabled: bool = "enabled",
    host: String = "host",
    port: i64 = "port",
    cors: CorsConfig = "cors",
    trusted_proxies: Vec<String> = "trustedProxies",
    read_timeout_seconds: i64 = "readTimeoutSeconds",
    write_timeout_seconds: i64 = "writeTimeoutSeconds",
}
pub struct CorsConfig {
    allowed_origins: Vec<String> = "allowedOrigins",
}
pub struct CrashRecoveryConfig {
    enabled: bool = "enabled",
    max_log_mb: i64 = "maxLogMB",
}
pub struct StoreConfig {
    folder: String = "folder",
    shards: i64 = "shards",
    flush_interval_seconds: i64 = "flushIntervalSeconds",
    crash_recovery: CrashRecoveryConfig = "crashRecovery",
    durability: String = "durability",
    memory: StoreMemoryConfig = "memory",
    min_free_disk_mb: i64 = "minFreeDiskMB",
    backup: StoreBackupConfig = "backup",
}
pub struct StoreBackupConfig {
    folder: String = "folder",
    every_minutes: i64 = "everyMinutes",
    keep: i64 = "keep",
}
pub struct StoreMemoryConfig {
    limit_mb: i64 = "limitMB",
}
pub struct StatsConfig {
    enabled: bool = "enabled",
}
pub struct ApiSchemaConfig {
    enabled: bool = "enabled",
    strict: bool = "strict",
}
pub struct ApiConfig {
    index: ApiIndexConfig = "index",
    cache: ApiCacheConfig = "cache",
    schema: ApiSchemaConfig = "schema",
    hooks: HooksConfig = "hooks",
    query: ApiQueryConfig = "query",
    response: ApiResponseConfig = "response",
}
pub struct ApiQueryConfig {
    workers: i64 = "workers",
    timeout_ms: i64 = "timeoutMs",
    max_queued: i64 = "maxQueued",
    queue_timeout_ms: i64 = "queueTimeoutMs",
    cancel_on_disconnect: Option<bool> = "cancelOnDisconnect",
}
pub struct ApiResponseConfig {
    spool_mb: i64 = "spoolMB",
}
pub struct HooksConfig {
    enabled: bool = "enabled",
    timeout_ms: i64 = "timeoutMs",
    request_budget_ms: i64 = "requestBudgetMs",
}
pub struct ApiIndexConfig {
    workers: i64 = "workers",
}
pub struct ApiCacheConfig {
    enabled: bool = "enabled",
    cleanup_interval_seconds: i64 = "cleanupIntervalSeconds",
    max_mb: i64 = "maxMB",
    max_entry_kb: i64 = "maxEntryKB",
}
pub struct EngineConfig {
    name: String = "name",
}
pub struct Config {
    store: StoreConfig = "store",
    server: ServersConfig = "server",
    log: LogConfig = "log",
    security: SecurityConfig = "security",
    stats: StatsConfig = "stats",
    api: ApiConfig = "api",
    admin_ui: AdminUiConfig = "adminui",
    engine: EngineConfig = "engine",
    monitoring: MonitoringConfig = "monitoring",
}
}

fn default_config() -> Config {
    let mut c = Config::default();
    c.log.level = "info".to_string();
    c.log.format = "text".to_string();
    c.log.slow_request_ms = 500;
    c.log.buffer_lines = 5000;
    c
}

fn golden_canon(j: &J) -> String {
    let t = j.field("t").str();
    match t {
        "null" => "null".to_string(),
        "nilseq" => "seq[]".to_string(),
        "bool" => format!("bool:{}", j.field("v").boolean()),
        "int" => format!("int:{}", j.field("v").str()),
        "uint" => format!("uint:{}", j.field("v").str()),
        "float" => format!("float:{}", j.field("v").str()),
        "str" => match j.get("v") {
            Some(v) => format!("str:{}", hex(v.str().as_bytes())),
            None => format!("str:{}", j.field("hex").str()),
        },
        "seq" => {
            let items: Vec<String> = j.field("v").arr().iter().map(golden_canon).collect();
            format!("seq[{}]", items.join(","))
        }
        "map" => {
            let mut entries: Vec<String> = j
                .field("v")
                .arr()
                .iter()
                .map(|pair| {
                    let p = pair.arr();
                    format!("{}={}", golden_canon(&p[0]), golden_canon(&p[1]))
                })
                .collect();
            entries.sort();
            format!("map{{{}}}", entries.join(","))
        }
        other => panic!("unknown rendering {other}"),
    }
}

fn rust_canon(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("bool:{b}"),
        Value::Int(i) => format!("int:{i}"),
        Value::Uint(u) => format!("uint:{u}"),
        Value::Float(f) => format!("float:{:016x}", f.to_bits()),
        Value::Str(s) => format!("str:{}", hex(s)),
        Value::Seq(items) => {
            let items: Vec<String> = items.iter().map(rust_canon).collect();
            format!("seq[{}]", items.join(","))
        }
        Value::Map(entries) => {
            let mut entries: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}={}", rust_canon(k), rust_canon(v)))
                .collect();
            entries.sort();
            format!("map{{{}}}", entries.join(","))
        }
    }
}

fn leaves(v: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Map(entries) => {
            for (k, val) in entries {
                let key = String::from_utf8(k.as_bytes().unwrap().to_vec()).unwrap();
                let path = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                leaves(val, &path, out);
            }
        }
        other => out.push((prefix.to_string(), rust_canon(other))),
    }
}

fn expected_of(s: &str) -> Expected {
    match s {
        "integer" => Expected::Integer,
        "boolean" => Expected::Boolean,
        "string" => Expected::String,
        "strings" => Expected::Strings,
        "array" => Expected::Array,
        "mapping" => Expected::Mapping,
        other => panic!("unknown expected kind {other}"),
    }
}

fn got_of(e: &J) -> Got {
    match e.field("got").str() {
        "scalar" => Got::Scalar(e.field("text").str().to_string()),
        "sequence" => Got::Sequence,
        "mapping" => Got::Mapping,
        other => panic!("unknown got kind {other}"),
    }
}

fn issue_of(e: &J) -> Issue {
    let path = e
        .field("path")
        .arr()
        .iter()
        .map(|seg| match seg {
            J::Str(s) => PathSegment::Key(s.clone()),
            J::Num(_) => PathSegment::Index(seg.int() as usize),
            other => panic!("bad path segment {other:?}"),
        })
        .collect();
    let kind = match e.field("kind").str() {
        "type" => IssueKind::Mismatch {
            expected: expected_of(e.field("expected").str()),
            got: got_of(e),
        },
        "range" => {
            assert_eq!(e.field("expected").str(), "integer");
            IssueKind::OutOfRange {
                text: e.field("text").str().to_string(),
            }
        }
        "key" => {
            assert_eq!(e.field("expected").str(), "string");
            IssueKind::Key { got: got_of(e) }
        }
        "unknown" => IssueKind::Unknown,
        "duplicate" => IssueKind::Duplicate,
        other => panic!("unknown issue kind {other}"),
    };
    Issue {
        line: e.field("line").int() as usize,
        path,
        kind,
    }
}

fn fatal_of(f: &J) -> Error {
    match f.field("class").str() {
        "aliasing" => Error::Aliasing,
        "recursive-anchor" => Error::RecursiveAnchor(f.field("anchor").str().to_string()),
        "unknown-anchor" => Error::UnknownAnchor(f.field("anchor").str().to_string()),
        "tag" => Error::TagMismatch {
            resolved: f.field("resolved").str().to_string(),
            tag: f.field("tag").str().to_string(),
            text: f.field("text").str().to_string(),
        },
        "merge" => Error::Merge,
        "binary" => Error::Binary,
        "map-key" => Error::MapKey(match f.field("got").str() {
            "mapping" => Got::Mapping,
            _ => Got::Sequence,
        }),
        other => panic!("unknown fatal class {other}"),
    }
}

fn check_parse_error(ctx: &str, want: &J, got: &Error) {
    if want.field("class").str() != "syntax" {
        assert_eq!(*got, fatal_of(want), "{ctx}");
        return;
    }
    let Error::Syntax(e) = got else {
        panic!("{ctx}: expected a syntax error, got {got:?}");
    };
    let kind = match want.field("kind").str() {
        "reader" => SyntaxKind::Reader,
        "scanner" => SyntaxKind::Scanner,
        _ => SyntaxKind::Parser,
    };
    assert_eq!(e.kind, kind, "{ctx}");
    assert_eq!(e.problem, want.field("problem").str(), "{ctx}");
    assert_eq!(
        e.line().unwrap_or(0) as i64,
        want.field("line").int(),
        "{ctx}"
    );
    let mark = |m: &yaml::Mark| vec![m.index as i64, m.line as i64, m.column as i64];
    let wmark = |k: &str| want.field(k).arr().iter().map(J::int).collect::<Vec<_>>();
    assert_eq!(mark(&e.problem_mark), wmark("problem_mark"), "{ctx}");
    assert_eq!(mark(&e.context_mark), wmark("context_mark"), "{ctx}");
    assert_eq!(
        e.problem_offset as i64,
        want.field("problem_offset").int(),
        "{ctx}"
    );
    assert_eq!(e.problem_value, want.field("problem_value").int(), "{ctx}");
}

fn kind_name(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Document => "doc",
        NodeKind::Mapping => "map",
        NodeKind::Sequence => "seq",
        NodeKind::Scalar => "scalar",
        NodeKind::Alias => "alias",
    }
}

fn check_parse(ctx: &str, input: &[u8], parse: &J) -> bool {
    let result = yaml::parse(input);
    if let Some(err) = parse.get("error") {
        let got = result.expect_err(ctx);
        check_parse_error(ctx, err, &got);
        let mut v = Value::Null;
        check_parse_error(ctx, err, &yaml::unmarshal(input, &mut v).expect_err(ctx));
        let mut c = default_config();
        check_parse_error(ctx, err, &yaml::unmarshal(input, &mut c).expect_err(ctx));
        let mut p = Probe::default();
        check_parse_error(
            ctx,
            err,
            &yaml::unmarshal_strict(input, &mut p).expect_err(ctx),
        );
        return false;
    }
    let doc = result.unwrap_or_else(|e| panic!("{ctx}: {e:?}"));
    if parse.get("empty").is_some() {
        assert!(doc.is_none(), "{ctx}");
        return true;
    }
    let doc = doc.unwrap_or_else(|| panic!("{ctx}: no document"));
    assert_eq!(doc.nodes.len() as i64, parse.field("count").int(), "{ctx}");
    assert_eq!(doc.depth() as i64, parse.field("depth").int(), "{ctx}");
    let Some(rows) = parse.get("nodes") else {
        return true;
    };
    for (n, row) in doc.nodes.iter().zip(rows.arr()) {
        let r = row.arr();
        assert_eq!(kind_name(n.kind), r[1].str(), "{ctx} node {}", r[0].int());
        assert_eq!(n.line as i64, r[2].int(), "{ctx} node {}", r[0].int());
        assert_eq!(n.column as i64, r[3].int(), "{ctx} node {}", r[0].int());
        match n.kind {
            NodeKind::Scalar => {
                assert_eq!(n.tag, r[4].str(), "{ctx}");
                assert_eq!(n.value, r[5].str(), "{ctx}");
                assert_eq!(n.implicit, r[6].boolean(), "{ctx}");
            }
            NodeKind::Alias => {
                assert_eq!(n.value, r[4].str(), "{ctx}");
                assert_eq!(n.alias.map(|a| a as i64), Some(r[5].int()), "{ctx}");
            }
            _ => {
                assert_eq!(n.tag, r[4].str(), "{ctx}");
                let ids: Vec<i64> = r[5].arr().iter().map(J::int).collect();
                let got: Vec<i64> = n.children.iter().map(|&c| c as i64).collect();
                assert_eq!(got, ids, "{ctx}");
            }
        }
    }
    let anchors: Vec<(String, i64)> = parse
        .field("anchors")
        .arr()
        .iter()
        .map(|a| (a.arr()[0].str().to_string(), a.arr()[1].int()))
        .collect();
    let got: Vec<(String, i64)> = doc
        .anchors
        .iter()
        .map(|(k, &v)| (k.clone(), v as i64))
        .collect();
    assert_eq!(got, anchors, "{ctx}");
    true
}

fn check_result(ctx: &str, want: &J, result: Result<(), Error>) {
    if let Some(fatal) = want.get("fatal") {
        assert_eq!(result.expect_err(ctx), fatal_of(fatal), "{ctx}");
        return;
    }
    let expected: Vec<Issue> = want.field("errors").arr().iter().map(issue_of).collect();
    match result {
        Ok(()) => assert!(expected.is_empty(), "{ctx}: expected {expected:?}"),
        Err(Error::Type(issues)) => assert_eq!(issues, expected, "{ctx}"),
        Err(other) => panic!("{ctx}: unexpected error {other:?}"),
    }
}

fn check_value<T: Target>(ctx: &str, want: &J, out: &T) {
    if let Some(v) = want.get("value") {
        assert_eq!(rust_canon(&out.to_value()), golden_canon(v), "{ctx}");
    }
}

fn decode_into<T: Target + Default + Send>(ctx: &str, input: &[u8], want: &J, strict: bool) {
    let mut out = T::default();
    let result = if strict {
        yaml::unmarshal_strict(input, &mut out)
    } else {
        yaml::unmarshal(input, &mut out)
    };
    check_result(ctx, want, result);
    if want.get("fatal").is_none() {
        check_value(ctx, want, &out);
    }
}

fn decode_string(ctx: &str, input: &[u8], want: &J) {
    let mut out = String::new();
    check_result(ctx, want, yaml::unmarshal(input, &mut out));
    let Some(value) = want.get("value") else {
        return;
    };
    match value.get("hex") {
        Some(raw) => {
            let lossy = String::from_utf8_lossy(&unhex(raw.str())).into_owned();
            assert_eq!(
                out, lossy,
                "{ctx}: invalid UTF-8 is replaced in a Rust String"
            );
        }
        None => assert_eq!(rust_canon(&out.to_value()), golden_canon(value), "{ctx}"),
    }
}

fn config_leaves(c: &Config) -> Vec<(String, String)> {
    let mut out = Vec::new();
    leaves(&c.to_value(), "", &mut out);
    out
}

fn check_config(ctx: &str, input: &[u8], want: &J) {
    let mut cfg = default_config();
    let result = yaml::unmarshal(input, &mut cfg);
    check_result(ctx, want, result);
    let Some(diff) = want.get("diff") else {
        return;
    };
    let base = config_leaves(&default_config());
    let mut expected: Vec<(String, String)> = diff
        .arr()
        .iter()
        .map(|e| {
            let p = e.arr();
            (p[0].str().to_string(), golden_canon(&p[1]))
        })
        .filter(|(path, value)| !base.iter().any(|(bp, bv)| bp == path && bv == value))
        .collect();
    expected.sort();
    let mut got: Vec<(String, String)> = config_leaves(&cfg)
        .into_iter()
        .filter(|leaf| !base.contains(leaf))
        .collect();
    got.sort();
    assert_eq!(got, expected, "{ctx}");
}

fn replay_file(rel: &str) -> usize {
    let rows = read_rows(rel);
    let mut checked = 0;
    for row in &rows {
        let name = row.field("name").str();
        let ctx = format!("{rel}: {name}");
        let input = input_of(row);
        let parsed = check_parse(&ctx, &input, row.field("parse"));
        if parsed {
            for (target, want) in row.field("targets").obj() {
                let ctx = format!("{ctx} [{target}]");
                match target.as_str() {
                    "iface" => decode_into::<Value>(&ctx, &input, want, false),
                    "int" => decode_into::<i64>(&ctx, &input, want, false),
                    "bool" => decode_into::<bool>(&ctx, &input, want, false),
                    "string" => decode_string(&ctx, &input, want),
                    "strings" => decode_into::<Vec<String>>(&ctx, &input, want, false),
                    "optbool" => decode_into::<Option<bool>>(&ctx, &input, want, false),
                    "struct" => decode_into::<Probe>(&ctx, &input, want, false),
                    "struct-strict" => decode_into::<Probe>(&ctx, &input, want, true),
                    "config" => check_config(&ctx, &input, want),
                    "config-strict" => decode_into::<Config>(&ctx, &input, want, true),
                    other => panic!("{ctx}: unknown target {other}"),
                }
            }
        }
        checked += 1;
    }
    assert_eq!(checked, line_count(rel));
    checked
}

#[test]
fn scalars_match_yaml_v2() {
    assert_eq!(replay_file("scalars.jsonl"), 328);
}

#[test]
fn documents_match_yaml_v2() {
    assert_eq!(replay_file("documents.jsonl"), 119);
}

#[test]
fn syntax_errors_match_yaml_v2() {
    assert_eq!(replay_file("syntax.jsonl"), 111);
}

#[test]
fn configurations_match_yaml_v2() {
    assert_eq!(replay_file("config.jsonl"), 1801);
}

#[test]
fn deep_documents_match_yaml_v2() {
    assert_eq!(replay_file("deep.jsonl"), 12);
}

#[test]
fn config_mirror_has_the_go_keys_and_defaults() {
    let rel = "config_keys.jsonl";
    let rows = read_rows(rel);
    let base = config_leaves(&default_config());
    let mut sections = Vec::new();
    let mut leaf_rows = Vec::new();
    for row in &rows {
        let path: Vec<&str> = row.field("path").arr().iter().map(J::str).collect();
        let dotted = path.join(".");
        if row.field("kind").str() == "section" {
            sections.push(dotted);
        } else {
            leaf_rows.push((dotted, golden_canon(row.field("default"))));
        }
    }
    assert_eq!(base, leaf_rows);
    let mut rust_sections = Vec::new();
    for (path, _) in &base {
        let parts: Vec<&str> = path.split('.').collect();
        for i in 1..parts.len() {
            let s = parts[..i].join(".");
            if !rust_sections.contains(&s) {
                rust_sections.push(s);
            }
        }
    }
    rust_sections.sort();
    sections.sort();
    assert_eq!(rust_sections, sections);
    assert_eq!(rows.len(), line_count(rel));
    assert_eq!(rows.len(), 81);
}

fn message_of(input: &str) -> (String, String) {
    let mut c = default_config();
    let err = yaml::unmarshal(input.as_bytes(), &mut c).unwrap_err();
    (err.message(), err.detail())
}

#[test]
fn syntax_messages_follow_d27() {
    let cases: &[(&str, &str)] = &[
        (
            "store:\n  folder: a\n folder2: b\n",
            "line 2: did not find expected key",
        ),
        (
            "a:\n\tb: c\n",
            "line 2: found character that cannot start any token",
        ),
        (
            "a: 1\nb: 2\nc: [x,\n  y\n",
            "line 4: did not find expected ',' or ']'",
        ),
        ("a: [1, 2\n", "line 1: did not find expected ',' or ']'"),
        ("\ta: b\n", "found character that cannot start any token"),
        ("a: *x\n", "unknown anchor 'x' referenced"),
        (
            "store:\n  shards: !!int abc\n",
            "cannot decode !!str \"abc\" as a !!int",
        ),
        (
            "server:\n  http:\n    <<: 1\n",
            "map merge requires map or sequence of maps as the value",
        ),
        (
            "store:\n  folder: !!binary \"%%\"\n",
            "!!binary value contains invalid base64 data",
        ),
        ("%YAML 1.2\n--- a\n", "found incompatible YAML document"),
        (
            "%YAML 1.1\n%YAML 1.1\n--- a\n",
            "line 1: found duplicate %YAML directive",
        ),
    ];
    for (input, detail) in cases {
        let (message, got) = message_of(input);
        assert_eq!(got, *detail, "{input:?}");
        assert_eq!(
            message,
            format!("invalid configuration file: {detail}"),
            "{input:?}"
        );
    }
    let err = yaml::unmarshal(b"a: \xff\n", &mut default_config()).unwrap_err();
    assert_eq!(
        err.message(),
        "invalid configuration file: invalid leading UTF-8 octet"
    );
    let mut v = Value::Null;
    let err = yaml::unmarshal(b"h: &a [*a]\n", &mut v).unwrap_err();
    assert_eq!(
        err.message(),
        "invalid configuration file: anchor 'a' value contains itself"
    );
    let err = yaml::unmarshal(b"{[a]: b}\n", &mut v).unwrap_err();
    assert_eq!(err.detail(), "invalid map key: a sequence");
    assert_eq!(
        err.to_string(),
        "invalid configuration file: invalid map key: a sequence"
    );
}

#[test]
fn type_messages_follow_d27() {
    let cases: &[(&str, &str)] = &[
        (
            "store:\n  shards: abc\n",
            "line 2: \"store.shards\" must be an integer, got \"abc\"",
        ),
        (
            "stats:\n  enabled: \"true\"\n",
            "line 2: \"stats.enabled\" must be a boolean, got \"true\"",
        ),
        (
            "server: 3\n",
            "line 1: \"server\" must be a mapping, got \"3\"",
        ),
        (
            "server:\n  - a\n",
            "line 2: \"server\" must be a mapping, got a sequence",
        ),
        (
            "server:\n  http:\n    port:\n      a: 1\n",
            "line 4: \"server.http.port\" must be an integer, got a mapping",
        ),
        (
            "server:\n  http:\n    port: .inf\n",
            "line 3: \"server.http.port\" must be an integer, got \".inf\"",
        ),
        (
            "store:\n  memory:\n    limitMB: 99999999999999999999\n",
            "line 3: \"store.memory.limitMB\": 99999999999999999999 is out of range",
        ),
        (
            "store:\n  memory:\n    limitMB: 18446744073709551615\n",
            "line 3: \"store.memory.limitMB\": 18446744073709551615 is out of range",
        ),
        (
            "server:\n  http:\n    port: \"8089\"\n",
            "line 3: \"server.http.port\" must be an integer, got \"8089\"",
        ),
        (
            "server:\n  http:\n    cors:\n      allowedOrigins: x\n",
            "line 4: \"server.http.cors.allowedOrigins\" must be an array of strings, got \"x\"",
        ),
        (
            "server:\n  http:\n    cors:\n      allowedOrigins: [a, {b: c}]\n",
            "line 4: \"server.http.cors.allowedOrigins[1]\" must be a string, got a mapping",
        ),
        (
            "api:\n  query:\n    cancelOnDisconnect: maybe\n",
            "line 3: \"api.query.cancelOnDisconnect\" must be a boolean, got \"maybe\"",
        ),
        (
            "- a\n- b\n",
            "line 1: the file must be a mapping, got a sequence",
        ),
        (
            "hello\n",
            "line 1: the file must be a mapping, got \"hello\"",
        ),
        (
            "store:\n  folder: \"a\\tb\\u0001\"\n  shards: \"\\u0001\"\n",
            "line 3: \"store.shards\" must be an integer, got \"\\u0001\"",
        ),
        (
            "store:\n  ? [a]\n  : 1\n",
            "line 2: keys of \"store\" must be strings, got a sequence",
        ),
        (
            "? {a: 1}\n: 1\n",
            "line 1: keys of the file must be strings, got a mapping",
        ),
    ];
    for (input, detail) in cases {
        let (message, got) = message_of(input);
        assert_eq!(got, *detail, "{input:?}");
        assert_eq!(
            message,
            format!("invalid configuration file: {detail}"),
            "{input:?}"
        );
    }
    let mut n: i64 = 0;
    let err = yaml::unmarshal(b"1e19\n", &mut n).unwrap_err();
    assert_eq!(err.detail(), "line 1: 1e19 is out of range");
}

#[test]
fn several_type_errors_give_one_line_each() {
    let input = "store:\n  shards: abc\n  folder: [x]\nserver:\n  http:\n    port: 1.5e30\n    enabled: maybe\nlog: [a]\n";
    let (message, detail) = message_of(input);
    let lines = [
        "line 2: \"store.shards\" must be an integer, got \"abc\"",
        "line 3: \"store.folder\" must be a string, got a sequence",
        "line 6: \"server.http.port\": 1.5e30 is out of range",
        "line 7: \"server.http.enabled\" must be a boolean, got \"maybe\"",
        "line 8: \"log\" must be a mapping, got a sequence",
    ];
    assert_eq!(detail, lines.join("\n"));
    let prefixed: Vec<String> = lines
        .iter()
        .map(|l| format!("invalid configuration file: {l}"))
        .collect();
    assert_eq!(message, prefixed.join("\n"));
    assert_eq!(
        format!("cannot parse the configuration file: {detail}"),
        "cannot parse the configuration file: line 2: \"store.shards\" must be an integer, got \"abc\"\nline 3: \"store.folder\" must be a string, got a sequence\nline 6: \"server.http.port\": 1.5e30 is out of range\nline 7: \"server.http.enabled\" must be a boolean, got \"maybe\"\nline 8: \"log\" must be a mapping, got a sequence"
    );
}

#[test]
fn strict_mode_reports_unknown_and_duplicate_keys_as_data() {
    let input = "server:\n  http:\n    cors:\n      allowed: [x]\n    bogus: 1\n  udp:\n    a: 1\nstore:\n  json: 1\n  folder: /a\n  folder: /b\nstore:\n  zzz: 1\nauthentication: {}\n";
    let mut c = Config::default();
    let err = yaml::unmarshal_strict(input.as_bytes(), &mut c).unwrap_err();
    assert_eq!(
        err.unknown_keys(),
        vec![
            "server.http.cors.allowed",
            "server.http.bogus",
            "server.udp",
            "store.json",
            "authentication"
        ]
    );
    assert_eq!(err.duplicate_keys(), vec!["store.folder", "store"]);
    assert!(
        err.issues()
            .iter()
            .all(|i| matches!(i.kind, IssueKind::Unknown | IssueKind::Duplicate))
    );
    assert_eq!(err.issues()[0].line, 4);
    assert_eq!(
        err.issues()[0].detail(),
        "line 4: unknown key \"server.http.cors.allowed\""
    );
    assert_eq!(err.issues()[5].detail(), "line 12: duplicate key \"store\"");
    assert_eq!(
        err.issues()[6].detail(),
        "line 14: unknown key \"authentication\""
    );
    let mut c = default_config();
    assert_eq!(yaml::unmarshal(input.as_bytes(), &mut c), Ok(()));
    assert_eq!(c.store.folder, "/b");
    let mut c = Config::default();
    assert_eq!(
        yaml::unmarshal_strict(b"store:\n  folder: /x\n", &mut c),
        Ok(())
    );
    let err = yaml::unmarshal_strict(
        b"a: &a {port: 5, zzz: 1}\nserver:\n  http:\n    <<: *a\n",
        &mut c,
    )
    .unwrap_err();
    assert_eq!(err.unknown_keys(), vec!["a", "server.http.zzz"]);
    assert!(err.duplicate_keys().is_empty());
}

#[test]
fn yaml_v2_values_are_kept() {
    let mut c = default_config();
    yaml::unmarshal(
        b"store:\n  shards: 0777\n  minFreeDiskMB: 0x1F\n  backup: {keep: 0b101, everyMinutes: +80}\nserver:\n  http: {port: 8_089, host: 127, enabled: on}\n  tcp: {port: 8089.7, enabled: Y}\nsecurity:\n  authentication: {token: 0x10, mode: yes}\nlog:\n  slowRequestMs: ~\napi:\n  query: {workers: -1.5, cancelOnDisconnect: off}\n",
        &mut c,
    )
    .unwrap();
    assert_eq!(c.store.shards, 511);
    assert_eq!(c.store.min_free_disk_mb, 31);
    assert_eq!(c.store.backup.keep, 5);
    assert_eq!(c.store.backup.every_minutes, 80);
    assert_eq!(c.server.http.port, 8089);
    assert_eq!(c.server.http.host, "127");
    assert!(c.server.http.enabled);
    assert_eq!(c.server.tcp.port, 8089);
    assert!(c.server.tcp.enabled);
    assert_eq!(c.security.authentication.token, "0x10");
    assert_eq!(c.security.authentication.mode, "yes");
    assert_eq!(c.log.slow_request_ms, 0);
    assert_eq!(c.log.buffer_lines, 5000);
    assert_eq!(c.api.query.workers, -1);
    assert_eq!(c.api.query.cancel_on_disconnect, Some(false));
    let mut c = default_config();
    yaml::unmarshal(b"log: ~\n", &mut c).unwrap();
    assert_eq!(c.log, LogConfig::default());
    let mut c = default_config();
    yaml::unmarshal(b"~\n", &mut c).unwrap();
    assert_eq!(c, Config::default());
    let mut c = default_config();
    yaml::unmarshal(b"", &mut c).unwrap();
    assert_eq!(c, default_config());
    let mut c = default_config();
    yaml::unmarshal(b"a: &a\n  port: 5\nserver:\n  http:\n    <<: *a\n    port: 6\n  tcp:\n    port: 6\n    <<: *a\n", &mut c).unwrap();
    assert_eq!((c.server.http.port, c.server.tcp.port), (6, 5));
    let mut c = default_config();
    yaml::unmarshal(
        b"store:\n  shards: 2\n---\nstore:\n  shards: [bad\n",
        &mut c,
    )
    .unwrap();
    assert_eq!(c.store.shards, 2);
}

config_struct! {
pub struct Boxed {
    inner: Box<ProbeInner> = "inner",
    maybe: Option<ProbeInner> = "maybe",
    list: Vec<i64> = "list",
}
}

#[test]
fn box_option_and_lists_follow_the_go_pointer_rules() {
    let mut b = Boxed::default();
    yaml::unmarshal(
        b"inner: {x: 3, y: [a]}\nmaybe: {x: 4}\nlist: [1, x, 3]\n",
        &mut b,
    )
    .unwrap_err();
    assert_eq!(b.inner.x, 3);
    assert_eq!(b.maybe.as_ref().map(|m| m.x), Some(4));
    assert_eq!(b.list, vec![1, 3]);
    let err = yaml::unmarshal(b"list: [1, x, 3]\n", &mut b).unwrap_err();
    assert_eq!(
        err.detail(),
        "line 1: \"list[1]\" must be an integer, got \"x\""
    );
    let err = yaml::unmarshal(b"list: x\n", &mut b).unwrap_err();
    assert_eq!(err.detail(), "line 1: \"list\" must be an array, got \"x\"");
    yaml::unmarshal(b"maybe: ~\ninner: ~\n", &mut b).unwrap();
    assert_eq!(b.maybe, None);
    assert_eq!(*b.inner, ProbeInner::default());
    let mut flag: Option<bool> = None;
    yaml::unmarshal(b"NULL\n", &mut flag).unwrap();
    assert_eq!(flag, Some(false));
    yaml::unmarshal(b"null\n", &mut flag).unwrap();
    assert_eq!(flag, None);
    let err = yaml::unmarshal(b"maybe\n", &mut flag).unwrap_err();
    assert_eq!(flag, Some(false));
    assert_eq!(
        err.detail(),
        "line 1: the file must be a boolean, got \"maybe\""
    );
    let mut list: Option<Vec<String>> = None;
    yaml::unmarshal(b"[]\n", &mut list).unwrap();
    assert_eq!(list, Some(Vec::new()));
    yaml::unmarshal(b"~\n", &mut list).unwrap();
    assert_eq!(list, None);
}

#[test]
fn node_lines_and_columns_are_yaml_v2_ones() {
    let doc = yaml::parse(b"a: &x\n  b: [1, *x]\n").unwrap().unwrap();
    let positions: Vec<(NodeKind, usize, usize)> = doc
        .nodes
        .iter()
        .map(|n| (n.kind, n.line, n.column))
        .collect();
    assert_eq!(
        positions,
        vec![
            (NodeKind::Document, 0, 0),
            (NodeKind::Mapping, 0, 0),
            (NodeKind::Scalar, 0, 0),
            (NodeKind::Mapping, 0, 3),
            (NodeKind::Scalar, 1, 2),
            (NodeKind::Sequence, 1, 5),
            (NodeKind::Scalar, 1, 6),
            (NodeKind::Alias, 1, 9),
        ]
    );
    assert_eq!(doc.nodes[7].alias, Some(3));
    assert_eq!(yaml::parse(b"").unwrap(), None);
    assert_eq!(yaml::parse(b"# only\n").unwrap(), None);
}

#[test]
fn float_two_to_the_63_follows_the_go_conversion_of_the_architecture() {
    let mut n: i64 = 0;
    yaml::unmarshal(b"9223372036854775808.0\n", &mut n).unwrap();
    let expected = if cfg!(target_arch = "x86_64") {
        i64::MIN
    } else {
        i64::MAX
    };
    assert_eq!(n, expected);
    yaml::unmarshal(b"-.inf\n", &mut n).unwrap();
    assert_eq!(n, i64::MIN);
    yaml::unmarshal(b"-1e20\n", &mut n).unwrap();
    assert_eq!(n, i64::MIN);
    yaml::unmarshal(b"-9.223372036854775808e18\n", &mut n).unwrap();
    assert_eq!(n, i64::MIN);
    yaml::unmarshal(b"9.223372036854775e18\n", &mut n).unwrap();
    assert_eq!(n, 9_223_372_036_854_774_784);
}
