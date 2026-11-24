mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_json_parse_reviver() {
    test_script(
        r#"
	JSON.parse('{"p": 5}', function(key, value) {
	  return typeof value === 'number'
        ? value * 2 // return value * 2 for numbers
	    : value     // return everything else unchanged
	 })["p"]
	"#,
        int(10),
    );
}

#[test]
fn test_quote_malformed_surrogate_pair() {
    test_script(r#"JSON.stringify("\uD800")"#, str_value(r#""\ud800""#));
}

#[test]
fn test_eof_wrapping() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let err = match vm.run_string("JSON.parse('{')") {
            Ok(_) => panic!("Expected error"),
            Err(e) => e.error(),
        };
        assert!(
            err.contains("invalid JSON: unexpected end of input"),
            "Error doesn't contain human-friendly wrapper: {err}"
        );
    });
}

#[test]
fn test_json_escape() {
    test_script(
        r#"
	var a = "\\+1";
	JSON.stringify(a);
	"#,
        str_value(r#""\\+1""#),
    );
}

#[test]
fn test_json_object_in_array() {
    test_script(
        r#"
	var a = "[{\"a\":1},{\"a\":2}]";
	JSON.stringify(JSON.parse(a)) == a;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_json_quirky_numbers() {
    test_script(
        r#"
	var s;
	s = JSON.stringify(NaN);
	if (s != "null") {
		throw new Error("NaN: " + s);
	}

	s = JSON.stringify(Infinity);
	if (s != "null") {
		throw new Error("Infinity: " + s);
	}

	s = JSON.stringify(-Infinity);
	if (s != "null") {
		throw new Error("-Infinity: " + s);
	}

	"#,
        Value::Undefined,
    );
}

#[test]
fn test_json_nil() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        vm.set("i", Value::Null).expect("set");
        let ret = match vm.run_string("\n\tJSON.stringify(i);\n\t") {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let s = ret.go_string(&mut vm).expect("string");
        assert_eq!(s, "null", "Expected 'null', got: {s}");
    });
}
