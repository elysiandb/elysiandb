mod base64;
#[allow(clippy::module_inception)]
mod bcrypt;

#[doc(hidden)]
pub use base64::{base64_decode, base64_encode};
pub use bcrypt::{
    DEFAULT_COST, Error, MAX_COST, MIN_COST, compare_hash_and_password, cost,
    generate_from_password,
};
#[doc(hidden)]
pub use bcrypt::{Hashed, bcrypt, new_from_hash, new_from_password};
