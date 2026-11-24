#![allow(clippy::excessive_precision, clippy::approx_constant)]

mod bits;
mod exp;
mod floor;
mod log;
mod pow;

pub use bits::{frexp, ldexp, modf, modf_arm64, modf_go};
pub use exp::{exp, exp_amd64, exp_arm64, exp_go};
pub use floor::{abs, floor, floor_arch, floor_go};
pub use log::{log, log_amd64, log_go};
pub use pow::{pow, pow_amd64, pow_arm64, pow_go};

#[cfg(target_arch = "x86_64")]
pub fn use_fma() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static USE_FMA: AtomicU8 = AtomicU8::new(0);
    match USE_FMA.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let v = std::is_x86_feature_detected!("avx") && std::is_x86_feature_detected!("fma");
            USE_FMA.store(if v { 2 } else { 1 }, Ordering::Relaxed);
            v
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn use_fma() -> bool {
    false
}
