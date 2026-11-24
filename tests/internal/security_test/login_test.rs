use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security::{self, LoginSource, Role, SecurityError};
use elysiandb::sync::Mutex;

use crate::{
    AttemptsRestore, ClockRestore, basic_user, ctx_from, ip, setup, string_list, update_config,
};

fn remote_source(s: &str) -> LoginSource {
    LoginSource {
        ip: security::parse_ip(s.as_bytes()),
        local: false,
    }
}

fn fixed_clock(now: &Arc<Mutex<Instant>>) {
    let now = now.clone();
    security::set_login_clock(Some(Arc::new(move || *now.lock())));
}

struct EnvRestore(&'static str);

impl Drop for EnvRestore {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(self.0) };
    }
}

#[test]
fn failed_logins_lock_the_client_for_a_while() {
    let _store = setup();
    security::reset_login_attempts();
    let _attempts = AttemptsRestore;

    let now = Arc::new(Mutex::new(Instant::now()));
    fixed_clock(&now);
    let _clock = ClockRestore;

    security::create_basic_user(&basic_user("ops", "ops-password", Role::admin())).expect("ops");

    let client = remote_source("203.0.113.1");
    for i in 0..5 {
        let (_, wait, ok) = security::authenticate_user_from(b"ops", b"wrong", client);
        assert!(
            !ok && wait.is_zero(),
            "attempt {i}: expected a plain failure"
        );
    }

    let (_, wait, ok) = security::authenticate_user_from(b"ops", b"ops-password", client);
    assert!(
        !ok && !wait.is_zero(),
        "expected the client to be locked, got ok={ok} wait={wait:?}"
    );

    let (_, _, ok) =
        security::authenticate_user_from(b"ops", b"ops-password", remote_source("203.0.113.2"));
    assert!(ok, "expected another client to log in");

    *now.lock() += Duration::from_secs(2);
    let (_, _, ok) = security::authenticate_user_from(b"ops", b"ops-password", client);
    assert!(ok, "expected the lock to expire");
}

#[test]
fn lockout_grows_and_is_capped() {
    let _store = setup();
    security::reset_login_attempts();
    let _attempts = AttemptsRestore;

    let now = Arc::new(Mutex::new(Instant::now()));
    fixed_clock(&now);
    let _clock = ClockRestore;

    let client = remote_source("203.0.113.3");
    let mut last = Duration::ZERO;
    for _ in 0..30 {
        let _ = security::authenticate_user_from(b"ghost", b"wrong", client);
        let (_, wait, _) = security::authenticate_user_from(b"ghost", b"wrong", client);
        assert!(
            wait <= Duration::from_secs(15 * 60),
            "expected the lock to be capped, got {wait:?}"
        );
        if !wait.is_zero() {
            last = wait;
            *now.lock() += wait;
        }
    }

    assert_eq!(
        last,
        Duration::from_secs(15 * 60),
        "expected the lock to reach its cap, got {last:?}"
    );
}

#[test]
fn default_admin_password_only_from_the_local_machine() {
    let _store = setup();
    security::reset_login_attempts();
    let _attempts = AttemptsRestore;

    security::init_admin_user_if_not_exists().expect("init admin");

    let (_, _, ok) =
        security::authenticate_user_from(b"admin", b"admin", remote_source("203.0.113.4"));
    assert!(!ok, "expected the default password to be refused remotely");

    let local = LoginSource {
        ip: Some(ip("127.0.0.1")),
        local: true,
    };
    let (_, _, ok) = security::authenticate_user_from(b"admin", b"admin", local);
    assert!(ok, "expected the default password to work locally");

    assert!(
        security::default_admin_password_is_in_use(),
        "expected the default password to be reported"
    );
}

#[test]
fn initial_admin_password_from_the_environment() {
    let _store = setup();
    unsafe { std::env::set_var(security::ADMIN_PASSWORD_ENV_VAR, "initial-admin-password") };
    let _env = EnvRestore(security::ADMIN_PASSWORD_ENV_VAR);

    security::init_admin_user_if_not_exists().expect("init admin");

    assert!(
        security::authenticate_user(b"admin", b"initial-admin-password").is_some(),
        "expected the password from the environment"
    );

    assert!(
        !security::default_admin_password_is_in_use(),
        "expected the default password not to be set"
    );
}

fn new_request_from(peer: &str, headers: &[(&str, &str)]) -> RequestCtx {
    let mut req = Request::new();
    for (name, value) in headers {
        req.header.add(name, value);
    }

    ctx_from(&req, Some(ip(peer)))
}

#[test]
fn login_source_from_request() {
    let _store = setup();

    struct Case {
        name: &'static str,
        peer: &'static str,
        headers: &'static [(&'static str, &'static str)],
        proxies: &'static [&'static str],
        ip: &'static str,
        local: bool,
    }

    let cases = [
        Case {
            name: "loopback",
            peer: "127.0.0.1",
            headers: &[],
            proxies: &[],
            ip: "127.0.0.1",
            local: true,
        },
        Case {
            name: "remote",
            peer: "203.0.113.5",
            headers: &[],
            proxies: &[],
            ip: "203.0.113.5",
            local: false,
        },
        Case {
            name: "forwarded without trusted proxy",
            peer: "127.0.0.1",
            headers: &[("X-Forwarded-For", "127.0.0.1")],
            proxies: &[],
            ip: "127.0.0.1",
            local: false,
        },
        Case {
            name: "forwarded header ignored from untrusted peer",
            peer: "203.0.113.5",
            headers: &[("X-Forwarded-For", "127.0.0.1")],
            proxies: &[],
            ip: "203.0.113.5",
            local: false,
        },
        Case {
            name: "trusted proxy",
            peer: "10.0.0.2",
            headers: &[("X-Forwarded-For", "127.0.0.1, 198.51.100.7, 10.0.0.3")],
            proxies: &["10.0.0.0/8"],
            ip: "198.51.100.7",
            local: false,
        },
        Case {
            name: "trusted proxy for a local client",
            peer: "10.0.0.2",
            headers: &[("X-Forwarded-For", "127.0.0.1")],
            proxies: &["10.0.0.2"],
            ip: "127.0.0.1",
            local: true,
        },
        Case {
            name: "invalid forwarded value",
            peer: "10.0.0.2",
            headers: &[("X-Forwarded-For", "not-an-ip")],
            proxies: &["10.0.0.2"],
            ip: "10.0.0.2",
            local: false,
        },
    ];

    for tc in cases {
        let _store = setup();
        update_config(|cfg| cfg.server.http.trusted_proxies = string_list(tc.proxies));

        let source = security::login_source_from_request(&new_request_from(tc.peer, tc.headers));
        assert!(
            source.to_string() == tc.ip && source.local == tc.local,
            "{}: expected {} local={}, got {} local={}",
            tc.name,
            tc.ip,
            tc.local,
            source,
            source.local
        );
    }
}

#[test]
fn username_and_password_rules() {
    let long = vec![0u8; 129];
    let refused: [&[u8]; 7] = [b"", b".", b"..", b"a/b", b"with space", b"tab\tname", &long];
    for username in refused {
        assert!(
            matches!(
                security::validate_username(username),
                Err(SecurityError::InvalidUsername)
            ),
            "expected {:?} to be refused",
            String::from_utf8_lossy(username)
        );
    }

    for username in ["admin", "john.doe@example.com", "o'neil", "élodie"] {
        let result = security::validate_username(username.as_bytes());
        assert!(
            result.is_ok(),
            "expected {username:?} to be accepted, got {result:?}"
        );
    }

    assert!(
        matches!(
            security::validate_password(b"1234567"),
            Err(SecurityError::PasswordTooShort)
        ) && security::validate_password(b"12345678").is_ok(),
        "unexpected password rule"
    );

    assert!(
        security::validate_password("éééééééé".as_bytes()).is_ok(),
        "expected characters to be counted, not bytes"
    );
}

#[test]
fn cross_origin_write_detection() {
    let _store = setup();
    update_config(|cfg| {
        cfg.server.http.cors.allowed_origins = string_list(&["https://app.example"])
    });

    struct Case {
        method: &'static str,
        headers: &'static [(&'static str, &'static str)],
        reject: bool,
    }

    let cases = [
        Case {
            method: "POST",
            headers: &[],
            reject: false,
        },
        Case {
            method: "POST",
            headers: &[("Sec-Fetch-Site", "same-origin")],
            reject: false,
        },
        Case {
            method: "POST",
            headers: &[("Sec-Fetch-Site", "none")],
            reject: false,
        },
        Case {
            method: "POST",
            headers: &[("Sec-Fetch-Site", "cross-site")],
            reject: true,
        },
        Case {
            method: "PUT",
            headers: &[("Sec-Fetch-Site", "same-site")],
            reject: true,
        },
        Case {
            method: "DELETE",
            headers: &[("Origin", "https://elsewhere.example")],
            reject: true,
        },
        Case {
            method: "POST",
            headers: &[("Origin", "null")],
            reject: true,
        },
        Case {
            method: "POST",
            headers: &[("Origin", "http://db.example:8089")],
            reject: false,
        },
        Case {
            method: "POST",
            headers: &[
                ("Origin", "https://app.example"),
                ("Sec-Fetch-Site", "cross-site"),
            ],
            reject: false,
        },
        Case {
            method: "GET",
            headers: &[("Sec-Fetch-Site", "cross-site")],
            reject: false,
        },
    ];

    for tc in cases {
        let mut ctx = new_request_from("203.0.113.9", tc.headers);
        ctx.request.header.set_method(tc.method);
        ctx.request.header.set_host("db.example:8089");

        assert_eq!(
            security::cross_origin_write_is_rejected(&ctx),
            tc.reject,
            "{} {:?}: expected reject={}",
            tc.method,
            tc.headers,
            tc.reject
        );
    }
}
