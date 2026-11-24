use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{read_jsonl, str_field};
use crate::{assert_manifest_entry, bool_of, int_of, security_store};
use elysiandb::security::{self, BasicUser, LoginSource, Role};
use elysiandb::sync::Mutex;

const ROWS: usize = 92;

struct ClockRestore;

impl Drop for ClockRestore {
    fn drop(&mut self) {
        security::set_login_clock(None);
        security::reset_login_attempts();
    }
}

fn source(client: &str) -> LoginSource {
    LoginSource {
        ip: security::parse_ip(client.as_bytes()),
        local: client == "127.0.0.1",
    }
}

#[test]
fn guard_rows_match_go() {
    assert_manifest_entry("guard.jsonl");
    let _store = security_store("user");
    security::init_admin_user_if_not_exists().expect("admin");
    security::create_basic_user(&BasicUser {
        username: b"ann".to_vec(),
        password: b"right-pass".to_vec(),
        role: Role::user(),
    })
    .expect("ann");

    let clock = Arc::new(Mutex::new(Instant::now()));
    let now = clock.clone();
    security::set_login_clock(Some(Arc::new(move || *now.lock())));
    let _restore = ClockRestore;

    let mut scenario = String::new();
    let mut checked = 0;
    for row in &read_jsonl("security/guard.jsonl") {
        let name = str_field(row, "scenario");
        if name != scenario {
            if name != "default-admin-local" {
                security::reset_login_attempts();
            }
            scenario = name.to_string();
        }

        let advance = int_of(row, "advance_ns");
        *clock.lock() += Duration::from_nanos(u64::try_from(advance).expect("advance"));

        let username = str_field(row, "username");
        let password = match (bool_of(row, "right_password"), username) {
            (false, _) => "wrong",
            (true, "admin") => "admin",
            (true, _) => "right-pass",
        };
        let src = source(str_field(row, "client"));
        assert_eq!(src.client_key(), str_field(row, "client"), "{row:?}");

        let (_, wait, ok) =
            security::authenticate_user_from(username.as_bytes(), password.as_bytes(), src);

        assert_eq!(ok, bool_of(row, "ok"), "{row:?}");
        assert_eq!(
            i64::try_from(wait.as_nanos()).expect("wait"),
            int_of(row, "wait_ns"),
            "{row:?}"
        );
        assert_eq!(
            security::retry_after_seconds(wait),
            int_of(row, "retry_after"),
            "{row:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
