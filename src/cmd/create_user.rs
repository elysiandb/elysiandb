use crate::globals::{self, GOLD, RED, RESET};
use crate::security::{self, BasicUser, Role, SecurityError};

use super::cmd::{print, printf, println, read_password, scanln_int, scanln_word};

pub fn create_user() {
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

    let new_user = match get_user_input() {
        Ok(user) => user,
        Err(message) => {
            println(message);
            return;
        }
    };

    if let Err(err) = validate_new_user(&new_user) {
        failed_to_create(&err);
        return;
    }

    if let Err(err) = security::create_basic_user(&new_user) {
        failed_to_create(&err);
        return;
    }

    let mut out = GOLD.as_bytes().to_vec();
    out.extend_from_slice(b"User '");
    out.extend_from_slice(&new_user.username);
    out.extend_from_slice(b"' created successfully.");
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
}

fn failed_to_create(err: &SecurityError) {
    let mut out = RED.as_bytes().to_vec();
    out.extend_from_slice(b"Failed to create user: ");
    out.extend_from_slice(&err.message());
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
}

fn get_user_input() -> Result<BasicUser, String> {
    print("Enter new username: ");
    let username = scanln_word();

    println("Select role:");
    println("1) admin");
    println("2) user");
    print("Choice: ");
    let role_input = scanln_int();

    let role = if role_input == 1 {
        Role::admin()
    } else {
        Role::user()
    };

    printf("Enter new password: ");
    let password = read_password(libc::STDIN_FILENO);
    println("");
    let password = match password {
        Ok(p) => p,
        Err(err) => return Err(format!("{RED}Failed to read password: {err}{RESET}")),
    };

    printf("Re-enter new password: ");
    let confirm = read_password(libc::STDIN_FILENO);
    println("");
    let confirm = match confirm {
        Ok(p) => p,
        Err(err) => {
            return Err(format!(
                "{RED}Failed to read password confirmation: {err}{RESET}"
            ));
        }
    };

    if password != confirm {
        return Err(format!("{RED}Passwords do not match.{RESET}"));
    }

    Ok(BasicUser {
        username,
        password,
        role,
    })
}

fn validate_new_user(user: &BasicUser) -> Result<(), SecurityError> {
    security::validate_username(&user.username)?;
    security::validate_password(&user.password)?;

    if security::basic_user_exists(&user.username) {
        return Err(SecurityError::UserAlreadyExists);
    }

    Ok(())
}
