mod jsrun;

use goja::Value;
use goja::deps::big;
use goja::value::new_big_int;
use jsrun::*;

#[test]
fn test_big_int() {
    const SCRIPT: &str = "0xabcdef0123456789abcdef0123n";
    let b = big::Int::set_string(b"0xabcdef0123456789abcdef0123", 0).expect("big int");
    test_script(SCRIPT, new_big_int(b));
}

#[test]
fn test_big_int_format() {
    const SCRIPT: &str = r#"
assert.sameValue((1n).toString(undefined), "1", "radius undefined");
assert.throws(RangeError, () => { (1n).toString(-1); }, "radius -1");
assert.throws(RangeError, () => { (1n).toString(37); }, "radius 37");
assert.sameValue((1n).toString(2), "1", "radius 2");
assert.sameValue((10n).toString(3), "101", "radius 3");
"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_big_int_operator() {
    const SCRIPT: &str = r#"
assert.throws(TypeError, () => { 1 - 1n; }, "mix type add");
assert.throws(TypeError, () => { 1n - 1; }, "mix type add");
assert.throws(TypeError, () => { 1n + 1; }, "mix type sub");
assert.throws(TypeError, () => { 1 + 1n; }, "mix type sub");
assert.throws(TypeError, () => { 1 * 1n; }, "mix type mul");
assert.throws(TypeError, () => { 1n * 1; }, "mix type mul");
assert.throws(TypeError, () => { 1 / 1n; }, "mix type div");
assert.throws(TypeError, () => { 1n / 1; }, "mix type div");
assert.throws(TypeError, () => { 1 % 1n; }, "mix type mod");
assert.throws(TypeError, () => { 1n % 1; }, "mix type mod");
assert.throws(TypeError, () => { 1n ** 1; }, "mix type exp");
assert.throws(TypeError, () => { 1 ** 1n; }, "mix type exp");
assert.throws(TypeError, () => { 1 & 1n; }, "mix type and");
assert.throws(TypeError, () => { 1n & 1; }, "mix type and");
assert.throws(TypeError, () => { 1 | 1n; }, "mix type or");
assert.throws(TypeError, () => { 1n | 1; }, "mix type or");
assert.throws(TypeError, () => { 1 ^ 1n; }, "mix type xor");
assert.throws(TypeError, () => { 1n ^ 1; }, "mix type xor");
assert.throws(TypeError, () => { 1 << 1n; }, "mix type lsh");
assert.throws(TypeError, () => { 1n << 1; }, "mix type lsh");
assert.throws(TypeError, () => { 1 >> 1n; }, "mix type rsh");
assert.throws(TypeError, () => { 1n >> 1; }, "mix type rsh");
assert.throws(TypeError, () => { 1 >>> 1n; }, "mix type ursh");
assert.throws(TypeError, () => { 1n >>> 1; }, "mix type ursh");

assert.sameValue(1n + 1n, 2n, "add");
assert.sameValue(1n - 1n, 0n, "sub");
assert.sameValue(1n * 2n, 2n, "mul");
assert.sameValue(1n / 2n, 0n, "div");
assert.sameValue(1n % 2n, 1n, "mod");
assert.sameValue(1n ** 2n, 1n, "exp");
assert.sameValue(1n & 1n, 1n, "and");
assert.sameValue(1n | 1n, 1n, "or");
assert.sameValue(2n ^ 1n, 3n, "xor");
assert.sameValue(1n << 1n, 2n, "lsh");
assert.sameValue(4n << -1n, 2n, "neg lsh");
assert.sameValue(4n >> 1n, 2n, "rsh");
assert.sameValue(2n >> -2n, 8n, "neg rsh");

let a = 1n;
assert.sameValue(++a, 2n, "inc");
assert.sameValue(--a, 1n, "dec");

assert.sameValue(Object(1n) - 1n, 0n, "primitive sub");
assert.sameValue(Object(Object(1n)) - 1n, 0n, "primitive sub");
assert.sameValue({ [Symbol.toPrimitive]: () => 1n } - 1n, 0n, "primitive sub");
assert.sameValue({ valueOf: () => 1n } - 1n, 0n, "valueOf sub");

assert.sameValue(1n > 0, true, "gt");
assert.sameValue(0 > 1n, false, "gt");
assert.sameValue(Object(1n) > 0, true, "gt");
assert.sameValue(0 > Object(1n), false, "gt");

assert.sameValue(1n < 0, false, "lt");
assert.sameValue(0 < 1n, true, "lt");
assert.sameValue(Object(1n) < 0, false, "lt");
assert.sameValue(0 < Object(1n), true, "lt");

assert.sameValue(1n >= 0, true, "ge");
assert.sameValue(0 >= 1n, false, "ge");
assert.sameValue(1n <= 0, false, "le");
assert.sameValue(0 <= 1n, true, "le");
"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}
