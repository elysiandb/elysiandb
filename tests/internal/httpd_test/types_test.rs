use elysiandb::gocompat::time::Time;
use elysiandb::httpd::{
    Args, Cookie, CookieSameSite, MultipartReader, Request, RequestCtx, Response, ResponseHeader,
    URI, normalize_path, parse_media_type, status_message, write_multipart_form,
};

use super::support::text;

#[test]
fn test_args_semantics() {
    let mut a = Args::new();
    a.parse("keys=a%2Cb+c&keys=second&flag&empty=&sort%5Btitle%5D=desc&bad=%zz&trail=%4");
    assert_eq!(a.peek("keys"), b"a,b c");
    assert_eq!(
        a.peek_multi("keys"),
        vec![b"a,b c".as_slice(), b"second".as_slice()]
    );
    assert_eq!(a.peek("flag"), b"");
    assert!(a.has("flag"));
    assert!(a.has("empty"));
    assert!(!a.has("missing"));
    assert_eq!(a.peek("missing"), b"");
    assert_eq!(a.peek_opt("missing"), None);
    assert_eq!(a.peek("sort[title]"), b"desc");
    assert_eq!(a.peek("bad"), b"%zz");
    assert_eq!(a.peek("trail"), b"%4");
    let keys: Vec<String> = a.all().map(|(k, _)| text(k)).collect();
    assert_eq!(
        keys,
        [
            "keys",
            "keys",
            "flag",
            "empty",
            "sort[title]",
            "bad",
            "trail"
        ]
    );
    assert_eq!(
        a.query_string(),
        b"keys=a%2Cb+c&keys=second&flag&empty=&sort%5Btitle%5D=desc&bad=%25zz&trail=%254"
    );

    let mut n = Args::new();
    n.parse("a=12&b=-1&c=1x&d=&e=99999999999999999999&f=007");
    assert_eq!(n.get_uint_or_zero("a"), 12);
    assert_eq!(n.get_uint_or_zero("b"), 0);
    assert_eq!(n.get_uint_or_zero("c"), 0);
    assert_eq!(n.get_uint_or_zero("d"), 0);
    assert_eq!(n.get_uint_or_zero("e"), 0);
    assert_eq!(n.get_uint_or_zero("f"), 7);
    assert_eq!(n.get_uint_or_zero("zz"), 0);

    let mut b = Args::new();
    b.parse("t=true&y=Yes&n=no&one=1&zero=0&up=TRUE&mixed=tRue");
    assert!(b.get_bool("t"));
    assert!(b.get_bool("y"));
    assert!(!b.get_bool("n"));
    assert!(b.get_bool("one"));
    assert!(!b.get_bool("zero"));
    assert!(b.get_bool("up"));
    assert!(!b.get_bool("mixed"));
}

#[test]
fn test_normalize_path_table() {
    let cases = [
        ("/", "/"),
        ("/abc", "/abc"),
        ("/a/b/c", "/a/b/c"),
        ("/abc/", "/abc/"),
        ("", "/"),
        ("a/", "/a/"),
        ("abc", "/abc"),
        ("//", "/"),
        ("/abc//", "/abc/"),
        ("/abc//def//ghi", "/abc/def/ghi"),
        ("///abc", "/abc"),
        ("./", "/"),
        ("/abc/./def", "/abc/def"),
        ("/./abc/def", "/abc/def"),
        ("..", "/"),
        ("../", "/"),
        ("../../", "/"),
        ("../..", "/"),
        ("../../abc", "/abc"),
        ("/abc/def/ghi/../jkl", "/abc/def/jkl"),
        ("/abc/def/../ghi/../jkl", "/abc/jkl"),
        ("/abc/def/..", "/abc/"),
        ("/abc/def/../..", "/"),
        ("/abc/def/../../..", "/"),
        ("/abc/def/../../../ghi/jkl/../../../mno", "/mno"),
        ("abc/./../def", "/def"),
        ("abc//./../def", "/def"),
        ("abc/../../././../def", "/def"),
        ("/a%2Fb", "/a/b"),
        ("/a%2fb%20c", "/a/b c"),
        ("/a+b", "/a+b"),
        ("/a%zz", "/a%zz"),
        ("/a%4", "/a%4"),
        ("/api/../health", "/health"),
        ("/admin/../kv", "/kv"),
    ];
    for (input, want) in cases {
        assert_eq!(text(&normalize_path(input.as_bytes())), want, "{input}");
    }
}

#[test]
fn test_uri_parsing() {
    let mut u = URI::new();
    let _ = u.parse(b"Example.COM:8089", b"/api/Book?x=1&y=a+b#frag");
    assert_eq!(u.host(), b"example.com:8089");
    assert_eq!(u.path_original(), b"/api/Book");
    assert_eq!(u.path(), b"/api/Book");
    assert_eq!(u.query_string(), b"x=1&y=a+b");
    assert_eq!(u.hash(), b"frag");
    assert_eq!(u.query_args().peek("y"), b"a b");
    assert_eq!(
        u.full_uri(),
        b"http://example.com:8089/api/Book?x=1&y=a+b#frag"
    );

    let mut u = URI::new();
    let _ = u.parse(b"h", b"http://other/health?q=1");
    assert_eq!(u.host(), b"other");
    assert_eq!(u.path(), b"/health");
    assert_eq!(u.scheme(), b"http");

    let mut u = URI::new();
    let _ = u.parse(b"h", b"/api/a b/");
    assert_eq!(u.request_uri(), b"/api/a%20b/");
    let mut u = URI::new();
    let _ = u.parse(b"h", b"/x?a#b?c");
    assert_eq!(u.query_string(), b"a");
    assert_eq!(u.hash(), b"b?c");
    let mut u = URI::new();
    let _ = u.parse(b"h", b"/x#b?c");
    assert_eq!(u.query_string(), b"");
    assert!(u.parse(b"h", b"/x\x01").is_err());
    assert_eq!(u.path(), b"/");
    assert!(u.parse(b"a b", b"/x").is_err());

    let mut u = URI::new();
    let _ = u.parse(b"h", b"/health/");
    u.update(b"/health");
    assert_eq!(u.full_uri(), b"http://h/health");
    let mut u = URI::new();
    let _ = u.parse(b"", b"/health/");
    u.update(b"/health");
    assert_eq!(u.full_uri(), b"http:///health");
}

#[test]
fn test_request_header_api() {
    let mut req = Request::new();
    req.header.set_method("POST");
    req.set_request_uri("/api/restore?x=1");
    req.header.set("content-length", "12");
    req.header.set(
        "content-type",
        "multipart/form-data; boundary=\"ab cd\"; charset=x",
    );
    req.header.add("X-Elysian-Internal-Body-Length", "5");
    req.header.add("X-Other", "1");
    assert!(req.header.is_post());
    assert_eq!(req.header.content_length(), 12);
    assert_eq!(req.header.peek("Content-Length"), b"12");
    assert_eq!(req.header.multipart_form_boundary(), b"ab cd");
    assert_eq!(req.header.peek("x-elysian-internal-body-length"), b"5");
    req.header.del("X-Elysian-Internal-Body-Length");
    assert_eq!(req.header.peek("X-Elysian-Internal-Body-Length"), b"");
    req.header.set_content_length(-1);
    assert_eq!(req.header.content_length(), -1);
    assert_eq!(req.header.peek("Transfer-Encoding"), b"chunked");
    req.header.set_content_length(0);
    assert_eq!(req.header.peek("Transfer-Encoding"), b"");
    assert_eq!(req.header.peek("Content-Length"), b"0");

    let mut h = Request::new();
    h.header
        .set_content_type("multipart/form-data; foo=1; boundary=zz");
    assert_eq!(h.header.multipart_form_boundary(), b"zz");
    h.header.set_content_type("multipart/form-data");
    assert_eq!(h.header.multipart_form_boundary(), b"");
    h.header.set_content_type("application/json");
    assert_eq!(h.header.multipart_form_boundary(), b"");

    let mut c = Request::new();
    c.header.add("Cookie", "a=1; edb_session=xyz");
    assert_eq!(c.header.cookie("edb_session"), b"xyz");
    c.header.set_cookie("edb_session", "new");
    assert_eq!(c.header.cookie("edb_session"), b"new");
    assert_eq!(c.header.peek("Cookie"), b"a=1; edb_session=new");

    let mut ctx = RequestCtx::new();
    let mut r = Request::new();
    r.header.set_method("GET");
    r.set_request_uri("/api/book?limit=2");
    r.header.set_host("Host.Example");
    ctx.init(&r, None);
    assert_eq!(ctx.path(), b"/api/book");
    assert_eq!(ctx.host(), b"host.example");
    assert_eq!(ctx.query_args().get_uint_or_zero("limit"), 2);
    assert_eq!(ctx.remote_ip().to_string(), "0.0.0.0");
    ctx.set_param("entity", b"book");
    assert_eq!(ctx.param("entity"), b"book");
    assert_eq!(ctx.param("missing"), b"");
    ctx.set_user_value("guard", 42u32);
    assert_eq!(ctx.user_value::<u32>("guard"), Some(&42));
    assert_eq!(ctx.user_value::<String>("guard"), None);
    ctx.remove_user_value("guard");
    assert!(!ctx.has_user_value("guard"));
}

#[test]
fn test_response_header_api() {
    let mut h = ResponseHeader::new();
    assert_eq!(h.status_code(), 200);
    h.set("A", "1");
    h.set("B", "2");
    h.set("C", "3");
    h.del("A");
    let keys: Vec<String> = h
        .all()
        .iter()
        .map(|(k, _)| text(k))
        .filter(|k| k != "Content-Type")
        .collect();
    assert_eq!(keys, ["C", "B"]);
    h.set("Content-Type", "application/json");
    assert_eq!(h.content_type(), b"application/json");
    h.set("Server", "S");
    assert_eq!(h.server(), b"S");
    h.set("Connection", "close");
    assert!(h.connection_close());
    h.set("Connection", "keep-alive");
    assert!(!h.connection_close());
    assert_eq!(h.peek("Connection"), b"keep-alive");
    h.set("X-Line", "a\r\nb");
    assert_eq!(h.peek("x-line"), b"a  b");

    let mut c = Cookie::new();
    c.set_key("edb_session");
    c.set_value("");
    c.set_expire(Some(Time::from_unix(0, 0)));
    c.set_path("/");
    c.set_http_only(true);
    c.set_secure(true);
    c.set_same_site(CookieSameSite::StrictMode);
    assert_eq!(
        text(&c.cookie()),
        "edb_session=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/; HttpOnly; secure; SameSite=Strict"
    );
    h.set_cookie(&c);
    assert_eq!(h.peek_cookie("edb_session"), c.cookie().as_slice());
    let mut parsed = Cookie::new();
    assert!(parsed.parse(c.cookie()).is_ok());
    assert_eq!(parsed.key(), b"edb_session");
    assert!(parsed.http_only());
    assert!(parsed.secure());
    assert_eq!(parsed.same_site(), CookieSameSite::StrictMode);
    assert_eq!(parsed.expire().map(|t| t.unix()), Some(0));
}

#[test]
fn test_status_messages() {
    assert_eq!(status_message(200), "OK");
    assert_eq!(status_message(308), "Permanent Redirect");
    assert_eq!(status_message(431), "Request Header Fields Too Large");
    assert_eq!(status_message(499), "Unknown Status Code");
    assert_eq!(status_message(507), "Insufficient Storage");
    assert_eq!(status_message(999), "Unknown Status Code");
}

#[test]
fn test_response_write() {
    let mut r = Response::new();
    r.set_status_code(204);
    r.header.set_content_type("application/json");
    let mut out = Vec::new();
    let _ = r.write(&mut out);
    let s = text(&out);
    assert!(s.starts_with("HTTP/1.1 204 No Content\r\nDate: "), "{s}");
    assert!(
        s.ends_with("\r\nContent-Type: application/json\r\n\r\n"),
        "{s}"
    );

    let mut r = Response::new();
    r.set_body_stream_writer(Box::new(|w| {
        let _ = std::io::Write::write_all(w, b"abc");
    }));
    assert!(r.is_body_stream());
    assert_eq!(r.body(), b"abc");
    assert!(!r.is_body_stream());

    let mut r = Response::new();
    r.set_body_stream_writer(Box::new(|w| {
        let _ = std::io::Write::write_all(w, b"abc");
    }));
    r.set_body("plain");
    let mut out = Vec::new();
    let _ = r.write(&mut out);
    let s = text(&out);
    assert!(!s.contains("Transfer-Encoding"), "{s}");
    assert!(s.contains("Content-Length: 5\r\n"), "{s}");
}

#[test]
fn test_multipart_reader() {
    let body = b"preamble\r\n--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"/tmp/../a.tar.gz\"\r\nContent-Type: application/gzip\r\n\r\nDATA\r\nMORE\r\n--B\r\nContent-Disposition: form-data; name=\"note\"\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\na=3Db=\r\nc\r\n--B--\r\nepilogue";
    let mut r = MultipartReader::new(body.as_slice(), b"B");
    let p = r.next_part().ok().flatten().unwrap_or_default();
    assert_eq!(p.form_name(), b"file");
    assert_eq!(p.file_name(), b"a.tar.gz");
    let mut data = Vec::new();
    let _ = std::io::Read::read_to_end(&mut r.part_reader(), &mut data);
    assert_eq!(data, b"DATA\r\nMORE");
    let p = r.next_part().ok().flatten().unwrap_or_default();
    assert_eq!(p.form_name(), b"note");
    assert!(p.file_name().is_empty());
    let mut data = Vec::new();
    let _ = std::io::Read::read_to_end(&mut r.part_reader(), &mut data);
    assert_eq!(data, b"a=bc");
    assert!(r.next_part().ok().flatten().is_none());

    let mut r = MultipartReader::new(b"--B\r\nbroken".as_slice(), b"B");
    assert!(r.next_part().is_err());
    let mut r = MultipartReader::new(b"--B\r\n\r\nx\r\n--B--\r\n".as_slice(), b"");
    assert!(r.next_part().is_err());

    let (mt, params) =
        parse_media_type(b"form-data; name=\"a\\\"b\"; filename*=UTF-8''%C3%A9t%C3%A9.txt")
            .unwrap_or_default();
    assert_eq!(mt, b"form-data");
    assert_eq!(
        params.get(b"name".as_slice()).map(|v| text(v)),
        Some("a\"b".to_string())
    );
    assert_eq!(
        params.get(b"filename".as_slice()).map(|v| text(v)),
        Some("été.txt".to_string())
    );
    assert!(parse_media_type(b"form-data; name=a; name=b").is_err());
    assert!(parse_media_type(b"").is_err());

    let mut form = elysiandb::httpd::Form::default();
    form.value.insert(b"z".to_vec(), vec![b"1".to_vec()]);
    form.value
        .insert(b"a".to_vec(), vec![b"2".to_vec(), b"3".to_vec()]);
    let mut out = Vec::new();
    assert!(write_multipart_form(&mut out, &form, b"XB").is_ok());
    assert_eq!(
        text(&out),
        "--XB\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n2\r\n--XB\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n3\r\n--XB\r\nContent-Disposition: form-data; name=\"z\"\r\n\r\n1\r\n--XB--\r\n"
    );
    let mut out = Vec::new();
    let err = write_multipart_form(&mut out, &form, b"bad\x01")
        .err()
        .unwrap_or_default();
    assert!(err.starts_with("cannot use form boundary"), "{err}");
}
