mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_set_evil_iterator() {
    const SCRIPT: &str = r#"
	var o = {};
	o[Symbol.iterator] = function() {
		return {
			next: function() {
				if (!this.flag) {
					this.flag = true;
					return {};
				}
				return {done: true};
			}
		}
	}
	new Set(o);
	undefined;
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_set_get_adder_get_iterator_order() {
    const SCRIPT: &str = r#"
	let getterCalled = 0;

	class S extends Set {
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
