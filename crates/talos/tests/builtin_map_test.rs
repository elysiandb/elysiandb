mod jsrun;

use jsrun::*;
use talos::{Runtime, Value};

#[test]
fn test_map_evil_iterator() {
    const SCRIPT: &str = r#"
	'use strict';
	var o = {};

	function Iter(value) {
		this.value = value;
		this.idx = 0;
	}

	Iter.prototype.next = function() {
		var idx = this.idx;
		if (idx === 0) {
			this.idx++;
			return this.value;
		}
		return {done: true};
	}

	o[Symbol.iterator] = function() {
		return new Iter({});
	}

	assert.throws(TypeError, function() {
		new Map(o);
	});

	o[Symbol.iterator] = function() {
		return new Iter({value: []});
	}

	function t(prefix) {
		var m = new Map(o);
		assert.sameValue(1, m.size, prefix+": m.size");
		assert.sameValue(true, m.has(undefined), prefix+": m.has(undefined)");
		assert.sameValue(undefined, m.get(undefined), prefix+": m.get(undefined)");
	}

	t("standard adder");

	var count = 0;
	var origSet = Map.prototype.set;

	Map.prototype.set = function() {
		count++;
		origSet.apply(this, arguments);
	}

	t("custom adder");
	assert.sameValue(1, count, "count");

	undefined;
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_map_get_adder_get_iterator_order() {
    const SCRIPT: &str = r#"
	let getterCalled = 0;

	class M extends Map {
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

#[test]
fn pending_error_survives_the_iterator_close() {
    const SCRIPT: &str = r#"
var iterable = {};
iterable[Symbol.iterator] = function () {
  return {
    next: function () { return { value: [], done: false }; },
    return: function () { throw new TypeError("ignored"); },
  };
};
Map.prototype.set = function () { throw new RangeError("kept"); };
var seen;
try {
  new Map(iterable);
} catch (e) {
  seen = typeof e + ":" + e.name + ":" + e.message;
}
seen;
"#;
    on_big_stack(|| {
        let mut rt = Runtime::new();
        rt.set_gc_stress(true);
        let v = rt.run_string(SCRIPT).expect("run");
        assert_eq!(
            v.go_string(&mut rt).expect("string"),
            "object:RangeError:kept"
        );
    });
}
