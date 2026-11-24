use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use elysiandb::httpd::{Handler, Request, RequestCtx};
use elysiandb::httprouter::Tree;

const ROUTE_KEY: &str = "__test_route__";

fn fake_handler(route: &str) -> Handler {
    let route = route.to_string();
    Arc::new(move |ctx: &mut RequestCtx| ctx.set_user_value(ROUTE_KEY, route.clone()))
}

pub fn catch_panic(f: impl FnOnce()) -> Option<String> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(prev);
    match res {
        Ok(()) => None,
        Err(p) => Some(
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
        ),
    }
}

fn ctx_for(path: &str) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(path);
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn params(ctx: &RequestCtx) -> BTreeMap<String, String> {
    ctx.user_value_keys()
        .into_iter()
        .filter(|k| *k != ROUTE_KEY)
        .map(|k| {
            (
                k.to_string(),
                String::from_utf8_lossy(ctx.param(k)).into_owned(),
            )
        })
        .collect()
}

type TreeCase = (
    &'static str,
    &'static str,
    bool,
    &'static [(&'static str, &'static str)],
);

struct Req {
    path: &'static str,
    nil_handler: bool,
    route: &'static str,
    ps: &'static [(&'static str, &'static str)],
}

fn check_requests(tree: &Tree, requests: &[Req]) {
    for request in requests {
        let mut ctx = ctx_for(request.path);
        let (handler, _) = tree.get(request.path.as_bytes(), Some(&mut ctx));
        match handler {
            None => assert!(
                request.nil_handler,
                "handle mismatch for route '{}': expected non-nil handle",
                request.path
            ),
            Some(h) => {
                assert!(
                    !request.nil_handler,
                    "handle mismatch for route '{}': expected nil handle",
                    request.path
                );
                h(&mut ctx);
                let got = ctx
                    .user_value::<String>(ROUTE_KEY)
                    .cloned()
                    .unwrap_or_default();
                assert_eq!(
                    got, request.route,
                    "wrong handle for route '{}'",
                    request.path
                );
            }
        }
        let want: BTreeMap<String, String> = request
            .ps
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(params(&ctx), want, "route {} user values", request.path);
    }
}

fn r(
    path: &'static str,
    nil_handler: bool,
    route: &'static str,
    ps: &'static [(&'static str, &'static str)],
) -> Req {
    Req {
        path,
        nil_handler,
        route,
        ps,
    }
}

fn test_routes(routes: &[(&str, bool)]) {
    let mut tree = Tree::new();
    for (route, conflict) in routes {
        let recv = catch_panic(|| tree.add(route, fake_handler(route)));
        if *conflict {
            assert!(recv.is_some(), "no panic for conflicting route '{route}'");
        } else {
            assert!(
                recv.is_none(),
                "unexpected panic for route '{route}': {recv:?}"
            );
        }
    }
}

#[test]
fn test_tree_add_and_get() {
    let mut tree = Tree::new();
    let routes = [
        "/hi",
        "/contact/",
        "/co",
        "/c",
        "/a",
        "/ab",
        "/doc/",
        "/doc/go_faq.html",
        "/doc/go1.html",
        "/α",
        "/β",
        "/hello/test",
        "/hello/tooth",
        "/hello/{name}",
        "/regex/{path:*}",
    ];
    for route in routes {
        tree.add(route, fake_handler(route));
    }
    check_requests(
        &tree,
        &[
            r("/a", false, "/a", &[]),
            r("/", true, "", &[]),
            r("/hi", false, "/hi", &[]),
            r("/contact", true, "", &[]),
            r("/co", false, "/co", &[]),
            r("/con", true, "", &[]),
            r("/cona", true, "", &[]),
            r("/no", true, "", &[]),
            r("/ab", false, "/ab", &[]),
            r("/α", false, "/α", &[]),
            r("/β", false, "/β", &[]),
            r("/hello/test", false, "/hello/test", &[]),
            r("/hello/tooth", false, "/hello/tooth", &[]),
            r(
                "/hello/testastretta",
                false,
                "/hello/{name}",
                &[("name", "testastretta")],
            ),
            r("/hello/tes", false, "/hello/{name}", &[("name", "tes")]),
            r("/hello/test/bye", true, "", &[]),
            r(
                "/regex/more_alt/hello",
                false,
                "/regex/{path:*}",
                &[("path", "more_alt/hello")],
            ),
        ],
    );
}

#[test]
fn test_tree_wildcard() {
    let mut tree = Tree::new();
    let routes = [
        "/",
        "/cmd/{tool}/{sub}",
        "/cmd/{tool}/",
        "/src/{filepath:*}",
        "/src/data",
        "/search/",
        "/search/{query}",
        "/user_{name}",
        "/user_{name}/about",
        "/files/{dir}/{filepath:*}",
        "/doc/",
        "/doc/go_faq.html",
        "/doc/go1.html",
        "/info/{user}/public",
        "/info/{user}/project/{project}",
    ];
    for route in routes {
        tree.add(route, fake_handler(route));
    }
    check_requests(
        &tree,
        &[
            r("/", false, "/", &[]),
            r("/cmd/test/", false, "/cmd/{tool}/", &[("tool", "test")]),
            r("/cmd/test", true, "", &[]),
            r(
                "/cmd/test/3",
                false,
                "/cmd/{tool}/{sub}",
                &[("tool", "test"), ("sub", "3")],
            ),
            r("/src/", false, "/src/{filepath:*}", &[("filepath", "")]),
            r(
                "/src/some/file.png",
                false,
                "/src/{filepath:*}",
                &[("filepath", "some/file.png")],
            ),
            r("/search/", false, "/search/", &[]),
            r(
                "/search/someth!ng+in+ünìcodé",
                false,
                "/search/{query}",
                &[("query", "someth!ng+in+ünìcodé")],
            ),
            r("/search/someth!ng+in+ünìcodé/", true, "", &[]),
            r("/user_gopher", false, "/user_{name}", &[("name", "gopher")]),
            r(
                "/user_gopher/about",
                false,
                "/user_{name}/about",
                &[("name", "gopher")],
            ),
            r(
                "/files/js/inc/framework.js",
                false,
                "/files/{dir}/{filepath:*}",
                &[("dir", "js"), ("filepath", "inc/framework.js")],
            ),
            r(
                "/info/gordon/public",
                false,
                "/info/{user}/public",
                &[("user", "gordon")],
            ),
            r(
                "/info/gordon/project/go",
                false,
                "/info/{user}/project/{project}",
                &[("user", "gordon"), ("project", "go")],
            ),
            r("/info/gordon", true, "", &[]),
        ],
    );
}

#[test]
fn test_tree_wildcard_conflict() {
    test_routes(&[
        ("/cmd/{tool}/{sub}", false),
        ("/cmd/vet", false),
        ("/src/{filepath:*}", false),
        ("/src/", false),
        ("/src/{filepathx:*}", true),
        ("/src1/", false),
        ("/src1/{filepath:*}", false),
        ("/src2{filepath:*}", true),
        ("/search/{query}", false),
        ("/search/invalid", false),
        ("/user_{name}", false),
        ("/user_x", false),
        ("/user_{name}", true),
        ("/id{id}", false),
        ("/id/{id}", false),
    ]);
}

#[test]
fn test_tree_child_conflict() {
    test_routes(&[
        ("/cmd/vet", false),
        ("/cmd/{tool}/{sub}", false),
        ("/src/AUTHORS", false),
        ("/src/{filepath:*}", false),
        ("/user_x", false),
        ("/user_{name}", false),
        ("/id/{id}", false),
        ("/id{id}", false),
        ("/{users}", false),
        ("/{id}/", true),
        ("/{filepath:*}", false),
        ("/asd{filepath:*}", true),
        ("/abc", false),
        ("/abd", false),
        ("/abcc", false),
    ]);
}

#[test]
fn test_tree_duplicate_path() {
    let mut tree = Tree::new();
    for route in [
        "/",
        "/doc/",
        "/src/{filepath:*}",
        "/search/{query}",
        "/user_{name}",
    ] {
        let h = fake_handler(route);
        assert!(
            catch_panic(|| tree.add(route, h.clone())).is_none(),
            "panic inserting route '{route}'"
        );
        assert!(
            catch_panic(|| tree.add(route, h.clone())).is_some(),
            "no panic for duplicate route '{route}'"
        );
    }
    check_requests(
        &tree,
        &[
            r("/", false, "/", &[]),
            r("/doc/", false, "/doc/", &[]),
            r(
                "/src/some/file.png",
                false,
                "/src/{filepath:*}",
                &[("filepath", "some/file.png")],
            ),
            r(
                "/search/someth!ng+in+ünìcodé",
                false,
                "/search/{query}",
                &[("query", "someth!ng+in+ünìcodé")],
            ),
            r("/user_gopher", false, "/user_{name}", &[("name", "gopher")]),
        ],
    );
}

#[test]
fn test_empty_wildcard_name() {
    for route in ["/user{}", "/user{}/", "/cmd/{}/", "/src/{:*}"] {
        let mut tree = Tree::new();
        assert!(
            catch_panic(|| tree.add(route, fake_handler(route))).is_some(),
            "{route}"
        );
    }
}

#[test]
fn test_tree_catch_all_conflict() {
    test_routes(&[
        ("/src/{filepath:*}/x", true),
        ("/src2/", false),
        ("/src2/{filepath:*}/x", true),
        ("/src3/{filepath:*}", false),
        ("/src3/{filepath:*}/x", true),
    ]);
}

#[test]
fn test_tree_catch_all_conflict_root() {
    test_routes(&[("/", false), ("/{filepath:*}", false)]);
}

#[test]
fn test_tree_double_wildcard() {
    for route in ["/{foo}{bar}", "/{foo}{bar}/", "/{foo}{bar:*}"] {
        let mut tree = Tree::new();
        let recv = catch_panic(|| tree.add(route, fake_handler(route))).unwrap_or_default();
        assert!(
            recv.starts_with("the wildcards must be separated by at least 1 char"),
            "{route}: {recv}"
        );
    }
}

#[test]
fn test_tree_trailing_slash_redirect() {
    let mut tree = Tree::new();
    let routes = [
        "/hi",
        "/b/",
        "/search/{query}",
        "/cmd/{tool}/",
        "/src/{filepath:*}",
        "/x",
        "/x/y",
        "/y/",
        "/y/z",
        "/0/{id}",
        "/0/{id}/1",
        "/1/{id}/",
        "/1/{id}/2",
        "/aa",
        "/a/",
        "/admin",
        "/admin/{category}",
        "/admin/{category}/{page}",
        "/doc",
        "/doc/go_faq.html",
        "/doc/go1.html",
        "/no/a",
        "/no/b",
        "/api/hello/{name}",
        "/foo/data/hello",
        "/foo/",
    ];
    for route in routes {
        assert!(
            catch_panic(|| tree.add(route, fake_handler(route))).is_none(),
            "{route}"
        );
    }
    for route in [
        "/hi/",
        "/b",
        "/search/gopher/",
        "/cmd/vet",
        "/src",
        "/x/",
        "/y",
        "/0/go/",
        "/1/go",
        "/a",
        "/admin/",
        "/admin/config/",
        "/admin/config/permissions/",
        "/doc/",
        "/foo/data/hello/",
        "/foo",
    ] {
        let (h, tsr) = tree.get(route.as_bytes(), None);
        assert!(h.is_none(), "non-nil handler for TSR route '{route}'");
        assert!(tsr, "expected TSR recommendation for route '{route}'");
    }
    for route in ["/", "/no", "/no/", "/_", "/_/", "/api/world/abc"] {
        let (h, tsr) = tree.get(route.as_bytes(), None);
        assert!(h.is_none(), "non-nil handler for No-TSR route '{route}'");
        assert!(!tsr, "expected no TSR recommendation for route '{route}'");
    }
}

#[test]
fn test_tree_root_trailing_slash_redirect() {
    let mut tree = Tree::new();
    tree.add("/{test}", fake_handler("/{test}"));
    let (h, tsr) = tree.get(b"/", None);
    assert!(h.is_none());
    assert!(!tsr);
}

#[test]
fn test_tree_find_case_insensitive_path() {
    let mut tree = Tree::new();
    let long_path = format!("/l{}ng", "o".repeat(128));
    let l_ong_path = format!("/l{}ng/", "O".repeat(128));
    let mut routes: Vec<String> = [
        "/hi",
        "/b/",
        "/ABC/",
        "/search/{query}",
        "/cmd/{tool}/",
        "/src/{filepath:*}",
        "/proc/{id}/status",
        "/x",
        "/x/y",
        "/y/",
        "/y/z",
        "/0/{id}",
        "/0/{id}/1",
        "/1/{id}/",
        "/1/{id}/2",
        "/aa",
        "/a/",
        "/doc",
        "/doc/go_faq.html",
        "/doc/go1.html",
        "/doc/go/away",
        "/no/a",
        "/no/b",
        "/Π",
        "/u/apfêl/",
        "/u/äpfêl/",
        "/u/äpkul/",
        "/u/öpfêl",
        "/v/Äpfêl/",
        "/v/Öpfêl",
        "/w/♬",
        "/w/♭/",
        "/w/𠜎",
        "/w/𠜏/",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    routes.push(long_path.clone());
    for route in &routes {
        assert!(
            catch_panic(|| tree.add(route, fake_handler(route))).is_none(),
            "{route}"
        );
    }
    for fix in [true, false] {
        for route in &routes {
            let mut buf = Vec::new();
            let found = tree.find_case_insensitive_path(route.as_bytes(), fix, &mut buf);
            assert!(found, "route '{route}' not found");
            assert_eq!(String::from_utf8_lossy(&buf), *route);
        }
    }
    let tests: Vec<(String, &str, bool, bool)> = vec![
        ("/HI".into(), "/hi", true, false),
        ("/HI/".into(), "/hi", true, true),
        ("/B".into(), "/b/", true, true),
        ("/B/".into(), "/b/", true, false),
        ("/abc".into(), "/ABC/", true, true),
        ("/abc/".into(), "/ABC/", true, false),
        ("/aBc".into(), "/ABC/", true, true),
        ("/aBc/".into(), "/ABC/", true, false),
        ("/abC".into(), "/ABC/", true, true),
        ("/abC/".into(), "/ABC/", true, false),
        ("/SEARCH/QUERY".into(), "/search/QUERY", true, false),
        ("/SEARCH/QUERY/".into(), "/search/QUERY", true, true),
        ("/CMD/TOOL/".into(), "/cmd/TOOL/", true, false),
        ("/CMD/TOOL".into(), "/cmd/TOOL/", true, true),
        ("/SRC/FILE/PATH".into(), "/src/FILE/PATH", true, false),
        ("/ProC/112".into(), "", false, false),
        ("/x/Y".into(), "/x/y", true, false),
        ("/x/Y/".into(), "/x/y", true, true),
        ("/X/y".into(), "/x/y", true, false),
        ("/X/y/".into(), "/x/y", true, true),
        ("/X/Y".into(), "/x/y", true, false),
        ("/X/Y/".into(), "/x/y", true, true),
        ("/Y/".into(), "/y/", true, false),
        ("/Y".into(), "/y/", true, true),
        ("/Y/z".into(), "/y/z", true, false),
        ("/Y/z/".into(), "/y/z", true, true),
        ("/Y/Z".into(), "/y/z", true, false),
        ("/Y/Z/".into(), "/y/z", true, true),
        ("/y/Z".into(), "/y/z", true, false),
        ("/y/Z/".into(), "/y/z", true, true),
        ("/Aa".into(), "/aa", true, false),
        ("/Aa/".into(), "/aa", true, true),
        ("/AA".into(), "/aa", true, false),
        ("/AA/".into(), "/aa", true, true),
        ("/aA".into(), "/aa", true, false),
        ("/aA/".into(), "/aa", true, true),
        ("/A/".into(), "/a/", true, false),
        ("/A".into(), "/a/", true, true),
        ("/DOC".into(), "/doc", true, false),
        ("/DOC/".into(), "/doc", true, true),
        ("/NO".into(), "", false, true),
        ("/DOC/GO".into(), "", false, true),
        ("/π".into(), "/Π", true, false),
        ("/π/".into(), "/Π", true, true),
        ("/u/ÄPFÊL/".into(), "/u/äpfêl/", true, false),
        ("/U/ÄPKUL/".into(), "/u/äpkul/", true, false),
        ("/u/ÄPFÊL".into(), "/u/äpfêl/", true, true),
        ("/u/ÖPFÊL/".into(), "/u/öpfêl", true, true),
        ("/u/ÖPFÊL".into(), "/u/öpfêl", true, false),
        ("/v/äpfêL/".into(), "/v/Äpfêl/", true, false),
        ("/v/äpfêL".into(), "/v/Äpfêl/", true, true),
        ("/v/öpfêL/".into(), "/v/Öpfêl", true, true),
        ("/v/öpfêL".into(), "/v/Öpfêl", true, false),
        ("/w/♬/".into(), "/w/♬", true, true),
        ("/w/♭".into(), "/w/♭/", true, true),
        ("/w/𠜎/".into(), "/w/𠜎", true, true),
        ("/w/𠜏".into(), "/w/𠜏/", true, true),
        (l_ong_path.clone(), long_path.as_str(), true, true),
    ];
    for (input, out, found_want, _) in &tests {
        let mut buf = Vec::new();
        let found = tree.find_case_insensitive_path(input.as_bytes(), true, &mut buf);
        assert_eq!(found, *found_want, "'{input}'");
        if found {
            assert_eq!(String::from_utf8_lossy(&buf), *out, "'{input}'");
        }
    }
    for (input, out, found_want, slash) in &tests {
        let mut buf = Vec::new();
        let found = tree.find_case_insensitive_path(input.as_bytes(), false, &mut buf);
        if *slash {
            assert!(!found, "found without fixTrailingSlash: {input}");
        } else {
            assert_eq!(found, *found_want, "'{input}'");
            if found {
                assert_eq!(String::from_utf8_lossy(&buf), *out, "'{input}'");
            }
        }
    }
}

#[test]
fn test_tree_wildcard_conflict_ex() {
    let routes = [
        "/con{tact}",
        "/who/are/{you:*}",
        "/who/foo/hello",
        "/whose/{users}/{name}",
        "/{filepath:*}",
        "/{id}",
    ];
    let conflicts: [(&str, Option<&str>); 15] = [
        ("/who/are/foo", None),
        ("/who/are/foo/", None),
        ("/who/are/foo/bar", None),
        ("/conxxx", None),
        ("/conooo/xxx", None),
        (
            "invalid/data",
            Some("path must begin with '/' in path 'invalid/data'"),
        ),
        (
            "/con{tact}",
            Some("a handler is already registered for path '/con{tact}'"),
        ),
        (
            "/con{something}",
            Some(
                "'{something}' in new path '/con{something}' conflicts with existing wild path '{tact}' in existing prefix '/con{tact}'",
            ),
        ),
        (
            "/who/are/{you:*}",
            Some("a wildcard handler is already registered for path '/who/are/{you:*}'"),
        ),
        (
            "/who/are/{me:*}",
            Some(
                "'{me:*}' in new path '/who/are/{me:*}' conflicts with existing wildcard '{you:*}' in existing prefix '/who/are/{you:*}'",
            ),
        ),
        (
            "/who/foo/hello",
            Some("a handler is already registered for path '/who/foo/hello'"),
        ),
        (
            "/{static:*}",
            Some(
                "'{static:*}' in new path '/{static:*}' conflicts with existing wildcard '{filepath:*}' in existing prefix '/{filepath:*}'",
            ),
        ),
        (
            "/static/{filepath:*}/other",
            Some(
                "wildcard routes are only allowed at the end of the path in path '/static/{filepath:*}/other'",
            ),
        ),
        (
            "/{user}/",
            Some(
                "'{user}' in new path '/{user}/' conflicts with existing wild path '{id}' in existing prefix '/{id}'",
            ),
        ),
        (
            "/prefix{filepath:*}",
            Some("no / before wildcard in path '/prefix{filepath:*}'"),
        ),
    ];
    for (route, want) in conflicts {
        let mut tree = Tree::new();
        for r in routes {
            tree.add(r, fake_handler(r));
        }
        let err = catch_panic(|| tree.add(route, fake_handler(route)));
        assert_eq!(err.as_deref(), want, "{route}");
    }
}

#[test]
fn test_tree_with_handlers_and_params() {
    let cases: [TreeCase; 10] = [
        (
            "/users/{name}",
            "/users/atreugo",
            false,
            &[("name", "atreugo")],
        ),
        ("/users", "/users", false, &[]),
        ("/user/", "/user", true, &[]),
        ("/", "/", false, &[]),
        (
            "/users/{name}/jobs",
            "/users/atreugo/jobs",
            false,
            &[("name", "atreugo")],
        ),
        ("/users/admin", "/users/admin", false, &[]),
        (
            "/users/{status}/proc",
            "/users/active/proc",
            false,
            &[("status", "active")],
        ),
        (
            "/static/{filepath:*}",
            "/static/assets/js/main.js",
            false,
            &[("filepath", "assets/js/main.js")],
        ),
        ("/data/orders", "/data/orders/", true, &[]),
        ("/data/", "/data", true, &[]),
    ];
    let mut tree = Tree::new();
    let handlers: Vec<Handler> = cases.iter().map(|(p, ..)| fake_handler(p)).collect();
    for ((path, ..), h) in cases.iter().zip(&handlers) {
        tree.add(path, h.clone());
    }
    for ((_, req, tsr_want, ps), h) in cases.iter().zip(&handlers) {
        let mut ctx = ctx_for(req);
        let (got, tsr) = tree.get(req.as_bytes(), Some(&mut ctx));
        assert_eq!(tsr, *tsr_want, "{req}");
        if *tsr_want {
            assert!(got.is_none(), "{req}");
        } else {
            assert!(got.as_ref().is_some_and(|g| Arc::ptr_eq(g, h)), "{req}");
        }
        let want: BTreeMap<String, String> = ps
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(params(&ctx), want, "{req}");
        let (got2, tsr2) = tree.get(req.as_bytes(), None);
        assert_eq!(got2.is_some(), got.is_some());
        assert_eq!(tsr2, tsr);
    }
    let filepath = fake_handler("/{filepath:*}");
    tree.add("/{filepath:*}", filepath.clone());
    let mut ctx = ctx_for("/js/main.js");
    let (got, _) = tree.get(b"/js/main.js", Some(&mut ctx));
    assert!(got.is_some_and(|g| Arc::ptr_eq(&g, &filepath)));
    assert_eq!(ctx.param("filepath"), b"js/main.js");
}

#[test]
fn test_tree_get_tsr() {
    let h = fake_handler("/api/");
    let mut tree = Tree::new();
    tree.add("/api/", h.clone());
    assert!(matches!(tree.get(b"/api", None), (None, true)));
    assert!(
        tree.get(b"/api/", None)
            .0
            .is_some_and(|g| Arc::ptr_eq(&g, &h))
    );
    assert!(matches!(tree.get(b"/notfound", None), (None, false)));

    let mut tree = Tree::new();
    tree.add("/api", h.clone());
    assert!(tree.get(b"/api", None).0.is_some());
    assert!(matches!(tree.get(b"/api/", None), (None, true)));
    assert!(matches!(tree.get(b"/notfound", None), (None, false)));
}

#[test]
fn test_tree_root_wildcard() {
    let h = fake_handler("/{filepath:*}");
    let mut tree = Tree::new();
    tree.add("/{filepath:*}", h.clone());
    let mut ctx = ctx_for("/");
    assert!(tree.get(b"/", Some(&mut ctx)).0.is_some());
    assert_eq!(ctx.param("filepath"), b"");
    assert!(ctx.has_param("filepath"));
    tree.add("/hello/{a}/{b}/{c}", h);
    let mut ctx = ctx_for("/hello/a");
    assert!(tree.get(b"/hello/a", Some(&mut ctx)).0.is_some());
    assert_eq!(ctx.param("filepath"), b"hello/a");
}

#[test]
fn test_tree_rejects_regex_parameters() {
    let mut tree = Tree::new();
    let recv = catch_panic(|| tree.add("/regex/{c1:big_alt|alt}/x", fake_handler("r")))
        .unwrap_or_default();
    assert!(recv.contains("not supported"), "{recv}");
    let mut tree = Tree::new();
    let recv = catch_panic(|| tree.add("/api/{file}.json", fake_handler("r"))).unwrap_or_default();
    assert!(recv.contains("not supported"), "{recv}");
}
