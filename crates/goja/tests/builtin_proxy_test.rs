mod jsrun;

use goja::builtin_proxy::ProxyTrapConfig;
use goja::builtin_symbol::SYM_ITERATOR;
use goja::func::NativeFn;
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
