use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::math;
use std::path::PathBuf;

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/gomath")
        .join(rel)
}

fn read(rel: &str) -> String {
    let p = path(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex"))
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan()
}

fn check(
    rel: &str,
    exp: &dyn Fn(f64) -> f64,
    log: &dyn Fn(f64) -> f64,
    pow: &dyn Fn(f64, f64) -> f64,
) -> usize {
    let mut checked = 0;
    for line in read(rel).lines() {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "exp" => {
                let (x, want) = (bits(f[1]), bits(f[2]));
                assert!(
                    same(exp(x), want),
                    "exp({x:e}) = {:e}, want {want:e}",
                    exp(x)
                );
            }
            "log" => {
                let (x, want) = (bits(f[1]), bits(f[2]));
                assert!(
                    same(log(x), want),
                    "log({x:e}) = {:e}, want {want:e}",
                    log(x)
                );
            }
            "pow" => {
                let (x, y, want) = (bits(f[1]), bits(f[2]), bits(f[3]));
                assert!(
                    same(pow(x, y), want),
                    "pow({x:e}, {y:e}) = {:e}, want {want:e}",
                    pow(x, y)
                );
            }
            other => panic!("unknown case {other}"),
        }
        checked += 1;
    }
    checked
}

#[test]
fn golden_files_match_the_manifest() {
    let manifest = json::unmarshal_value(read("MANIFEST.json").as_bytes()).expect("manifest");
    assert_eq!(manifest.get("part").and_then(Value::as_str), Some("gomath"));
    let files = manifest
        .get("files")
        .and_then(Value::as_array)
        .expect("files");
    let mut names = Vec::new();
    for f in files {
        let name = f.get("path").and_then(Value::as_str).expect("path");
        let data = std::fs::read(path(name)).expect("golden file");
        let sum: String = sha256::sum256(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(f.get("sha256").and_then(Value::as_str), Some(sum.as_str()));
        names.push(name);
    }
    assert_eq!(names, ["amd64.cases", "arm64.cases"]);
}

#[test]
fn exp_log_and_pow_follow_go_on_arm64() {
    let n = check(
        "arm64.cases",
        &math::exp_arm64,
        &|x| math::log_go(x, true),
        &math::pow_arm64,
    );
    assert!(n > 10000);
}

#[test]
fn exp_log_and_pow_follow_go_on_amd64_without_fma() {
    let n = check(
        "amd64.cases",
        &|x| math::exp_amd64(x, false),
        &math::log_amd64,
        &|x, y| math::pow_amd64(x, y, false),
    );
    assert!(n > 10000);
}

#[cfg(target_arch = "aarch64")]
#[test]
fn dispatch_follows_go_on_this_architecture() {
    let n = check("arm64.cases", &math::exp, &math::log, &math::pow);
    assert!(n > 10000);
}
