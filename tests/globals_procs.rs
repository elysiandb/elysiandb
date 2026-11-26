use std::process::Command;

use elysiandb::globals::{self, PROCS_ENV, parse_procs};

const CHILD: &str = "ELYSIANDB_PROCS_CHILD";
const TEST: &str = "procs_follow_elysiandb_procs_like_gomaxprocs";

fn child_procs(value: Option<&str>) -> usize {
    let mut cmd = Command::new(std::env::current_exe().expect("test binary"));
    cmd.args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1");
    match value {
        Some(v) => cmd.env(PROCS_ENV, v),
        None => cmd.env_remove(PROCS_ENV),
    };
    let out = cmd.output().expect("run the child");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let after = stdout
        .split_once("procs=")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("no procs count in {stdout:?}"));
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("procs count")
}

#[test]
fn procs_follow_elysiandb_procs_like_gomaxprocs() {
    if std::env::var_os(CHILD).is_some() {
        println!("procs={}", globals::procs());
        return;
    }

    let cases: [(&str, Option<usize>); 14] = [
        ("3", Some(3)),
        ("1", Some(1)),
        ("03", Some(3)),
        ("2147483647", Some(2_147_483_647)),
        ("2147483648", None),
        ("0", None),
        ("-0", None),
        ("-1", None),
        ("-", None),
        ("+3", None),
        (" 3", None),
        ("3 ", None),
        ("abc", None),
        ("", None),
    ];
    for (value, want) in cases {
        assert_eq!(parse_procs(value.as_bytes()), want, "{value:?}");
    }

    let default = child_procs(None);
    assert!(default >= 1);
    assert_eq!(child_procs(Some("3")), 3);
    assert_eq!(child_procs(Some("0")), default);
    assert_eq!(child_procs(Some("+3")), default);
}
