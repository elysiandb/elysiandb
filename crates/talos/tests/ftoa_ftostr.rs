use talos::ftoa::{self, FToStrMode};

fn _test_f_to_str(num: f64, mode: FToStrMode, precision: i64, expected: &str) {
    let mut buf = Vec::new();
    ftoa::f_to_str(num, mode, precision, &mut buf);
    let s = String::from_utf8(buf).expect("ascii");
    assert_eq!(s, expected, "expected: '{expected}', actual: '{s}");
    if !num.is_nan() && num != 0.0 && !num.is_sign_negative() {
        _test_f_to_str(-num, mode, precision, &format!("-{expected}"));
    }
}

fn test_f_to_str(num: f64, mode: FToStrMode, precision: i64, expected: &str) {
    _test_f_to_str(num, mode, precision, expected);
}

#[test]
fn test_dtostr() {
    test_f_to_str(0.0, ftoa::MODE_STANDARD, 0, "0");
    test_f_to_str(1.0, ftoa::MODE_STANDARD, 0, "1");
    test_f_to_str(
        9007199254740991.0,
        ftoa::MODE_STANDARD,
        0,
        "9007199254740991",
    );
    test_f_to_str(
        i64::MAX as f64,
        ftoa::MODE_STANDARD_EXPONENTIAL,
        0,
        "9.223372036854776e+18",
    );
    test_f_to_str(1e-5, ftoa::MODE_FIXED, 1, "0.0");
    test_f_to_str(8.85, ftoa::MODE_EXPONENTIAL, 2, "8.8e+0");
    test_f_to_str(885.0, ftoa::MODE_EXPONENTIAL, 2, "8.9e+2");
    test_f_to_str(25.0, ftoa::MODE_EXPONENTIAL, 1, "3e+1");
    test_f_to_str(1e-6, ftoa::MODE_FIXED, 7, "0.0000010");
    test_f_to_str(
        std::f64::consts::PI,
        ftoa::MODE_STANDARD_EXPONENTIAL,
        0,
        "3.141592653589793e+0",
    );
    test_f_to_str(f64::INFINITY, ftoa::MODE_STANDARD, 0, "Infinity");
    test_f_to_str(f64::NAN, ftoa::MODE_STANDARD, 0, "NaN");
    test_f_to_str(
        f64::from_bits(1),
        ftoa::MODE_EXPONENTIAL,
        40,
        "4.940656458412465441765687928682213723651e-324",
    );
    test_f_to_str(
        3.5844466002796428e+298,
        ftoa::MODE_STANDARD,
        0,
        "3.5844466002796428e+298",
    );
    test_f_to_str(
        f64::from_bits(0x0010000000000000),
        ftoa::MODE_STANDARD,
        0,
        "2.2250738585072014e-308",
    );
    test_f_to_str(
        f64::from_bits(0x000FFFFFFFFFFFFF),
        ftoa::MODE_STANDARD,
        0,
        "2.225073858507201e-308",
    );
    test_f_to_str(4294967272.0, ftoa::MODE_PRECISION, 14, "4294967272.0000");
}
