mod jsrun;

use goja::func::NativeFn;
use goja::object::{Flag, PropertyDescriptor};
use goja::{Runtime, Value};
use jsrun::*;

fn try_set(rt: &mut Runtime, o: goja::ObjRef, name: &str, v: Value) -> Result<(), String> {
    let r = rt.vm_try_result(|rt| {
        o.set_own_str(rt, &name.into(), v, true)?;
        Ok(())
    });
    match r {
        Ok(None) => Ok(()),
        Ok(Some(mut ex)) => {
            ex.resolve_value_string(rt);
            Err(ex.error())
        }
        Err(_) => Err("uncatchable".to_string()),
    }
}

fn get_int(rt: &mut Runtime, o: goja::ObjRef, name: &str) -> i64 {
    let v = o
        .get_str(rt, &name.into(), None)
        .expect("get")
        .unwrap_or_default();
    v.to_integer(rt).expect("to_integer")
}

#[test]
fn test_define_property() {
    on_big_stack(|| {
        let mut r = new_runtime();
        let o = r.new_object();
        let res = o.define_own_property_str(
            &mut r,
            &"data".into(),
            PropertyDescriptor {
                value: Some(Value::Int(42)),
                writable: Flag::True,
                configurable: Flag::True,
                enumerable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        );
        assert!(res.is_ok());

        let ro = r.new_native_func(NativeFn::Fn(|_, _| Ok(Value::Int(1))), "", 0);
        let res = o.define_own_property_str(
            &mut r,
            &"accessor_ro".into(),
            PropertyDescriptor {
                getter: Some(Value::Object(ro)),
                configurable: Flag::True,
                enumerable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        );
        assert!(res.is_ok());

        let getter = r.new_native_func(
            NativeFn::closure_with(vec![Value::Object(o)], |rt, d, _call| {
                let o = d[0].as_object().expect("o");
                Ok(o.get_str(rt, &"__hidden".into(), None)?.unwrap_or_default())
            }),
            "",
            0,
        );
        let setter = r.new_native_func(
            NativeFn::closure_with(vec![Value::Object(o)], |rt, d, call| {
                let o = d[0].as_object().expect("o");
                o.set_own_str(rt, &"__hidden".into(), call.argument(0), true)?;
                Ok(Value::Undefined)
            }),
            "",
            0,
        );
        let res = o.define_own_property_str(
            &mut r,
            &"accessor_rw".into(),
            PropertyDescriptor {
                getter: Some(Value::Object(getter)),
                setter: Some(Value::Object(setter)),
                configurable: Flag::True,
                enumerable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        );
        assert!(res.is_ok());

        assert_eq!(
            get_int(&mut r, o, "accessor_ro"),
            1,
            "Unexpected accessor value"
        );

        match try_set(&mut r, o, "accessor_ro", Value::Int(2)) {
            Ok(()) => panic!("Expected an error"),
            Err(msg) => assert_eq!(
                msg, "TypeError: Cannot assign to read only property 'accessor_ro'",
                "Unexpected error"
            ),
        }

        if let Err(e) = try_set(&mut r, o, "accessor_rw", Value::Int(42)) {
            panic!("{e}");
        }
        assert_eq!(get_int(&mut r, o, "accessor_rw"), 42, "Unexpected value");
    });
}

#[test]
fn test_define_properties_symbol() {
    const SCRIPT: &str = r#"
	var desc = {};
	desc[Symbol.toStringTag] = {value: "Test"};
	var o = {};
	Object.defineProperties(o, desc);
	o[Symbol.toStringTag] === "Test";
	"#;
    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_object_shorthand_properties() {
    const SCRIPT: &str = r##"
	var b = 1;
	var a = {b, get() {return "c"}};

	assert.sameValue(a.b, b, "#1");
	assert.sameValue(a.get(), "c", "#2");

	var obj = {
		with() { return 42; }
    };

	assert.sameValue(obj['with'](), 42, 'property exists');
	"##;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_object_assign() {
    const SCRIPT: &str = r##"
	assert.sameValue(Object.assign({ b: 1 }, { get a() {
          Object.defineProperty(this, "b", {
            value: 3,
            enumerable: false
          });
        }, b: 2 }).b, 1, "#1");

	assert.sameValue(Object.assign({ b: 1 }, { get a() {
          delete this.b;
        }, b: 2 }).b, 1, "#2");
	"##;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}
