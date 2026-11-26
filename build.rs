use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=ELYSIANDB_VERSION");
    let version = match env::var("ELYSIANDB_VERSION") {
        Ok(v) => match v.strip_prefix('v') {
            Some(rest) => rest.to_string(),
            None => v,
        },
        Err(_) => "dev".to_string(),
    };
    println!("cargo:rustc-env=ELYSIANDB_BUILD_VERSION={version}");

    println!("cargo:rerun-if-env-changed=RUSTC");
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let rust_version = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| {
            let mut words = text.split_whitespace();
            match (words.next(), words.next()) {
                (Some(name), Some(number)) => Some(format!("{name} {number}")),
                _ => None,
            }
        })
        .unwrap_or_else(|| "rustc".to_string());
    println!("cargo:rustc-env=ELYSIANDB_RUST_VERSION={rust_version}");

    println!("cargo:rerun-if-env-changed=ELYSIANDB_ADMINUI_DIST");
    let dist = env::var_os("ELYSIANDB_ADMINUI_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("adminui/dist"));
    println!("cargo:rerun-if-changed={}", dist.display());
    let dist = fs::canonicalize(&dist).unwrap_or(dist);
    let mut files = Vec::new();
    collect(&dist, "", true, &mut files);
    files.sort();
    let mut table = String::from("pub static ADMINUI_FILES: &[(&str, &[u8])] = &[\n");
    for (name, path) in files {
        table.push_str(&format!("    ({name:?}, include_bytes!({path:?})),\n"));
    }
    table.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out.join("adminui_files.rs"), table).expect("cannot write the Admin UI table");
}

fn is_version_control_name(name: &str) -> bool {
    matches!(name, ".bzr" | ".hg" | ".git" | ".svn")
}

fn collect(dir: &Path, prefix: &str, top: bool, files: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if top && is_version_control_name(&name) {
            panic!("the Admin UI folder holds {rel}, which cannot be embedded");
        }
        if !top && (name.starts_with('.') || name.starts_with('_')) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let before = files.len();
            collect(&path, &rel, false, files);
            if top && files.len() == before {
                panic!("the Admin UI folder {rel} holds no file to embed");
            }
        } else if file_type.is_file() {
            files.push((rel, path.to_string_lossy().into_owned()));
        } else if top {
            panic!("the Admin UI folder holds {rel}, which is not a regular file or folder");
        }
    }
}
