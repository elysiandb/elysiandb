mod audit;
#[allow(clippy::module_inception)]
mod observe;

pub use audit::audit;
pub use observe::{ERROR_KEY, REQUEST_ID_HEADER, REQUEST_ID_KEY, error, request_id, set_error};
