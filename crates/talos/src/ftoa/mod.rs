mod common;
#[allow(clippy::module_inception)]
mod ftoa;
mod ftobasestr;
mod ftostr;
pub mod internal;

pub use ftoa::ftoa;
pub use ftobasestr::f_to_base_str;
pub use ftostr::{
    FToStrMode, MODE_EXPONENTIAL, MODE_FIXED, MODE_PRECISION, MODE_STANDARD,
    MODE_STANDARD_EXPONENTIAL, f_to_str,
};
