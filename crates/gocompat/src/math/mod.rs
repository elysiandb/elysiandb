#![allow(clippy::excessive_precision, clippy::approx_constant)]

mod bits;
mod cbrt;
mod dim;
mod exp;
mod expm1;
mod floor;
mod hyper;
mod inverse;
mod log;
mod log10;
mod log1p;
mod pow;
mod trig;

pub use bits::{frexp, ldexp, modf, modf_arm64, modf_go};
pub use cbrt::cbrt_go;
pub use dim::{max, min};
pub use exp::{exp, exp_amd64, exp_arm64, exp_go};
pub use expm1::expm1_go;
pub use floor::{
    abs, ceil, ceil_arch, ceil_go, floor, floor_arch, floor_go, sqrt, trunc, trunc_arch, trunc_go,
};
pub use hyper::{acosh_arch, asinh_arch, atanh_arch, cosh_arch, sinh_arch, tanh_arch};
pub use inverse::{acos_go, asin_go, atan_go, atan2_go};
pub use log::{log, log_amd64, log_go};
pub use log1p::log1p_go;
pub use pow::{pow, pow_amd64, pow_arm64, pow_go};
pub use trig::{cos_go, sin_go, tan_go, trig_reduce};

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

#[inline(always)]
pub(crate) fn madd<const F: bool>(n: f64, m: f64, a: f64) -> f64 {
    if F { n.mul_add(m, a) } else { a + n * m }
}

#[inline(always)]
pub(crate) fn msub<const F: bool>(n: f64, m: f64, a: f64) -> f64 {
    if F { (-n).mul_add(m, a) } else { a - n * m }
}

#[inline(always)]
pub(crate) fn nmsub<const F: bool>(n: f64, m: f64, a: f64) -> f64 {
    if F { n.mul_add(m, -a) } else { n * m - a }
}

#[derive(Clone, Copy)]
pub struct Arch {
    pub fused: bool,
    pub exp: fn(f64) -> f64,
    pub log: fn(f64) -> f64,
}

fn log_fused(x: f64) -> f64 {
    log_go(x, true)
}

fn log_plain(x: f64) -> f64 {
    log_go(x, false)
}

fn exp_amd64_plain(x: f64) -> f64 {
    exp_amd64(x, false)
}

fn exp_amd64_detected(x: f64) -> f64 {
    exp_amd64(x, use_fma())
}

pub const ARM64: Arch = Arch {
    fused: true,
    exp: exp_arm64,
    log: log_fused,
};

pub const AMD64: Arch = Arch {
    fused: false,
    exp: exp_amd64_detected,
    log: log_amd64,
};

pub const AMD64_WITHOUT_FMA: Arch = Arch {
    fused: false,
    exp: exp_amd64_plain,
    log: log_amd64,
};

pub const GENERIC: Arch = Arch {
    fused: false,
    exp: exp_go,
    log: log_plain,
};

#[cfg(target_arch = "aarch64")]
pub const NATIVE: Arch = ARM64;

#[cfg(target_arch = "x86_64")]
pub const NATIVE: Arch = AMD64;

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub const NATIVE: Arch = GENERIC;

pub fn sin_arch(x: f64, a: Arch) -> f64 {
    sin_go(x, a.fused)
}

pub fn cos_arch(x: f64, a: Arch) -> f64 {
    cos_go(x, a.fused)
}

pub fn tan_arch(x: f64, a: Arch) -> f64 {
    tan_go(x, a.fused)
}

pub fn asin_arch(x: f64, a: Arch) -> f64 {
    asin_go(x, a.fused)
}

pub fn acos_arch(x: f64, a: Arch) -> f64 {
    acos_go(x, a.fused)
}

pub fn atan_arch(x: f64, a: Arch) -> f64 {
    atan_go(x, a.fused)
}

pub fn atan2_arch(y: f64, x: f64, a: Arch) -> f64 {
    atan2_go(y, x, a.fused)
}

pub fn cbrt_arch(x: f64, a: Arch) -> f64 {
    cbrt_go(x, a.fused)
}

pub fn expm1_arch(x: f64, a: Arch) -> f64 {
    expm1_go(x, a.fused)
}

pub fn log1p_arch(x: f64, a: Arch) -> f64 {
    log1p_go(x, a.fused)
}

pub fn log10_arch(x: f64, a: Arch) -> f64 {
    log10::log10_impl(x, a.log)
}

pub fn log2_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        log10::log2_impl::<true>(x, a.log)
    } else {
        log10::log2_impl::<false>(x, a.log)
    }
}

pub fn sin(x: f64) -> f64 {
    sin_arch(x, NATIVE)
}

pub fn cos(x: f64) -> f64 {
    cos_arch(x, NATIVE)
}

pub fn tan(x: f64) -> f64 {
    tan_arch(x, NATIVE)
}

pub fn asin(x: f64) -> f64 {
    asin_arch(x, NATIVE)
}

pub fn acos(x: f64) -> f64 {
    acos_arch(x, NATIVE)
}

pub fn atan(x: f64) -> f64 {
    atan_arch(x, NATIVE)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    atan2_arch(y, x, NATIVE)
}

pub fn sinh(x: f64) -> f64 {
    sinh_arch(x, NATIVE)
}

pub fn cosh(x: f64) -> f64 {
    cosh_arch(x, NATIVE)
}

pub fn tanh(x: f64) -> f64 {
    tanh_arch(x, NATIVE)
}

pub fn asinh(x: f64) -> f64 {
    asinh_arch(x, NATIVE)
}

pub fn acosh(x: f64) -> f64 {
    acosh_arch(x, NATIVE)
}

pub fn atanh(x: f64) -> f64 {
    atanh_arch(x, NATIVE)
}

pub fn cbrt(x: f64) -> f64 {
    cbrt_arch(x, NATIVE)
}

pub fn expm1(x: f64) -> f64 {
    expm1_arch(x, NATIVE)
}

pub fn log1p(x: f64) -> f64 {
    log1p_arch(x, NATIVE)
}

pub fn log10(x: f64) -> f64 {
    log10_arch(x, NATIVE)
}

pub fn log2(x: f64) -> f64 {
    log2_arch(x, NATIVE)
}
