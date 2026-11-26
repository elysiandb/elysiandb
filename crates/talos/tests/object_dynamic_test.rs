mod jsrun;

use jsrun::{
    new_runtime, on_big_stack, rt_test_script_with_test_lib, rt_test_script_with_test_lib_x,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use talos::heap::Tracer;
use talos::host::{DocValue, DynamicArray, DynamicObject};
use talos::{JsResult, Runtime, Value};

#[derive(Default)]
struct TestDynObject {
    m: RefCell<BTreeMap<String, Value>>,
}

impl DynamicObject for TestDynObject {
    fn get(&self, _rt: &mut Runtime, key: &str) -> JsResult<Option<Value>> {
        Ok(self.m.borrow().get(key).cloned())
    }

    fn set(&self, _rt: &mut Runtime, key: &str, val: Value) -> JsResult<bool> {
        self.m.borrow_mut().insert(key.to_string(), val);
        Ok(true)
    }

    fn has(&self, _rt: &mut Runtime, key: &str) -> JsResult<bool> {
        Ok(self.m.borrow().contains_key(key))
    }

    fn delete(&self, _rt: &mut Runtime, key: &str) -> JsResult<bool> {
        self.m.borrow_mut().remove(key);
        Ok(true)
    }

    fn keys(&self, _rt: &mut Runtime) -> JsResult<Vec<String>> {
        Ok(self.m.borrow().keys().cloned().collect())
    }

    fn export(self: Rc<Self>) -> DocValue {
        DocValue::Nil
    }

    fn trace(&self, t: &mut Tracer) {
        for v in self.m.borrow().values() {
            t.value(v);
        }
    }
}

#[derive(Default)]
struct TestDynArray {
    a: RefCell<Vec<Option<Value>>>,
}

impl TestDynArray {
    fn expand(a: &mut Vec<Option<Value>>, new_len: usize) {
        a.resize(new_len, None);
    }
}

impl DynamicArray for TestDynArray {
    fn length(&self, _rt: &mut Runtime) -> JsResult<i64> {
        Ok(self.a.borrow().len() as i64)
    }

    fn get(&self, _rt: &mut Runtime, idx: i64) -> JsResult<Option<Value>> {
        let a = self.a.borrow();
        let mut idx = idx;
        if idx < 0 {
            idx += a.len() as i64;
        }
        if idx >= 0 && (idx as usize) < a.len() {
            return Ok(a[idx as usize].clone());
        }
        Ok(None)
    }

    fn set(&self, _rt: &mut Runtime, idx: i64, val: Value) -> JsResult<bool> {
        let mut a = self.a.borrow_mut();
        let mut idx = idx;
        if idx < 0 {
            idx += a.len() as i64;
        }
        if idx < 0 {
            return Ok(false);
        }
        if idx as usize >= a.len() {
            TestDynArray::expand(&mut a, idx as usize + 1);
        }
        a[idx as usize] = Some(val);
        Ok(true)
    }

    fn set_length(&self, _rt: &mut Runtime, i: i64) -> JsResult<bool> {
        let mut a = self.a.borrow_mut();
        if i > a.len() as i64 {
            TestDynArray::expand(&mut a, i as usize);
            return Ok(true);
        }
        if i < 0 {
            return Ok(false);
        }
        a.truncate(i as usize);
        Ok(true)
    }

    fn export(self: Rc<Self>) -> DocValue {
        DocValue::Nil
    }

    fn trace(&self, t: &mut Tracer) {
        for v in self.a.borrow().iter().flatten() {
            t.value(v);
        }
    }
}

#[test]
#[ignore = "deepEqual needs Reflect.getPrototypeOf, ported by the Reflect batch"]
fn dynamic_object() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = rt.new_dynamic_object(Rc::new(TestDynObject::default()));
        rt.set("o", Value::Object(o)).expect("set");
        rt_test_script_with_test_lib_x(
            &mut rt,
            r#"
	assert(o instanceof Object, "instanceof Object");
	assert(o === o, "self equality");
	assert(o !== {}, "non-equality");

	o.test = 42;
	assert("test" in o, "'test' in o");
	assert(deepEqual(Object.getOwnPropertyDescriptor(o, "test"), {value: 42, writable: true, enumerable: true, configurable: true}), "prop desc");

	assert.throws(TypeError, function() {
		"use strict";
		Object.defineProperty(o, "test1", {value: 0, writable: false, enumerable: false, configurable: true});
	}, "define prop");

	var keys = [];
	for (var key in o) {
		keys.push(key);
	}
	assert(compareArray(keys, ["test"]), "for-in");

	assert(delete o.test, "delete");
	assert(!("test" in o), "'test' in o after delete");

	assert("__proto__" in o, "__proto__ in o");
	assert.sameValue(o.__proto__, Object.prototype, "__proto__");
	o.__proto__ = null;
	assert(!("__proto__" in o), "__proto__ in o after setting to null");
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn dynamic_object_custom_proto() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let d = Rc::new(TestDynObject::default());
        let o = rt.new_dynamic_object(d.clone());
        rt.set("o", Value::Object(o)).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	var proto = {
		valueOf: function() {
			return this.num;
		}
	};
	proto[Symbol.toStringTag] = "GoObject";
	Object.setPrototypeOf(o, proto);
	o.num = 41;
	assert(o instanceof Object, "instanceof");
	assert.sameValue(o+1, 42);
	assert.sameValue(o.toString(), "[object GoObject]");
	"#,
            &Value::Undefined,
        );
        let v = d.m.borrow().get("num").cloned().expect("num");
        match &rt.export(&v).expect("export") {
            DocValue::Int64(41) => {}
            _ => panic!("num is not 41"),
        }
    });
}

#[test]
#[ignore = "deepEqual and Reflect.ownKeys need the Reflect builtins, ported by the Reflect batch"]
fn dynamic_array() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = rt.new_dynamic_array(Rc::new(TestDynArray::default()));
        rt.set("a", Value::Object(a)).expect("set");
        rt_test_script_with_test_lib_x(
            &mut rt,
            r#"
	assert(a instanceof Array, "instanceof Array");
	assert(a instanceof Object, "instanceof Object");
	assert(a === a, "self equality");
	assert(a !== [], "non-equality");
	assert(Array.isArray(a), "isArray()");
	assert("length" in a, "length in a");
	assert.sameValue(a.length, 0, "len == 0");
	assert.sameValue(a[0], undefined, "a[0] (1)");

	a[0] = 0;
	assert.sameValue(a[0], 0, "a[0] (2)");
	assert.sameValue(a.length, 1, "length");
	assert(deepEqual(Object.getOwnPropertyDescriptor(a, 0), {value: 0, writable: true, enumerable: true, configurable: true}), "prop desc");
	assert(deepEqual(Object.getOwnPropertyDescriptor(a, "length"), {value: 1, writable: true, enumerable: false, configurable: false}), "length prop desc");

	assert("__proto__" in a, "__proto__ in a");
	assert.sameValue(a.__proto__, Array.prototype, "__proto__");

	assert(compareArray(Object.keys(a), ["0"]), "Object.keys()");
	assert(compareArray(Reflect.ownKeys(a), ["0", "length"]), "Reflect.ownKeys()");

	a.length = 2;
	assert.sameValue(a.length, 2, "length after grow");
	assert.sameValue(a[1], undefined, "a[1]");

	a[1] = 1;
	assert.sameValue(a[1], 1, "a[1] after set");
	a.length = 1;
	assert.sameValue(a.length, 1, "length after shrink");
	assert.sameValue(a[1], undefined, "a[1] after shrink");
	a.length = 2;
	assert.sameValue(a.length, 2, "length after shrink and grow");
	assert.sameValue(a[1], undefined, "a[1] after grow");

	a[0] = 3; a[1] = 1; a[2] = 2;
	assert.sameValue(a.length, 3);
	var keys = [];
	for (var key in a) {
		keys.push(key);
	}
	assert(compareArray(keys, ["0","1","2"]), "for-in");

	var vals = [];
	for (var val of a) {
		vals.push(val);
	}
	assert(compareArray(vals, [3,1,2]), "for-of");

	a.sort();
	assert(compareArray(a, [1,2,3]), "sort: "+a);

	assert.sameValue(a[-1], 3);
	assert.sameValue(a[-4], undefined);

	assert.throws(TypeError, function() {
		"use strict";
		delete a.length;
	}, "delete length");

	assert.throws(TypeError, function() {
		"use strict";
		a.test = true;
	}, "set string prop");

	assert.throws(TypeError, function() {
		"use strict";
		Object.defineProperty(a, 0, {value: 0, writable: false, enumerable: false, configurable: true});
	}, "define prop");

	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn dynamic_object_without_reflect() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = rt.new_dynamic_object(Rc::new(TestDynObject::default()));
        rt.set("o", Value::Object(o)).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	assert(o instanceof Object, "instanceof Object");
	assert(o === o, "self equality");
	o.test = 42;
	assert("test" in o, "'test' in o");
	var d = Object.getOwnPropertyDescriptor(o, "test");
	assert(d.value === 42 && d.writable && d.enumerable && d.configurable, "prop desc");
	assert.throws(TypeError, function() {
		"use strict";
		Object.defineProperty(o, "test1", {value: 0, writable: false, enumerable: false, configurable: true});
	}, "define prop");
	var keys = [];
	for (var key in o) {
		keys.push(key);
	}
	assert(compareArray(keys, ["test"]), "for-in");
	assert(delete o.test, "delete");
	assert(!("test" in o), "'test' in o after delete");
	assert.sameValue(o.__proto__, Object.prototype, "__proto__");
	o.__proto__ = null;
	assert(!("__proto__" in o), "__proto__ in o after setting to null");
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn dynamic_array_without_reflect() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = rt.new_dynamic_array(Rc::new(TestDynArray::default()));
        rt.set("a", Value::Object(a)).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	assert(Array.isArray(a), "isArray()");
	assert.sameValue(a.length, 0, "len == 0");
	a[0] = 0;
	assert.sameValue(a.length, 1, "length");
	var d = Object.getOwnPropertyDescriptor(a, "length");
	assert(d.value === 1 && d.writable && !d.enumerable && !d.configurable, "length prop desc");
	assert(compareArray(Object.keys(a), ["0"]), "Object.keys()");
	assert(compareArray(Object.getOwnPropertyNames(a), ["0", "length"]), "names");
	a.length = 2;
	a[1] = 1;
	a.length = 1;
	assert.sameValue(a[1], undefined, "a[1] after shrink");
	a[0] = 3; a[1] = 1; a[2] = 2;
	var vals = [];
	for (var val of a) {
		vals.push(val);
	}
	assert(compareArray(vals, [3,1,2]), "for-of");
	a.sort();
	assert(compareArray(a, [1,2,3]), "sort: "+a);
	assert.sameValue(a[-1], 3);
	assert.throws(TypeError, function() {
		"use strict";
		delete a.length;
	}, "delete length");
	assert.throws(TypeError, function() {
		"use strict";
		a.test = true;
	}, "set string prop");
	"#,
            &Value::Undefined,
        );
    });
}
