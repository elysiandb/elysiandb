use crate::globals::{self, GOLD, RED, RESET};
use crate::security::{self, SecurityError};

use super::cmd::{print, printf, println, read_password, scanln_word};

pub fn change_password() {
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
            "{GOLD}Create user command only supports basic and user authentication modes.{RESET}\n"
        ));
        return;
    }

    print("Enter username: ");
    let username = scanln_word();

    printf("Enter new password: ");
    let password = read_password(libc::STDIN_FILENO);
    println("");
    let password = match password {
        Ok(p) => p,
        Err(err) => {
            printf(format!("{RED}Failed to read password: {err}{RESET}\n"));
            return;
        }
    };

    printf("Re-enter new password: ");
    let confirm = read_password(libc::STDIN_FILENO);
    println("");
    let confirm = match confirm {
        Ok(p) => p,
        Err(err) => {
            printf(format!(
                "{RED}Failed to read password confirmation: {err}{RESET}\n"
            ));
            return;
        }
    };

    if password != confirm {
        printf(format!("{RED}Passwords do not match.{RESET}\n"));
        return;
    }

    if let Err(err) = security::validate_password(&password) {
        failed_to_update(&err);
        return;
    }

    if let Err(err) = security::change_user_password(&username, &password) {
        failed_to_update(&err);
        return;
    }

    let mut out = GOLD.as_bytes().to_vec();
    out.extend_from_slice(b"Password updated successfully for user '");
    out.extend_from_slice(&username);
    out.extend_from_slice(b"'.");
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
}

fn failed_to_update(err: &SecurityError) {
    let mut out = RED.as_bytes().to_vec();
    out.extend_from_slice(b"Failed to update password: ");
    out.extend_from_slice(&err.message());
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
}
