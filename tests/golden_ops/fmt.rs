use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv;
use elysiandb::gocompat::time::Duration;
use elysiandb::kv;
use elysiandb::log::{self, Options, ToValue};

use super::cli::{BIN, cli_config};
use super::common::{self, str_field, unhex};
use super::pty;
use super::tempdir::TempDir;
use super::{Capture, Restore, log_state};

fn text_value(capture: &Capture, v: &dyn ToValue) -> String {
    log::info("fmt", kv!["v" => v]);
    let line = capture.take();
    let at = line.find(" v=").expect("value attribute");
    line[at + 3..]
        .strip_suffix('\n')
        .expect("one line")
        .to_string()
}

fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

#[test]
fn fmt_vectors_match_go() {
    let _state = log_state();
    let _restore = Restore;

    log::configure(&Options::default()).expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);

    let rows = common::read_jsonl("ops/fmt/cases.jsonl");
    let (mut floats, mut durations, mut quotes, mut banners) = (0, 0, 0, 0);

    for row in &rows {
        match str_field(row, "fn") {
            "metrics-float" => {
                let bits = u64::from_str_radix(str_field(row, "bits"), 16).expect("bits");
                let f = f64::from_bits(bits);
                assert_eq!(
                    strconv::format_float(f, b'g', -1, 64),
                    str_field(row, "prometheus_g"),
                    "{bits:016x}"
                );
                assert_eq!(
                    strconv::format_float(f / 1000.0, b'g', -1, 64),
                    str_field(row, "bucket_le"),
                    "{bits:016x}"
                );
                if f.is_finite() {
                    let rounded = (f * 1000.0).round() / 1000.0;
                    let want = str_field(row, "series_json");
                    assert_eq!(
                        strconv::format_float(rounded, b'f', -1, 64),
                        want,
                        "{bits:016x}"
                    );
                    assert_eq!(text_value(&capture, &rounded), want, "{bits:016x}");
                } else {
                    assert!(row.get("series_json").is_none(), "{bits:016x}");
                }
                floats += 1;
            }
            "duration" => {
                let ns: i64 = str_field(row, "ns").parse().expect("ns");
                let d = Duration(ns);
                let want = str_field(row, "string");
                assert_eq!(d.to_string(), want);
                assert_eq!(
                    d.round(Duration::MILLISECOND).to_string(),
                    str_field(row, "round_ms")
                );
                assert_eq!(
                    d.round(Duration::SECOND).to_string(),
                    str_field(row, "round_s")
                );
                assert_eq!(d.milliseconds(), int_field(row, "ms"));
                assert_eq!(text_value(&capture, &d), want);
                durations += 1;
            }
            "quote" => {
                let input = unhex(str_field(row, "in"));
                let want = str_field(row, "out");
                assert_eq!(strconv::quote(&input), want);
                let rendered = text_value(&capture, &input);
                assert!(
                    rendered == want || rendered.as_bytes() == input.as_slice(),
                    "{rendered:?} is neither the raw value nor {want:?}"
                );
                quotes += 1;
            }
            "banner" => {
                let want = unhex(str_field(row, "hex"));
                let shown = run_on_terminal();
                assert!(
                    shown.starts_with(&want),
                    "the banner differs: {:?}",
                    String::from_utf8_lossy(&shown)
                );
                banners += 1;
            }
            name => panic!("unexpected fmt case {name}"),
        }
    }

    assert_eq!((floats, durations, quotes, banners), (321, 12, 13, 1));
    assert_eq!(floats + durations + quotes + banners, rows.len());
}

fn run_on_terminal() -> Vec<u8> {
    use std::process::{Command, Stdio};

    let dir = TempDir::new();
    std::fs::write(dir.path().join("c.yaml"), cli_config("", 1)).expect("config");

    let mut term = pty::open();
    let mut child = Command::new(BIN)
        .args(["-config", "c.yaml", "help"])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stdout(Stdio::from(term.slave()))
        .stderr(Stdio::null())
        .spawn()
        .expect("start the CLI");

    assert!(
        term.wait_for(b"Start ElysianDB server\n"),
        "no help on the terminal"
    );
    let status = child.wait().expect("wait for the CLI");
    assert!(status.success(), "{status:?}");
    term.seen
}
