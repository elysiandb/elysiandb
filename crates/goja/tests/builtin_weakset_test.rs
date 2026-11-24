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
