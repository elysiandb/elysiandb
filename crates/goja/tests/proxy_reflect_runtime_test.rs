mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_nested_enumerate() {
    test_script_with_test_lib(
        r#"
	var o = {baz: true, foo: true, bar: true};
	var res = "";
	for (var i in o) {
		delete o.baz;
		Object.defineProperty(o, "hidden", {value: true, configurable: true});
		for (var j in o) {
			Object.defineProperty(o, "0", {value: true, configurable: true});
			Object.defineProperty(o, "1", {value: true, configurable: true});
			for (var k in o) {}
			res += i + "-" + j + " ";
		}
	}
	assert(compareArray(Reflect.ownKeys(o), ["0","1","foo","bar","hidden"]), "keys");
	res;
	"#,
        str_value("baz-foo baz-bar foo-foo foo-bar bar-foo bar-bar "),
    );
}

#[test]
fn test_error_stack() {
    test_script(
        r#"
	const err = new Error("test");
	if (!("stack" in err)) {
		throw new Error("in");
	}
	if (Reflect.ownKeys(err)[0] !== "stack") {
		throw new Error("property order");
	}
	const stack = err.stack;
	if (stack !== "Error: test\n\tat test.js:2:14(3)\n") {
		throw new Error(stack);
	}
	delete err.stack;
	if ("stack" in err) {
		throw new Error("stack still in err after delete");
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_property_order() {
    test_script(
        r#"
	var o = {};
	var sym1 = Symbol(1);
	var sym2 = Symbol(2);
	o[sym2] = 1;
	o[4294967294] = 1;
	o[2] = 1;
	o[1] = 1;
	o[0] = 1;
	o["02"] = 1;
	o[4294967295] = 1;
	o["01"] = 1;
	o["00"] = 1;
	o[sym1] = 1;
	var expected = ["0", "1", "2", "4294967294", "02", "4294967295", "01", "00", sym2, sym1];
	var actual = Reflect.ownKeys(o);
	if (actual.length !== expected.length) {
		throw new Error("Unexpected length: "+actual.length);
	}
	for (var i = 0; i < actual.length; i++) {
		if (actual[i] !== expected[i]) {
			throw new Error("Unexpected list: " + actual);
		}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_set_foreign_return_value() {
    test_script(
        r#"
	var array = [1, 2, 3];
	var arrayTarget = new Proxy(array, {});

	Object.preventExtensions(array);

	!Reflect.set(arrayTarget, "foo", 2);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_define_properties_undefined_val() {
    test_script(
        r#"
var target = {};
var sym = Symbol();
target[sym] = 1;
target.foo = 2;
target[0] = 3;

var getOwnKeys = [];
var proxy = new Proxy(target, {
  getOwnPropertyDescriptor: function(_target, key) {
    getOwnKeys.push(key);
  },
});

Object.defineProperties({}, proxy);
	true;
	"#,
        Value::Bool(true),
    );
}
