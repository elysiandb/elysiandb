use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use elysiandb::configuration::AuthenticationConfig;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::httpd::RequestCtx;
use elysiandb::security::{self, Role};

use crate::{ctx_with_header, seed_user, setup_temp_store, update_config};

struct Writable(String);

impl Drop for Writable {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
    }
}

#[test]
fn sessions_are_kept_in_memory_and_written_on_change() {
    let store = setup_temp_store();
    let dir = store.path();
    seed_user("user", Role::admin());

    let s = security::create_session(b"user", Role::admin(), Duration::from_secs(3600))
        .expect("create session");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).expect("chmod");
    let _writable = Writable(dir.clone());

    for _ in 0..3 {
        let found = security::get_session(s.id.as_bytes());
        assert!(
            matches!(found, Ok(Some(_))),
            "reading a session needs no file access, got {found:?}"
        );
    }

    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    security::delete_session(s.id.as_bytes()).expect("delete");

    assert!(
        security::get_session(s.id.as_bytes())
            .ok()
            .flatten()
            .is_none(),
        "a deleted session must be gone"
    );

    let raw = fs::read(Path::new(&dir).join(security::SESSIONS_FILENAME));
    assert!(
        raw.as_ref().is_ok_and(|r| !r.is_empty()),
        "the change is written to the file: {raw:?}"
    );
}

#[test]
fn delete_all_sessions_ends_every_session() {
    let _store = setup_temp_store();
    seed_user("user", Role::admin());

    let s = security::create_session(b"user", Role::admin(), Duration::from_secs(3600))
        .expect("create session");

    security::delete_all_sessions().expect("delete all");

    assert!(
        security::get_session(s.id.as_bytes())
            .ok()
            .flatten()
            .is_none(),
        "deleting every session ends every session"
    );
}

#[test]
fn active_session_count() {
    let _store = setup_temp_store();
    seed_user("a", Role::admin());
    seed_user("b", Role::admin());

    let n = security::active_session_count();
    assert_eq!(n, 0, "expected no session, got {n}");

    security::create_session(b"a", Role::admin(), Duration::from_secs(3600)).expect("a");
    security::create_session(b"b", Role::admin(), Duration::from_secs(3600)).expect("b");

    let n = security::active_session_count();
    assert_eq!(n, 2, "expected 2 sessions, got {n}");

    security::create_session(b"a", Role::admin(), Duration::from_secs(1)).expect("short");
    std::thread::sleep(Duration::from_millis(1100));
    let n = security::active_session_count();
    assert_eq!(n, 2, "expired sessions are not counted, got {n}");
}

#[test]
fn actor_names_the_caller() {
    let _store = setup_temp_store();

    let mut ctx = RequestCtx::new();
    let got = security::actor(&ctx);
    assert_eq!(
        got,
        b"anonymous",
        "without authentication the actor is anonymous, got {:?}",
        String::from_utf8_lossy(&got)
    );

    update_config(|cfg| {
        cfg.security.authentication = AuthenticationConfig {
            enabled: true,
            mode: "token".to_string(),
            token: "t".to_string(),
        }
    });
    let got = security::actor(&ctx);
    assert_eq!(
        got,
        b"token",
        "in token mode the actor is token, got {:?}",
        String::from_utf8_lossy(&got)
    );

    update_config(|cfg| {
        cfg.security.authentication = AuthenticationConfig {
            enabled: true,
            mode: "basic".to_string(),
            ..AuthenticationConfig::default()
        }
    });
    let header = format!("Basic {}", STD_ENCODING.encode_to_string(b"alice:secret"));
    let basic = ctx_with_header("Authorization", &header);
    let got = security::actor(&basic);
    assert_eq!(
        got,
        b"alice",
        "in basic mode the actor is the username, got {:?}",
        String::from_utf8_lossy(&got)
    );
    let got = security::actor(&ctx);
    assert_eq!(
        got,
        b"unknown",
        "without a basic header the actor is unknown, got {:?}",
        String::from_utf8_lossy(&got)
    );

    ctx.set_user_value(security::USERNAME_KEY, "bob".to_string());
    let got = security::actor(&ctx);
    assert_eq!(
        got,
        b"bob",
        "a session user wins, got {:?}",
        String::from_utf8_lossy(&got)
    );
}
