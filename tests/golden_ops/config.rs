use std::collections::BTreeMap;
use std::path::PathBuf;

use elysiandb::configuration::{default_config, load_config};
use elysiandb::log::{self, Options};

use super::common;
use super::tempdir::TempDir;
use super::yaml::without_pprof;
use super::{Capture, Restore, log_state};

const CORPUS: usize = 20;

fn golden_text(rel: &str) -> String {
    String::from_utf8(common::read_golden(rel)).expect("UTF-8 golden file")
}

#[test]
fn config_json_matches_go() {
    let _state = log_state();
    let _restore = Restore;

    log::configure(&Options::default()).expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);

    let files = std::fs::read_dir(common::golden_path("ops/config"))
        .expect("ops/config")
        .count();
    assert_eq!(files, 2 * CORPUS + 1);

    let mut checked = 0;

    let want = without_pprof(&golden_text("ops/config/default.json"));
    assert_eq!(default_config().to_json(), want, "default");
    checked += 1;

    let tmp = TempDir::new();
    let path = tmp.path().join("c.yaml");
    for n in 1..=CORPUS {
        let input = common::read_golden(&format!("ops/config/{n:02}.yaml"));
        std::fs::write(&path, &input).expect("write the case");
        let mut cfg = load_config(&path).unwrap_or_else(|e| panic!("{n:02}: {e}"));
        cfg.security.authentication.token = "secret-token".to_string();
        cfg.monitoring.prometheus.token = "metrics-token".to_string();
        let want = without_pprof(&golden_text(&format!("ops/config/{n:02}.json")));
        assert_eq!(cfg.to_json(), want, "{n:02}");
        checked += 1;
    }

    assert_eq!(capture.take(), "");
    assert_eq!(checked, CORPUS + 1);
}

fn unwrapped(ty: &str) -> &str {
    for wrapper in ["Option<", "Vec<", "Box<"] {
        if let Some(inner) = ty.strip_prefix(wrapper).and_then(|t| t.strip_suffix('>')) {
            return inner;
        }
    }
    ty
}

fn declared_structs() -> BTreeMap<String, Vec<(String, String)>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/configuration");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("src/configuration")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();

    let mut structs = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("source file");
        let mut current: Option<(String, Vec<(String, String)>)> = None;
        for line in text.lines() {
            let t = line.trim();
            match current.take() {
                None => {
                    if let Some(name) = t
                        .strip_prefix("pub struct ")
                        .and_then(|r| r.strip_suffix(" {"))
                    {
                        current = Some((name.to_string(), Vec::new()));
                    }
                }
                Some((name, fields)) if t == "}" => {
                    assert!(
                        structs.insert(name.clone(), fields).is_none(),
                        "{name} twice"
                    );
                }
                Some((name, mut fields)) => {
                    let (_, rest) = t
                        .split_once(": ")
                        .unwrap_or_else(|| panic!("{name}: bad field line {t:?}"));
                    let (ty, key) = rest
                        .split_once(" = ")
                        .unwrap_or_else(|| panic!("{name}: bad field line {t:?}"));
                    let key = key
                        .strip_prefix('"')
                        .and_then(|k| k.strip_suffix("\","))
                        .unwrap_or_else(|| panic!("{name}: bad field line {t:?}"));
                    fields.push((unwrapped(ty).to_string(), key.to_string()));
                    current = Some((name, fields));
                }
            }
        }
        assert!(current.is_none(), "unclosed struct in {}", file.display());
    }
    structs
}

fn collect_keys(
    structs: &BTreeMap<String, Vec<(String, String)>>,
    name: &str,
    prefix: &str,
    out: &mut Vec<String>,
) {
    for (ty, key) in &structs[name] {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        out.push(full.clone());
        if structs.contains_key(ty) {
            collect_keys(structs, ty, &full, out);
        }
    }
}

#[test]
fn configuration_files_declare_the_frozen_keys() {
    let structs = declared_structs();
    let mut keys = Vec::new();
    collect_keys(&structs, "Config", "", &mut keys);
    keys.sort();

    let frozen: Vec<String> = common::lines(&common::read_golden("guards/capabilities.tsv"))
        .into_iter()
        .filter_map(|l| l.strip_prefix(b"key\t"))
        .map(|k| String::from_utf8(k.to_vec()).expect("UTF-8 key"))
        .collect();

    assert_eq!(keys, frozen);
    assert_eq!(keys.len(), 79);
}
