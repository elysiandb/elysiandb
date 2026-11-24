use super::bits::modf;

pub fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
pub fn floor(x: f64) -> f64 {
    floor_arch(x)
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub fn floor(x: f64) -> f64 {
    floor_go(x)
}

pub fn floor_arch(x: f64) -> f64 {
    x.floor()
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
pub fn ceil(x: f64) -> f64 {
    ceil_arch(x)
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub fn ceil(x: f64) -> f64 {
    ceil_go(x)
}

pub fn ceil_arch(x: f64) -> f64 {
    x.ceil()
}

pub fn ceil_go(x: f64) -> f64 {
    -floor(-x)
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
pub fn trunc(x: f64) -> f64 {
    trunc_arch(x)
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub fn trunc(x: f64) -> f64 {
    trunc_go(x)
}

pub fn trunc_arch(x: f64) -> f64 {
    x.trunc()
}

pub fn trunc_go(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let (d, _) = modf(x);
    d
}

pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub fn floor_go(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    if x < 0.0 {
        let (mut d, fract) = modf(-x);
        if fract != 0.0 {
            d += 1.0;
        }
        return -d;
    }
    let (d, _) = modf(x);
    d
}
