use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use elysiandb::httpd::{Handler, MATCHED_ROUTE_PATH_PARAM, RequestCtx, handler};
use elysiandb::httprouter::{Router, clean_path, get_optional_paths, in_recovery_point};

use super::tree_test::catch_panic;

const HTTP_METHODS: [&str; 11] = [
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "CONNECT", "OPTIONS", "TRACE", "*", "CUSTOM",
];

fn noop() -> Handler {
    handler(|_ctx: &mut RequestCtx| {})
}

fn flag() -> (Arc<AtomicBool>, Handler) {
    let f = Arc::new(AtomicBool::new(false));
    let g = f.clone();
    (
        f,
        handler(move |_ctx: &mut RequestCtx| g.store(true, Ordering::SeqCst)),
    )
}

fn request(router: &Router, ctx: &mut RequestCtx, method: &str, path: &str) {
    ctx.request.header.set_method(method);
    ctx.request.set_request_uri(path);
    router.handler(ctx);
}

#[test]
fn test_router() {
    let mut router = Router::new();
    let routed = Arc::new(AtomicBool::new(false));
    let r = routed.clone();
    router.handle(
        "GET",
        "/user/{name}",
        handler(move |ctx: &mut RequestCtx| {
            assert_eq!(ctx.param("name"), b"gopher");
            r.store(true, Ordering::SeqCst);
        }),
    );
    let mut ctx = RequestCtx::new();
    ctx.request.set_request_uri("/user/gopher");
    router.handler(&mut ctx);
    assert!(routed.load(Ordering::SeqCst));
}

#[test]
fn test_router_api() {
    let mut router = Router::new();
    let (get, h) = flag();
    router.get("/GET", h);
    let (head, h) = flag();
    router.head("/HEAD", h);
    let (post, h) = flag();
    router.post("/POST", h);
    let (put, h) = flag();
    router.put("/PUT", h);
    let (patch, h) = flag();
    router.patch("/PATCH", h);
    let (delete, h) = flag();
    router.delete("/DELETE", h);
    let (connect, h) = flag();
    router.connect("/CONNECT", h);
    let (options, h) = flag();
    router.options("/OPTIONS", h);
    let (trace, h) = flag();
    router.trace("/TRACE", h);
    let (any, h) = flag();
    router.any("/ANY", h);
    let (handled, h) = flag();
    router.handle("GET", "/Handler", h);

    let mut ctx = RequestCtx::new();
    for (method, path, f) in [
        ("GET", "/GET", &get),
        ("HEAD", "/HEAD", &head),
        ("POST", "/POST", &post),
        ("PUT", "/PUT", &put),
        ("PATCH", "/PATCH", &patch),
        ("DELETE", "/DELETE", &delete),
        ("CONNECT", "/CONNECT", &connect),
        ("OPTIONS", "/OPTIONS", &options),
        ("TRACE", "/TRACE", &trace),
        ("GET", "/Handler", &handled),
    ] {
        request(&router, &mut ctx, method, path);
        assert!(f.load(Ordering::SeqCst), "routing {method} {path} failed");
    }
    for method in HTTP_METHODS {
        request(&router, &mut ctx, method, "/ANY");
        assert!(
            any.swap(false, Ordering::SeqCst),
            "routing ANY failed - method {method}"
        );
    }
}

#[test]
fn test_router_invalid_input() {
    let mut router = Router::new();
    assert!(catch_panic(|| router.handle("", "/", noop())).is_some());
    assert!(catch_panic(|| router.get("", noop())).is_some());
    assert!(catch_panic(|| router.get("noSlashRoot", noop())).is_some());
}

#[test]
fn test_validate_path() {
    let mut router = Router::new();
    assert!(catch_panic(|| router.get("", noop())).is_some());
    assert!(catch_panic(|| router.get("foo", noop())).is_some());
    assert!(catch_panic(|| router.get("/foo", noop())).is_none());
}

#[test]
fn test_router_chaining() {
    let mut router2 = Router::new();
    let (bar, h) = flag();
    router2.post(
        "/bar",
        handler(move |ctx: &mut RequestCtx| {
            h(ctx);
            ctx.set_status_code(200);
        }),
    );
    let mut router1 = Router::new();
    router1.not_found = Some(router2.into_handler());
    let (foo, h) = flag();
    router1.post(
        "/foo",
        handler(move |ctx: &mut RequestCtx| {
            h(ctx);
            ctx.set_status_code(200);
        }),
    );
    let mut ctx = RequestCtx::new();
    request(&router1, &mut ctx, "POST", "/foo");
    assert!(ctx.response.status_code() == 200 && foo.load(Ordering::SeqCst));
    request(&router1, &mut ctx, "POST", "/bar");
    assert!(ctx.response.status_code() == 200 && bar.load(Ordering::SeqCst));
    request(&router1, &mut ctx, "POST", "/qax");
    assert_eq!(ctx.response.status_code(), 404);
}

#[test]
fn test_router_options() {
    let mut router = Router::new();
    router.post("/path", noop());
    let mut ctx = RequestCtx::new();
    let check = |router: &Router, ctx: &mut RequestCtx, path: &str, allowed: &str, code: u16| {
        request(router, ctx, "OPTIONS", path);
        assert_eq!(ctx.response.status_code(), code, "{path}");
        assert_eq!(
            String::from_utf8_lossy(ctx.response.header.peek("Allow")),
            allowed,
            "{path}"
        );
    };
    check(&router, &mut ctx, "*", "OPTIONS, POST", 200);
    check(&router, &mut ctx, "/path", "OPTIONS, POST", 200);
    request(&router, &mut ctx, "OPTIONS", "/doesnotexist");
    assert_eq!(ctx.response.status_code(), 404);

    router.get("/path", noop());
    router.global_options = Some(handler(|ctx: &mut RequestCtx| ctx.set_status_code(204)));
    let mut ctx = RequestCtx::new();
    check(&router, &mut ctx, "*", "GET, OPTIONS, POST", 204);
    check(&router, &mut ctx, "/path", "GET, OPTIONS, POST", 204);

    let (custom, h) = flag();
    router.options("/path", h);
    let mut ctx = RequestCtx::new();
    check(&router, &mut ctx, "*", "GET, OPTIONS, POST", 204);
    assert!(!custom.load(Ordering::SeqCst));
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "OPTIONS", "/path");
    assert!(custom.load(Ordering::SeqCst));
}

#[test]
fn test_router_not_allowed() {
    let mut router = Router::new();
    router.post("/path", noop());
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "GET", "/path");
    assert_eq!(ctx.response.status_code(), 405);
    assert_eq!(ctx.response.header.peek("Allow"), b"OPTIONS, POST");
    assert_eq!(ctx.response.body(), b"Method Not Allowed");

    router.delete("/path", noop());
    router.options("/path", noop());
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "GET", "/path");
    assert_eq!(ctx.response.status_code(), 405);
    assert_eq!(ctx.response.header.peek("Allow"), b"DELETE, OPTIONS, POST");

    router.method_not_allowed = Some(handler(|ctx: &mut RequestCtx| {
        ctx.set_status_code(418);
        ctx.write_string("custom method");
    }));
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "GET", "/path");
    assert_eq!(ctx.response.body(), b"custom method");
    assert_eq!(ctx.response.status_code(), 418);
    assert_eq!(ctx.response.header.peek("Allow"), b"DELETE, OPTIONS, POST");
}

fn not_found_by_method(method: &str) {
    let host = "fast";
    let mut router = Router::new();
    for p in [
        "/path",
        "/dir/",
        "/",
        "/{proc}/StaTus",
        "/USERS/{name}/enTRies/",
        "/static/{filepath:*}",
    ] {
        router.handle(method, p, noop());
    }
    let req_method = if method == "*" { "PATCH" } else { method };
    let expected = match req_method {
        "CONNECT" => 404,
        "GET" => 301,
        _ => 308,
    };
    let mut routes: Vec<(&str, u16, String)> = vec![
        ("", 200, String::new()),
        ("/../path", expected, format!("http://{host}/path")),
        ("/nope", 404, String::new()),
    ];
    if method != "CONNECT" {
        for (route, loc) in [
            ("/path/", "/path"),
            ("/dir", "/dir/"),
            ("/PATH", "/path"),
            ("/DIR/", "/dir/"),
            ("/PATH/", "/path"),
            ("/DIR", "/dir/"),
            ("/paTh/?name=foo", "/path?name=foo"),
            ("/paTh?name=foo", "/path?name=foo"),
            ("/sergio/status/", "/sergio/StaTus"),
            ("/users/atreugo/eNtriEs", "/USERS/atreugo/enTRies/"),
            ("/STatiC/test.go", "/static/test.go"),
        ] {
            routes.push((route, expected, format!("http://{host}{loc}")));
        }
    }
    for (route, code, location) in routes {
        let mut ctx = RequestCtx::new();
        ctx.request.header.set_method(req_method);
        ctx.request.set_request_uri(route);
        ctx.request.set_host(host);
        router.handler(&mut ctx);
        let status = ctx.response.status_code();
        let loc = String::from_utf8_lossy(ctx.response.header.peek("Location")).into_owned();
        assert!(
            status == code && (status == 404 || loc == location),
            "route '{route}' method {method}/{req_method}: got {status} {loc}, want {code} {location}"
        );
    }
    let (nf, h) = flag();
    router.not_found = Some(handler(move |ctx: &mut RequestCtx| {
        ctx.set_status_code(404);
        h(ctx);
    }));
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, req_method, "/nope");
    assert!(ctx.response.status_code() == 404 && nf.load(Ordering::SeqCst));
}

#[test]
fn test_router_not_found() {
    for method in HTTP_METHODS {
        not_found_by_method(method);
    }
    let mut router = Router::new();
    router.patch("/path", noop());
    let mut ctx = RequestCtx::new();
    ctx.request.header.set_method("PATCH");
    ctx.request.set_request_uri("/path/?key=val");
    ctx.request.set_host("fast");
    router.handler(&mut ctx);
    assert_eq!(ctx.response.status_code(), 308);
    assert_eq!(
        ctx.response.header.peek("Location"),
        b"http://fast/path?key=val"
    );

    let mut router = Router::new();
    router.get("/a", noop());
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "PATCH", "/");
    assert_eq!(ctx.response.status_code(), 404);
    assert_eq!(ctx.response.body(), b"Not Found");
    assert_eq!(
        ctx.response.header.content_type(),
        b"text/plain; charset=utf-8"
    );
}

#[test]
fn test_router_not_found_method_wild() {
    let mut router = Router::new();
    let (any, h) = flag();
    router.any("/{path:*}", h);
    let (post, h) = flag();
    router.post("/specific", h);
    for i in 0..100u64 {
        let m = [
            "GET", "HEAD", "PUT", "PATCH", "DELETE", "CONNECT", "OPTIONS", "TRACE", "CUSTOM",
        ][(i % 9) as usize];
        router.handle(m, &format!("/{}", i * 7919 + 13), noop());
    }
    for method in HTTP_METHODS {
        let mut ctx = RequestCtx::new();
        request(&router, &mut ctx, method, "/specific");
        if method == "POST" {
            assert!(post.swap(false, Ordering::SeqCst));
        } else {
            assert!(any.swap(false, Ordering::SeqCst), "{method}");
        }
        assert_eq!(ctx.response.status_code(), 200);
    }
}

#[test]
fn test_router_panic_handler() {
    let mut router = Router::new();
    let handled = Arc::new(AtomicBool::new(false));
    let saw_recovery = Arc::new(AtomicBool::new(false));
    let (h2, r2) = (handled.clone(), saw_recovery.clone());
    router.panic_handler = Some(Arc::new(move |ctx: &mut RequestCtx, payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert_eq!(msg, "oops!");
        assert!(!in_recovery_point());
        ctx.set_status_code(500);
        h2.store(true, Ordering::SeqCst);
    }));
    router.handle(
        "PUT",
        "/user/{name}",
        handler(move |_ctx: &mut RequestCtx| {
            r2.store(in_recovery_point(), Ordering::SeqCst);
            std::panic::panic_any("oops!");
        }),
    );
    let mut ctx = RequestCtx::new();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    request(&router, &mut ctx, "PUT", "/user/gopher");
    std::panic::set_hook(prev);
    assert!(handled.load(Ordering::SeqCst));
    assert!(saw_recovery.load(Ordering::SeqCst));
    assert!(!in_recovery_point());
    assert_eq!(ctx.response.status_code(), 500);
}

fn lookup_by_method(method: &str) {
    let req_method = if method == "*" { "GET" } else { method };
    let mut router = Router::new();
    let mut ctx = RequestCtx::new();
    let (h, tsr) = router.lookup(req_method, b"/nope", Some(&mut ctx));
    assert!(h.is_none() && !tsr);

    let (routed, want) = flag();
    router.handle(method, "/user/{name}", want.clone());
    let (h, _) = router.lookup(req_method, b"/user/gopher", Some(&mut ctx));
    assert!(h.is_some_and(|h| Arc::ptr_eq(&h, &want)));
    assert_eq!(ctx.param("name"), b"gopher");

    router.handle(method, "/user", want.clone());
    let (h, _) = router.lookup(req_method, b"/user", Some(&mut ctx));
    let mut tmp = RequestCtx::new();
    if let Some(h) = h {
        h(&mut tmp);
    }
    assert!(routed.load(Ordering::SeqCst));
    assert_eq!(ctx.param("name"), b"gopher");

    let (h, tsr) = router.lookup(req_method, b"/user/gopher/", Some(&mut ctx));
    assert!(h.is_none() && tsr);
    let (h, tsr) = router.lookup(req_method, b"/nope", Some(&mut ctx));
    assert!(h.is_none() && !tsr);
}

#[test]
fn test_router_lookup() {
    for method in HTTP_METHODS {
        lookup_by_method(method);
    }
}

#[test]
fn test_router_matched_route_path() {
    let mut router = Router::new();
    router.save_matched_route_path = true;
    let routes = ["/user/{name}", "/user/{name}/details", "/"];
    for route in routes {
        router.handle(
            "GET",
            route,
            handler(move |ctx: &mut RequestCtx| {
                assert_eq!(ctx.matched_route_path(), Some(route));
                ctx.set_user_value("hit", route.to_string());
            }),
        );
    }
    for (path, route) in [
        ("/user/gopher", "/user/{name}"),
        ("/user/gopher/details", "/user/{name}/details"),
        ("/", "/"),
    ] {
        let mut ctx = RequestCtx::new();
        request(&router, &mut ctx, "GET", path);
        assert_eq!(
            ctx.user_value::<String>("hit").map(String::as_str),
            Some(route)
        );
        assert!(ctx.has_user_value(MATCHED_ROUTE_PATH_PARAM));
    }
    let mut ctx = RequestCtx::new();
    request(&router, &mut ctx, "GET", "/nope/nope");
    assert_eq!(ctx.matched_route_path(), None);
}

#[test]
fn test_router_list() {
    let mut r = Router::new();
    r.get("/bar", noop());
    r.patch("/foo", noop());
    r.post("/v1/users/{name}/{surname?}", noop());
    r.delete("/v1/users/{id?}", noop());
    let list = r.list();
    let flat: Vec<(String, Vec<String>)> =
        list.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(
        flat,
        vec![
            ("DELETE".to_string(), vec!["/v1/users/{id?}".to_string()]),
            ("GET".to_string(), vec!["/bar".to_string()]),
            ("PATCH".to_string(), vec!["/foo".to_string()]),
            (
                "POST".to_string(),
                vec!["/v1/users/{name}/{surname?}".to_string()]
            ),
        ]
    );
}

#[test]
fn test_router_same_prefix_param_route() {
    let mut r = Router::new();
    r.get(
        "/v1/foo/{id}/{pageSize}/{page}",
        handler(|ctx: &mut RequestCtx| {
            let v = format!(
                "1:{}:{}:{}",
                String::from_utf8_lossy(ctx.param("id")),
                String::from_utf8_lossy(ctx.param("pageSize")),
                String::from_utf8_lossy(ctx.param("page"))
            );
            ctx.set_body_string(v);
        }),
    );
    r.get(
        "/v1/foo/{id}/{iid}",
        handler(|ctx: &mut RequestCtx| {
            let v = format!(
                "2:{}:{}",
                String::from_utf8_lossy(ctx.param("id")),
                String::from_utf8_lossy(ctx.param("iid"))
            );
            ctx.set_body_string(v);
        }),
    );
    r.get(
        "/v1/foo/{id}",
        handler(|ctx: &mut RequestCtx| {
            let v = format!("3:{}", String::from_utf8_lossy(ctx.param("id")));
            ctx.set_body_string(v);
        }),
    );
    for (path, want) in [
        ("/v1/foo/1/20/4", "1:1:20:4"),
        ("/v1/foo/2/3", "2:2:3"),
        ("/v1/foo/v3", "3:v3"),
    ] {
        let mut ctx = RequestCtx::new();
        ctx.request.set_request_uri(path);
        r.handler(&mut ctx);
        assert_eq!(ctx.response.body(), want.as_bytes(), "{path}");
    }
}

#[test]
fn test_clean_path() {
    for (input, want) in [
        ("/abc/.", "/abc/"),
        ("/abc", "/abc"),
        (".", ""),
        ("/a/b.", "/a/b"),
    ] {
        assert_eq!(clean_path(input.as_bytes()), want.as_bytes(), "{input}");
    }
}

#[test]
fn test_get_optional_path() {
    let cases: [(&str, &[&str]); 9] = [
        ("/hello", &[]),
        ("/{name}", &[]),
        ("/{name?:[a-zA-Z]{5}}", &["/", "/{name:[a-zA-Z]{5}}"]),
        ("/{filepath:^(?!api).*}", &[]),
        (
            "/static/{filepath?:^(?!api).*}",
            &["/static", "/static/{filepath:^(?!api).*}"],
        ),
        ("/show/{name?}", &["/show", "/show/{name}"]),
        ("/a/{b?}/{c?}", &["/a", "/a/{b}", "/a/{b}/{c}"]),
        (
            "/show/{name}/{surname?}/at/{address?}/{id}",
            &[
                "/show/{name}",
                "/show/{name}/{surname}",
                "/show/{name}/{surname}/at",
                "/show/{name}/{surname}/at/{address}",
            ],
        ),
        (
            "/show/{name}/{surname?}/at/{address?}/{id}/{phone?:.*}",
            &[
                "/show/{name}",
                "/show/{name}/{surname}",
                "/show/{name}/{surname}/at",
                "/show/{name}/{surname}/at/{address}",
                "/show/{name}/{surname}/at/{address}/{id}",
                "/show/{name}/{surname}/at/{address}/{id}/{phone:.*}",
            ],
        ),
    ];
    for (path, want) in cases {
        let got = get_optional_paths(path);
        let want: Vec<String> = want.iter().map(|w| w.to_string()).collect();
        assert_eq!(got, want, "{path}");
    }

    let mut r = Router::new();
    let h = noop();
    r.get("/show/{name}/{surname?}/at/{address?}/{id}", h.clone());
    let expected: [(&str, bool, bool); 10] = [
        ("/show/{name}", false, true),
        ("/show/{name}/", true, false),
        ("/show/{name}/{surname}", false, true),
        ("/show/{name}/{surname}/", true, false),
        ("/show/{name}/{surname}/at", false, true),
        ("/show/{name}/{surname}/at/", true, false),
        ("/show/{name}/{surname}/at/{address}", false, true),
        ("/show/{name}/{surname}/at/{address}/", true, false),
        ("/show/{name}/{surname}/at/{address}/{id}", false, false),
        ("/show/{name}/{surname}/at/{address}/{id}/", false, false),
    ];
    for (path, tsr_want, has_handler) in expected {
        let mut ctx = RequestCtx::new();
        let (got, tsr) = r.lookup("GET", path.as_bytes(), Some(&mut ctx));
        assert_eq!(tsr, tsr_want, "{path}");
        assert_eq!(got.is_some(), has_handler, "{path}");
    }
}
