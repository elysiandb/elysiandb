use elysiandb::httpd::{Request, RequestCtx, STATUS_OK};
use elysiandb::transport::http::adminui;

fn new_ctx(method: &str, path: &str, body: &str) -> RequestCtx {
    let mut req = Request::new();
    req.header.set_method(method);
    req.set_request_uri(path);
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn admin_get(path: &str) -> (u16, Option<String>, Vec<u8>) {
    let mut ctx = new_ctx("GET", path, "");
    adminui::admin_ui_handler(&mut ctx);

    let mut wire = Vec::new();
    if let Err(e) = ctx.response.write(&mut wire) {
        panic!("{path}: {e}");
    }
    let text = String::from_utf8_lossy(&wire).into_owned();
    let head = text.split("\r\n\r\n").next().unwrap_or("").to_string();
    let content_type = head
        .split("\r\n")
        .find_map(|l| l.strip_prefix("Content-Type: "))
        .map(str::to_string);

    (
        ctx.response.status_code(),
        content_type,
        ctx.response.body_bytes().to_vec(),
    )
}

fn expected_content_type(name: &str, data: &[u8]) -> Option<&'static str> {
    let base = name.rsplit('/').next().unwrap_or(name);
    let ext = base.rfind('.').map(|i| &base[i..]).unwrap_or("");
    match ext {
        ".js" => Some("text/javascript"),
        ".css" => Some("text/css"),
        ".html" => Some("text/html"),
        ".svg" => Some("image/svg+xml"),
        ".png" => Some("image/png"),
        ".ico" => Some("image/x-icon"),
        _ if data.is_empty() => None,
        _ => Some("text/plain; charset=utf-8"),
    }
}

#[test]
fn admin_ui_handler_serves_the_embedded_assets() {
    for (name, data) in elysiandb::adminui::files() {
        assert!(
            name.split('/')
                .skip(1)
                .all(|part| !part.starts_with('.') && !part.starts_with('_')),
            "{name} must not be embedded"
        );

        let (status, content_type, body) = admin_get(&format!("/admin/{name}"));
        assert_eq!(status, STATUS_OK, "/admin/{name}");
        assert_eq!(body.as_slice(), *data, "/admin/{name}");
        assert_eq!(
            content_type.as_deref(),
            expected_content_type(name, data),
            "/admin/{name}"
        );
    }

    let index = elysiandb::adminui::read_file(b"dist/index.html");
    for path in [
        "/admin",
        "/admin/",
        "/admin/nope/deep",
        "/admin/assets",
        "/admin/../admin/x",
    ] {
        let (status, content_type, body) = admin_get(path);
        match index {
            Some(index) => {
                assert_eq!(status, STATUS_OK, "{path}");
                assert_eq!(content_type.as_deref(), Some("text/html"), "{path}");
                assert_eq!(body.as_slice(), index, "{path}");
            }
            None => {
                assert_eq!(status, 404, "{path}");
                assert_eq!(content_type, None, "{path}");
                assert!(body.is_empty(), "{path}");
            }
        }
    }
}
