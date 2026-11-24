mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_weak_map() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let r = vm.run_string(
            r#"
	var m = new WeakMap();
	var m1 = new WeakMap();
	var key = {};
	m.set(key, true);
	m1.set(key, false);
	if (!m.has(key)) {
		throw new Error("has");
	}
	if (m.get(key) !== true) {
		throw new Error("value does not match");
	}
	if (!m1.has(key)) {
		throw new Error("has (m1)");
	}
	if (m1.get(key) !== false) {
		throw new Error("m1 value does not match");
	}
	m.delete(key);
	if (m.has(key)) {
		throw new Error("m still has after delete");
	}
	if (!m1.has(key)) {
		throw new Error("m1 does not have after delete from m");
	}
	"#,
        );
        if let Err(e) = r {
            panic!("{e}");
        }
    });
}

#[test]
fn test_weak_map_get_adder_get_iterator_order() {
    const SCRIPT: &str = r#"
	let getterCalled = 0;

	class M extends WeakMap {
	    get set() {
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
	    new M(iterable);
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
