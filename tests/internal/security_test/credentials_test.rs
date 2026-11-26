use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json;
use elysiandb::httpd::RequestCtx;
use elysiandb::security::{self, LoginSource, Role, SecurityError, SessionsFile};
use elysiandb::sync::Mutex;

use crate::{basic_user, ctx_with_header, ip, seed_user, set_authentication, setup, update_config};

fn local_source() -> LoginSource {
    LoginSource {
        ip: Some(ip("127.0.0.1")),
        local: true,
    }
}

#[test]
fn token_matches() {
    let _store = setup();
    set_authentication(true, "token", "secret-token");

    assert!(
        security::token_matches(b"secret-token"),
        "expected the token to match"
    );

    for wrong in [
        "",
        "secret",
        "secret-token ",
        "secret-tokens",
        "SECRET-TOKEN",
    ] {
        assert!(
            !security::token_matches(wrong.as_bytes()),
            "expected {wrong:?} not to match"
        );
    }

    set_authentication(true, "token", "");
    assert!(
        !security::token_matches(b""),
        "an empty configured token must never match"
    );
}

#[test]
fn authenticate_credentials() {
    let _store = setup();

    security::create_basic_user(&basic_user("ops", "ops pass", Role::admin())).expect("ops");
    security::create_basic_user(&basic_user("alice", "alice-pass", Role::user())).expect("alice");

    set_authentication(false, "", "");
    assert!(
        security::authenticate_credentials(b"", local_source())
            && !security::credentials_are_required(),
        "expected no credentials to be required with authentication disabled"
    );

    set_authentication(true, "token", "tcp-token");
    assert!(
        security::credentials_are_required()
            && security::authenticate_credentials(b" tcp-token ", local_source())
            && !security::authenticate_credentials(b"ops ops pass", local_source()),
        "unexpected token mode result"
    );

    set_authentication(true, "basic", "");
    assert!(
        security::authenticate_credentials(b"alice alice-pass", local_source())
            && security::authenticate_credentials(b"ops ops pass", local_source()),
        "expected any valid user in basic mode"
    );
    assert!(
        !security::authenticate_credentials(b"alice wrong", local_source())
            && !security::authenticate_credentials(b"alice", local_source())
            && !security::authenticate_credentials(b"ghost x", local_source()),
        "expected invalid credentials to be refused in basic mode"
    );

    set_authentication(true, "user", "");
    assert!(
        security::authenticate_credentials(b"ops ops pass", local_source()),
        "expected an admin to authenticate in user mode"
    );
    assert!(
        !security::authenticate_credentials(b"alice alice-pass", local_source()),
        "expected a regular user to be refused in user mode"
    );
}

#[test]
fn current_user_is_admin_without_authentication() {
    let _store = setup();
    set_authentication(false, "", "");

    let ctx = RequestCtx::new();
    assert!(
        security::current_user_is_admin(&ctx),
        "expected every request to be allowed with authentication disabled"
    );
}

#[test]
fn current_user_is_admin_in_basic_mode() {
    let _store = setup();
    set_authentication(true, "basic", "");

    security::create_basic_user(&basic_user("john", "secret", Role::user())).expect("john");

    let basic =
        |credentials: &[u8]| format!("Basic {}", STD_ENCODING.encode_to_string(credentials));

    assert!(
        security::current_user_is_admin(&ctx_with_header("Authorization", &basic(b"john:secret"))),
        "expected valid basic credentials to be accepted"
    );

    assert!(
        !security::current_user_is_admin(&ctx_with_header("Authorization", ""))
            && !security::current_user_is_admin(&ctx_with_header(
                "Authorization",
                &basic(b"john:wrong")
            )),
        "expected missing or wrong credentials to be refused"
    );
}

#[test]
fn concurrent_sessions_are_all_kept() {
    let _store = setup();

    const COUNT: usize = 40;
    for i in 0..COUNT {
        seed_user(&format!("user{i}"), Role::admin());
        seed_user(&format!("extra{i}"), Role::user());
    }

    let ids = Arc::new(Mutex::new(vec![String::new(); COUNT]));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for i in 0..COUNT {
        let ids = ids.clone();
        let errors = errors.clone();
        handles.push(std::thread::spawn(move || {
            match security::create_session(
                format!("user{i}").as_bytes(),
                Role::admin(),
                Duration::from_secs(3600),
            ) {
                Ok(s) => ids.lock()[i] = s.id,
                Err(e) => errors.lock().push(format!("CreateSession: {e}")),
            }
        }));
    }
    for h in handles {
        h.join().expect("join");
    }
    assert!(errors.lock().is_empty(), "{:?}", errors.lock());

    let ids = ids.lock().clone();
    for (i, id) in ids.iter().enumerate() {
        let s = security::get_session(id.as_bytes());
        assert!(
            matches!(s, Ok(Some(_))),
            "session {i} was lost (err={:?})",
            s.err()
        );
    }

    let mut readers = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let id = id.clone();
        let errors_read = errors.clone();
        readers.push(std::thread::spawn(move || {
            if !matches!(security::get_session(id.as_bytes()), Ok(Some(_))) {
                errors_read
                    .lock()
                    .push(format!("concurrent read failed for {id}"));
            }
        }));
        let errors_write = errors.clone();
        readers.push(std::thread::spawn(move || {
            if let Err(e) = security::create_session(
                format!("extra{i}").as_bytes(),
                Role::user(),
                Duration::from_secs(3600),
            ) {
                errors_write.lock().push(format!("CreateSession: {e}"));
            }
        }));
    }
    for h in readers {
        h.join().expect("join");
    }
    assert!(errors.lock().is_empty(), "{:?}", errors.lock());

    for id in ids.iter().take(COUNT / 2) {
        security::delete_session(id.as_bytes()).expect("delete");
    }

    for (i, id) in ids.iter().enumerate() {
        let s = security::get_session(id.as_bytes()).ok().flatten();
        assert!(
            (i < COUNT / 2) == s.is_none(),
            "session {i}: unexpected state after deleting the first half"
        );
    }
}

#[test]
fn delete_basic_user_protects_default_and_last_admin() {
    let _store = setup();

    for u in [
        basic_user(security::DEFAULT_ADMIN_USERNAME, "x", Role::admin()),
        basic_user("ops", "x", Role::admin()),
        basic_user("alice", "x", Role::user()),
    ] {
        security::create_basic_user(&u).expect("create");
    }

    let err = security::delete_basic_user(security::DEFAULT_ADMIN_USERNAME.as_bytes());
    assert!(
        matches!(err, Err(SecurityError::DefaultAdminProtected)),
        "expected the default admin to be protected, got {err:?}"
    );

    let err = security::delete_basic_user(b"alice");
    assert!(
        err.is_ok(),
        "expected a regular user to be deleted, got {err:?}"
    );

    let err = security::change_user_role(security::DEFAULT_ADMIN_USERNAME.as_bytes(), b"user");
    assert!(
        err.is_ok(),
        "expected a demotion to succeed while another admin exists, got {err:?}"
    );

    let err = security::delete_basic_user(b"ops");
    assert!(
        matches!(err, Err(SecurityError::LastAdmin)),
        "expected the last admin to be protected, got {err:?}"
    );

    let err = security::change_user_role(b"ops", b"user");
    assert!(
        matches!(err, Err(SecurityError::LastAdmin)),
        "expected the last admin to keep its role, got {err:?}"
    );

    let err = security::change_user_role(b"ops", b"root");
    assert!(
        matches!(err, Err(SecurityError::InvalidRole)),
        "expected an invalid role to be refused, got {err:?}"
    );

    let err = security::change_user_role(b"ghost", b"user");
    assert!(
        err.as_ref()
            .is_err_and(|e| e.is_user_not_found() && e.to_string() == "user 'ghost' not found"),
        "expected a not found error, got {err:?}"
    );
}

#[test]
fn get_session_removes_only_expired_sessions() {
    let store = setup();

    seed_user("b", Role::admin());
    let live = security::create_session(b"b", Role::admin(), Duration::from_secs(3600))
        .expect("live session");

    security::create_session(b"b", Role::admin(), Duration::from_secs(1)).expect("short session");
    std::thread::sleep(Duration::from_millis(1100));

    let s = security::get_session(live.id.as_bytes());
    assert!(
        matches!(&s, Ok(Some(found)) if found.username.as_bytes() == b"b"),
        "expected the live session, got {s:?}"
    );

    security::delete_session(b"no-such-session").expect("delete");

    let path = Path::new(&store.path()).join(security::SESSIONS_FILENAME);
    let raw = std::fs::read(&path).expect("read sessions file");

    let mut sf = SessionsFile::default();
    json::unmarshal(&raw, &mut sf).unwrap_or_else(|e| {
        panic!(
            "expected a valid sessions file, got {:?}: {e}",
            String::from_utf8_lossy(&raw)
        )
    });

    let sessions = sf.sessions.unwrap_or_default();
    assert!(
        sessions.len() == 1 && sessions[0].username.as_bytes() == b"b",
        "expected only the live session to remain, got {sessions:?}"
    );

    let tmp = Path::new(&store.path()).join(format!("{}.tmp", security::SESSIONS_FILENAME));
    assert!(!tmp.exists(), "expected no temporary file to be left");
}

#[test]
fn create_session_reports_write_errors() {
    let _store = setup();
    seed_user("admin", Role::admin());
    let missing = crate::tempdir::TempDir::new();
    let folder = missing
        .path()
        .join("missing")
        .to_string_lossy()
        .into_owned();
    update_config(|cfg| cfg.store.folder = folder);

    assert!(
        security::create_session(b"admin", Role::admin(), Duration::from_secs(3600)).is_err(),
        "expected an error when the sessions file cannot be written"
    );
}
