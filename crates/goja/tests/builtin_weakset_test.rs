mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_weak_set_basic() {
    const SCRIPT: &str = r#"
	var s = new WeakSet();
	var o = {};
	s.add(o);
	if (!s.has(o)) {
		throw new Error("has");
	}
	s.delete(o);
	if (s.has(o)) {
		throw new Error("still has");
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_weak_set_array_simple() {
    const SCRIPT: &str = r#"
	var o1 = {}, o2 = {}, o3 = {};

	var s = new WeakSet([o1, o2, o3]);
	s.has(o1) && s.has(o2) && s.has(o3);
	"#;
    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_weak_set_array_generic() {
    const SCRIPT: &str = r#"
	var o1 = {}, o2 = {}, o3 = {};
	var a = new Array();
	var s;
	var thrown = false;
	a[1] = o2;

	try {
		s = new WeakSet(a);
	} catch (e) {
		if (e instanceof TypeError) {
			thrown = true;
		}
	}
	if (!thrown) {
		throw new Error("Case 1 does not throw");
	}

	Object.defineProperty(a.__proto__, "0", {value: o1, writable: true, enumerable: true, configurable: true});
	s = new WeakSet(a);
	if (!(s.has(o1) && s.has(o2) && !s.has(o3))) {
		throw new Error("Case 2 failed");
	}

	Object.defineProperty(a, "2", {value: o3, configurable: true});
	s = new WeakSet(a);
	s.has(o1) && s.has(o2) && s.has(o3);
	"#;
    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_weak_set_get_adder_get_iterator_order() {
    const SCRIPT: &str = r#"
	let getterCalled = 0;

	class S extends WeakSet {
	    get add() {
	        getterCalled++;
	        return null;
	    }
	}

	let getIteratorCalled = 0;

	let iterable = {};
	iterable[Symbol.iterator] = () => {
	    getIteratorCalled++
	    return {
	        next: 1
	    };
	}

	let thrown = false;

	try {
	    new S(iterable);
	} catch (e) {
	    if (e instanceof TypeError) {
	        thrown = true;
	    } else {
	        throw e;
	    }
	}

	thrown && getterCalled === 1 && getIteratorCalled === 0;
	"#;
    test_script(SCRIPT, Value::Bool(true));
}
