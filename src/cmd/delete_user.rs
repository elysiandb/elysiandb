use crate::globals::{self, GOLD, RESET};
use crate::security;

use super::cmd::{printf, println, scanln_word};

pub fn delete_user() {
    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };
    if !cfg.security.authentication.enabled {
        printf(format!(
            "{GOLD}Authentication is disabled in the configuration.{RESET}\n"
        ));
        return;
    }

    let mode = cfg.security.authentication.mode.as_str();
    if mode != "basic" && mode != "user" {
        printf(format!(
            "{GOLD}Delete user command only supports basic and user authentication modes.{RESET}\n"
        ));
        return;
    }

    let username = match get_username_input() {
        Ok(name) => name,
        Err(message) => {
            println(message);
            return;
        }
    };

    if let Err(err) = security::delete_basic_user(&username) {
        let mut out = GOLD.as_bytes().to_vec();
        out.extend_from_slice(b"User '");
        out.extend_from_slice(&username);
        out.extend_from_slice(b"' was not deleted: ");
        out.extend_from_slice(&err.message());
        out.push(b'.');
        out.extend_from_slice(RESET.as_bytes());
        out.push(b'\n');
        printf(out);
        return;
    }

    let mut out = GOLD.as_bytes().to_vec();
    out.extend_from_slice(b"User '");
    out.extend_from_slice(&username);
    out.extend_from_slice(b"' deleted successfully.");
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
}

fn get_username_input() -> Result<Vec<u8>, String> {
    printf("Enter username to delete: ");
    let username = scanln_word();

    if username.is_empty() {
        return Err("username cannot be empty".to_string());
    }

    Ok(username)
}
