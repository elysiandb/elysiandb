use std::io::{self, Write};
use std::sync::Arc;

use elysiandb::configuration::{self as cfgpkg, Config, LoadError, load_config};
use elysiandb::gocompat::json::{self, Value};
use elysiandb::log;
use elysiandb::sync::Mutex;

use super::serial;
use super::tempdir::TempDir;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Release;

impl Drop for Release {
    fn drop(&mut self) {
        log::set_output(None, false);
    }
}

fn capture_stdout(f: impl FnOnce()) -> String {
    let buf = Capture::default();
    log::set_output(Some(Box::new(buf.clone())), false);
    let release = Release;
    f();
    drop(release);
    let bytes = std::mem::take(&mut *buf.0.lock());
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn load_config_unknown_keys_are_reported_but_ignored() {
    let _serial = serial();
    let tmp = TempDir::new();
    let path = tmp.path().join("elysian.yaml");
    let content = "
store:
  folder: /tmp/x
  shards: 8
  json:
    arenaChunkSize: 1048576
authentication:
  enabled: true
server:
  http:
    enabled: true
    port: 8089
    cors:
      allowedOrigins: [\"https://app.example\", \"http://localhost:5173\"]
";
    std::fs::write(&path, content).unwrap();

    let mut result: Option<Result<Config, LoadError>> = None;
    let out = capture_stdout(|| result = Some(load_config(&path)));

    let cfg = match result {
        Some(Ok(cfg)) => cfg,
        other => panic!("expected unknown keys not to break loading, got {other:?}"),
    };

    assert!(
        cfg.store.shards == 8
            && cfg.server.http.port == 8089
            && !cfg.security.authentication.enabled,
        "unexpected config {cfg:?}"
    );

    let got = &cfg.server.http.cors.allowed_origins;
    assert!(
        got.len() == 2 && got[0] == "https://app.example",
        "expected the CORS allow-list to be parsed, got {got:?}"
    );

    for want in ["WARN", "keys=store.json,authentication"] {
        assert!(
            out.contains(want),
            "expected the warning to mention {want:?}, got {out:?}"
        );
    }
}

#[test]
fn load_config_known_keys_produce_no_warning() {
    let _serial = serial();
    let tmp = TempDir::new();
    let path = tmp.path().join("elysian.yaml");
    let content = "
store:
  folder: /tmp/x
security:
  authentication:
    enabled: true
    mode: user
";
    std::fs::write(&path, content).unwrap();

    let out = capture_stdout(|| {
        if let Err(e) = load_config(&path) {
            panic!("LoadConfig: {e}");
        }
    });

    assert!(!out.contains("WARN"), "expected no warning, got {out:?}");
}

fn field<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |v, key| {
        v.get(key)
            .unwrap_or_else(|| panic!("missing {key} in {v:?}"))
    })
}

fn text<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    field(v, path)
        .as_str()
        .unwrap_or_else(|| panic!("{path:?} is not a string"))
}

#[test]
fn config_to_json_redacts_secrets() {
    let _serial = serial();
    let mut cfg = Config::default();
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "token".to_string();
    cfg.security.authentication.token = "test-token".to_string();
    cfg.monitoring.prometheus.token = "prom-token".to_string();
    cfg.engine.name = "internal".to_string();

    let out = cfg.to_json();
    assert!(
        !out.contains("test-token") && !out.contains("prom-token"),
        "expected secrets to be redacted, got {out}"
    );

    let decoded = json::unmarshal_value(out.as_bytes()).unwrap_or_else(|e| panic!("{e}"));

    let auth = ["Security", "Authentication"];
    assert!(
        text(&decoded, &[auth[0], auth[1], "Token"]) == cfgpkg::REDACTED_VALUE
            && text(&decoded, &[auth[0], auth[1], "Mode"]) == "token",
        "unexpected authentication block {:?}",
        field(&decoded, &auth)
    );

    assert!(
        text(&decoded, &["Monitoring", "Prometheus", "Token"]) == cfgpkg::REDACTED_VALUE
            && text(&decoded, &["Engine", "Name"]) == "internal",
        "unexpected decoded configuration {decoded:?}"
    );

    assert!(
        cfg.security.authentication.token == "test-token"
            && cfg.monitoring.prometheus.token == "prom-token",
        "ToJson must not change the configuration"
    );

    let empty = Config::default().to_json();
    let decoded_empty = json::unmarshal_value(empty.as_bytes()).unwrap_or(Value::Null);
    assert!(
        text(&decoded_empty, &["Security", "Authentication", "Token"]).is_empty()
            && text(&decoded_empty, &["Monitoring", "Prometheus", "Token"]).is_empty(),
        "expected empty values to stay empty, got {empty}"
    );
}

#[test]
fn load_config_checks_the_engine() {
    let _serial = serial();
    let cases = [
        ("empty name", "engine:\n  name: \"\"\n", false, false),
        (
            "former uri key",
            "engine:\n  name: internal\n  uri: \"db://user:secret@localhost:1/x\"\n",
            false,
            true,
        ),
        ("internal", "engine:\n  name: internal\n", false, false),
        ("no engine block", "", false, false),
        ("other engine", "engine:\n  name: other\n", true, false),
        (
            "surrounding text",
            "engine:\n  name: \" internal\"\n",
            true,
            false,
        ),
        ("wrong case", "engine:\n  name: Internal\n", true, false),
    ];

    for (name, block, want_err, warns) in cases {
        let tmp = TempDir::new();
        let path = tmp.path().join("elysian.yaml");
        let content = format!("store:\n  folder: /tmp/x\n{block}");
        std::fs::write(&path, content).unwrap();

        let mut result: Option<Result<Config, LoadError>> = None;
        let out = capture_stdout(|| result = Some(load_config(&path)));
        let result = result.expect("LoadConfig ran");

        if want_err {
            match &result {
                Err(err) if err.to_string().contains("engine.name") => {}
                other => panic!("{name}: expected an engine.name error, got {other:?}"),
            }
            continue;
        }

        let cfg = match result {
            Ok(cfg) => cfg,
            Err(err) => panic!("{name}: unexpected error {err}"),
        };

        assert_eq!(
            out.contains("WARN"),
            warns,
            "{name}: unexpected warning output {out:?}"
        );

        assert!(
            !out.contains("secret") && !cfg.to_json().contains("secret"),
            "{name}: the former uri key must not be kept, got {out:?} and {}",
            cfg.to_json()
        );
    }
}

#[test]
fn load_config_checks_the_authentication_mode() {
    let _serial = serial();
    let cases = [
        (
            "disabled",
            "enabled: false\n    mode: anything",
            "anything",
            false,
        ),
        ("missing mode", "enabled: true", "", true),
        (
            "mixed case",
            "enabled: true\n    mode: \" User \"",
            "user",
            false,
        ),
        (
            "unknown mode",
            "enabled: true\n    mode: tokne\n    token: abc",
            "",
            true,
        ),
    ];

    for (name, block, mode, want_err) in cases {
        let tmp = TempDir::new();
        let path = tmp.path().join("elysian.yaml");
        let content =
            format!("store:\n  folder: /tmp/x\nsecurity:\n  authentication:\n    {block}\n");
        std::fs::write(&path, content).unwrap();

        let mut result: Option<Result<Config, LoadError>> = None;
        capture_stdout(|| result = Some(load_config(&path)));
        let result = result.expect("LoadConfig ran");

        if want_err {
            assert!(result.is_err(), "{name}: expected an error");
            continue;
        }

        match result {
            Ok(cfg) if cfg.security.authentication.mode == mode => {}
            other => panic!("{name}: unexpected result {other:?}"),
        }
    }
}
