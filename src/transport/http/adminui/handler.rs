use crate::adminui;
use crate::gocompat::path;
use crate::httpd::{RequestCtx, STATUS_NOT_FOUND};

const INDEX: &[u8] = b"dist/index.html";

pub fn is_built() -> bool {
    adminui::read_file(INDEX).is_some()
}

pub fn admin_ui_handler(ctx: &mut RequestCtx) {
    let mut p = ctx.path().to_vec();

    if p == b"/admin" || p == b"/admin/" {
        p = b"/admin/index.html".to_vec();
    }

    let rel = path::clean(&p[b"/admin".len()..]);

    let mut fs_path = b"dist".to_vec();
    fs_path.extend_from_slice(&rel);

    let Some(data) = adminui::read_file(&fs_path) else {
        let Some(index) = adminui::read_file(INDEX) else {
            ctx.set_status_code(STATUS_NOT_FOUND);
            return;
        };
        ctx.set_content_type("text/html");
        ctx.set_body(index);
        return;
    };

    match path::ext(&fs_path) {
        b".js" => ctx.set_content_type("text/javascript"),
        b".css" => ctx.set_content_type("text/css"),
        b".html" => ctx.set_content_type("text/html"),
        b".svg" => ctx.set_content_type("image/svg+xml"),
        b".png" => ctx.set_content_type("image/png"),
        b".ico" => ctx.set_content_type("image/x-icon"),
        _ => {}
    }

    ctx.set_body(data);
}
