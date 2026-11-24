mod jsrun;

use goja::host::{DocMap, DocValue};
use goja::{Runtime, Value};
use jsrun::{new_runtime, on_big_stack, rt_test_script_with_test_lib, run_error_string};

fn doc_map(entries: &[(&str, DocValue)]) -> DocValue {
    let mut m = DocMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v.clone());
    }
    DocValue::new_map(m)
}

fn run(rt: &mut Runtime, src: &str) -> Value {
    match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

#[test]
fn gomap_prop() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = doc_map(&[("a", DocValue::Int64(40)), ("b", DocValue::Int64(2))]);
        rt.set_value("o", &o).expect("set");
        let v = run(&mut rt, "o.a + o.b;");
        assert_eq!(v.to_integer(&mut rt).expect("int"), 42);
    });
}

#[test]
fn gomap_enumerate() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = doc_map(&[("x", DocValue::Int64(40)), ("y", DocValue::Int64(2))]);
        rt.set_value("o", &o).expect("set");
        let v = run(
            &mut rt,
            r#"
	var hasX = false;
	var hasY = false;
	for (var key in o) {
		switch (key) {
		case "x":
			if (hasX) {
				throw "Already have x";
			}
			hasX = true;
			break;
		case "y":
			if (hasY) {
				throw "Already have y";
			}
			hasY = true;
			break;
		default:
			throw "Unexpected property: " + key;
		}
	}
	hasX && hasY;
	"#,
        );
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn gomap_delete_while_enumerate() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = doc_map(&[("x", DocValue::Int64(40)), ("y", DocValue::Int64(2))]);
        rt.set_value("o", &o).expect("set");
        let v = run(
            &mut rt,
            r#"
	var hasX = false;
	var hasY = false;
	for (var key in o) {
		switch (key) {
		case "x":
			if (hasX) {
				throw "Already have x";
			}
			hasX = true;
			delete o.y;
			break;
		case "y":
			if (hasY) {
				throw "Already have y";
			}
			hasY = true;
			delete o.x;
			break;
		default:
			throw "Unexpected property: " + key;
		}
	}
	hasX && !hasY || hasY && !hasX;
	"#,
        );
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn gomap_instance_of() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("o", &doc_map(&[])).expect("set");
        let v = run(&mut rt, "(o instanceof Object) && !(o instanceof Error);");
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn gomap_type_of() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("o", &doc_map(&[])).expect("set");
        let v = run(&mut rt, "typeof o;");
        assert_eq!(v.go_string(&mut rt).expect("s"), "object");
    });
}

#[test]
fn gomap_proto() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("o", &doc_map(&[("test", DocValue::Int64(42))]))
            .expect("set");
        let v = run(&mut rt, r#"o.hasOwnProperty("test");"#);
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn go_map_extensibility() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("o", &doc_map(&[])).expect("set");
        let v = run(
            &mut rt,
            r#"
	"use strict";
	o.test = 42;
	Object.preventExtensions(o);
	o.test = 43;
	try {
		o.test1 = 42;
	} catch (e) {
		if (!(e instanceof TypeError)) {
			throw e;
		}
	}
	o.test === 43 && o.test1 === undefined;
	"#,
        );
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn go_map_with_proto() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let m = doc_map(&[("t", DocValue::string("42"))]);
        rt.set_value("m", &m).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	(function() {
	'use strict';
	var proto = {};
	var getterAllowed = false;
	var setterAllowed = false;
	var tHolder = "proto t";
	Object.defineProperty(proto, "t", {
		get: function() {
			if (!getterAllowed) throw new Error("getter is called");
			return tHolder;
		},
		set: function(v) {
			if (!setterAllowed) throw new Error("setter is called");
			tHolder = v;
		}
	});
	var t1Holder;
	Object.defineProperty(proto, "t1", {
		get: function() {
			return t1Holder;
		},
		set: function(v) {
			t1Holder = v;
		}
	});
	Object.setPrototypeOf(m, proto);
	assert.sameValue(m.t, "42");
	m.t = 43;
	assert.sameValue(m.t, 43);
	t1Holder = "test";
	assert.sameValue(m.t1, "test");
	m.t1 = "test1";
	assert.sameValue(m.t1, "test1");
	delete m.t;
	getterAllowed = true;
	assert.sameValue(m.t, "proto t", "after delete");
	setterAllowed = true;
	m.t = true;
	assert.sameValue(m.t, true);
	assert.sameValue(tHolder, true);
	Object.preventExtensions(m);
	assert.throws(TypeError, function() {
		m.t2 = 1;
	});
	m.t1 = "test2";
	assert.sameValue(m.t1, "test2");
	})();
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn go_map_proto_prop() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("m", &doc_map(&[])).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	(function() {
	"use strict";
	var proto = {};
	Object.defineProperty(proto, "ro", {value: 42});
	Object.setPrototypeOf(m, proto);
	assert.throws(TypeError, function() {
		m.ro = 43;
	});
	Object.defineProperty(m, "ro", {value: 43});
	assert.sameValue(m.ro, 43);
	})();
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn go_map_proto_prop_chain() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("m", &doc_map(&[])).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	(function() {
	"use strict";
	var p1 = Object.create(null);
	m.__proto__ = p1;

	Object.defineProperty(p1, "test", {
		value: 42
	});

	Object.defineProperty(m, "test", {
		value: 43,
		writable: true,
	});
	var o = Object.create(m);
	o.test = 44;
	assert.sameValue(o.test, 44);

	var sym = Symbol(true);
	Object.defineProperty(p1, sym, {
		value: 42
	});

	Object.defineProperty(m, sym, {
		value: 43,
		writable: true,
	});
	o[sym] = 44;
	assert.sameValue(o[sym], 44);
	})();
	"#,
            &Value::Undefined,
        );
    });
}
