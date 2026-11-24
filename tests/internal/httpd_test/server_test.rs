use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use elysiandb::httpd::{
    Cookie, CookieSameSite, RequestConfig, RequestCtx, RequestHeader, ServerConfig, ShutdownError,
    handler,
};

use super::support::{
    LogCapture, TestServer, Until, elysian_config, mask_date, parse_responses, read_response,
    roundtrip, roundtrip_half_close, serial, text,
};

fn echo(ctx: &mut RequestCtx) {
    let path = ctx.path().to_vec();
    match path.as_slice() {
        b"/plain" => ctx.set_body_string("hello"),
        b"/empty" => {}
        b"/json-empty" => ctx.set_content_type("application/json"),
        b"/nocontent" => {
            ctx.set_content_type("application/json");
            ctx.set_status_code(204);
        }
        b"/unknown" => {
            ctx.set_content_type("application/json");
            ctx.set_status_code(499);
            ctx.set_body_string(r#"{"error":"request abandoned by the client"}"#);
        }
        b"/headers" => {
            ctx.response.header.set("X-A", "1");
            ctx.response.header.add("X-B", "2");
            ctx.response.header.set("x-a", "3");
            ctx.response.header.add("X-B", "4");
            let mut c = Cookie::new();
            c.set_key("edb_session");
            c.set_value("abc");
            c.set_path("/");
            c.set_http_only(true);
            c.set_same_site(CookieSameSite::StrictMode);
            ctx.response.header.set_cookie(&c);
            ctx.set_body_string("h");
        }
        b"/error" => {
            ctx.response.header.set("X-Gone", "1");
            ctx.set_content_type("application/json");
            ctx.error("Not Found", 404);
        }
        b"/stream" => {
            ctx.set_content_type("application/json");
            ctx.response.header.set("X-Before", "1");
            ctx.set_body_stream_writer(|w| {
                let _ = w.write_all(b"[");
                let _ = w.flush();
                let _ = w.write_all(&vec![b'a'; 10_000]);
                let _ = w.write_all(b"]");
            });
            ctx.response.header.set("X-After", "2");
        }
        b"/sized" => {
            ctx.set_body_stream(Box::new(std::io::Cursor::new(b"0123456789".to_vec())), 10);
        }
        b"/sleep" => {
            std::thread::sleep(Duration::from_millis(600));
            ctx.set_body_string("slept");
        }
        _ => {
            let mut out = Vec::new();
            out.extend_from_slice(ctx.method());
            out.push(b' ');
            out.extend_from_slice(ctx.request_uri());
            out.push(b'|');
            out.extend_from_slice(ctx.uri().path_original());
            out.push(b'|');
            out.extend_from_slice(ctx.path());
            out.push(b'|');
            out.extend_from_slice(ctx.host());
            out.push(b'|');
            out.extend_from_slice(ctx.post_body());
            ctx.set_body(out);
        }
    }
}

fn server() -> TestServer {
    TestServer::start(elysian_config(echo))
}

#[test]
fn test_default_headers_and_order() {
    let _g = serial();
    let ts = server();
    let cases: [(&str, &str); 7] = [
        (
            "/plain",
            "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        ),
        (
            "/empty",
            "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        (
            "/json-empty",
            "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        (
            "/nocontent",
            "HTTP/1.1 204 No Content\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        ),
        (
            "/unknown",
            "HTTP/1.1 499 Unknown Status Code\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nContent-Length: 43\r\nConnection: close\r\n\r\n{\"error\":\"request abandoned by the client\"}",
        ),
        (
            "/headers",
            "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 1\r\nX-A: 3\r\nX-B: 2\r\nX-B: 4\r\nSet-Cookie: edb_session=abc; path=/; HttpOnly; SameSite=Strict\r\nConnection: close\r\n\r\nh",
        ),
        (
            "/error",
            "HTTP/1.1 404 Not Found\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
        ),
    ];
    for (path, want) in cases {
        let req = format!("GET {path} HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
        let (raw, closed) = roundtrip(&ts, req.as_bytes());
        assert_eq!(mask_date(&raw), want, "path {path}");
        assert!(closed, "path {path}");
    }
}

#[test]
fn test_date_header_format() {
    let _g = serial();
    let ts = server();
    let (raw, _) = roundtrip(
        &ts,
        b"GET /empty HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    );
    let r = &parse_responses(&raw)[0];
    let date = r.header("Date").unwrap_or("");
    assert_eq!(date.len(), 29, "{date}");
    assert!(date.ends_with(" GMT"), "{date}");
    assert_eq!(&date[3..5], ", ", "{date}");
}

#[test]
fn test_keep_alive_and_pipelining() {
    let _g = serial();
    let ts = server();
    let mut c = ts.connect();
    let _ = c.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\nGET /a?x=1 HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    let rs = parse_responses(&raw);
    assert_eq!(rs.len(), 2, "{}", text(&raw));
    assert_eq!(
        rs[0].names(),
        ["Server", "Date", "Content-Type", "Content-Length"]
    );
    assert_eq!(rs[0].body, b"hello");
    assert_eq!(rs[1].header("Connection"), Some("close"));
    assert_eq!(rs[1].body, b"GET /a?x=1|/a|/a|h|");
    assert!(closed);

    let mut c = ts.connect();
    for _ in 0..3 {
        let _ = c.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
        let (raw, closed) = read_response(
            &mut c,
            Until::Quiet(Duration::from_millis(300), Duration::from_secs(5)),
        );
        assert!(!closed);
        assert_eq!(parse_responses(&raw)[0].body, b"hello");
    }
}

#[test]
fn test_http10_and_version_tokens() {
    let _g = serial();
    let ts = server();
    let (raw, closed) = roundtrip(&ts, b"GET /empty HTTP/1.0\r\nHost: h\r\n\r\n");
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(closed);

    let mut c = ts.connect();
    let _ = c.write_all(b"GET /empty HTTP/1.0\r\nHost: h\r\nConnection: keep-alive\r\n\r\n");
    let (raw, closed) = read_response(
        &mut c,
        Until::Quiet(Duration::from_millis(400), Duration::from_secs(5)),
    );
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n"
    );
    assert!(!closed);

    let (raw, closed) = roundtrip(&ts, b"GET /empty HTTP/2.0\r\nHost: h\r\n\r\n");
    assert!(
        mask_date(&raw).ends_with("Connection: close\r\n\r\n"),
        "{}",
        text(&raw)
    );
    assert!(closed);
}

#[test]
fn test_parser_tolerance() {
    let _g = serial();
    let ts = server();
    let cases: [(&[u8], &str); 6] = [
        (
            b"GET /kv/a b HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
            "GET /kv/a b|/kv/a b|/kv/a b|h|",
        ),
        (
            b"GET http://Other/health?q=1 HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
            "GET http://Other/health?q=1|/health|/health|other|",
        ),
        (
            b"\r\n\r\nGET /x HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
            "GET /x|/x|/x|h|",
        ),
        (
            b"GET /lf HTTP/1.1\nHost: h\r\nConnection: close\r\n\r\n",
            "GET /lf|/lf|/lf|h|",
        ),
        (
            b"GET /api//book/../a%2Fb/./c HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
            "GET /api//book/../a%2Fb/./c|/api//book/../a%2Fb/./c|/api/a/b/c|h|",
        ),
        (
            b"POST /p HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Length: 3\r\n\r\nabc",
            "POST /p|/p|/p|h|abc",
        ),
    ];
    for (req, want) in cases {
        let (raw, _) = roundtrip(&ts, req);
        let rs = parse_responses(&raw);
        assert_eq!(rs.len(), 1, "{}", text(req));
        assert_eq!(text(&rs[0].body), want, "{}", text(req));
    }
}

#[test]
fn test_header_parsing() {
    let _g = serial();
    let mut cfg = elysian_config(|ctx: &mut RequestCtx| {
        let h = &ctx.request.header;
        let all: Vec<String> = h.peek_all("X-Dup").iter().map(|v| text(v)).collect();
        let body = format!(
            "{}|{}|{}|{}|{}",
            text(h.peek("x-dup")),
            all.join(","),
            text(h.peek("X-Fold")),
            text(h.cookie("edb_session")),
            text(h.peek("content-type")),
        );
        ctx.set_body_string(body);
    });
    cfg.name = String::new();
    let ts = TestServer::start(cfg);
    let req = b"GET / HTTP/1.1\r\nhost: h\r\nx-dup: 1\r\nX-DUP: 2\r\nX-Fold: a\r\n  b\r\nCookie: a=1; edb_session=\"s1\"\r\nContent-type: text/x\r\nConnection: close\r\n\r\n";
    let (raw, _) = roundtrip(&ts, req);
    let rs = parse_responses(&raw);
    assert_eq!(text(&rs[0].body), "1|1,2|a b|s1|text/x");
    assert_eq!(
        rs[0].names(),
        ["Date", "Content-Type", "Content-Length", "Connection"]
    );
}

#[test]
fn test_connection_errors() {
    let _g = serial();
    let logs = LogCapture::start();
    let ts = server();
    let cases: [(&[u8], &str, Option<&str>); 9] = [
        (
            b"BLAH\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid request line"),
        ),
        (
            b"\x00\x01\x02\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid request line"),
        ),
        (
            b"G@T / HTTP/1.1\r\nHost: h\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid request line"),
        ),
        (
            b"GET / HTTP/1\r\nHost: h\r\n\r\n",
            "400 Bad Request|bad request",
            Some("unsupported HTTP version"),
        ),
        (
            b"GET / HTTP/1.1\r\nBad Header\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid header line"),
        ),
        (
            b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n{}x",
            "400 Bad Request|bad request",
            Some("duplicate Content-Length header"),
        ),
        (
            b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 1x\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid Content-Length"),
        ),
        (
            b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: gzip\r\n\r\n",
            "400 Bad Request|bad request",
            Some("invalid header line"),
        ),
        (
            b"POST / HTTP/1.1\r\nHost: h\r\nTrailer: Content-Length\r\n\r\n",
            "400 Bad Request|bad request",
            None,
        ),
    ];
    for (req, want, reason) in cases {
        logs.buf.clear();
        let (raw, closed) = roundtrip(&ts, req);
        let rs = parse_responses(&raw);
        assert_eq!(rs.len(), 1, "{}", text(req));
        let got = format!("{}|{}", &rs[0].status[9..], text(&rs[0].body));
        assert_eq!(got, want, "{}", text(req));
        assert_eq!(
            rs[0].names(),
            [
                "Server",
                "Date",
                "Content-Type",
                "Content-Length",
                "Connection"
            ]
        );
        assert_eq!(
            rs[0].header("Content-Type"),
            Some("text/plain; charset=utf-8")
        );
        assert!(closed);
        std::thread::sleep(Duration::from_millis(50));
        let line = logs.buf.text();
        match reason {
            Some(r) => {
                assert!(line.contains("http connection error"), "{line}");
                assert!(line.contains(&format!("error=\"{r}\"")), "{line}");
                assert!(line.contains("remote_addr=127.0.0.1"), "{line}");
            }
            None => assert!(!line.contains("http connection error"), "{line}"),
        }
    }

    logs.buf.clear();
    let (raw, closed) = roundtrip_half_close(&ts, b"GET / HTTP/1.1\r\nHost: h\r\n");
    assert!(
        text(&raw).ends_with("\r\n\r\nbad request"),
        "{}",
        text(&raw)
    );
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        logs.buf
            .text()
            .contains("unexpected end of the request head"),
        "{}",
        logs.buf.text()
    );

    logs.buf.clear();
    let (raw, closed) = roundtrip_half_close(&ts, b"\r\n\r\n");
    assert!(raw.is_empty());
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!logs.buf.text().contains("http connection error"));
}

#[test]
fn test_limits() {
    let _g = serial();
    let logs = LogCapture::start();
    let ts = TestServer::start(elysian_config(|ctx: &mut RequestCtx| {
        let n = ctx.post_body().len();
        ctx.set_body_string(n.to_string());
    }));

    let body = vec![b'a'; 4 << 20];
    let mut req = format!(
        "PUT /kv/big HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    req.extend_from_slice(&body);
    let (raw, _) = roundtrip(&ts, &req);
    assert_eq!(parse_responses(&raw)[0].body, b"4194304");

    logs.buf.clear();
    let mut req = format!(
        "PUT /kv/big HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len() + 1
    )
    .into_bytes();
    req.extend_from_slice(&body);
    req.push(b'a');
    let (raw, closed) = roundtrip(&ts, &req);
    let r = &parse_responses(&raw)[0];
    assert_eq!(r.status, "HTTP/1.1 400 Bad Request");
    assert_eq!(r.body, b"request body too large");
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(logs.buf.text().contains("error=\"request body too large\""));

    let chunked = |n: usize| {
        let mut req = b"PUT /kv/c HTTP/1.1\r\nHost: h\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        let data = vec![b'b'; n];
        for part in data.chunks(1 << 20) {
            req.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
            req.extend_from_slice(part);
            req.extend_from_slice(b"\r\n");
        }
        req.extend_from_slice(b"0\r\nX-Trailer: t\r\n\r\n");
        req
    };
    let (raw, _) = roundtrip(&ts, &chunked(4 << 20));
    assert_eq!(parse_responses(&raw)[0].body, b"4194304");
    let (raw, _) = roundtrip(&ts, &chunked((4 << 20) + 1));
    let r = &parse_responses(&raw)[0];
    assert_eq!(r.status, "HTTP/1.1 400 Bad Request");
    assert_eq!(r.body, b"request body too large");

    let head = |n: usize| {
        format!(
            "GET /h HTTP/1.1\r\nHost: h\r\nConnection: close\r\nX-Big: {}\r\n\r\n",
            "x".repeat(n)
        )
    };
    let (raw, _) = roundtrip(&ts, head(60 << 10).as_bytes());
    assert_eq!(parse_responses(&raw)[0].status, "HTTP/1.1 200 OK");
    logs.buf.clear();
    let (raw, closed) = roundtrip(&ts, head(70 << 10).as_bytes());
    let r = &parse_responses(&raw)[0];
    assert_eq!(r.status, "HTTP/1.1 431 Request Header Fields Too Large");
    assert_eq!(r.body, b"request header fields too large");
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!logs.buf.text().contains("http connection error"));
}

#[test]
fn test_chunked_request_bodies() {
    let _g = serial();
    let logs = LogCapture::start();
    let ts = server();
    let (raw, _) = roundtrip(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;ext=1\r\nde\r\n0\r\nX-T: 1\r\n\r\n",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"POST /c|/c|/c|h|abcde");

    let (raw, closed) = roundtrip_half_close(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n",
    );
    assert!(raw.is_empty(), "{}", text(&raw));
    assert!(closed);

    let (raw, _) = roundtrip_half_close(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"POST /c|/c|/c|h|abc");

    logs.buf.clear();
    let (raw, closed) = roundtrip_half_close(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n3",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"bad request");
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(logs.buf.text().contains("error=\"invalid chunked body\""));

    logs.buf.clear();
    let (raw, _) = roundtrip(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nabc\r\n0\r\n\r\n",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"bad request");
    std::thread::sleep(Duration::from_millis(50));
    assert!(logs.buf.text().contains("error=\"invalid chunked body\""));

    logs.buf.clear();
    let (raw, _) = roundtrip_half_close(
        &ts,
        b"POST /c HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"bad request");
    std::thread::sleep(Duration::from_millis(50));
    assert!(!logs.buf.text().contains("http connection error"));
}

#[test]
fn test_expect_continue() {
    let _g = serial();
    let ts = server();
    let mut c = ts.connect();
    let _ = c.write_all(b"POST /e HTTP/1.1\r\nHost: h\r\nConnection: close\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\n");
    let (raw, _) = read_response(
        &mut c,
        Until::Quiet(Duration::from_millis(300), Duration::from_secs(5)),
    );
    assert_eq!(raw, b"HTTP/1.1 100 Continue\r\n\r\n");
    let _ = c.write_all(b"hello");
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    assert_eq!(parse_responses(&raw)[0].body, b"POST /e|/e|/e|h|hello");
    assert!(closed);

    let mut c = ts.connect();
    let _ = c.write_all(
        format!(
            "POST /e HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: {}\r\n\r\n",
            (4 << 20) + 1
        )
        .as_bytes(),
    );
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    let s = text(&raw);
    assert!(
        s.starts_with("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 400 Bad Request\r\n"),
        "{s}"
    );
    assert!(s.ends_with("request body too large"), "{s}");
    assert!(closed);
}

#[test]
fn test_head_and_streams() {
    let _g = serial();
    let ts = server();
    let (raw, closed) = roundtrip(
        &ts,
        b"HEAD /plain HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 5\r\nConnection: close\r\n\r\n"
    );
    assert!(closed);

    let (raw, _) = roundtrip(
        &ts,
        b"GET /stream HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    );
    let s = mask_date(&raw);
    let head = "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nX-Before: 1\r\nTransfer-Encoding: chunked\r\nX-After: 2\r\nConnection: close\r\n\r\n";
    assert!(s.starts_with(head), "{s}");
    assert!(s.ends_with("\r\n0\r\n\r\n"), "{}", &s[s.len() - 20..]);
    let body = &parse_responses(&raw)[0].body;
    assert_eq!(body.len(), 10_002);
    assert_eq!(body[0], b'[');
    assert_eq!(body[body.len() - 1], b']');
    assert!(
        s[head.len()..].starts_with("1\r\n[\r\n"),
        "{}",
        &s[head.len()..head.len() + 20]
    );

    let (raw, _) = roundtrip(
        &ts,
        b"GET /sized HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789"
    );
}

#[test]
fn test_timeouts() {
    let _g = serial();
    let logs = LogCapture::start();
    let mut cfg = elysian_config(echo);
    cfg.read_timeout = Duration::from_millis(300);
    cfg.idle_timeout = Duration::from_millis(500);
    let ts = TestServer::start(cfg);

    let start = Instant::now();
    let mut c = ts.connect();
    let _ = c.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n");
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    let r = &parse_responses(&raw)[0];
    assert_eq!(r.status, "HTTP/1.1 400 Bad Request");
    assert_eq!(r.body, b"bad request");
    assert!(closed);
    assert!(start.elapsed() >= Duration::from_millis(250));

    let mut c = ts.connect();
    let _ = c.write_all(b"POST /b HTTP/1.1\r\nHost: h\r\nContent-Length: 50\r\n\r\n{\"id\":");
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    let r = &parse_responses(&raw)[0];
    assert_eq!(r.status, "HTTP/1.1 408 Request Timeout");
    assert_eq!(r.body, b"request timeout");
    assert_eq!(
        r.names(),
        [
            "Server",
            "Date",
            "Content-Type",
            "Content-Length",
            "Connection"
        ]
    );
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !logs.buf.text().contains("http connection error"),
        "{}",
        logs.buf.text()
    );

    let mut c = ts.connect();
    let _ = c.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let start = Instant::now();
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    assert_eq!(parse_responses(&raw)[0].body, b"hello");
    assert!(closed);
    assert!(
        start.elapsed() >= Duration::from_millis(450),
        "{:?}",
        start.elapsed()
    );

    let mut c = ts.connect();
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_millis(1200)));
    assert!(raw.is_empty());
    assert!(!closed);
}

#[test]
fn test_continue_after_write_deadline() {
    let _g = serial();
    let mut cfg = elysian_config(echo);
    cfg.write_timeout = Duration::from_millis(200);
    let ts = TestServer::start(cfg);
    let mut c = ts.connect();
    let _ = c.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let (raw, _) = read_response(
        &mut c,
        Until::Quiet(Duration::from_millis(200), Duration::from_secs(5)),
    );
    assert_eq!(parse_responses(&raw)[0].body, b"hello");
    std::thread::sleep(Duration::from_millis(400));
    let _ = c.write_all(
        b"POST /e HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\n",
    );
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    assert!(raw.is_empty(), "{}", text(&raw));
    assert!(closed);
}

#[test]
fn test_hijack_and_header_hook() {
    let _g = serial();
    fn hook(h: &mut RequestHeader) -> RequestConfig {
        if h.is_post() && h.request_uri().starts_with(b"/upload") {
            h.del("X-Hidden");
            let n = h.content_length();
            if n > 0 || n == -1 {
                h.set("X-Hidden", n.to_string());
                h.set_content_length(0);
            }
        }
        RequestConfig::default()
    }
    let mut cfg = elysian_config(|ctx: &mut RequestCtx| {
        let hidden = String::from_utf8_lossy(ctx.request.header.peek("X-Hidden")).into_owned();
        if hidden.is_empty() {
            ctx.set_body_string("plain");
            return;
        }
        let n: usize = hidden.parse().unwrap_or(0);
        ctx.response.header.set("X-Set-Before", "1");
        ctx.hijack_set_no_response(true);
        ctx.hijack(move |ctx, conn| {
            let mut body = vec![0u8; n];
            let _ = conn.read_exact(&mut body);
            ctx.response.header.set("Content-Type", "application/json");
            ctx.response.header.set_connection_close();
            ctx.response.set_body(&body);
            let mut out = Vec::new();
            let _ = ctx.response.write(&mut out);
            let _ = conn.write_all(&out);
        });
    });
    cfg.header_received = Some(hook);
    let ts = TestServer::start(cfg);

    let mut c = ts.connect();
    let _ = c.write_all(
        b"POST /upload HTTP/1.1\r\nHost: h\r\nX-Hidden: 99\r\nContent-Length: 10\r\n\r\n01234",
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(ts.server.open_connections(), 0);
    let _ = c.write_all(b"56789");
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nContent-Length: 10\r\nX-Set-Before: 1\r\nConnection: close\r\n\r\n0123456789"
    );
    assert!(closed);

    let (raw, _) = roundtrip(
        &ts,
        b"POST /upload HTTP/1.1\r\nHost: h\r\nConnection: close\r\nX-Hidden: 3\r\n\r\n",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"plain");
}

#[test]
fn test_concurrency_limit() {
    let _g = serial();
    let logs = LogCapture::start();
    let mut cfg = elysian_config(echo);
    cfg.concurrency = 1;
    let ts = TestServer::start(cfg);
    let mut hold = ts.connect();
    let _ = hold.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let (raw, _) = read_response(
        &mut hold,
        Until::Quiet(Duration::from_millis(300), Duration::from_secs(5)),
    );
    assert_eq!(parse_responses(&raw)[0].body, b"hello");
    let (raw, closed) = roundtrip(&ts, b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    assert_eq!(
        mask_date(&raw),
        "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: text/plain\r\nContent-Length: 20\r\n\r\ntoo many connections"
    );
    assert!(closed);
    assert!(logs.buf.text().contains("http connection limit reached"));
    assert!(logs.buf.text().contains("limit=1"));
}

#[test]
fn test_shutdown() {
    let _g = serial();
    let mut ts = server();
    let mut idle = ts.connect();
    let _ = idle.write_all(b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let _ = read_response(
        &mut idle,
        Until::Quiet(Duration::from_millis(200), Duration::from_secs(5)),
    );
    let mut slow = ts.connect();
    let _ = slow.write_all(b"GET /sleep HTTP/1.1\r\nHost: h\r\n\r\n");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(ts.server.open_connections(), 2);

    let server = ts.server.clone();
    let started = Instant::now();
    let stopper = std::thread::spawn(move || server.shutdown_with_timeout(Duration::from_secs(5)));
    let (raw, closed) = read_response(&mut idle, Until::Close(Duration::from_secs(2)));
    assert!(raw.is_empty());
    assert!(closed);
    let (raw, closed) = read_response(&mut slow, Until::Close(Duration::from_secs(3)));
    let rs = parse_responses(&raw);
    assert_eq!(rs[0].body, b"slept");
    assert_eq!(rs[0].header("Connection"), None);
    assert!(closed);
    let res = stopper
        .join()
        .unwrap_or(Err(ShutdownError::DeadlineExceeded));
    assert_eq!(res, Ok(()));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(ts.server.open_connections(), 0);
    ts.stop();
    assert!(std::net::TcpStream::connect(ts.addr).is_err());
}

#[test]
fn test_shutdown_deadline() {
    let _g = serial();
    let ts = TestServer::start(elysian_config(|ctx: &mut RequestCtx| {
        std::thread::sleep(Duration::from_millis(1500));
        ctx.set_body_string("late");
    }));
    let mut c = ts.connect();
    let _ = c.write_all(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    std::thread::sleep(Duration::from_millis(100));
    let res = ts.server.shutdown_with_timeout(Duration::from_millis(200));
    assert_eq!(res, Err(ShutdownError::DeadlineExceeded));
    assert_eq!(
        ShutdownError::DeadlineExceeded.to_string(),
        "drain timeout expired"
    );
    let (raw, closed) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
    assert_eq!(parse_responses(&raw)[0].body, b"late");
    assert!(closed);
}

#[test]
fn test_multipart_pre_parse() {
    let _g = serial();
    let logs = LogCapture::start();
    let ts = server();
    let body = "--XX\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n2\r\n--XX\r\nContent-Disposition: form-data; name=\"a\"; filename=\"dir/f.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\r\n--XX\r\nContent-Disposition: form-data; name=\"a0\"\r\n\r\n1\r\n--XX--\r\n";
    let req = format!(
        "POST /m HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Type: multipart/form-data; boundary=XX\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (raw, _) = roundtrip(&ts, req.as_bytes());
    let want = "POST /m|/m|/m|h|--XX\r\nContent-Disposition: form-data; name=\"a0\"\r\n\r\n1\r\n--XX\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n2\r\n--XX\r\nContent-Disposition: form-data; name=\"a\"; filename=\"dir/f.txt\"\r\nContent-Type: text/plain\r\n\r\nfile\r\n--XX--\r\n";
    assert_eq!(text(&parse_responses(&raw)[0].body), want);

    logs.buf.clear();
    let bad = "--XYZ\r\nbroken";
    let req = format!(
        "POST /m HTTP/1.1\r\nHost: h\r\nContent-Type: multipart/form-data; boundary=XYZ\r\nContent-Length: {}\r\n\r\n{bad}",
        bad.len()
    );
    let (raw, closed) = roundtrip(&ts, req.as_bytes());
    assert_eq!(parse_responses(&raw)[0].body, b"bad request");
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        logs.buf.text().contains("error=\"invalid multipart body\""),
        "{}",
        logs.buf.text()
    );
}

#[test]
fn test_handler_runs_on_elastic_workers() {
    let _g = serial();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (a, p) = (active.clone(), peak.clone());
    let mut cfg = ServerConfig::new(handler(move |ctx: &mut RequestCtx| {
        let now = a.fetch_add(1, Ordering::SeqCst) + 1;
        p.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        a.fetch_sub(1, Ordering::SeqCst);
        ctx.set_body_string("ok");
    }));
    cfg.read_timeout = Duration::from_secs(3);
    let ts = TestServer::start(cfg);
    let addr = ts.addr;
    let threads: Vec<_> = (0..16)
        .map(|_| {
            std::thread::spawn(move || {
                let mut c = std::net::TcpStream::connect(addr).ok()?;
                c.write_all(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
                    .ok()?;
                let (raw, _) = read_response(&mut c, Until::Close(Duration::from_secs(5)));
                Some(
                    parse_responses(&raw)
                        .first()
                        .map(|r| r.body.clone())
                        .unwrap_or_default(),
                )
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().ok().flatten().unwrap_or_default(), b"ok");
    }
    assert!(
        peak.load(Ordering::SeqCst) >= 8,
        "peak {}",
        peak.load(Ordering::SeqCst)
    );
}

#[test]
fn test_handler_panic_releases_the_connection() {
    let _g = serial();
    let ts = TestServer::start(elysian_config(|ctx: &mut RequestCtx| {
        if ctx.path() == b"/panic" {
            std::panic::panic_any("handler failure");
        }
        ctx.set_body_string("ok");
    }));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (raw, closed) = roundtrip(&ts, b"GET /panic HTTP/1.1\r\nHost: h\r\n\r\n");
    std::panic::set_hook(prev);
    assert!(raw.is_empty());
    assert!(closed);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(ts.server.open_connections(), 0);
    let (raw, _) = roundtrip(
        &ts,
        b"GET /ok HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(parse_responses(&raw)[0].body, b"ok");
}
