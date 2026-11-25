mod jsrun;

use goja::builtin_proxy::ProxyTrapConfig;
use goja::builtin_symbol::SYM_ITERATOR;
use goja::func::NativeFn;
use goja::host::{DocValue, HandleKind, JsHandle};
use goja::object::{Flag, PropertyDescriptor};
use goja::runtime::Throw;
use goja::string::{ascii_string, new_string_value};
use goja::{ObjRef, Runtime, Value};
use jsrun::*;
use std::cell::Cell;
use std::rc::Rc;

fn s(v: &str) -> Value {
    Value::String(new_string_value(v.as_bytes()))
}

fn set_global(rt: &mut Runtime, name: &str, v: Value) {
    rt.set(name, v).expect("set");
}

fn run(rt: &mut Runtime, src: &str) -> Value {
    match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

fn get(rt: &mut Runtime, o: ObjRef, name: &str) -> Value {
    o.get_str(rt, &name.into(), None)
        .expect("get")
        .unwrap_or(Value::Undefined)
}

fn type_error<T>(rt: &mut Runtime, msg: &str) -> Result<T, Throw> {
    Err(Throw::Value(rt.new_type_error(msg)))
}

#[test]
fn test_proxy_object_target_get_prototype_of() {
    test_script_with_test_lib(
        r#"
    var proto = {};
	var obj = Object.create(proto);
	var proxy = new Proxy(obj, {});
	var p = Object.getPrototypeOf(proxy);
	assert.sameValue(proto, p);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_object_proxy_get_prototype_of() {
    test_script_with_test_lib(
        r#"
    var proto = {};
	var proto2 = {};
	var obj = Object.create(proto);
	var proxy = new Proxy(obj, {
		getPrototypeOf: function(target) {
			return proto2;
		}
	});
	var p = Object.getPrototypeOf(proxy);
	assert.sameValue(proto2, p);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_object_native_proxy_get_prototype_of() {
    on_big_stack(|| {
        let mut runtime = new_runtime();
        let prototype = runtime.new_object();
        set_global(&mut runtime, "proto", Value::Object(prototype));
        let target = runtime.new_object();
        let proxy = runtime
            .new_proxy(
                target,
                ProxyTrapConfig {
                    data: vec![Value::Object(prototype)],
                    get_prototype_of: Some(Box::new(move |_r, _t| Ok(Some(prototype)))),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut runtime, "proxy", proxy.to_value());
        rt_test_script_with_test_lib(
            &mut runtime,
            r#"
	var p = Object.getPrototypeOf(proxy);
	assert.sameValue(proto, p);
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn test_proxy_object_target_set_prototype_of() {
    test_script_with_test_lib(
        r#"
    var proto = {};
	var obj = {};
	Object.setPrototypeOf(obj, proto);
	var proxy = new Proxy(obj, {});
	var p = Object.getPrototypeOf(proxy);
	assert.sameValue(proto, p);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_object_proxy_set_prototype_of() {
    test_script_with_test_lib(
        r#"
    var proto = {};
	var proto2 = {};
	var obj = {};
	Object.setPrototypeOf(obj, proto);
	var proxy = new Proxy(obj, {
		setPrototypeOf: function(target, prototype) {
			return Object.setPrototypeOf(target, proto2);
		}
	});
	Object.setPrototypeOf(proxy, null);
	var p = Object.getPrototypeOf(proxy);
	assert.sameValue(proto2, p);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_object_target_is_extensible() {
    test_script(
        r#"
	var obj = {};
	Object.seal(obj);
	var proxy = new Proxy(obj, {});
	Object.isExtensible(proxy);
	"#,
        Value::Bool(false),
    );
}

#[test]
fn test_proxy_proxy_is_extensible() {
    test_script(
        r#"
	var obj = {};
	Object.seal(obj);
	var proxy = new Proxy(obj, {
		isExtensible: function(target) {
			return false;
		}
	});
	Object.isExtensible(proxy);
	"#,
        Value::Bool(false),
    );
}

#[test]
fn test_proxy_native_proxy_is_extensible() {
    on_big_stack(|| {
        let mut runtime = new_runtime();
        let target = runtime.new_object();
        set_global(&mut runtime, "target", Value::Object(target));
        let proxy = runtime
            .new_proxy(
                target,
                ProxyTrapConfig {
                    is_extensible: Some(Box::new(|_r, _t| Ok(false))),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut runtime, "proxy", proxy.to_value());
        let val = run(
            &mut runtime,
            r#"
	(function() {
		Object.preventExtensions(target);
		return Object.isExtensible(proxy);
	})();
	"#,
        );
        assert!(!val.to_boolean());
    });
}

#[test]
fn test_proxy_object_target_prevent_extensions() {
    test_script(
        r#"
	var obj = {
		canEvolve: true
	};
	var proxy = new Proxy(obj, {});
	Object.preventExtensions(proxy);
	proxy.canEvolve
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_proxy_proxy_prevent_extensions() {
    test_script(
        r#"
	var obj = {
		canEvolve: true
	};
	var proxy = new Proxy(obj, {
		preventExtensions: function(target) {
			target.canEvolve = false;
			Object.preventExtensions(obj);
			return true;
		}
	});
	Object.preventExtensions(proxy);
	proxy.canEvolve;
	"#,
        Value::Bool(false),
    );
}

#[test]
fn test_proxy_native_proxy_prevent_extensions() {
    on_big_stack(|| {
        let mut runtime = new_runtime();
        let target = runtime.new_object();
        target
            .set_own_str(&mut runtime, &"canEvolve".into(), Value::Bool(true), true)
            .expect("set");
        set_global(&mut runtime, "target", Value::Object(target));
        let proxy = runtime
            .new_proxy(
                target,
                ProxyTrapConfig {
                    prevent_extensions: Some(Box::new(|r, target| {
                        target.set_own_str(r, &"canEvolve".into(), Value::Bool(false), true)?;
                        if let Err(e) = r.run_string("Object.preventExtensions(target)") {
                            panic!("{}", run_error_string(&e));
                        }
                        Ok(true)
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut runtime, "proxy", proxy.to_value());
        let val = run(
            &mut runtime,
            r#"
	(function() {
		Object.preventExtensions(proxy);
		return proxy.canEvolve;
	})();
	"#,
        );
        assert!(!val.to_boolean());
    });
}

#[test]
fn test_proxy_object_target_get_own_property_descriptor() {
    test_script(
        r#"
	var desc = {
		configurable: false,
		enumerable: false,
		value: 42,
		writable: false
	};

	var obj = {};
	Object.defineProperty(obj, "foo", desc);

	var proxy = new Proxy(obj, {});

	var desc2 = Object.getOwnPropertyDescriptor(proxy, "foo");
	desc2.value
	"#,
        int(42),
    );
}

#[test]
fn test_proxy_proxy_get_own_property_descriptor() {
    test_script_with_test_lib(
        r#"
	var desc = {
		configurable: false,
		enumerable: false,
		value: 42,
		writable: false
	};
	var proxy_desc = {
		configurable: false,
		enumerable: false,
		value: 24,
		writable: false
	};

	var obj = {};
	Object.defineProperty(obj, "foo", desc);

	var proxy = new Proxy(obj, {
		getOwnPropertyDescriptor: function(target, property) {
			return proxy_desc;
		}
	});

	assert.throws(TypeError, function() {
		Object.getOwnPropertyDescriptor(proxy, "foo");
	});
	undefined;
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_native_proxy_get_own_property_descriptor() {
    on_big_stack(|| {
        let mut runtime = new_runtime();
        let val = run(
            &mut runtime,
            r#"
	(function() {
		var desc = {
			configurable: true,
			enumerable: false,
			value: 42,
			writable: false
		};
		var proxy_desc = {
			configurable: true,
			enumerable: false,
			value: 24,
			writable: false
		};

		var obj = {};
		Object.defineProperty(obj, "foo", desc);

		return function(constructor) {
			var proxy = constructor(obj, proxy_desc);

			var desc2 = Object.getOwnPropertyDescriptor(proxy, "foo");
			return desc2.value
		}
	})();
	"#,
        );
        let f = val.as_object().expect("function");
        let c = f.assert_callable(&runtime).expect("not a function");
        let constructor = runtime.new_native_func(
            NativeFn::closure_with(vec![], |r, _d, call| {
                let Value::Object(target) = call.argument(0) else {
                    panic!("target");
                };
                let Value::Object(proxy_desc) = call.argument(1) else {
                    panic!("proxy desc");
                };
                let p = r.new_proxy(
                    target,
                    ProxyTrapConfig {
                        data: vec![Value::Object(proxy_desc)],
                        get_own_property_descriptor: Some(Box::new(move |r, _t, _prop| {
                            r.to_property_descriptor(&Value::Object(proxy_desc))
                        })),
                        ..ProxyTrapConfig::default()
                    },
                )?;
                Ok(p.to_value())
            }),
            "",
            0,
        );
        let v = c
            .call(&mut runtime, val.clone(), &[Value::Object(constructor)])
            .expect("call");
        let i = v.to_integer(&mut runtime).expect("integer");
        assert_eq!(i, 24, "val: {i}");
    });
}

#[test]
fn test_proxy_native_proxy_get_own_property_descriptor_idx() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let a = vm.new_array_object();
        let proxy1 = vm
            .new_proxy(
                a,
                ProxyTrapConfig {
                    get_own_property_descriptor: Some(Box::new(|r, _t, prop| {
                        type_error(
                            r,
                            &format!("GetOwnPropertyDescriptor was called for {prop:?}"),
                        )
                    })),
                    get_own_property_descriptor_idx: Some(Box::new(|_r, _t, prop| {
                        if (-1..=1).contains(&prop) {
                            return Ok(PropertyDescriptor {
                                value: Some(int(prop)),
                                configurable: Flag::True,
                                ..PropertyDescriptor::default()
                            });
                        }
                        Ok(PropertyDescriptor::default())
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy1");
        let proxy2 = vm
            .new_proxy(
                a,
                ProxyTrapConfig {
                    get_own_property_descriptor: Some(Box::new(|_r, _t, prop| match prop {
                        "-1" | "0" | "1" => Ok(PropertyDescriptor {
                            value: Some(s(prop)),
                            configurable: Flag::True,
                            ..PropertyDescriptor::default()
                        }),
                        _ => Ok(PropertyDescriptor::default()),
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy2");
        let proxy3 = vm
            .new_proxy(
                a,
                ProxyTrapConfig {
                    get_own_property_descriptor: Some(Box::new(|_r, _t, prop| {
                        Ok(PropertyDescriptor {
                            value: Some(s(prop)),
                            configurable: Flag::True,
                            ..PropertyDescriptor::default()
                        })
                    })),
                    get_own_property_descriptor_idx: Some(Box::new(|r, _t, prop| {
                        type_error(
                            r,
                            &format!("GetOwnPropertyDescriptorIdx was called for {prop}"),
                        )
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy3");
        set_global(&mut vm, "proxy1", proxy1.to_value());
        set_global(&mut vm, "proxy2", proxy2.to_value());
        set_global(&mut vm, "proxy3", proxy3.to_value());
        rt_test_script_with_test_lib_x(
            &mut vm,
            r#"
	var desc;
	for (var i = -1; i <= 1; i++) {
		desc = Object.getOwnPropertyDescriptor(proxy1, i);
		assert(deepEqual(desc, {value: i, writable: false, enumerable: false, configurable: true}), "1. int "+i);

		desc = Object.getOwnPropertyDescriptor(proxy1, ""+i);
		assert(deepEqual(desc, {value: i, writable: false, enumerable: false, configurable: true}), "1. str "+i);

		desc = Object.getOwnPropertyDescriptor(proxy2, i);
		assert(deepEqual(desc, {value: ""+i, writable: false, enumerable: false, configurable: true}), "2. int "+i);

		desc = Object.getOwnPropertyDescriptor(proxy2, ""+i);
		assert(deepEqual(desc, {value: ""+i, writable: false, enumerable: false, configurable: true}), "2. str "+i);
	}

	for (const prop of ["00", " 0", "-0", "01"]) {
		desc = Object.getOwnPropertyDescriptor(proxy3, prop);
		assert(deepEqual(desc, {value: prop, writable: false, enumerable: false, configurable: true}), "3. "+prop);
	}
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn test_proxy_native_proxy_get_own_property_descriptor_sym() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let o = vm.new_object();
        let sym = vm.new_symbol(Some(ascii_string("42")));
        set_global(&mut vm, "sym", Value::Symbol(sym));
        let proxy = vm
            .new_proxy(
                o,
                ProxyTrapConfig {
                    data: vec![Value::Object(o), Value::Symbol(sym)],
                    get_own_property_descriptor_sym: Some(Box::new(move |r, target, s| {
                        if target != o {
                            return type_error(r, "Invalid target");
                        }
                        if s == sym {
                            return Ok(PropertyDescriptor {
                                value: Some(self::s("passed")),
                                writable: Flag::True,
                                configurable: Flag::True,
                                ..PropertyDescriptor::default()
                            });
                        }
                        Ok(PropertyDescriptor::default())
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut vm, "proxy", proxy.to_value());
        rt_test_script_with_test_lib_x(
            &mut vm,
            r#"
	var desc = Object.getOwnPropertyDescriptor(proxy, sym);
	assert(deepEqual(desc, {value: "passed", writable: true, enumerable: false, configurable: true}));
	assert.sameValue(Object.getOwnPropertyDescriptor(proxy, Symbol.iterator), undefined);
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn test_proxy_native_proxy_get_own_property_descriptor_non_existing() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let o = vm.new_object();
        let proxy = vm
            .new_proxy(
                o,
                ProxyTrapConfig {
                    get_own_property_descriptor: Some(Box::new(|_r, _t, _p| {
                        Ok(PropertyDescriptor::default())
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut vm, "proxy", proxy.to_value());
        let res = run(
            &mut vm,
            r#"Object.getOwnPropertyDescriptor(proxy, "foo") === undefined"#,
        );
        assert!(matches!(res, Value::Bool(true)), "{res:?}");
    });
}

#[test]
fn test_proxy_object_target_define_property() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {});
	Object.defineProperty(proxy, "foo", {
		value: "test123"
	});
	proxy.foo;
	"#,
        str_value("test123"),
    );
}

#[test]
fn test_proxy_proxy_define_property() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {
		defineProperty: function(target, prop, descriptor) {
			target.foo = "321tset";
			return true;
		}
	});
	Object.defineProperty(proxy, "foo", {
		value: "test123"
	});
	proxy.foo;
	"#,
        str_value("321tset"),
    );
}

#[test]
fn test_proxy_native_proxy_define_property() {
    on_big_stack(|| {
        let mut runtime = new_runtime();
        let target = runtime.new_object();
        let proxy = runtime
            .new_proxy(
                target,
                ProxyTrapConfig {
                    define_property: Some(Box::new(|r, target, key, desc| {
                        let v = desc.value.unwrap_or_default().go_string(r)?;
                        target.set_own_str(r, &key.into(), s(&format!("{v}-passed-str")), true)?;
                        Ok(true)
                    })),
                    define_property_idx: Some(Box::new(|r, target, key, desc| {
                        let v = desc.value.unwrap_or_default().go_string(r)?;
                        target.set_own_str(
                            r,
                            &key.to_string().as_str().into(),
                            s(&format!("{v}-passed-idx")),
                            true,
                        )?;
                        Ok(true)
                    })),
                    define_property_sym: Some(Box::new(|r, target, key, desc| {
                        let v = desc.value.unwrap_or_default().go_string(r)?;
                        target.set_own_sym(r, key, s(&format!("{v}-passed-sym")), true)?;
                        Ok(true)
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut runtime, "proxy", proxy.to_value());
        rt_test_script_with_test_lib(
            &mut runtime,
            r#"
	Object.defineProperty(proxy, "foo", {
		value: "teststr"
	});
	Object.defineProperty(proxy, "0", {
		value: "testidx"
	});
	Object.defineProperty(proxy, Symbol.toStringTag, {
		value: "testsym"
	});
	assert.sameValue(proxy.foo, "teststr-passed-str");
	assert.sameValue(proxy[0], "testidx-passed-idx");
	assert.sameValue(proxy[Symbol.toStringTag], "testsym-passed-sym");
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn test_proxy_target_has_in() {
    test_script(
        r#"
	var obj = {
		secret: true
	};
	var proxy = new Proxy(obj, {});

	"secret" in proxy
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_proxy_proxy_has_in() {
    test_script(
        r#"
	var obj = {
		secret: true
	};
	var proxy = new Proxy(obj, {
		has: function(target, key) {
			return key !== "secret";
		}
	});

	"secret" in proxy
	"#,
        Value::Bool(false),
    );
}

#[test]
fn test_proxy_target_has_with() {
    test_script(
        r#"
	var obj = {
		secret: true
	};
	var proxy = new Proxy(obj, {});

	with(proxy) {
		(secret);
	}
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_proxy_proxy_has_with() {
    test_script(
        r#"
	var obj = {
		secret: true
	};
	var proxy = new Proxy(obj, {
		has: function(target, key) {
			return key !== "secret";
		}
	});

	var thrown = false;
	try {
		with(proxy) {
			(secret);
		}
	} catch (e) {
		if (e instanceof ReferenceError) {
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
fn test_proxy_target_get() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {});
	Object.defineProperty(proxy, "foo", {
		value: "test123"
	});
	proxy.foo;
	"#,
        str_value("test123"),
    );
}

#[test]
fn test_proxy_proxy_get() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {
		get: function(target, prop, receiver) {
			return "321tset"
		}
	});
	Object.defineProperty(proxy, "foo", {
		value: "test123",
		configurable: true,
	});
	proxy.foo;
	"#,
        str_value("321tset"),
    );
}

#[test]
fn test_proxy_proxy_get_json_stringify() {
    test_script_with_test_lib(
        r#"
	var obj = {};
	var propValue = "321tset";
	var _handler, _target, _prop, _receiver;
	var proxy = new Proxy(obj, {
		ownKeys: function() {
			return ["foo"];
		},
		getOwnPropertyDescriptor: function(target, prop) {
			if (prop === "foo") {
				return {
					value: propValue,
					enumerable: true,
					configurable: true
				}
			}
		},
		get: function(target, prop, receiver) {
			if (prop === "foo") {
				_prop = prop;
				_receiver = receiver;
				return propValue;
			}
			return obj[prop];
		}
	});
	var res = JSON.stringify(proxy);
	assert.sameValue(res, '{"foo":"321tset"}');
	assert.sameValue(_prop, "foo");
	assert.sameValue(_receiver, proxy);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_native_proxy_get() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let prop_value_str = s("321tset");
        let prop_value_idx = s("idx");
        let prop_value_sym = s("sym");
        let sym = vm.new_symbol(Some(ascii_string("test")));
        let obj = vm.new_object();
        let (pvs, pvs2, pvi, pvi2, pvy) = (
            prop_value_str.clone(),
            prop_value_str.clone(),
            prop_value_idx.clone(),
            prop_value_idx.clone(),
            prop_value_sym.clone(),
        );
        let proxy = vm
            .new_proxy(
                obj,
                ProxyTrapConfig {
                    data: vec![Value::Object(obj), Value::Symbol(sym)],
                    own_keys: Some(Box::new(|r, _t| {
                        Ok(r.new_array_values(vec![s("0"), s("foo")]))
                    })),
                    get_own_property_descriptor: Some(Box::new(move |r, _t, prop| {
                        if prop == "foo" {
                            return Ok(PropertyDescriptor {
                                value: Some(pvs.clone()),
                                enumerable: Flag::True,
                                configurable: Flag::True,
                                ..PropertyDescriptor::default()
                            });
                        }
                        if prop == "0" {
                            return type_error(r, "GetOwnPropertyDescriptor(0) was called");
                        }
                        Ok(PropertyDescriptor::default())
                    })),
                    get_own_property_descriptor_idx: Some(Box::new(move |_r, _t, prop| {
                        if prop == 0 {
                            return Ok(PropertyDescriptor {
                                value: Some(pvi.clone()),
                                enumerable: Flag::True,
                                configurable: Flag::True,
                                ..PropertyDescriptor::default()
                            });
                        }
                        Ok(PropertyDescriptor::default())
                    })),
                    get: Some(Box::new(move |r, _t, property, _recv| {
                        if property == "foo" {
                            return Ok(pvs2.clone());
                        }
                        if property == "0" {
                            return type_error(r, "Get(0) was called");
                        }
                        Ok(get(r, obj, property))
                    })),
                    get_idx: Some(Box::new(move |r, _t, property, _recv| {
                        if property == 0 {
                            return Ok(pvi2.clone());
                        }
                        Ok(get(r, obj, &property.to_string()))
                    })),
                    get_sym: Some(Box::new(move |r, _t, property, _recv| {
                        if property == sym {
                            return Ok(pvy.clone());
                        }
                        Ok(obj.get_sym(r, property, None)?.unwrap_or_default())
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut vm, "proxy", proxy.to_value());
        let res = run(&mut vm, "JSON.stringify(proxy)");
        assert!(
            res.same_as(&s(r#"{"0":"idx","foo":"321tset"}"#)),
            "res: {res:?}"
        );
        let res = run(&mut vm, "proxy[Symbol.toPrimitive]");
        assert!(res.is_undefined(), "res: {res:?}");
        let res = run(&mut vm, "proxy.hasOwnProperty(Symbol.toPrimitive)");
        assert!(res.same_as(&Value::Bool(false)), "res: {res:?}");
        let val = proxy.proxy.get_sym(&mut vm, sym, None).expect("get");
        assert!(
            matches!(&val, Some(v) if v.same_as(&prop_value_sym)),
            "Get(symbol): {val:?}"
        );
        let res = run(&mut vm, "proxy.toString()");
        assert!(res.same_as(&s("[object Object]")), "res: {res:?}");
    });
}

#[test]
fn test_proxy_native_proxy_set() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let prop_value_str = s("321tset");
        let prop_value_idx = s("idx");
        let prop_value_sym = s("sym");
        let sym = vm.new_symbol(Some(ascii_string("test")));
        let obj = vm.new_object();
        let (pvs, pvi, pvy) = (
            prop_value_str.clone(),
            prop_value_idx.clone(),
            prop_value_sym.clone(),
        );
        let proxy = vm
            .new_proxy(
                obj,
                ProxyTrapConfig {
                    data: vec![Value::Object(obj), Value::Symbol(sym)],
                    set: Some(Box::new(move |r, _t, property, _v, _recv| {
                        if property == "str" {
                            obj.set_own_str(r, &property.into(), pvs.clone(), true)?;
                            return Ok(true);
                        }
                        type_error(r, &format!("Setter for unexpected property: {property:?}"))
                    })),
                    set_idx: Some(Box::new(move |r, _t, property, _v, _recv| {
                        if property == 0 {
                            obj.set_own_str(
                                r,
                                &property.to_string().as_str().into(),
                                pvi.clone(),
                                true,
                            )?;
                            return Ok(true);
                        }
                        type_error(
                            r,
                            &format!("Setter for unexpected idx property: {property}"),
                        )
                    })),
                    set_sym: Some(Box::new(move |r, _t, property, _v, _recv| {
                        if property == sym {
                            obj.set_own_sym(r, property, pvy.clone(), true)?;
                            return Ok(true);
                        }
                        type_error(r, "Setter for unexpected sym property")
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        let proxy_obj = proxy.proxy;
        proxy_obj
            .set_own_str(&mut vm, &"str".into(), s(""), true)
            .expect("str");
        proxy_obj
            .set_own_str(&mut vm, &"0".into(), s(""), true)
            .expect("0");
        proxy_obj
            .set_own_sym(&mut vm, sym, s(""), true)
            .expect("sym");
        let v = get(&mut vm, obj, "str");
        assert!(prop_value_str.same_as(&v), "{v:?}");
        let v = get(&mut vm, obj, "0");
        assert!(prop_value_idx.same_as(&v), "{v:?}");
        let v = obj
            .get_sym(&mut vm, sym, None)
            .expect("get")
            .unwrap_or_default();
        assert!(prop_value_sym.same_as(&v), "{v:?}");
    });
}

#[test]
fn test_proxy_target_set_prop() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {});
	proxy.foo = "test123";
	proxy.foo;
	"#,
        str_value("test123"),
    );
}

#[test]
fn test_proxy_proxy_set_prop() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {
		set: function(target, prop, receiver) {
			target.foo = "321tset";
			return true;
		}
	});
	proxy.foo = "test123";
	proxy.foo;
	"#,
        str_value("321tset"),
    );
}

#[test]
fn test_proxy_target_set_associative() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {});
	proxy["foo"] = "test123";
	proxy.foo;
	"#,
        str_value("test123"),
    );
}

#[test]
fn test_proxy_proxy_set_associative() {
    test_script(
        r#"
	var obj = {};
	var proxy = new Proxy(obj, {
		set: function(target, property, value, receiver) {
			target["foo"] = "321tset";
			return true;
		}
	});
	proxy["foo"] = "test123";
	proxy.foo;
	"#,
        str_value("321tset"),
    );
}

#[test]
fn test_proxy_target_delete() {
    test_script(
        r#"
	var obj = {
		foo: "test"
	};
	var proxy = new Proxy(obj, {});
	delete proxy.foo;

	proxy.foo;
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_proxy_delete() {
    test_script(
        r#"
	var obj = {
		foo: "test"
	};
	var proxy = new Proxy(obj, {
		deleteProperty: function(target, prop) {
			return true;
		}
	});
	delete proxy.foo;

	proxy.foo;
	"#,
        str_value("test"),
    );
}

#[test]
fn test_proxy_native_delete() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let sym = vm.new_symbol(Some(ascii_string("test")));
        let obj = vm.new_object();
        let flags: Rc<[Cell<bool>; 6]> = Rc::new(Default::default());
        let (f1, f2, f3) = (flags.clone(), flags.clone(), flags.clone());
        let proxy = vm
            .new_proxy(
                obj,
                ProxyTrapConfig {
                    data: vec![Value::Object(obj), Value::Symbol(sym)],
                    delete_property: Some(Box::new(move |r, _t, property| {
                        if property == "str" {
                            f1[0].set(true);
                            return Ok(true);
                        }
                        if property == "strNeg" {
                            f1[3].set(true);
                            return Ok(false);
                        }
                        type_error(
                            r,
                            &format!("DeleteProperty for unexpected property: {property:?}"),
                        )
                    })),
                    delete_property_idx: Some(Box::new(move |r, _t, property| {
                        if property == 0 {
                            f2[1].set(true);
                            return Ok(true);
                        }
                        if property == 1 {
                            f2[4].set(true);
                            return Ok(false);
                        }
                        type_error(
                            r,
                            &format!("DeletePropertyIdx for unexpected idx property: {property}"),
                        )
                    })),
                    delete_property_sym: Some(Box::new(move |r, _t, property| {
                        if property == sym {
                            f3[2].set(true);
                            return Ok(true);
                        }
                        if property == SYM_ITERATOR {
                            f3[5].set(true);
                            return Ok(false);
                        }
                        type_error(r, "DeletePropertySym for unexpected sym property")
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        let proxy_obj = proxy.proxy;
        proxy_obj
            .delete_str(&mut vm, &"str".into(), true)
            .expect("str");
        proxy_obj.delete_str(&mut vm, &"0".into(), true).expect("0");
        proxy_obj.delete_sym(&mut vm, sym, true).expect("sym");
        assert!(flags[0].get(), "str");
        assert!(flags[1].get(), "idx");
        assert!(flags[2].get(), "sym");
        set_global(&mut vm, "proxy", proxy.to_value());
        run(
            &mut vm,
            r#"
	if (delete proxy.strNeg) {
		throw new Error("strNeg");
	}
	if (delete proxy[1]) {
		throw new Error("idxNeg");
	}
	if (delete proxy[Symbol.iterator]) {
		throw new Error("symNeg");
	}
	"#,
        );
        assert!(flags[3].get(), "strNeg");
        assert!(flags[4].get(), "idxNeg");
        assert!(flags[5].get(), "symNeg");
    });
}

#[test]
fn test_proxy_target_keys() {
    test_script(
        r#"
	var obj = {
		foo: "test"
	};
	var proxy = new Proxy(obj, {});

	var keys = Object.keys(proxy);
	if (keys.length != 1) {
		throw new Error("assertion error");
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_proxy_keys() {
    test_script(
        r#"
	var obj = {
		foo: "test"
	};
	var proxy = new Proxy(obj, {
		ownKeys: function(target) {
			return ["foo", "bar"];
		}
	});

	var keys = Object.keys(proxy);
	if (keys.length !== 1) {
		throw new Error("length is "+keys.length);
	}
	if (keys[0] !== "foo") {
		throw new Error("keys[0] is "+keys[0]);
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_target_call() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {});

	proxy();
	"#,
        str_value("test"),
    );
}

#[test]
fn test_proxy_proxy_call() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {
		apply: function(target, thisArg, args) {
			return "tset"
		}
	});

	proxy();
	"#,
        str_value("tset"),
    );
}

#[test]
fn test_proxy_target_func_apply() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {});

	proxy.apply();
	"#,
        str_value("test"),
    );
}

#[test]
fn test_proxy_proxy_func_apply() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {
		apply: function(target, thisArg, args) {
			return "tset"
		}
	});

	proxy.apply();
	"#,
        str_value("tset"),
    );
}

#[test]
fn test_proxy_target_func_call() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {});

	proxy.call();
	"#,
        str_value("test"),
    );
}

#[test]
fn test_proxy_proxy_func_call() {
    test_script(
        r#"
	var obj = function() {
		return "test"
	}

	var proxy = new Proxy(obj, {
		apply: function(target, thisArg, args) {
			return "tset"
		}
	});

	proxy.call();
	"#,
        str_value("tset"),
    );
}

#[test]
fn test_proxy_target_new() {
    test_script(
        r#"
	var obj = function(word) {
		this.foo = function() {
			return word;
		}
	}

	var proxy = new Proxy(obj, {});

	var instance = new proxy("test");
	instance.foo();
	"#,
        str_value("test"),
    );
}

#[test]
fn test_proxy_proxy_new() {
    test_script(
        r#"
	var obj = function(word) {
		this.foo = function() {
			return word;
		}
	}

	var proxy = new Proxy(obj, {
		construct: function(target, args, newTarget) {
			var word = args[0];
			return {
				foo: function() {
					return "caught-" + word
				}
			}
		}
	});

	var instance = new proxy("test");
	instance.foo();
	"#,
        str_value("caught-test"),
    );
}

#[test]
fn test_proxy_object_native_proxy_own_keys() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let target = vm.new_object();
        let proxy = vm
            .new_proxy(
                target,
                ProxyTrapConfig {
                    own_keys: Some(Box::new(|r, _t| Ok(r.new_array_values(vec![s("k0")])))),
                    get_own_property_descriptor: Some(Box::new(|r, _t, prop| {
                        if prop == "k0" {
                            let v = r.new_array_values(Vec::new());
                            return Ok(PropertyDescriptor {
                                value: Some(Value::Object(v)),
                                enumerable: Flag::True,
                                configurable: Flag::True,
                                ..PropertyDescriptor::default()
                            });
                        }
                        Ok(PropertyDescriptor::default())
                    })),
                    ..ProxyTrapConfig::default()
                },
            )
            .expect("proxy");
        set_global(&mut vm, "headers", proxy.to_value());
        let v = run(
            &mut vm,
            r#"
		var keys = Object.keys(headers);
		keys.length === 1 && keys[0] === "k0";
		"#,
        );
        assert!(matches!(v, Value::Bool(true)), "not true {v:?}");
    });
}

#[test]
fn test_proxy_proxy_for_in() {
    test_script(
        r#"
	var proto = {
		a: 2,
		protoProp: 1
	}
	Object.defineProperty(proto, "protoNonEnum", {
		value: 2,
		writable: true,
		configurable: true
	});
	var target = Object.create(proto);
	var proxy = new Proxy(target, {
		ownKeys: function() {
			return ["a", "b"];
		},
		getOwnPropertyDescriptor: function(target, p) {
			switch (p) {
			case "a":
			case "b":
				return {
					value: 42,
					enumerable: true,
					configurable: true
				}
			}
		},
	});

	var forInResult = [];
	for (var key in proxy) {
		if (forInResult.indexOf(key) !== -1) {
			throw new Error("Duplicate property "+key);
		}
		forInResult.push(key);
	}
	forInResult.length === 3 && forInResult[0] === "a" && forInResult[1] === "b" && forInResult[2] === "protoProp";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_proxy_proxy_create_target_not_callable() {
    test_script_with_test_lib(
        r#"
	var p = new Proxy({}, {});

	assert.throws(TypeError, function() {
		  p();
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_proxy_enumerable_symbols() {
    test_script_with_test_lib(
        r#"
	var getOwnKeys = [];
	var ownKeysResult = [Symbol(), "foo", "0"];
	var proxy = new Proxy({}, {
	  getOwnPropertyDescriptor: function(_target, key) {
		getOwnKeys.push(key);
	  },
	  ownKeys: function() {
		return ownKeysResult;
	  },
	});

	let {...$} = proxy;
	compareArray(getOwnKeys, ownKeysResult);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_proxy_unicode_props() {
    test_script_with_test_lib(
        r#"
    const proxy = new Proxy({}, {
		ownKeys: () => ["â"]
	});
	compareArray(Reflect.ownKeys(proxy), ["â"]);
	"#,
        Value::Bool(true),
    );
}
const PROXY_CASES: &[(&str, &str)] = &[
    (
        r##"var t={}; Object.defineProperty(t,"x",{value:1}); var p=new Proxy(t,{get:function(){return 2}}); try{p.x}catch(e){String(e)}"##,
        r##"TypeError: 'get' on proxy: property 'x' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected '1' but got '{}')"##,
    ),
    (
        r##"var t={}; Object.defineProperty(t,"x",{get:undefined, set: function(){}}); var p=new Proxy(t,{get:function(){return 2}}); try{p.x}catch(e){String(e)}"##,
        r##"TypeError: 'get' on proxy: property 'x' is a non-configurable accessor property on the proxy target and does not have a getter function, but the trap did not return 'undefined' (got '{}')"##,
    ),
    (
        r##"var p=new Proxy({},{ownKeys:function(){return [1]}}); try{Object.keys(p)}catch(e){String(e)}"##,
        r##"TypeError: 1 is not a valid property name"##,
    ),
    (
        r##"var p=new Proxy({},{ownKeys:function(){return [{}]}}); try{Object.keys(p)}catch(e){String(e)}"##,
        r##"TypeError: [object Object] is not a valid property name"##,
    ),
    (
        r##"var t={}; Object.defineProperty(t,"x",{value:1}); var p=new Proxy(t,{defineProperty:function(){return true}}); try{Object.defineProperty(p,"x",{value:2})}catch(e){String(e)+"|"+e.message.length}"##,
        r##"TypeError|0"##,
    ),
    (
        r##"var p=new Proxy(function(){},{}); try{({}) instanceof p}catch(e){String(e)}"##,
        r##"TypeError: Expecting a function in instanceof check, but got function () { [native code] }"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); try{Object.prototype.toString.call(r.proxy)}catch(e){String(e)}"##,
        r##"TypeError: Cannot perform 'IsArray' on a proxy that has been revoked"##,
    ),
    (
        r##"var r=Proxy.revocable(function(){},{}); r.revoke(); typeof r.proxy"##,
        r##"function"##,
    ),
    (
        r##"var p = new Proxy({}, {isExtensible: function(){return false}}); try{Object.isExtensible(p)}catch(e){String(e)}"##,
        r##"TypeError: 'isExtensible' on proxy: trap result does not reflect extensibility of proxy target (which is 'true')"##,
    ),
    (
        r##"var s=Symbol("foo"); var t={}; Object.defineProperty(t,s,{value:1}); var p=new Proxy(t,{has:function(){return false}}); try{s in p}catch(e){String(e)}"##,
        r##"TypeError: 'has' on proxy: trap returned falsish for property 'foo' which exists in the proxy target as non-configurable"##,
    ),
    (
        r##"var p=new Proxy({},{ownKeys:function(){return ["a","a"]}}); try{Object.keys(p)}catch(e){String(e)}"##,
        r##"TypeError: 'ownKeys' on proxy: trap returned duplicate entries"##,
    ),
    (
        r##"var p=new Proxy({},{getOwnPropertyDescriptor:function(t,k){return {value:undefined,writable:true,enumerable:true,configurable:true}}, ownKeys:function(){return ["a","b"]}}); JSON.stringify(Object.keys(p))+Object.getOwnPropertyNames(p).length"##,
        r##"["a","b"]2"##,
    ),
    (
        r##"var p=new Proxy({},{ownKeys:function(){return 5}}); try{Object.keys(p)}catch(e){String(e)}"##,
        r##"TypeError: Value is not an object: 5"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); var out=[]; ["getPrototypeOf","isExtensible","keys","getOwnPropertyNames"].forEach(function(m){try{Object[m](r.proxy); out.push("ok")}catch(e){out.push(String(e))}}); out.join("|")"##,
        r##"TypeError: Proxy already revoked|TypeError: Proxy already revoked|TypeError: Proxy already revoked|TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); try{r.proxy.x}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); try{r.proxy.x=1}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); try{delete r.proxy.x}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable({},{}); r.revoke(); try{"x" in r.proxy}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable(function(){},{}); r.revoke(); try{r.proxy()}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable(function(){},{}); r.revoke(); try{new r.proxy()}catch(e){String(e)}"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r=Proxy.revocable([],{}); r.revoke(); try{Array.isArray(r.proxy)}catch(e){String(e)}"##,
        r##"TypeError: Cannot perform 'IsArray' on a proxy that has been revoked"##,
    ),
    (
        r##"try { Proxy() } catch(e) { String(e) }"##,
        r##"TypeError: Constructor Proxy requires 'new'"##,
    ),
    (
        r##"try { new Proxy(1, {}) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot create proxy with a non-object as target or handler"##,
    ),
    (
        r##"try { new Proxy({}) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot create proxy with a non-object as target or handler"##,
    ),
    (
        r##"try { Proxy.revocable({}) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot create proxy with a non-object as target or handler"##,
    ),
    (
        r##"Proxy.length + Proxy.name + Proxy.revocable.length + Proxy.revocable.name + ("prototype" in Proxy)"##,
        r##"2Proxy2revocablefalse"##,
    ),
    (
        r##"var r = Proxy.revocable({}, {}); Object.keys(r).join() + typeof r.revoke + r.revoke.name + "|" + r.revoke.length"##,
        r##"proxy,revokefunction|0"##,
    ),
    (
        r##"var log=[]; var h={}; ["get","set","has","deleteProperty","ownKeys","getOwnPropertyDescriptor","defineProperty","getPrototypeOf","setPrototypeOf","isExtensible","preventExtensions"].forEach(function(k){h[k]=function(){log.push(k); return Reflect[k].apply(null, arguments)}}); var p=new Proxy({a:1},h); p.a; p.b=2; "a" in p; delete p.a; Object.keys(p); Object.getOwnPropertyDescriptor(p,"b"); Object.defineProperty(p,"c",{value:3}); Object.getPrototypeOf(p); Object.setPrototypeOf(p,{}); Object.isExtensible(p); Object.preventExtensions(p); log.join()"##,
        r##"get,set,getOwnPropertyDescriptor,defineProperty,has,deleteProperty,ownKeys,getOwnPropertyDescriptor,getOwnPropertyDescriptor,defineProperty,getPrototypeOf,setPrototypeOf,isExtensible,preventExtensions"##,
    ),
    (
        r##"var log=[]; var p=new Proxy({}, {get: function(t,k,r){ log.push(typeof k + ":" + String(k)); return undefined }}); p[0]; p["1"]; p[Symbol.iterator]; p[-1]; p[1.5]; log.join()"##,
        r##"string:0,string:1,symbol:Symbol(Symbol.iterator),string:-1,string:1.5"##,
    ),
    (
        r##"var p=new Proxy([1,2,3], {}); Array.isArray(p) + "," + p.length + "," + JSON.stringify(p) + "," + Object.prototype.toString.call(p)"##,
        r##"true,3,[1,2,3],[object Array]"##,
    ),
    (
        r##"var p=new Proxy(function(){}, {}); Object.prototype.toString.call(p) + typeof p"##,
        r##"[object Function]function"##,
    ),
    (
        r##"var p=new Proxy({}, {}); Object.prototype.toString.call(p) + typeof p"##,
        r##"[object Object]object"##,
    ),
    (
        r##"var p=new Proxy(function(){}, {}); Function.prototype.toString.call(p)"##,
        r##"function () { [native code] }"##,
    ),
    (
        r##"var p=new Proxy(function f(a,b){}, {}); p.length + p.name"##,
        r##"2f"##,
    ),
    (
        r##"var p = new Proxy(function(){ return this }, {}); var o = {m: p}; o.m() === o"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy(function(a,b){ return a+b }, {apply: function(t, th, args){ return args.length + ":" + Array.isArray(args) + ":" + (th === undefined) }}); p(1,2,3)"##,
        r##"3:true:true"##,
    ),
    (
        r##"var p = new Proxy(function(){}, {construct: function(t, args, nt){ return 1 }}); try { new p() } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"var P = new Proxy(function(){ this.x = 1 }, {}); var o = new P(); o.x + ":" + (Object.getPrototypeOf(o) === P.prototype)"##,
        r##"1:true"##,
    ),
    (
        r##"class A { constructor() { this.nt = new.target } }; var P = new Proxy(A, {}); var o = new P(); o.nt === P"##,
        r##"true"##,
    ),
    (
        r##"var P = new Proxy(Array, {}); var a = new P(3); a.length + ":" + Array.isArray(a)"##,
        r##"3:true"##,
    ),
    (
        r##"var o = Reflect.construct(Proxy, [{}, {}]); typeof o"##,
        r##"object"##,
    ),
    (
        r##"var p = new Proxy({}, {}); Object.isExtensible(p)"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {}); Object.preventExtensions(p); Object.isExtensible(p)"##,
        r##"false"##,
    ),
    (
        r##"var p = new Proxy({a:1}, {}); for (var k in p) k"##,
        r##"a"##,
    ),
    (
        r##"var p = new Proxy(Object.create({inh: 1}, {own: {value: 2, enumerable: true}}), {}); var r=[]; for (var k in p) r.push(k); r.join()"##,
        r##"own,inh"##,
    ),
    (
        r##"var p = new Proxy({}, {getPrototypeOf: function(){ return Array.prototype }}); p instanceof Array"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {getPrototypeOf: function(){ return 5 }}); try { Object.getPrototypeOf(p) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 5"##,
    ),
    (
        r##"var t = Object.preventExtensions({}); var p = new Proxy(t, {getPrototypeOf: function(){ return Array.prototype }}); try { Object.getPrototypeOf(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'getPrototypeOf' on proxy: proxy target is non-extensible but the trap did not return its actual prototype"##,
    ),
    (
        r##"var t = Object.preventExtensions({}); var p = new Proxy(t, {setPrototypeOf: function(){ return true }}); try { Object.setPrototypeOf(p, Array.prototype) } catch(e) { String(e) }"##,
        r##"TypeError: 'setPrototypeOf' on proxy: trap returned truish for setting a new prototype on the non-extensible proxy target"##,
    ),
    (
        r##"var p = new Proxy({}, {setPrototypeOf: function(){ return false }}); try { Object.setPrototypeOf(p, null) } catch(e) { String(e) }"##,
        r##"TypeError: 'setPrototypeOf' on proxy: trap returned falsish"##,
    ),
    (
        r##"var p = new Proxy({}, {setPrototypeOf: function(){ return false }}); Reflect.setPrototypeOf(p, null)"##,
        r##"false"##,
    ),
    (
        r##"var p = new Proxy({}, {preventExtensions: function(){ return false }}); try { Object.preventExtensions(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'preventExtensions' on proxy: trap returned falsish"##,
    ),
    (
        r##"var p = new Proxy({}, {preventExtensions: function(){ return true }}); try { Object.preventExtensions(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'preventExtensions' on proxy: trap returned truish but the proxy target is extensible"##,
    ),
    (
        r##"var p = new Proxy({}, {preventExtensions: function(){ return false }}); Reflect.preventExtensions(p)"##,
        r##"false"##,
    ),
    (
        r##"var p = new Proxy({}, {defineProperty: function(){ return false }}); try { Object.defineProperty(p, "x", {value: 1}) } catch(e) { String(e) }"##,
        r##"TypeError: 'defineProperty' on proxy: trap returned falsish"##,
    ),
    (
        r##"var p = new Proxy({}, {defineProperty: function(){ return false }}); Reflect.defineProperty(p, "x", {value: 1})"##,
        r##"false"##,
    ),
    (
        r##"var p = new Proxy(Object.preventExtensions({}), {defineProperty: function(){ return true }}); try { Object.defineProperty(p, "x", {value: 1}) } catch(e) { "E:" + e.message }"##,
        r##"E:"##,
    ),
    (
        r##"var p = new Proxy({}, {defineProperty: function(){ return true }}); try { Object.defineProperty(p, "x", {value: 1, configurable: false}) } catch(e) { "E:" + e.message }"##,
        r##"E:"##,
    ),
    (
        r##"var p = new Proxy({}, {set: function(){ return false }}); p.x = 1; "ok""##,
        r##"ok"##,
    ),
    (
        r##"var p = new Proxy({}, {set: function(){ return false }}); try { (function(){ "use strict"; p.x = 1 })() } catch(e) { String(e) }"##,
        r##"TypeError: 'set' on proxy: trap returned falsish for property 'x'"##,
    ),
    (
        r##"var p = new Proxy({}, {set: function(){ return false }}); Reflect.set(p, "x", 1)"##,
        r##"false"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: 1}); var p = new Proxy(t, {set: function(){ return true }}); try { p.x = 2 } catch(e) { String(e) }"##,
        r##"TypeError: 'set' on proxy: trap returned truish for property 'x' which exists in the proxy target as a non-configurable and non-writable data property with a different value"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {get: function(){}}); var p = new Proxy(t, {set: function(){ return true }}); try { p.x = 2 } catch(e) { String(e) }"##,
        r##"TypeError: 'set' on proxy: trap returned truish for property 'x' which exists in the proxy target as a non-configurable and non-writable accessor property without a setter"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: 1}); var p = new Proxy(t, {set: function(){ return true }}); p.x = 1; "same""##,
        r##"same"##,
    ),
    (
        r##"var p = new Proxy({}, {deleteProperty: function(){ return false }}); delete p.x"##,
        r##"false"##,
    ),
    (
        r##"var p = new Proxy({}, {deleteProperty: function(){ return false }}); try { (function(){ "use strict"; delete p.x })() } catch(e) { String(e) }"##,
        r##"TypeError: 'deleteProperty' on proxy: trap returned falsish for property 'x'"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: 1}); var p = new Proxy(t, {deleteProperty: function(){ return true }}); try { delete p.x } catch(e) { String(e) }"##,
        r##"TypeError: 'deleteProperty' on proxy: property 'x' is a non-configurable property but the trap returned truish"##,
    ),
    (
        r##"var t = Object.preventExtensions({x: 1}); var p = new Proxy(t, {deleteProperty: function(){ return true }}); try { delete p.x } catch(e) { String(e) }"##,
        r##"TypeError: 'deleteProperty' on proxy: trap returned truish for property 'x' but the proxy target is non-extensible"##,
    ),
    (
        r##"var t = Object.preventExtensions({x: 1}); var p = new Proxy(t, {has: function(){ return false }}); try { "x" in p } catch(e) { String(e) }"##,
        r##"TypeError: 'has' on proxy: trap returned falsish for property 'x' but the proxy target is not extensible"##,
    ),
    (
        r##"var p = new Proxy({}, {getOwnPropertyDescriptor: function(){ return 1 }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap returned neither object nor undefined for property 'x'"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: 1}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return undefined }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { "E:" + e.message }"##,
        r##"E:"##,
    ),
    (
        r##"var t = Object.preventExtensions({x: 1}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return undefined }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { "E:" + e.message }"##,
        r##"E:"##,
    ),
    (
        r##"var p = new Proxy({}, {getOwnPropertyDescriptor: function(){ return {value: 1, configurable: false} }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property 'x' which is non-existent in the proxy target"##,
    ),
    (
        r##"var t = {x: 1}; var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return {value: 1, configurable: false} }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property 'x' which is configurable in the proxy target"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: 1, writable: true}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return {value: 1, configurable: false, writable: false} }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap reported non-configurable and writable for property 'x' which is non-configurable, non-writable in the proxy target"##,
    ),
    (
        r##"var t = Object.preventExtensions({}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return {value: 1, configurable: true} }}); try { Object.getOwnPropertyDescriptor(p, "x") } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap returned descriptor for property 'x' that is incompatible with the existing property in the proxy target"##,
    ),
    (
        r##"var p = new Proxy({}, {getOwnPropertyDescriptor: function(){ return {get: function(){ return 7 }, configurable: true} }}); var d = Object.getOwnPropertyDescriptor(p, "x"); typeof d.get + d.get() + d.configurable + d.enumerable"##,
        r##"function7truefalse"##,
    ),
    (
        r##"var cnt=0; var p = new Proxy({}, {getOwnPropertyDescriptor: function(){ return {get value(){ cnt++; return 5 }, configurable: true, writable: false} }}); var d = Object.getOwnPropertyDescriptor(p, "x"); d.value + ":" + cnt"##,
        r##"5:2"##,
    ),
    (
        r##"var t = Object.preventExtensions({a: 1}); var p = new Proxy(t, {ownKeys: function(){ return [] }}); try { Object.keys(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'ownKeys' on proxy: trap result did not include 'a'"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "a", {value: 1}); var p = new Proxy(t, {ownKeys: function(){ return [] }}); try { Object.keys(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'ownKeys' on proxy: trap result did not include non-configurable 'a'"##,
    ),
    (
        r##"var t = Object.preventExtensions({a: 1}); var p = new Proxy(t, {ownKeys: function(){ return ["a", "b"] }}); try { Object.keys(p) } catch(e) { String(e) }"##,
        r##"TypeError: 'ownKeys' on proxy: trap returned extra keys but proxy target is non-extensible"##,
    ),
    (
        r##"var s = Symbol("q"); var p = new Proxy({}, {ownKeys: function(){ return ["a", s] }, getOwnPropertyDescriptor: function(t, k){ return {value: 1, enumerable: true, configurable: true} }}); Object.keys(p).length + ":" + Object.getOwnPropertySymbols(p).length + ":" + Reflect.ownKeys(p).length"##,
        r##"1:1:2"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function(){ return ["b", "a"] }, getOwnPropertyDescriptor: function(t, k){ return {value: k, enumerable: k === "a", configurable: true} }}); JSON.stringify(Object.keys(p)) + JSON.stringify(Object.entries(p)) + JSON.stringify(Object.assign({}, p))"##,
        r##"["a"][]{}"##,
    ),
    (
        r##"var p = new Proxy({}, {has: function(t, k){ return k === "x" }}); with (p) { typeof x }"##,
        r##"undefined"##,
    ),
    (
        r##"var p = new Proxy({x: 1}, {}); Object.freeze(p); Object.isFrozen(p) + ":" + Object.isFrozen(Object.getPrototypeOf(p) === Object.prototype ? p : p)"##,
        r##"true:true"##,
    ),
    (
        r##"var t = {x: 1}; var p = new Proxy(t, {}); Object.seal(p); Object.isSealed(t)"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {get: function(t, k, r) { return r }}); p.foo === p"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {get: function(t, k, r) { return r }}); var o = Object.create(p); o.foo === o"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {set: function(t, k, v, r) { t.recv = r; return true }}); var o = Object.create(p); o.foo = 1; Object.getPrototypeOf(o).recv === undefined ? "noproto" : "x""##,
        r##"x"##,
    ),
    (
        r##"var log = []; var p = new Proxy({}, {set: function(t, k, v, r) { log.push(r === o); return true }}); var o = Object.create(p); o.foo = 1; log.join()"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy([], {}); p.push(1, 2); p.length + ":" + p.join()"##,
        r##"2:1,2"##,
    ),
    (
        r##"var p = new Proxy([3,1,2], {}); p.sort(); p.join()"##,
        r##"1,2,3"##,
    ),
    (
        r##"var p = new Proxy({}, {}); p.__proto__ === Object.prototype"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({}, {getPrototypeOf: function(){ log2 = 1; return null }}); var log2 = 0; p.__proto__; log2"##,
        r##"1"##,
    ),
    (
        r##"var p = new Proxy({a: 1, b: 2}, {}); var {a, ...rest} = p; JSON.stringify(rest)"##,
        r##"{"b":2}"##,
    ),
    (
        r##"var p = new Proxy([1, 2, 3], {}); [...p].join()"##,
        r##"1,2,3"##,
    ),
    (
        r##"var p = new Proxy({length: 2, 0: "a", 1: "b"}, {}); Array.prototype.join.call(p)"##,
        r##"a,b"##,
    ),
    (
        r##"var p = new Proxy(new Map([[1,2]]), {}); try { p.get(1) } catch(e) { String(e) }"##,
        r##"TypeError: Method Map.prototype.get called on incompatible receiver [object Map]"##,
    ),
    (
        r##"var p = new Proxy(new Date(0), {}); try { p.getTime() } catch(e) { String(e) }"##,
        r##"TypeError: Method Date.prototype.getTime is called on incompatible receiver"##,
    ),
    (
        r##"var p = new Proxy({}, {}); String(p)"##,
        r##"[object Object]"##,
    ),
    (
        r##"var p = new Proxy({}, {get: function(t, k) { if (k === Symbol.toPrimitive) return function(){ return 42 }; }}); p + 1"##,
        r##"43"##,
    ),
    (
        r##"var handler = new Proxy({}, {get: function(t, k) { return function(t2, k2) { return "trap:" + k + ":" + k2 } }}); var p = new Proxy({}, handler); p.foo"##,
        r##"trap:get:foo"##,
    ),
    (
        r##"var p = new Proxy({}, {get: 1}); try { p.x } catch(e) { String(e) }"##,
        r##"TypeError: 1 is not a method"##,
    ),
    (
        r##"var p = new Proxy({}, {get: null}); p.x === undefined"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy({x: 5}, {get: undefined}); p.x"##,
        r##"5"##,
    ),
    (
        r##"var p = new Proxy(new Proxy({x: 9}, {}), {}); p.x"##,
        r##"9"##,
    ),
    (
        r##"var r = Proxy.revocable({}, {}); var p = new Proxy(r.proxy, {}); r.revoke(); try { p.x } catch(e) { String(e) }"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (
        r##"var r = Proxy.revocable({}, {}); r.revoke(); var p = new Proxy(r.proxy, {}); typeof p"##,
        r##"object"##,
    ),
    (
        r##"var r = Proxy.revocable({}, {}); r.revoke(); r.revoke(); "twice""##,
        r##"twice"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function(){ return ["a"] }}); try { JSON.stringify(Reflect.ownKeys(p)) } catch(e) { String(e) }"##,
        r##"["a"]"##,
    ),
    (
        r##"var p = new Proxy({}, {}); Reflect.ownKeys(Proxy).join()"##,
        r##"length,name,revocable"##,
    ),
    (
        r##"var p = new Proxy(function(){}, {apply: function(t, th, a){ return th }}); var o = {}; p.call(o) === o"##,
        r##"true"##,
    ),
    (
        r##"var p = new Proxy(function(){}, {apply: function(t, th, a){ return a.length }}); p.apply(null, [1,2,3])"##,
        r##"3"##,
    ),
    (
        r##"var p = new Proxy(function(){ return 3 }, {}); Reflect.apply(p, null, [])"##,
        r##"3"##,
    ),
    (
        r##"var p = new Proxy({}, {}); try { p() } catch(e) { String(e) }"##,
        r##"TypeError: proxy target is not a function"##,
    ),
    (
        r##"var p = new Proxy({}, {}); try { new p() } catch(e) { String(e) }"##,
        r##"TypeError: Value is not a constructor"##,
    ),
    (
        r##"var p = new Proxy(() => 1, {}); try { new p() } catch(e) { String(e) }"##,
        r##"TypeError: Value is not a constructor"##,
    ),
    (
        r##"var p = new Proxy(function(){}, {}); var e = new Error("x"); try { (function f(){ p(); throw e })() } catch(x) { "ok" }"##,
        r##"ok"##,
    ),
    (
        r##"var p = new Proxy(function(){ throw new Error("boom") }, {}); try { p() } catch(e) { e.stack.split("\n").slice(0, 3).join("|") }"##,
        r##"Error: boom|	at <eval>:1:37(3)|	at proxy (native)"##,
    ),
    (
        r##"var p = new Proxy(function(){ throw new Error("boom") }, {apply: function(t, th, a) { return Reflect.apply(t, th, a) }}); try { p() } catch(e) { e.stack.split("\n").slice(0, 4).join("|") }"##,
        r##"Error: boom|	at <eval>:1:37(3)|	at apply (native)|	at apply (<eval>:1:107(6))"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: {}, configurable: false, writable: false}); var p = new Proxy(t, {get: function(){ return {toString: function(){ return "Z" }} }}); try { p.x } catch(e) { String(e) }"##,
        r##"TypeError: 'get' on proxy: property 'x' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected '[object Object]' but got '{}')"##,
    ),
    (
        r##"var t = {}; Object.defineProperty(t, "x", {value: {toString: function(){ return "TV" }}, configurable: false, writable: false}); var p = new Proxy(t, {get: function(){ return 1 }}); try { p.x } catch(e) { String(e) }"##,
        r##"TypeError: 'get' on proxy: property 'x' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected 'TV' but got '{}')"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function(){ return [undefined] }}); try { Reflect.ownKeys(p) } catch(e) { String(e) }"##,
        r##"TypeError: undefined is not a valid property name"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function(){ return [null] }}); try { Reflect.ownKeys(p) } catch(e) { String(e) }"##,
        r##"TypeError: null is not a valid property name"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function(){ return {length: 1, 0: "a"} }}); Reflect.ownKeys(p).join()"##,
        r##"a"##,
    ),
    (
        r##"var p = new Proxy([], {defineProperty: function(t, k, d){ log3.push(k + ":" + JSON.stringify(d)); return Reflect.defineProperty(t, k, d) }}); var log3 = []; p.push(5); log3.join("|")"##,
        r##"0:{"value":5,"writable":true,"enumerable":true,"configurable":true}|length:{"value":1}"##,
    ),
    (
        r##"var p = new Proxy({}, {defineProperty: function(t, k, d){ return Reflect.defineProperty(t, k, d) }}); p.x = 1; Object.getOwnPropertyDescriptor(p, "x").writable"##,
        r##"true"##,
    ),
    (
        r##"var t = {}; var p = new Proxy(t, {}); Object.defineProperty(p, "x", {get: function(){ return 1 }, configurable: false}); try { Object.defineProperty(p, "x", {get: function(){ return 2 }}) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot redefine property: x"##,
    ),
    (
        r##"var g = function(){ return 1 }; var t = {}; Object.defineProperty(t, "x", {get: g, configurable: false}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return {get: g, configurable: false} }}); try { Object.getOwnPropertyDescriptor(p, "x").get === g } catch(e) { String(e) }"##,
        r##"TypeError: 'getOwnPropertyDescriptor' on proxy: trap returned descriptor for property 'x' that is incompatible with the existing property in the proxy target"##,
    ),
    (
        r##"var g = function(){ return 1 }; var t = {}; Object.defineProperty(t, "x", {get: g, configurable: false}); var p = new Proxy(t, {getOwnPropertyDescriptor: function(){ return {get: function(){}, configurable: false} }}); try { typeof Object.getOwnPropertyDescriptor(p, "x").get } catch(e) { String(e) }"##,
        r##"function"##,
    ),
    (
        r##"var p = new Proxy({}, {isExtensible: function(){ return 1 }}); Object.isExtensible(p)"##,
        r##"true"##,
    ),
    (
        r##"Object.prototype.toString.call(Proxy)"##,
        r##"[object Function]"##,
    ),
    (
        r##"JSON.stringify(Object.getOwnPropertyNames(Reflect).sort())"##,
        r##"["apply","construct","defineProperty","deleteProperty","get","getOwnPropertyDescriptor","getPrototypeOf","has","isExtensible","ownKeys","preventExtensions","set","setPrototypeOf"]"##,
    ),
    (
        r##"try { Reflect.apply(1) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"try { Reflect.construct(function(){}, 1) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"try { Reflect.construct(() => 1, []) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not a constructor"##,
    ),
    (
        r##"try { Reflect.construct(function(){}, [], 1) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"function F(){ this.nt = new.target }; function G(){}; var o = Reflect.construct(F, [], G); (o.nt === G) + ":" + (Object.getPrototypeOf(o) === G.prototype)"##,
        r##"true:true"##,
    ),
    (
        r##"try { Reflect.defineProperty(1, "x", {}) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"Reflect.defineProperty(Object.freeze({}), "x", {value: 1})"##,
        r##"false"##,
    ),
    (
        r##"try { Reflect.deleteProperty(null, "x") } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: null"##,
    ),
    (
        r##"Reflect.deleteProperty(Object.freeze({x: 1}), "x")"##,
        r##"false"##,
    ),
    (
        r##"Reflect.get({x: 1}, "x") + Reflect.get([5], 0) + Reflect.get({get x(){ return this.y }}, "x", {y: 10})"##,
        r##"16"##,
    ),
    (
        r##"try { Reflect.get(1, "x") } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"JSON.stringify(Reflect.getOwnPropertyDescriptor({x: 1}, "x"))"##,
        r##"{"value":1,"writable":true,"enumerable":true,"configurable":true}"##,
    ),
    (
        r##"Reflect.getOwnPropertyDescriptor({x: 1}, "y")"##,
        r##"undefined"##,
    ),
    (
        r##"Reflect.getPrototypeOf(Object.create(null))"##,
        r##"null"##,
    ),
    (
        r##"Reflect.has({x: 1}, "x") + ":" + Reflect.has({}, "toString") + ":" + Reflect.has([1], 0)"##,
        r##"true:true:true"##,
    ),
    (
        r##"Reflect.isExtensible({}) + ":" + Reflect.isExtensible(Object.seal({}))"##,
        r##"true:false"##,
    ),
    (
        r##"JSON.stringify(Reflect.ownKeys({b: 1, a: 2, 1: 3, [Symbol.iterator]: 4}).map(String))"##,
        r##"["1","b","a","Symbol(Symbol.iterator)"]"##,
    ),
    (
        r##"var o = {}; Reflect.preventExtensions(o) + ":" + Object.isExtensible(o)"##,
        r##"true:false"##,
    ),
    (
        r##"var o = {}; Reflect.set(o, "x", 1) + ":" + o.x"##,
        r##"true:1"##,
    ),
    (
        r##"var o = {set x(v) { this.y = v }}; var r = {}; Reflect.set(o, "x", 5, r); r.y + ":" + o.y"##,
        r##"5:undefined"##,
    ),
    (
        r##"var o = Object.freeze({x: 1}); Reflect.set(o, "x", 2)"##,
        r##"false"##,
    ),
    (r##"Reflect.set({}, "x", 1, 5)"##, r##"false"##),
    (
        r##"try { Reflect.setPrototypeOf({}, 1) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: 1"##,
    ),
    (
        r##"try { Reflect.setPrototypeOf({}) } catch(e) { String(e) }"##,
        r##"TypeError: Value is not an object: undefined"##,
    ),
    (
        r##"Reflect.setPrototypeOf({}, null) + ":" + Reflect.setPrototypeOf(Object.preventExtensions({}), {})"##,
        r##"true:false"##,
    ),
    (
        r##"var a = {}; var b = Object.create(a); Reflect.setPrototypeOf(a, b)"##,
        r##"false"##,
    ),
    (
        r##"String(Reflect) + Object.prototype.toString.call(Reflect)"##,
        r##"[object Reflect][object Reflect]"##,
    ),
    (
        r##"Reflect.apply.length + Reflect.construct.length + Reflect.defineProperty.length + Reflect.set.length + Reflect.getPrototypeOf.length"##,
        r##"12"##,
    ),
    (
        r##"try { new Reflect.get() } catch(e) { String(e) }"##,
        r##"TypeError: Value is not a constructor"##,
    ),
    (r##"Reflect.get({}, Symbol.iterator)"##, r##"undefined"##),
    (r##"Reflect.set([1,2,3], "length", 1)"##, r##"true"##),
    (
        r##"var a = [1,2,3]; Reflect.set(a, "length", 1); a.length"##,
        r##"1"##,
    ),
    (
        r##"Reflect.defineProperty([], "length", {value: -1})"##,
        r##"!RangeError: Invalid array length at defineProperty (native)"##,
    ),
    (
        r##"try { Reflect.defineProperty({}, "x", 1) } catch(e) { String(e) }"##,
        r##"TypeError: Property description must be an object: 1"##,
    ),
    (r##"Reflect.ownKeys([1,2]).join()"##, r##"0,1,length"##),
    (
        r##"Reflect.ownKeys("abc" === "abc" ? new String("ab") : 0).join()"##,
        r##"0,1,length"##,
    ),
    (
        r##"var inner = new Proxy(Object.preventExtensions({}), {isExtensible: function(t) { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return false }}); var p = new Proxy(inner, {getPrototypeOf: function() { return Object.getPrototypeOf(inner) }}); var r = []; for (var k = 0; k < 20; k++) r.push(Object.getPrototypeOf(p) === Object.prototype); r.join()"##,
        r##"true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true"##,
    ),
    (
        r##"var t = new Proxy({}, {getOwnPropertyDescriptor: function() { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return undefined }}); var p = new Proxy(t, {get: function() { return {fresh: [1, 2, 3]} }}); var s = 0; for (var k = 0; k < 50; k++) { s += p.x.fresh.length } s"##,
        r##"150"##,
    ),
    (
        r##"var t = {}; var p = new Proxy(t, {ownKeys: function() { return ["a", "b", Symbol("s")] }, getOwnPropertyDescriptor: function(t, k) { var junk = []; for (var i = 0; i < 30; i++) junk.push({i: i}); return {value: k, enumerable: true, configurable: true} }}); var r = []; for (var k = 0; k < 20; k++) r.push(Object.keys(p).join("")); r.join()"##,
        r##"ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab,ab"##,
    ),
    (
        r##"var p = new Proxy(function() { return 1 }, {apply: function(t, th, args) { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return args.length + args[0].v }}); var s = 0; for (var k = 0; k < 50; k++) { s += p({v: k}, 2, 3) } s"##,
        r##"1375"##,
    ),
    (
        r##"var P = new Proxy(function(a) { this.a = a }, {construct: function(t, args, nt) { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return {a: args[0].x} }}); var s = 0; for (var k = 0; k < 50; k++) { s += new P({x: k}).a } s"##,
        r##"1225"##,
    ),
    (
        r##"var p = new Proxy({}, {defineProperty: function(t, k, d) { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return Reflect.defineProperty(t, k, d) }}); for (var k = 0; k < 30; k++) { Object.defineProperty(p, "k" + k, {value: {v: k}, enumerable: true, configurable: true, writable: true}) } JSON.stringify(p).length"##,
        r##"431"##,
    ),
    (
        r##"var p = new Proxy({}, {getOwnPropertyDescriptor: function(t, k) { var junk = []; for (var i = 0; i < 50; i++) junk.push({i: i}); return {get value() { var j2 = []; for (var i = 0; i < 20; i++) j2.push([i]); return {deep: k} }, configurable: true, enumerable: true, writable: true} }}); var r = []; for (var k = 0; k < 20; k++) r.push(Object.getOwnPropertyDescriptor(p, "q" + k).value.deep); r.join()"##,
        r##"q0,q1,q2,q3,q4,q5,q6,q7,q8,q9,q10,q11,q12,q13,q14,q15,q16,q17,q18,q19"##,
    ),
    (
        r##"var o = {a: {b: {c: [1, 2, {d: "x"}]}}, e: [{f: 1}, {g: 2}]}; var r = []; for (var k = 0; k < 30; k++) r.push(JSON.stringify(o, function(key, v) { var junk = []; for (var i = 0; i < 10; i++) junk.push({i: i}); return typeof v === "number" && key !== "n" ? {n: v} : v })); r[29]"##,
        r##"{"a":{"b":{"c":[{"n":1},{"n":2},{"d":"x"}]}},"e":[{"f":{"n":1}},{"g":{"n":2}}]}"##,
    ),
    (
        r##"var o = {a: 1, b: [1, 2]}; var r = []; for (var k = 0; k < 30; k++) r.push(JSON.stringify({x: {toJSON: function() { var junk = []; for (var i = 0; i < 20; i++) junk.push({i: i}); return {fresh: [k, {k: k}]} }}})); r[29]"##,
        r##"{"x":{"fresh":[29,{"k":29}]}}"##,
    ),
    (
        r##"var r = []; for (var k = 0; k < 30; k++) r.push(JSON.stringify(JSON.parse('{"a":[1,{"b":2}],"c":{"d":[3]}}', function(key, v) { var junk = []; for (var i = 0; i < 10; i++) junk.push({i: i}); return typeof v === "number" ? [v] : v }))); r[29]"##,
        r##"{"a":[[1],{"b":[2]}],"c":{"d":[[3]]}}"##,
    ),
    (
        r##"var s = Symbol("k"); var p = new Proxy({}, {get: function(t, k) { var junk = []; for (var i = 0; i < 30; i++) junk.push(Symbol("x" + i)); return Symbol("fresh") }}); var r = []; for (var k = 0; k < 30; k++) r.push(String(p.x)); r[29]"##,
        r##"Symbol(fresh)"##,
    ),
    (
        r##"var p = new Proxy({}, {ownKeys: function() { var junk = []; for (var i = 0; i < 30; i++) junk.push({i: i}); return [Symbol("a"), "b"] }, getOwnPropertyDescriptor: function(t, k) { var junk = []; for (var i = 0; i < 30; i++) junk.push(Symbol()); return {value: 1, enumerable: true, configurable: true} }}); var r = []; for (var k = 0; k < 20; k++) r.push(Reflect.ownKeys(p).map(String).join("") + Object.getOwnPropertySymbols(p).length); r.join()"##,
        r##"Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1,Symbol(a)b1"##,
    ),
    (
        r##"var p = new Proxy([1, 2, 3], {get: function(t, k, r) { var junk = []; for (var i = 0; i < 30; i++) junk.push({i: i}); return Reflect.get(t, k, r) }}); var r = []; for (var k = 0; k < 20; k++) r.push(JSON.stringify(p)); r.join("|")"##,
        r##"[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]|[1,2,3]"##,
    ),
    (
        r##"var p = new Proxy({a: 1}, {}); var r = []; for (var k = 0; k < 50; k++) { var x = {}; for (var j in p) x[j] = [k]; r.push(x.a[0]) } r.join()"##,
        r##"0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49"##,
    ),
    (
        r##"var p = new Proxy({}, {has: function(t, k) { var junk = []; for (var i = 0; i < 30; i++) junk.push({i: i}); return k === "z" }}); var c = 0; for (var k = 0; k < 50; k++) { with (p) { if (typeof z === "undefined") c++ } } c"##,
        r##"50"##,
    ),
];

#[test]
fn proxy_and_reflect_builtins_match_goja() {
    assert!(PROXY_CASES.len() > 100);
    on_big_stack(|| {
        let mut failures = Vec::new();
        for (script, want) in PROXY_CASES {
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(v) => v.go_string(&mut rt).expect("go_string"),
                Err(e) => format!("!{}", e.error()),
            };
            if got != *want {
                failures.push(format!("{script}\n  got:  {got}\n  want: {want}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn test_proxy_export() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let v = run(&mut vm, "\n\tnew Proxy({}, {});\n\t");
        let v1 = vm.export(&v).expect("export");
        assert!(
            matches!(
                &v1,
                DocValue::Object(JsHandle {
                    kind: HandleKind::Proxy,
                    ..
                })
            ),
            "Export returned an unexpected value"
        );
    });
}
