mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_number() {
    const SCRIPT: &str = r#"
	(new Number(100111122133144155)).toString()
	"#;

    test_script(SCRIPT, str_value("100111122133144160"));
}

#[test]
fn test_fractional_number_to_string_radix() {
    const SCRIPT: &str = r#"
	(new Number(123.456)).toString(36)
	"#;

    test_script(SCRIPT, str_value("3f.gez4w97ry"));
}

#[test]
fn test_number_format_rounding() {
    const SCRIPT: &str = r#"
	assert.sameValue((123.456).toExponential(undefined), "1.23456e+2", "undefined");
	assert.sameValue((0.000001).toPrecision(2), "0.0000010")
	assert.sameValue((-7).toPrecision(1), "-7");
	assert.sameValue((-42).toPrecision(1), "-4e+1");
	assert.sameValue((0.000001).toPrecision(1), "0.000001");
	assert.sameValue((123.456).toPrecision(1), "1e+2", "1");
	assert.sameValue((123.456).toPrecision(2), "1.2e+2", "2");

	var n = new Number("0.000000000000000000001");
	assert.sameValue((n).toPrecision(1), "1e-21");
	assert.sameValue((25).toExponential(0), "3e+1");
	assert.sameValue((-25).toExponential(0), "-3e+1");
	assert.sameValue((12345).toExponential(3), "1.235e+4");
	assert.sameValue((25.5).toFixed(0), "26");
	assert.sameValue((-25.5).toFixed(0), "-26");
	assert.sameValue((99.9).toFixed(0), "100");
	assert.sameValue((99.99).toFixed(1), "100.0");
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
#[ignore = "requires the Reflect builtin"]
fn test_native_ctor_new_target() {
    const SCRIPT: &str = r#"
	function NewTarget() {
	}

	var o = Reflect.construct(Number, [1], NewTarget);
	o.__proto__ === NewTarget.prototype && o.toString() === "[object Number]";
	"#;
    test_script(SCRIPT, Value::Bool(true));
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_to_number() {
    const SCRIPT: &str = r#"
	assert(isNaN(Number("+")));
	assert(isNaN(Number("++")));
	assert(isNaN(Number("-")));
	assert(isNaN(Number("0xfp1")));
	assert(isNaN(Number("0Xfp1")));
	assert(isNaN(Number("+0xfp1")));
	assert(isNaN(Number(" +0xfp1")));
	assert(isNaN(Number(" + 0xfp1")));
	assert(isNaN(Number(" 0xfp1")));
	assert(isNaN(Number("-0xfp1")));
	assert(isNaN(Number("- 0xfp1")));
	assert(isNaN(Number(" - 0xfp1")));
	assert.sameValue(Number("0."), 0);
	assert.sameValue(Number(" "), 0);
	assert.sameValue(Number(" Infinity"), Infinity);

	let a = [1];
	assert.sameValue(1, a.at("0xfp1"));
	assert.sameValue(1, a.at(" 0xfp1"));
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}
