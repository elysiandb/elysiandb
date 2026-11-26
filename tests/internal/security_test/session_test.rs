use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use elysiandb::gocompat::json;
use elysiandb::httpd::RequestCtx;
use elysiandb::security::{self, Role, Session, SessionsFile};

use crate::{basic_user, ctx_with_cookie, seed_user, setup_temp_store, update_config};

const HOUR: Duration = Duration::from_secs(3600);
const SECOND: Duration = Duration::from_secs(1);

fn enable_user_mode() {
    update_config(|cfg| {
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "user".to_string();
    });
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn write_sessions(dir: &str, sf: &SessionsFile) {
    let b = json::marshal(sf).expect("marshal");
    let _ = fs::write(Path::new(dir).join(security::SESSIONS_FILENAME), b);
}

fn read_sessions(dir: &str) -> Vec<Session> {
    let data = fs::read(Path::new(dir).join(security::SESSIONS_FILENAME)).unwrap_or_default();
    let mut sf = SessionsFile::default();
    let _ = json::unmarshal(&data, &mut sf);
    sf.sessions.unwrap_or_default()
}

fn old_session(id: &str, username: &str, role: Role) -> Session {
    Session {
        id: id.to_string(),
        username: username.into(),
        role,
        expires_at: unix_now() - 10,
        ..Session::default()
    }
}

fn session_ctx(role: Role, username: &str, ttl: Duration) -> RequestCtx {
    seed_user(username, role.clone());
    let s = security::create_session(username.as_bytes(), role, ttl).unwrap_or_default();
    ctx_with_cookie(&s.id)
}

#[test]
fn get_current_username() {
    let mut ctx = RequestCtx::new();
    assert!(
        security::get_current_username(&ctx).is_empty(),
        "expected empty username"
    );

    ctx.set_user_value(security::USERNAME_KEY, b"alice".to_vec());
    assert_eq!(
        security::get_current_username(&ctx),
        b"alice",
        "expected alice"
    );
}

#[test]
fn create_session() {
    let store = setup_temp_store();
    seed_user("user1", Role::admin());

    let s = security::create_session(b"user1", Role::admin(), SECOND)
        .unwrap_or_else(|e| panic!("unexpected error: {e}"));
    assert!(
        s.username.as_bytes() == b"user1" && s.role == Role::admin(),
        "invalid session data"
    );
    assert!(!s.id.is_empty(), "session id not generated");

    assert_eq!(read_sessions(&store.path()).len(), 1, "session not saved");
}

#[test]
fn create_session_cleans_expired() {
    let store = setup_temp_store();

    let expired = old_session("old", "u", Role::user());
    write_sessions(
        &store.path(),
        &SessionsFile {
            sessions: Some(vec![expired]),
        },
    );
    seed_user("new", Role::user());

    security::create_session(b"new", Role::user(), SECOND).expect("create session");

    assert_eq!(
        read_sessions(&store.path()).len(),
        1,
        "expired sessions not cleaned"
    );
}

#[test]
fn get_session() {
    let _store = setup_temp_store();
    seed_user("user", Role::user());

    let s = security::create_session(b"user", Role::user(), HOUR).expect("create session");

    let res =
        security::get_session(s.id.as_bytes()).unwrap_or_else(|e| panic!("unexpected error: {e}"));
    assert!(
        res.is_some_and(|r| r.username.as_bytes() == b"user"),
        "session not returned"
    );
}

#[test]
fn get_session_expired() {
    let store = setup_temp_store();

    let s = old_session("expired", "user", Role::user());
    write_sessions(
        &store.path(),
        &SessionsFile {
            sessions: Some(vec![s]),
        },
    );

    let res = security::get_session(b"expired").unwrap_or_else(|e| panic!("unexpected error: {e}"));
    assert!(res.is_none(), "expired session should be nil");

    seed_user("other", Role::user());
    security::create_session(b"other", Role::user(), HOUR).expect("create session");

    let sessions = read_sessions(&store.path());
    assert!(
        sessions.len() == 1 && sessions[0].username.as_bytes() == b"other",
        "expired sessions are removed at the next session change, got {sessions:?}"
    );
}

#[test]
fn delete_session() {
    let store = setup_temp_store();
    seed_user("user", Role::user());

    let s = security::create_session(b"user", Role::user(), HOUR).expect("create session");

    security::delete_session(s.id.as_bytes()).unwrap_or_else(|e| panic!("unexpected error: {e}"));

    assert!(
        read_sessions(&store.path()).is_empty(),
        "session not deleted"
    );
}

#[test]
fn delete_session_non_existing() {
    let store = setup_temp_store();

    write_sessions(
        &store.path(),
        &SessionsFile {
            sessions: Some(Vec::new()),
        },
    );

    security::delete_session(b"none").unwrap_or_else(|e| panic!("unexpected error: {e}"));

    assert!(
        read_sessions(&store.path()).is_empty(),
        "file modified incorrectly"
    );
}

#[test]
fn current_session() {
    let _store = setup_temp_store();

    let ctx = session_ctx(Role::user(), "me", SECOND);

    let cs = security::current_session(&ctx);
    assert!(
        matches!(&cs, Ok(Some(s)) if s.username.as_bytes() == b"me"),
        "current session not resolved"
    );
}

#[test]
fn current_user_is_admin() {
    let _store = setup_temp_store();

    let ctx = session_ctx(Role::admin(), "admin", SECOND);

    assert!(security::current_user_is_admin(&ctx), "expected admin");
}

#[test]
fn current_user_is_admin_false() {
    let _store = setup_temp_store();
    enable_user_mode();

    let ctx = session_ctx(Role::user(), "user", SECOND);

    assert!(!security::current_user_is_admin(&ctx), "expected non admin");
}

fn setup_token_mode() -> crate::Store {
    let store = setup_temp_store();
    update_config(|cfg| {
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "token".to_string();
        cfg.security.authentication.token = "secret-token".to_string();
    });
    store
}

#[test]
fn current_user_is_admin_token_mode_valid_token() {
    let _store = setup_token_mode();

    let ctx = crate::ctx_with_header("Authorization", "Bearer secret-token");

    assert!(security::current_user_is_admin(&ctx), "expected admin");
}

#[test]
fn current_user_is_admin_token_mode_invalid_token() {
    let _store = setup_token_mode();

    let ctx = crate::ctx_with_header("Authorization", "Bearer wrong-token");

    assert!(!security::current_user_is_admin(&ctx), "expected non admin");
}

#[test]
fn current_user_is_admin_token_mode_session_cookie_is_not_enough() {
    let _store = setup_token_mode();

    let ctx = session_ctx(Role::admin(), "admin", SECOND);

    assert!(!security::current_user_is_admin(&ctx), "expected non admin");
}

#[test]
fn current_user_can_manage_user_admin() {
    let _store = setup_temp_store();

    let ctx = session_ctx(Role::admin(), "admin", SECOND);

    let ok = security::current_user_can_manage_user(&ctx, b"someone");
    assert!(matches!(ok, Ok(true)), "admin should manage anyone");
}

#[test]
fn current_user_can_manage_user_self() {
    let _store = setup_temp_store();
    enable_user_mode();

    let ctx = session_ctx(Role::user(), "bob", SECOND);

    let ok = security::current_user_can_manage_user(&ctx, b"bob");
    assert!(matches!(ok, Ok(false)), "user should not manage self");
}

#[test]
fn current_user_can_manage_user_other() {
    let _store = setup_temp_store();

    let ctx = session_ctx(Role::user(), "bob", SECOND);

    let ok = security::current_user_can_manage_user(&ctx, b"alice");
    assert!(matches!(ok, Ok(true)), "user should manage other user");
}

#[test]
fn sessions_file_holds_no_usable_id() {
    let store = setup_temp_store();
    seed_user("admin", Role::admin());

    let s = security::create_session(b"admin", Role::admin(), HOUR).expect("create session");

    let path = Path::new(&store.path()).join(security::SESSIONS_FILENAME);
    let data = fs::read(&path).unwrap_or_default();
    assert!(
        !String::from_utf8_lossy(&data).contains(&s.id),
        "expected the session ID not to be stored, got {}",
        String::from_utf8_lossy(&data)
    );

    let mode = fs::metadata(&path).map(|m| m.permissions().mode() & 0o777);
    assert!(
        matches!(mode, Ok(0o600)),
        "expected the sessions file to be private, got {mode:?}"
    );
}

#[test]
fn session_ends_when_the_account_changes() {
    type Change = fn();
    let cases: [(&str, Change); 4] = [
        ("deleted", || {
            seed_user("other", Role::admin());
            security::delete_basic_user(b"bob").expect("delete");
        }),
        ("password changed", || {
            security::change_user_password(b"bob", b"another-password").expect("password");
        }),
        ("demoted", || {
            seed_user("other", Role::admin());
            security::change_user_role(b"bob", b"user").expect("role");
        }),
        ("recreated", || {
            seed_user("bob", Role::admin());
        }),
    ];

    for (name, change) in cases {
        let _store = setup_temp_store();
        security::create_basic_user(&basic_user("bob", "bob-password", Role::admin()))
            .expect("create bob");

        let s = security::create_session(b"bob", Role::admin(), HOUR).expect("create session");

        change();

        let got = security::get_session(s.id.as_bytes()).ok().flatten();
        assert!(
            got.is_none(),
            "{name}: expected the session to end, got {got:?}"
        );
    }
}

#[test]
fn session_follows_the_current_role() {
    let _store = setup_temp_store();
    seed_user("bob", Role::user());

    let s = security::create_session(b"bob", Role::admin(), HOUR).expect("create session");

    let got = security::get_session(s.id.as_bytes()).ok().flatten();
    assert!(
        got.as_ref().is_some_and(|g| g.role == Role::user()),
        "expected the stored role to apply, got {got:?}"
    );
}

#[test]
fn password_change_can_keep_the_current_session() {
    let _store = setup_temp_store();
    security::create_basic_user(&basic_user("bob", "bob-password", Role::admin()))
        .expect("create bob");

    let current = security::create_session(b"bob", Role::admin(), HOUR).expect("current");
    let other = security::create_session(b"bob", Role::admin(), HOUR).expect("other");

    security::change_user_password_keeping_session(
        b"bob",
        b"another-password",
        current.id.as_bytes(),
    )
    .expect("change password");

    assert!(
        security::get_session(current.id.as_bytes())
            .ok()
            .flatten()
            .is_some(),
        "expected the current session to stay open"
    );

    assert!(
        security::get_session(other.id.as_bytes())
            .ok()
            .flatten()
            .is_none(),
        "expected the other session to end"
    );
}

#[test]
fn create_session_requires_an_existing_user() {
    let _store = setup_temp_store();

    assert!(
        security::create_session(b"ghost", Role::admin(), HOUR).is_err(),
        "expected an error for an unknown user"
    );
}

#[test]
fn protect_sessions_file() {
    let store = setup_temp_store();
    let path = Path::new(&store.path()).join(security::SESSIONS_FILENAME);
    fs::write(&path, br#"{"sessions":[]}"#).expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");

    security::protect_sessions_file();

    let mode = fs::metadata(&path).map(|m| m.permissions().mode() & 0o777);
    assert!(
        matches!(mode, Ok(0o600)),
        "expected the sessions file to be private, got {mode:?}"
    );

    security::protect_sessions_file();
}
