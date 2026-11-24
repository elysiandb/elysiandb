use std::time::Duration;

use crate::crypto::subtle;
use crate::kv;
use crate::log::{self, Logger, Throttle};
use crate::metrics;
use crate::sync::RwLock;

use super::authentication::{basic_authentication_is_enabled, user_authentication_is_enabled};
use super::basic_user::{
    ADMIN_PASSWORD_ENV_VAR, BasicHashedUser, DEFAULT_ADMIN_PASSWORD, DEFAULT_ADMIN_USERNAME,
    authenticate_user,
};
use super::login_guard::{record_failure, record_success, retry_after};
use super::login_source::LoginSource;

const SECURITY_LOG: Logger = log::for_component("security");
const FAILED_LOGIN_THROTTLE_INTERVAL: Duration = Duration::from_secs(60);

static FAILED_LOGIN_THROTTLE: RwLock<Throttle> =
    RwLock::new(Throttle::new(FAILED_LOGIN_THROTTLE_INTERVAL));

pub const LOGIN_FAILURE_BAD_CREDENTIALS: &str = "bad_credentials";
pub const LOGIN_FAILURE_DEFAULT_PASSWORD_REMOTELY: &str = "default_password_remote";
pub const LOGIN_FAILURE_NOT_ADMIN: &str = "not_admin";

pub fn reset_failed_login_throttle() {
    *FAILED_LOGIN_THROTTLE.write() = Throttle::new(FAILED_LOGIN_THROTTLE_INTERVAL);
}

pub fn log_login(username: &[u8], source: LoginSource) {
    SECURITY_LOG.info(
        "login succeeded",
        kv!["user" => username, "remote_addr" => source.to_string()],
    );
}

pub fn log_logout(username: &[u8]) {
    SECURITY_LOG.debug("logout", kv!["user" => username]);
}

pub fn log_failed_login(username: &[u8], source: LoginSource, reason: &str) {
    let addr = source.to_string();
    metrics::failed_login(&addr);

    let mut key = addr.as_bytes().to_vec();
    key.push(b'|');
    key.extend_from_slice(username);

    let (suppressed, ok) = FAILED_LOGIN_THROTTLE.read().allow(&key);
    if ok {
        SECURITY_LOG.warn(
            "login failed",
            &log::with_suppressed(
                kv!["user" => username, "remote_addr" => addr, "reason" => reason],
                suppressed,
            ),
        );
    }
}

pub fn authenticate_user_from(
    username: &[u8],
    password: &[u8],
    source: LoginSource,
) -> (Option<BasicHashedUser>, Duration, bool) {
    let wait = retry_after(source, username);
    if !wait.is_zero() {
        return (None, wait, false);
    }

    let mut user = authenticate_user(username, password);
    let mut reason = LOGIN_FAILURE_BAD_CREDENTIALS;
    if user.is_some() && !source.local && is_default_admin_credentials(username, password) {
        reason = LOGIN_FAILURE_DEFAULT_PASSWORD_REMOTELY;
        user = None;
    }

    if user.is_none() {
        record_failure(source, username);
        log_failed_login(username, source, reason);

        return (None, Duration::ZERO, false);
    }

    record_success(source, username);

    (user, Duration::ZERO, true)
}

pub fn is_default_admin_credentials(username: &[u8], password: &[u8]) -> bool {
    let user_matches =
        subtle::constant_time_compare(username, DEFAULT_ADMIN_USERNAME.as_bytes()) == 1;
    user_matches && subtle::constant_time_compare(password, DEFAULT_ADMIN_PASSWORD.as_bytes()) == 1
}

pub fn default_admin_password_is_in_use() -> bool {
    authenticate_user(
        DEFAULT_ADMIN_USERNAME.as_bytes(),
        DEFAULT_ADMIN_PASSWORD.as_bytes(),
    )
    .is_some()
}

pub fn retry_after_seconds(wait: Duration) -> i64 {
    let ns = i64::try_from(wait.as_nanos()).unwrap_or(i64::MAX);
    let second = 1_000_000_000i64;
    let seconds = ns.saturating_add(second - 1) / second;
    if seconds < 1 {
        return 1;
    }

    seconds
}

pub fn warn_if_default_admin_password() {
    if !basic_authentication_is_enabled() && !user_authentication_is_enabled() {
        return;
    }

    if default_admin_password_is_in_use() {
        let hint = format!(
            "it is only accepted from this machine: change it from the Admin UI or with `elysiandb change-password`, or set {ADMIN_PASSWORD_ENV_VAR} before the first start"
        );
        SECURITY_LOG.warn(
            "default admin password still valid",
            kv!["user" => DEFAULT_ADMIN_USERNAME, "hint" => hint],
        );
    }
}
