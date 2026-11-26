use std::time::{Duration, Instant};

use elysiandb::configuration::Config;
use elysiandb::gocompat::json::Value;

use crate::harness::{start_admin_session, start_api_server, vn};

#[test]
fn login_is_throttled_after_repeated_failures() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-password","role":"admin"}"#,
        &[],
    );

    let client = s.from("203.0.113.10");
    for _ in 0..5 {
        client.must_do(
            401,
            "POST",
            "/api/security/login",
            r#"{"username":"ops","password":"wrong"}"#,
            &[],
        );
    }

    let resp = client.must_do(
        429,
        "POST",
        "/api/security/login",
        r#"{"username":"ops","password":"ops-password"}"#,
        &[],
    );
    if resp.header("Retry-After").is_empty() {
        panic!("expected a Retry-After header");
    }

    s.from("203.0.113.11").login("ops", "ops-password");
}

#[test]
fn default_admin_password_is_only_accepted_locally() {
    let mut s = start_api_server(None);

    s.from("203.0.113.20").must_do(
        401,
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"admin"}"#,
        &[],
    );
    s.must_do(
        401,
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"admin"}"#,
        &["X-Forwarded-For", "203.0.113.21"],
    );

    s.login("admin", "admin");
    s.must_do(
        200,
        "PUT",
        "/api/security/user/admin/password",
        r#"{"password":"a-better-password"}"#,
        &[],
    );
    s.must_do(200, "GET", "/api/security/me", "", &[]);

    s.from("203.0.113.20").login("admin", "a-better-password");
}

#[test]
fn trusted_proxy_forwards_the_client_address() {
    let configure = |cfg: &mut Config| {
        cfg.server.http.trusted_proxies = vec!["10.0.0.0/8".to_string()].into();
    };
    let s = start_api_server(Some(&configure));

    let mut proxy = s.from("10.0.0.5");
    proxy.must_do(
        401,
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"admin"}"#,
        &["X-Forwarded-For", "127.0.0.1, 203.0.113.30"],
    );
    proxy.headers.set("X-Forwarded-For", "127.0.0.1");
    proxy.login("admin", "admin");
    proxy.must_do(
        200,
        "PUT",
        "/api/security/user/admin/password",
        r#"{"password":"a-better-password"}"#,
        &[],
    );
    proxy.headers.remove("X-Forwarded-For");

    for _ in 0..5 {
        proxy.must_do(
            401,
            "POST",
            "/api/security/login",
            r#"{"username":"admin","password":"wrong"}"#,
            &["X-Forwarded-For", "203.0.113.31"],
        );
    }

    proxy.must_do(
        429,
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"a-better-password"}"#,
        &["X-Forwarded-For", "203.0.113.31"],
    );
    proxy.must_do(
        200,
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"a-better-password"}"#,
        &["X-Forwarded-For", "203.0.113.32"],
    );
}

#[test]
fn sessions_end_with_the_account() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-password","role":"admin"}"#,
        &[],
    );

    let mut ops = s.anonymous();
    ops.login("ops", "ops-password");
    ops.must_do(200, "GET", "/api/security/me", "", &[]);

    s.must_do(
        200,
        "PUT",
        "/api/security/user/ops/password",
        r#"{"password":"another-password"}"#,
        &[],
    );
    ops.must_do(401, "GET", "/api/security/me", "", &[]);

    ops.login("ops", "another-password");
    s.must_do(
        200,
        "PUT",
        "/api/security/user/ops/role",
        r#"{"role":"user"}"#,
        &[],
    );
    ops.must_do(401, "GET", "/api/security/user", "", &[]);

    s.must_do(
        200,
        "PUT",
        "/api/security/user/ops/role",
        r#"{"role":"admin"}"#,
        &[],
    );
    ops.login("ops", "another-password");
    s.must_do(200, "DELETE", "/api/security/user/ops", "", &[]);
    ops.must_do(
        401,
        "POST",
        "/api/security/user",
        r#"{"username":"ghost","password":"ghost-password","role":"admin"}"#,
        &[],
    );

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-password","role":"admin"}"#,
        &[],
    );
    ops.must_do(401, "GET", "/api/security/me", "", &[]);
}

#[test]
fn account_changes_are_validated() {
    let s = start_admin_session(None);

    s.must_do(
        400,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"short","role":"admin"}"#,
        &[],
    );
    s.must_do(
        400,
        "POST",
        "/api/security/user",
        r#"{"username":"","password":"ops-password","role":"admin"}"#,
        &[],
    );
    s.must_do(
        400,
        "POST",
        "/api/security/user",
        r#"{"username":"a/b","password":"ops-password","role":"admin"}"#,
        &[],
    );
    s.must_do(
        400,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-password","role":"root"}"#,
        &[],
    );

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-password","role":"user"}"#,
        &[],
    );
    s.must_do(
        409,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"other-password","role":"admin"}"#,
        &[],
    );

    s.must_do(400, "PUT", "/api/security/user/ops/password", "{}", &[]);
    s.must_do(
        400,
        "PUT",
        "/api/security/user/ops/password",
        r#"{"password":"short"}"#,
        &[],
    );
    s.must_do(
        200,
        "PUT",
        "/api/security/user/ops/password",
        r#"{"password":"ops-password-2"}"#,
        &[],
    );

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"o\"q","password":"quote-password","role":"user"}"#,
        &[],
    );
    let users = match s
        .must_do(200, "GET", "/api/security/user", "", &[])
        .object()
        .get("users")
    {
        Some(Value::Array(list)) => list.clone(),
        other => panic!("users is not a list: {other:?}"),
    };
    if users.len() != 3 {
        panic!("expected 3 users, got {users:?}");
    }

    let user = s
        .must_do(200, "GET", "/api/security/user/ops", "", &[])
        .object();
    if user.get("username") != Some(&Value::String("ops".to_string()))
        || user.get("role") != Some(&Value::String("user".to_string()))
        || user.len() != 2
    {
        panic!("unexpected user {user:?}");
    }
}

#[test]
fn cross_origin_writes_are_rejected() {
    let s = start_admin_session(None);

    s.must_do(
        403,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &[
            "Origin",
            "https://elsewhere.example",
            "Content-Type",
            "text/plain",
        ],
    );
    s.must_do(
        403,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &["Sec-Fetch-Site", "cross-site"],
    );
    s.must_do(
        403,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &["Sec-Fetch-Site", "same-site"],
    );
    s.must_do(
        403,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &["Origin", "null"],
    );

    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &["Sec-Fetch-Site", "same-origin", "Origin", "http://test"],
    );
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &["Origin", "http://test"],
    );
    s.must_do(200, "POST", "/api/book", r#"{"title":"x"}"#, &[]);
    s.must_do(
        200,
        "GET",
        "/api/book",
        "",
        &["Sec-Fetch-Site", "cross-site"],
    );

    let configure = |cfg: &mut Config| {
        cfg.server.http.cors.allowed_origins = vec!["https://app.example".to_string()].into();
    };
    let allowed = start_admin_session(Some(&configure));
    allowed.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"title":"x"}"#,
        &[
            "Origin",
            "https://app.example",
            "Sec-Fetch-Site",
            "cross-site",
        ],
    );
}

#[test]
fn responses_carry_security_headers() {
    let configure = |cfg: &mut Config| cfg.admin_ui.enabled = true;
    let s = start_admin_session(Some(&configure));

    let api = s.must_do(200, "GET", "/api/book", "", &[]);
    let expected = [
        ("Cache-Control", "no-store"),
        (
            "Content-Security-Policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("Referrer-Policy", "no-referrer"),
        ("X-Content-Type-Options", "nosniff"),
        ("X-Frame-Options", "DENY"),
    ];
    for (name, value) in expected {
        let got = api.header(name);
        if got != value {
            panic!("{name}: expected {value:?}, got {got:?}");
        }
    }

    let ui = s.do_("GET", "/admin/", "", &[]);
    let csp = ui.header("Content-Security-Policy");
    if !csp.contains("script-src 'self'") || !csp.contains("frame-ancestors 'none'") {
        panic!("unexpected Admin UI policy {csp:?}");
    }
}

#[test]
fn import_keeps_internal_entities_unchanged() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/book/create",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );

    s.must_do(403, "POST", "/api/import", r#"{"book":[{"id":"b1","title":"x","owner":{"@entity":"_elysiandb_core_user","id":"mallory","username":"mallory","password":"x","role":"admin"}}]}"#, &[]);
    s.must_do(
        200,
        "POST",
        "/api/import",
        r#"{"_elysiandb_core_schema":[],"book":[{"id":"b1","title":"x"}]}"#,
        &[],
    );

    s.must_do(400, "POST", "/api/book", r#"{"id":"b2"}"#, &[]);

    let users = match s
        .must_do(200, "GET", "/api/security/user", "", &[])
        .object()
        .get("users")
    {
        Some(Value::Array(list)) => list.clone(),
        other => panic!("users is not a list: {other:?}"),
    };
    if users.len() != 1 {
        panic!("expected the accounts to stay unchanged, got {users:?}");
    }
}

#[test]
fn hooks_work_on_a_copy_of_the_document() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/note",
        r#"{"id":"n1","meta":{"n":1}}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/hook/note", r#"{"name":"inc","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.meta.n = ctx.entity.meta.n + 1; ctx.entity.accounts = ctx.query(\"_elysiandb_core_user\", {}).length }"}"#, &[]);

    for _ in 0..3 {
        let note = s.must_do(200, "GET", "/api/note/n1", "", &[]).object();
        let n = match note.get("meta") {
            Some(Value::Object(meta)) => meta.get("n").cloned(),
            _ => None,
        };
        if n != Some(vn(2.0)) || note.get("accounts") != Some(&vn(0.0)) {
            panic!("unexpected hook result {note:?}");
        }
    }
}

#[test]
fn endless_hook_script_is_stopped() {
    let s = start_admin_session(None);
    s.must_do(200, "POST", "/api/loop", r#"{"id":"l1"}"#, &[]);
    s.must_do(200, "POST", "/api/hook/loop", r#"{"name":"spin","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { while (true) {} }"}"#, &[]);

    let started = Instant::now();
    s.must_do(200, "GET", "/api/loop/l1", "", &[]);
    let elapsed = started.elapsed();
    if elapsed > Duration::from_secs(10) {
        panic!("expected the script to be stopped, took {elapsed:?}");
    }
}

#[test]
fn unusual_requests_keep_the_server_running() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/cyc",
        r#"{"id":"x","self":{"@entity":"cyc","id":"x"},"items":[{"@entity":"cyc","id":"x"}]}"#,
        &[],
    );

    s.must_do(200, "GET", "/api/cyc/x?includes=all", "", &[]);
    s.must_do(200, "GET", "/api/cyc?includes=all", "", &[]);
    s.must_do(200, "GET", "/api/cyc/x?includes=self.self.self", "", &[]);

    s.must_do(
        200,
        "POST",
        "/api/query",
        r#"{"entity":"cyc","offset":-1}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/query",
        r#"{"entity":"cyc","offset":1,"limit":9223372036854775807}"#,
        &[],
    );

    s.must_do(400, "POST", "/api/cyc", "[null]", &[]);
    s.must_do(400, "POST", "/api/cyc", r#"[{"id":"y"},null]"#, &[]);

    s.must_do(200, "GET", "/api/cyc/x", "", &[]);
}
