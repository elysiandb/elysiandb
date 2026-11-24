mod actor;
mod authentication;
mod basic_user;
mod cors;
mod http_protection;
mod login;
mod login_guard;
mod login_source;
mod session;
mod session_cookie;
mod token;
mod user_auth;

use std::sync::Arc;

use crate::configuration::Config;
use crate::globals;
use crate::gocompat::utf8;

pub use actor::actor;
pub use authentication::{
    authenticate, authenticate_admin, authenticate_credentials, authentication_is_enabled,
    basic_authentication_is_enabled, credentials_are_required, register,
    token_authentication_is_enabled, user_authentication_is_enabled,
};
pub use basic_user::{
    ADMIN_PASSWORD_ENV_VAR, BasicHashedUser, BasicHeader, BasicUser, ByteString, Check,
    DEFAULT_ADMIN_PASSWORD, DEFAULT_ADMIN_USERNAME, KEY_FILENAME, MAX_USERNAME_LENGTH,
    MIN_PASSWORD_LENGTH, ROLE_ADMIN, ROLE_USER, Role, SESSIONS_FILENAME, SecurityError,
    USER_ENTITY, authenticate_user, basic_user_exists, change_user_password,
    change_user_password_keeping_session, change_user_role, check_basic_authentication,
    create_basic_user, create_key_file_or_get_key, delete_basic_user, generate_key,
    get_basic_user_by_username, init_admin_user_if_not_exists, init_basic_users_storage,
    list_basic_users, parse_basic_header, set_check_basic_authentication, user_entity_schema,
    validate_password, validate_username, write_key,
};
pub use cors::{cors, origin_is_allowed};
pub use http_protection::{
    ADMIN_UI_CONTENT_SECURITY_POLICY, API_CONTENT_SECURITY_POLICY, cross_origin_write_is_rejected,
    protect, reject_cross_origin_writes, security_headers,
};
pub use login::{
    LOGIN_FAILURE_BAD_CREDENTIALS, LOGIN_FAILURE_DEFAULT_PASSWORD_REMOTELY,
    LOGIN_FAILURE_NOT_ADMIN, authenticate_user_from, default_admin_password_is_in_use,
    is_default_admin_credentials, log_failed_login, log_login, log_logout,
    reset_failed_login_throttle, retry_after_seconds, warn_if_default_admin_password,
};
pub use login_guard::{Clock, reset_login_attempts, set_login_clock};
pub use login_source::{
    IpNet, LoginSource, login_source_from_addr, login_source_from_ip, login_source_from_request,
    parse_ip, parse_network,
};
pub use session::{
    SESSION_TTL, Session, SessionsFile, active_session_count, create_session,
    credentials_fingerprint, current_session, current_user_can_manage_user, current_user_is_admin,
    delete_all_sessions, delete_session, delete_user_sessions, get_current_username, get_session,
    hash_session_id, protect_sessions_file, read_sessions_file,
};
pub use session_cookie::{clear_session_cookie, request_is_secure, set_session_cookie};
pub use token::{check_token_authentication, set_check_token_authentication, token_matches};
pub use user_auth::{
    Middleware, ROLE_KEY, SESSION_COOKIE_NAME, USERNAME_KEY, set_user_auth, user_auth,
};

fn config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

pub(crate) fn text(b: &[u8]) -> String {
    utf8::decode_string(b)
}
