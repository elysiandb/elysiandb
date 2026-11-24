use goja::file::{File, resolve_sourcemap_url};
use std::sync::Arc;

#[test]
fn test_position() {
    const SRC: &str = "line1\nline2\nline3";
    let f = File::new("", SRC.as_bytes(), 0);
    let tests: [(i64, i64, i64); 11] = [
        (0, 1, 1),
        (2, 1, 3),
        (2, 1, 3),
        (6, 2, 1),
        (7, 2, 2),
        (12, 3, 1),
        (12, 3, 1),
        (13, 3, 2),
        (13, 3, 2),
        (16, 3, 5),
        (17, 3, 6),
    ];
    for (i, (offset, line, col)) in tests.into_iter().enumerate() {
        let p = f.position(offset);
        assert!(
            p.line == line && p.column == col,
            "{i}. Line: {}, col: {}",
            p.line,
            p.column
        );
    }
}

#[test]
fn test_file_concurrency() {
    const SRC: &str = "line1\nline2\nline3";
    let f = Arc::new(File::new("", SRC.as_bytes(), 0));
    let g = Arc::clone(&f);
    let h = std::thread::spawn(move || {
        g.position(12);
    });
    f.position(2);
    h.join().expect("thread");
}

#[test]
fn test_get_source_filename() {
    let tests: [(&str, &str, &str); 14] = [
        ("test.js", "base.js", "test.js"),
        ("test.js", "../base.js", "../test.js"),
        ("test.js", "/somewhere/base.js", "/somewhere/test.js"),
        ("/test.js", "/somewhere/base.js", "/test.js"),
        ("/test.js", "file:///somewhere/base.js", "file:///test.js"),
        ("file:///test.js", "base.js", "file:///test.js"),
        ("file:///test.js", "/somwehere/base.js", "file:///test.js"),
        (
            "file:///test.js",
            "file:///somewhere/base.js",
            "file:///test.js",
        ),
        (
            "../test.js",
            "/somewhere/else/base.js",
            "/somewhere/test.js",
        ),
        (
            "../test.js",
            "file:///somewhere/else/base.js",
            "file:///somewhere/test.js",
        ),
        (
            "../test.js",
            "https://example.com/somewhere/else/base.js",
            "https://example.com/somewhere/test.js",
        ),
        ("\ntest.js", "base123.js", "test.js"),
        ("\rtest2.js\t\n  ", "base123.js", "test2.js"),
        ("z:/file.map", "a.js", "z:/file.map"),
    ];
    for (source, basename, result) in tests {
        let got = resolve_sourcemap_url(basename.as_bytes(), source.as_bytes())
            .map(|u| u.string())
            .unwrap_or_default();
        assert_eq!(
            got, result,
            "source: {source:?}, basename {basename:?} produced {got:?} instead of {result:?}"
        );
    }
}
