mod jsrun;

use goja::object::ObjExt;
use goja::{ObjRef, Runtime, Value};
use jsrun::*;

#[test]
fn test_sparse_array_set_length_with_prop_items() {
    test_script(
        r#"
	var a = [1,2,3,4];
	a[100000] = 5;
	var thrown = false;

	Object.defineProperty(a, "2", {value: 42, configurable: false, writable: false});
	try {
		Object.defineProperty(a, "length", {value: 0, writable: false});
	} catch (e) {
		thrown = e instanceof TypeError;
	}
	thrown && a.length === 3;
	"#,
        Value::Bool(true),
    );
}

fn run(rt: &mut Runtime, src: &str) {
    if let Err(e) = rt.run_string(src) {
        panic!("{}", run_error_string(&e));
    }
}

fn is_sparse(rt: &Runtime, a: ObjRef) -> bool {
    matches!(rt.obj(a).ext, ObjExt::SparseArray(_))
}

fn is_dense(rt: &Runtime, a: ObjRef) -> bool {
    matches!(rt.obj(a).ext, ObjExt::Array(_))
}

#[test]
#[ignore = "requires the Math builtin"]
fn test_sparse_array_switch() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        run(
            &mut vm,
            r#"
	var a = [];
	a[20470] = 5;"#,
        );
        let a = match vm.get("a").expect("get") {
            Some(Value::Object(o)) => o,
            other => panic!("a is {other:?}"),
        };
        assert!(is_sparse(&vm, a), "1: array is not sparse");
        run(
            &mut vm,
            r#"
	var cutoffIdx = Math.round(20470 - 20470/8);
	for (var i = a.length - 1; i >= cutoffIdx; i--) {
		a[i] = i;
	}

	if (a.length != 20471) {
		throw new Error("Invalid length: " + a.length);
	}

	for (var i = 0; i < cutoffIdx; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}

	for (var i = cutoffIdx; i < a.length; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}"#,
        );
        assert!(is_dense(&vm, a), "2: array is not normal");
        run(
            &mut vm,
            r#"
	a[20471] = 20471;
	if (a.length != 20472) {
		throw new Error("Invalid length: " + a.length);
	}

	for (var i = 0; i < cutoffIdx; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}

	for (var i = cutoffIdx; i < a.length; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}"#,
        );
        assert!(is_dense(&vm, a), "3: array is not normal");
        run(
            &mut vm,
            r#"
	var cutoffIdx1 = Math.round(20472 - 20472/10);
	for (var i = cutoffIdx; i < cutoffIdx1; i++) {
		delete a[i];
	}

	a[25590] = 25590;
	if (a.length != 25591) {
		throw new Error("Invalid length: " + a.length);
	}

	for (var i = 0; i < cutoffIdx1; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}

	for (var i = cutoffIdx1; i < 20472; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}

	for (var i = 20472; i < 25590; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}

	if (a[25590] !== 25590) {
		throw new Error("Invalid value at 25590: " + a[25590]);
	}
	"#,
        );
        assert!(is_sparse(&vm, a), "4: array is not sparse");
    });
}

#[test]
fn test_sparse_array_own_keys() {
    test_script(
        r#"
	var a1 = [];
	a1[500000] = 1;
	var seen = false;
	var count = 0;
	var keys = Object.keys(a1);
	keys.length === 1 && keys[0] === "500000";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_sparse_array_enumerate() {
    test_script(
        r#"
	var a1 = [];
	a1[500000] = 1;
	var seen = false;
	var count = 0;
	for (var i in a1) {
		if (i === "500000") {
			if (seen) {
				throw new Error("seen twice");
			}
			seen = true;
		}
		count++;
	}
	seen && count === 1;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_array_sparse_max_length() {
    test_script(
        r#"
	var a = [];
	a[4294967294]=1;
	a.length === 4294967295 && a[4294967294] === 1;
	"#,
        Value::Bool(true),
    );
}
