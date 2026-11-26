mod jsrun;

use jsrun::*;
use talos::Value;

#[test]
fn test_sort_comparator_return_values() {
    test_script(
        r#"
	var a = [];
	for (var i = 0; i < 12; i++) {
	    a[i] = i;
	}

	a.sort(function(x, y) { return y - x });

	for (var i = 0; i < 12; i++) {
	    if (a[i] !== 11-i) {
		throw new Error("Value at index " + i + " is incorrect: " + a[i]);
	    }
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_sort_comparator_return_value_floats() {
    test_script(
        r#"
	var a = [
		5.97,
		9.91,
		4.13,
		9.28,
		3.29,
	];
	a.sort( function(a, b) { return a - b; } );
	for (var i = 1; i < a.length; i++) {
		if (a[i] < a[i-1]) {
			throw new Error("Array is not sorted: " + a);
		}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_sort_comparator_return_value_neg_zero() {
    test_script(
        r#"
	var a = [2, 1];
	a.sort( function(a, b) { return a > b ? 0 : -0; } );
	for (var i = 1; i < a.length; i++) {
		if (a[i] < a[i-1]) {
			throw new Error("Array is not sorted: " + a);
		}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_array_concat_sparse() {
    test_script(
        r#"
	var a1 = [];
	var a2 = [];
	a1[500000] = 1;
	a2[1000000] = 2;
	var a3 = a1.concat(a2);
	a3.length === 1500002 && a3[500000] === 1 && a3[1500001] == 2;
	"#,
        Value::Bool(true),
    );
}
