use super::bits::frexp;
use super::madd;

const INV_LN10: f64 = f64::from_bits(0x3fdb_cb7b_1526_e50e);
const INV_LN2: f64 = f64::from_bits(0x3ff7_1547_652b_82fe);

pub fn log10_impl(x: f64, log: fn(f64) -> f64) -> f64 {
    log(x) * INV_LN10
}

pub fn log2_impl<const F: bool>(x: f64, log: fn(f64) -> f64) -> f64 {
    let (frac, exp) = frexp(x);
    if frac == 0.5 {
        return (exp - 1) as f64;
    }
    madd::<F>(log(frac), INV_LN2, exp as f64)
}
