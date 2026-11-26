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

const MATH_CASES: [&str; 24] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
    "cbrt", "expm1", "log1p", "log2", "log10", "trunc", "ceil", "floor", "sqrt", "atan2", "max",
    "min",
];

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
            other if MATH_CASES.contains(&other) => continue,
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

type Unary = fn(f64) -> f64;
type BoxedUnary = Box<dyn Fn(f64) -> f64>;
type Binary = fn(f64, f64) -> f64;

struct MathSet {
    unary: Vec<(&'static str, BoxedUnary)>,
    binary: Vec<(&'static str, Binary)>,
}

fn arch_set(a: math::Arch, trunc: Unary, ceil: Unary, floor: Unary) -> MathSet {
    MathSet {
        unary: vec![
            ("sin", Box::new(move |x| math::sin_arch(x, a))),
            ("cos", Box::new(move |x| math::cos_arch(x, a))),
            ("tan", Box::new(move |x| math::tan_arch(x, a))),
            ("asin", Box::new(move |x| math::asin_arch(x, a))),
            ("acos", Box::new(move |x| math::acos_arch(x, a))),
            ("atan", Box::new(move |x| math::atan_arch(x, a))),
            ("sinh", Box::new(move |x| math::sinh_arch(x, a))),
            ("cosh", Box::new(move |x| math::cosh_arch(x, a))),
            ("tanh", Box::new(move |x| math::tanh_arch(x, a))),
            ("asinh", Box::new(move |x| math::asinh_arch(x, a))),
            ("acosh", Box::new(move |x| math::acosh_arch(x, a))),
            ("atanh", Box::new(move |x| math::atanh_arch(x, a))),
            ("cbrt", Box::new(move |x| math::cbrt_arch(x, a))),
            ("expm1", Box::new(move |x| math::expm1_arch(x, a))),
            ("log1p", Box::new(move |x| math::log1p_arch(x, a))),
            ("log2", Box::new(move |x| math::log2_arch(x, a))),
            ("log10", Box::new(move |x| math::log10_arch(x, a))),
            ("trunc", Box::new(trunc)),
            ("ceil", Box::new(ceil)),
            ("floor", Box::new(floor)),
            ("sqrt", Box::new(math::sqrt)),
        ],
        binary: vec![
            ("atan2", if a.fused { atan2_fused } else { atan2_plain }),
            ("max", math::max),
            ("min", math::min),
        ],
    }
}

fn atan2_fused(y: f64, x: f64) -> f64 {
    math::atan2_go(y, x, true)
}

fn atan2_plain(y: f64, x: f64) -> f64 {
    math::atan2_go(y, x, false)
}

fn check_math(rel: &str, set: &MathSet) -> std::collections::BTreeMap<&'static str, usize> {
    let mut checked = std::collections::BTreeMap::new();
    for line in read(rel).lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if matches!(f[0], "exp" | "log" | "pow") {
            continue;
        }
        if let Some((name, u)) = set.unary.iter().find(|(n, _)| *n == f[0]) {
            let (x, want) = (bits(f[1]), bits(f[2]));
            let got = u(x);
            assert!(
                same(got, want),
                "{rel}: {name}({x:e} {}) = {got:e} {}, want {want:e} {}",
                f[1],
                hex(got),
                f[2]
            );
            *checked.entry(*name).or_insert(0) += 1;
            continue;
        }
        if let Some((name, b)) = set.binary.iter().find(|(n, _)| *n == f[0]) {
            let (y, x, want) = (bits(f[1]), bits(f[2]), bits(f[3]));
            let got = b(y, x);
            assert!(
                same(got, want),
                "{rel}: {name}({y:e}, {x:e}) = {got:e}, want {want:e}"
            );
            *checked.entry(*name).or_insert(0) += 1;
            continue;
        }
        panic!("unknown case {}", f[0]);
    }
    assert_eq!(checked.len(), MATH_CASES.len());
    checked
}

fn hex(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}

#[test]
fn math_functions_follow_go_on_arm64() {
    let set = arch_set(
        math::ARM64,
        math::trunc_arch,
        math::ceil_arch,
        math::floor_arch,
    );
    let n = check_math("arm64.cases", &set);
    assert!(n.values().all(|&c| c > 1000), "{n:?}");
}

#[test]
fn math_functions_follow_go_on_amd64_without_fma() {
    let set = arch_set(
        math::AMD64_WITHOUT_FMA,
        math::trunc_arch,
        math::ceil_arch,
        math::floor_arch,
    );
    let n = check_math("amd64.cases", &set);
    assert!(n.values().all(|&c| c > 1000), "{n:?}");
}

#[test]
fn go_fallbacks_follow_go_on_amd64() {
    let set = arch_set(math::GENERIC, math::trunc_go, math::ceil_go, math::floor_go);
    let mut checked = 0;
    for line in read("amd64.cases").lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if !matches!(
            f[0],
            "trunc" | "ceil" | "floor" | "max" | "min" | "atan2" | "sin"
        ) {
            continue;
        }
        if let Some((_, u)) = set.unary.iter().find(|(n, _)| *n == f[0]) {
            let (x, want) = (bits(f[1]), bits(f[2]));
            assert!(same(u(x), want), "{line}");
        } else if let Some((_, b)) = set.binary.iter().find(|(n, _)| *n == f[0]) {
            let (y, x, want) = (bits(f[1]), bits(f[2]), bits(f[3]));
            assert!(same(b(y, x), want), "{line}");
        }
        checked += 1;
    }
    assert!(checked > 10000);
}

#[cfg(target_arch = "aarch64")]
#[test]
fn math_dispatch_follows_go_on_this_architecture() {
    let mut set = arch_set(math::ARM64, math::trunc, math::ceil, math::floor);
    set.unary = vec![
        ("sin", Box::new(math::sin)),
        ("cos", Box::new(math::cos)),
        ("tan", Box::new(math::tan)),
        ("asin", Box::new(math::asin)),
        ("acos", Box::new(math::acos)),
        ("atan", Box::new(math::atan)),
        ("sinh", Box::new(math::sinh)),
        ("cosh", Box::new(math::cosh)),
        ("tanh", Box::new(math::tanh)),
        ("asinh", Box::new(math::asinh)),
        ("acosh", Box::new(math::acosh)),
        ("atanh", Box::new(math::atanh)),
        ("cbrt", Box::new(math::cbrt)),
        ("expm1", Box::new(math::expm1)),
        ("log1p", Box::new(math::log1p)),
        ("log2", Box::new(math::log2)),
        ("log10", Box::new(math::log10)),
        ("trunc", Box::new(math::trunc)),
        ("ceil", Box::new(math::ceil)),
        ("floor", Box::new(math::floor)),
        ("sqrt", Box::new(math::sqrt)),
    ];
    set.binary[0] = ("atan2", math::atan2);
    let n = check_math("arm64.cases", &set);
    assert!(n.values().all(|&c| c > 1000), "{n:?}");
}
