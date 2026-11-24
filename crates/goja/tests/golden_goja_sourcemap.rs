mod common;

use common::{b, opt_s, read_jsonl, s};
use gocompat::json::Value;
use goja::deps::sourcemap::url::{self, ErrorKind, Url};
use goja::deps::sourcemap::{self, Error, path};
use goja::file::resolve_sourcemap_url;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn check_url(u: &Url, want: &Value, ctx: &str) {
    assert_eq!(text(&u.scheme), s(want, "scheme"), "scheme {ctx}");
    assert_eq!(text(&u.opaque), s(want, "opaque"), "opaque {ctx}");
    assert_eq!(text(&u.host), s(want, "host"), "host {ctx}");
    assert_eq!(text(&u.path), s(want, "path"), "path {ctx}");
    assert_eq!(text(&u.raw_path), s(want, "raw_path"), "raw_path {ctx}");
    assert_eq!(u.omit_host, b(want, "omit_host"), "omit_host {ctx}");
    assert_eq!(u.force_query, b(want, "force_query"), "force_query {ctx}");
    assert_eq!(text(&u.raw_query), s(want, "raw_query"), "raw_query {ctx}");
    assert_eq!(text(&u.fragment), s(want, "fragment"), "fragment {ctx}");
    assert_eq!(
        text(&u.raw_fragment),
        s(want, "raw_fragment"),
        "raw_fragment {ctx}"
    );
    assert_eq!(u.string(), s(want, "string"), "string {ctx}");
    assert_eq!(
        text(&u.escaped_path()),
        s(want, "escaped_path"),
        "escaped_path {ctx}"
    );
    assert_eq!(u.is_abs(), b(want, "is_abs"), "is_abs {ctx}");
    match (&u.user, want.get("user")) {
        (None, Some(Value::Null)) => {}
        (Some(user), Some(w)) => {
            assert_eq!(text(user.username()), s(w, "username"), "username {ctx}");
            let (p, set) = user.password();
            assert_eq!(text(p), s(w, "password"), "password {ctx}");
            assert_eq!(set, b(w, "password_set"), "password_set {ctx}");
            assert_eq!(text(&user.string()), s(w, "string"), "user string {ctx}");
        }
        (got, want) => panic!("user {ctx}: {got:?} vs {want:?}"),
    }
}

fn url_class(kind: &ErrorKind) -> String {
    match kind {
        ErrorKind::Escape(e) => format!("escape:{}", text(e)),
        ErrorKind::InvalidHost(h) => format!("host:{}", text(h)),
        ErrorKind::ControlCharacter => "control".to_string(),
        ErrorKind::MissingScheme => "scheme".to_string(),
        ErrorKind::ColonInFirstSegment => "colon".to_string(),
        ErrorKind::InvalidPort(_) => "port".to_string(),
        ErrorKind::InvalidUserinfo => "userinfo".to_string(),
        ErrorKind::MissingBracket => "bracket".to_string(),
    }
}

#[test]
fn url_parse_follows_go() {
    for row in &read_jsonl("goja-front/url/parse.jsonl") {
        let input = s(row, "in");
        match url::parse(input.as_bytes()) {
            Ok(u) => check_url(&u, row.get("url").expect("url"), input),
            Err(e) => assert_eq!(
                Some(url_class(&e.kind).as_str()),
                opt_s(row, "err"),
                "{input:?}"
            ),
        }
    }
}

#[test]
fn url_resolve_reference_follows_go() {
    for row in &read_jsonl("goja-front/url/resolve.jsonl") {
        let base = url::parse(s(row, "base").as_bytes()).expect("base");
        let r = url::parse(s(row, "ref").as_bytes()).expect("ref");
        let ctx = format!("{:?} + {:?}", s(row, "base"), s(row, "ref"));
        check_url(
            &base.resolve_reference(&r),
            row.get("url").expect("url"),
            &ctx,
        );
    }
}

#[test]
fn resolve_sourcemap_url_follows_go() {
    for row in &read_jsonl("goja-front/url/resolve_sourcemap.jsonl") {
        let ctx = format!("{:?} {:?}", s(row, "basename"), s(row, "source"));
        let got = resolve_sourcemap_url(s(row, "basename").as_bytes(), s(row, "source").as_bytes());
        match (got, row.get("url")) {
            (None, Some(Value::Null)) => {}
            (Some(u), Some(want)) if !want.is_null() => check_url(&u, want, &ctx),
            (got, want) => panic!("{ctx}: {got:?} vs {want:?}"),
        }
    }
}

#[test]
fn path_functions_follow_go() {
    for row in &read_jsonl("goja-front/url/path.jsonl") {
        let a = s(row, "a").as_bytes();
        match s(row, "op") {
            "clean" => assert_eq!(text(&path::clean(a)), s(row, "r"), "clean {a:?}"),
            "dir" => assert_eq!(text(&path::dir(a)), s(row, "r"), "dir {a:?}"),
            "isabs" => assert_eq!(path::is_abs(a), b(row, "r"), "isabs {a:?}"),
            "join" => {
                let bb = s(row, "b").as_bytes();
                assert_eq!(
                    text(&path::join(&[a, bb])),
                    s(row, "r"),
                    "join {a:?} {bb:?}"
                );
            }
            op => panic!("unknown op {op}"),
        }
    }
}

fn class(e: &Error) -> &'static str {
    match e {
        Error::Json(_) => "json-syntax",
        Error::Type { .. } | Error::Range { .. } => "json-type",
        Error::Version(_) => "version",
        Error::MappingsEmpty => "mappings-empty",
        Error::UnexpectedEndOfMappings => "vlq-eof",
        Error::Url(_) => "url",
    }
}

fn quiet<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(prev);
    r
}

#[test]
fn sourcemap_consumer_follows_go() {
    for row in &read_jsonl("goja-front/sourcemap/consumer.jsonl") {
        let name = s(row, "name");
        let parsed = quiet(|| sourcemap::parse(s(row, "url"), s(row, "data").as_bytes()));
        let c = match (parsed, opt_s(row, "err")) {
            (Err(_), Some("panic")) => continue,
            (Ok(Err(e)), Some(class_want)) => {
                assert_eq!(class(&e), class_want, "{name}: {e}");
                if let (Error::Json(se), Some(Value::Number(off))) = (&e, row.get("offset")) {
                    assert_eq!(se.offset.to_string(), off.as_str(), "{name}");
                }
                continue;
            }
            (Ok(Ok(c)), None) => c,
            (got, want) => panic!("{name}: {:?} vs {want:?}", got.map(|r| r.map(|_| ()))),
        };
        assert_eq!(c.file(), s(row, "file"), "{name}");
        assert_eq!(c.sourcemap_url(), s(row, "sourcemap_url"), "{name}");
        for q in row
            .get("queries")
            .and_then(Value::as_array)
            .expect("queries")
        {
            let q = q.as_array().expect("query");
            let line = q[0].as_i64().expect("line");
            let col = q[1].as_i64().expect("col");
            let got = quiet(|| c.source(line, col));
            if q[2].as_str() == Some("panic") && q.len() == 3 {
                assert!(got.is_err(), "{name} {line}:{col} must panic");
                continue;
            }
            let (src, nm, sl, sc, ok) =
                got.unwrap_or_else(|_| panic!("{name} {line}:{col} panicked"));
            assert_eq!(src, q[2].as_str().expect("source"), "{name} {line}:{col}");
            assert_eq!(nm, q[3].as_str().expect("name"), "{name} {line}:{col}");
            assert_eq!(sl, q[4].as_i64().expect("line"), "{name} {line}:{col}");
            assert_eq!(sc, q[5].as_i64().expect("column"), "{name} {line}:{col}");
            assert_eq!(ok, q[6].as_bool().expect("ok"), "{name} {line}:{col}");
        }
        for (src, content) in row
            .get("contents")
            .and_then(Value::as_object)
            .expect("contents")
            .iter()
        {
            assert_eq!(
                c.source_content(src),
                content.as_str().expect("content"),
                "{name} {src}"
            );
        }
    }
}

#[test]
fn sourcemap_messages_follow_the_catalog() {
    let msg = |data: &str| {
        sourcemap::parse("", data.as_bytes())
            .err()
            .map(|e| e.to_string())
    };
    assert_eq!(
        msg(r#"{"version":3,"sources":["a"],"mappings":""}"#).as_deref(),
        Some("mappings are empty")
    );
    assert_eq!(msg("{}").as_deref(), Some("mappings are empty"));
    assert_eq!(
        msg(r#"{"version":2,"mappings":"AACA"}"#).as_deref(),
        Some("got version=2, but only 3rd version is supported")
    );
    assert_eq!(
        msg(r#"{"version":"3","mappings":"AACA"}"#).as_deref(),
        Some(r#""version" must be an integer, got a string"#)
    );
    assert_eq!(
        msg(r#"{"version":99999999999999999999}"#).as_deref(),
        Some(r#""version": 99999999999999999999 is out of range"#)
    );
    assert_eq!(
        msg(r#"{"sources":[1]}"#).as_deref(),
        Some(r#""sources[0]" must be a string, got a number"#)
    );
    assert_eq!(
        msg(r#"{"Sources":"x"}"#).as_deref(),
        Some(r#""Sources" must be an array of strings, got a string"#)
    );
    assert_eq!(
        msg(r#"{"sections":[{"offset":{"line":true}}]}"#).as_deref(),
        Some(r#""sections[0].offset.line" must be an integer, got a boolean"#)
    );
    assert_eq!(
        msg("[]").as_deref(),
        Some("the value must be an object, got an array")
    );
    assert_eq!(
        msg("{bad}").as_deref(),
        Some("byte 2: unexpected character 'b'")
    );
    assert_eq!(
        msg(r#"{"version":3,"#).as_deref(),
        Some("unexpected end of input")
    );
    assert_eq!(msg("").as_deref(), Some("unexpected end of input"));
    assert_eq!(
        msg(r#"{"version":3,"mappings":"g"}"#).as_deref(),
        Some("unexpected end of mappings")
    );
    assert_eq!(
        msg(r#"{"sourceRoot":"%zz","mappings":"AACA"}"#).as_deref(),
        Some(r#"invalid URL "%zz": invalid escape "%zz""#)
    );
}

#[test]
fn scripts_with_source_maps_follow_go() {
    for row in &read_jsonl("goja-front/sourcemap/scripts.jsonl") {
        let name = s(row, "name");
        let src = s(row, "src");
        let (program, err) =
            goja::parser::parse_file(None, s(row, "filename"), src.as_bytes(), 0, &[]);
        match (err, row.get("err")) {
            (None, None) => {}
            (Some(list), Some(Value::Array(want))) => {
                assert_eq!(list.0.len(), want.len(), "{name}: {list}");
                for (e, w) in list.0.iter().zip(want) {
                    match w.as_str().expect("class") {
                        "load" => assert!(
                            e.message.starts_with("Could not load source map: "),
                            "{name}: {e}"
                        ),
                        "parse" => assert!(
                            e.message.starts_with("Could not parse source map: "),
                            "{name}: {e}"
                        ),
                        text => assert_eq!(e.error(), text, "{name}"),
                    }
                }
            }
            (got, want) => panic!("{name}: {got:?} vs {want:?}"),
        }
        let file = program.file.expect("file");
        for p in row
            .get("positions")
            .and_then(Value::as_array)
            .expect("positions")
        {
            let p = p.as_array().expect("position");
            let off = p[0].as_i64().expect("offset");
            let pos = file.position(off);
            assert_eq!(
                pos.filename,
                p[1].as_str().expect("filename"),
                "{name} @{off}"
            );
            assert_eq!(pos.line, p[2].as_i64().expect("line"), "{name} @{off}");
            assert_eq!(pos.column, p[3].as_i64().expect("column"), "{name} @{off}");
        }
    }
}

#[test]
fn source_map_load_messages_follow_the_catalog() {
    let parse = |src: &str| {
        let (_, err) = goja::parser::parse_file(None, "", src.as_bytes(), 0, &[]);
        err.map(|e| e.error())
    };
    assert_eq!(
        parse("function postRead(ctx) { ctx.entity.r = 1 }\n//# sourceMappingURL=missing.js.map")
            .as_deref(),
        Some(
            "(anonymous): Line 2:36 Could not load source map: open missing.js.map: No such file or directory (os error 2)"
        )
    );
    assert_eq!(
        parse("function postRead(ctx) { ctx.entity.r = 1 }\n//# sourceMappingURL=data:application/json;base64,e30=")
            .as_deref(),
        Some("(anonymous): Line 2:55 Could not parse source map: mappings are empty")
    );
    assert_eq!(
        parse("x\n//# sourceMappingURL=data:application/json;base64,e30").as_deref(),
        Some("(anonymous): Line 2:54 Could not load source map: invalid base64 at byte 0")
    );
    assert_eq!(
        parse("x\n//# sourceMappingURL=http://example.com/x.map").as_deref(),
        Some(
            "(anonymous): Line 2:46 Could not load source map: unsupported source map URL scheme: http"
        )
    );
    let dir = parse("x\n//# sourceMappingURL=.").unwrap_or_default();
    assert!(
        dir.starts_with("(anonymous): Line 2:23 Could not load source map: read .: "),
        "{dir}"
    );
}
