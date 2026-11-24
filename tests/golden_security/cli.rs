use std::io::Cursor;
use std::sync::Arc;

use crate::assert_manifest_entry;
use crate::common::{read_jsonl, str_field, unhex};
use crate::tempdir::TempDir;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::fmt;
use elysiandb::gocompat::json::Value;
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;

const ROWS: usize = 12;

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn open_store(mode: &str) -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = !mode.is_empty();
    cfg.security.authentication.mode = mode.to_string();
    if mode == "token" {
        cfg.security.authentication.token = "golden-token".to_string();
    }
    cfg.api.schema.enabled = true;
    cfg.log.level = "error".to_string();
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    security::reset_login_attempts();
    let _ = security::init_admin_user_if_not_exists();
    Store { _dir: dir }
}

fn user_names() -> Vec<String> {
    let mut names: Vec<String> = security::list_basic_users()
        .expect("list users")
        .iter()
        .filter_map(|u| {
            u.get("username")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    names.sort();
    names
}

fn printed(row: &Value) -> String {
    String::from_utf8(unhex(str_field(row, "printf_hex"))).expect("utf8 output")
}

fn passwords(row: &Value) -> Vec<String> {
    row.get("passwords")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn assert_message(row: &Value, prefix: &str, message: &str, suffix: &str) {
    let expected = format!("{prefix}{message}{suffix}");
    assert!(
        printed(row).contains(&expected),
        "{row:?}: expected {expected:?} in the output"
    );
}

#[test]
fn cli_security_outcomes_match_go() {
    assert_manifest_entry("cli.jsonl");
    let rows = read_jsonl("security/cli.jsonl");
    let mut checked = 0;

    for row in &rows {
        let mode = str_field(row, "mode");
        let command = str_field(row, "command");
        let _store = open_store(mode);

        let mut stdin = Cursor::new(unhex(str_field(row, "stdin_hex")));
        let (username, _) = fmt::fscanln_word(&mut stdin);
        let pw = passwords(row);
        let supported = matches!(mode, "basic" | "user");

        match command {
            "create-user" if supported && pw[0] == pw[1] => {
                match security::validate_password(pw[0].as_bytes()) {
                    Err(err) => {
                        assert_message(row, "Failed to create user: ", &err.to_string(), "")
                    }
                    Ok(()) => {
                        security::validate_username(&username).expect("valid username");
                        security::create_basic_user(&BasicUser {
                            username: username.clone(),
                            password: pw[0].as_bytes().to_vec(),
                            role: Role::user(),
                        })
                        .expect("create user");
                        let name = String::from_utf8_lossy(&username).into_owned();
                        assert_message(
                            row,
                            &format!("User '{name}'"),
                            "",
                            " created successfully.",
                        );
                    }
                }
            }
            "change-password" if supported => {
                match security::change_user_password(&username, pw[0].as_bytes()) {
                    Err(err) => {
                        assert_message(row, "Failed to update password: ", &err.to_string(), "")
                    }
                    Ok(()) => {
                        assert!(
                            security::authenticate_user(&username, pw[0].as_bytes()).is_some(),
                            "{row:?}"
                        );
                    }
                }
            }
            "delete-user" if supported => {
                let name = String::from_utf8_lossy(&username).into_owned();
                match security::delete_basic_user(&username) {
                    Err(err) => assert_message(
                        row,
                        &format!("User '{name}' was not deleted: "),
                        &err.to_string(),
                        ".",
                    ),
                    Ok(()) => {
                        assert_message(row, &format!("User '{name}'"), "", " deleted successfully.")
                    }
                }
            }
            _ => {}
        }

        let expected: Vec<String> = row
            .get("users")
            .and_then(Value::as_array)
            .expect("users")
            .iter()
            .map(|u| u.as_str().expect("user").to_string())
            .collect();
        assert_eq!(user_names(), expected, "{row:?}");
        checked += 1;
    }

    assert_eq!(checked, ROWS);
}
