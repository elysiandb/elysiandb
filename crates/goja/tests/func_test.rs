mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_func_proto() {
    test_script(
        r#"
	"use strict";
	function A() {}
	A.__proto__ = Object;
	A.prototype = {};

	function B() {}
	B.__proto__ = Object.create(null);
	var thrown = false;
	try {
		delete B.prototype;
	} catch (e) {
		thrown = e instanceof TypeError;
	}
	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_func_prototype_redefine() {
    test_script(
        r#"
	let thrown = false;
	try {
		Object.defineProperty(function() {}, "prototype", {
			set: function(_value) {},
		});
	} catch (e) {
		if (e instanceof TypeError) {
			thrown = true;
		} else {
			throw e;
		}
	}
	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn example_assert_constructor() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let res = rt
            .run_string(
                r#"
		(class C {
			constructor(x) {
				this.x = x;
			}
		})
	"#,
            )
            .unwrap_or_else(|e| panic!("{}", run_error_string(&e)));
        let Value::Object(c) = res else {
            panic!("Not a constructor");
        };
        let ctor = c.assert_constructor(&rt).expect("Not a constructor");
        let obj = ctor
            .construct(&mut rt, &[str_value("Test")], None)
            .unwrap_or_else(|_| panic!("construct failed"));
        let x = obj
            .get_str(&mut rt, &"x".into(), None)
            .unwrap_or_else(|_| panic!("get failed"))
            .unwrap_or_default();
        let s = x.go_string(&mut rt).unwrap_or_default();
        assert_eq!(s, "Test");
    });
}
