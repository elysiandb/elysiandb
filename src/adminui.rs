include!(concat!(env!("OUT_DIR"), "/adminui_files.rs"));

pub const ROOT: &str = "dist";

pub fn files() -> &'static [(&'static str, &'static [u8])] {
    ADMINUI_FILES
}

pub fn read_file(name: &[u8]) -> Option<&'static [u8]> {
    let rel = name.strip_prefix(ROOT.as_bytes())?.strip_prefix(b"/")?;
    ADMINUI_FILES
        .binary_search_by(|(n, _)| n.as_bytes().cmp(rel))
        .ok()
        .map(|i| ADMINUI_FILES[i].1)
}
