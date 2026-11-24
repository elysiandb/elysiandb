mod jsrun;

use goja::host::{DocValue, FuncRef, HostFn};
use goja::{Throw, Value};
use jsrun::{new_runtime, on_big_stack, run_error_string};

#[test]
fn object_get_set() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = rt.new_object();
        let v = rt.to_value(&DocValue::Int64(42)).expect("to_value");
        o.set_own_str(&mut rt, &"test".into(), v, true)
            .expect("set");
        rt.set_var("input", Value::Object(o)).expect("input");
        let v = match rt.run_string(
            r#"
		input.test++;
		input;
	"#,
        ) {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let o1 = v.as_object().expect("object");
        let v1 = o1
            .get_str(&mut rt, &"test".into(), None)
            .expect("get")
            .expect("test");
        assert!(matches!(
            rt.export(&v1).expect("export"),
            DocValue::Int64(43)
        ));
    });
}

#[test]
fn throw_from_native_func() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let f = HostFn::new("f", |rt, _call| {
            let v = rt.to_value(&DocValue::string("testError"))?;
            Err(Throw::Value(v))
        });
        rt.set_value("f", &DocValue::Func(FuncRef::Native(f)))
            .expect("f");
        let v = match rt.run_string(
            r#"
	var thrown;
	try {
		f();
	} catch (e) {
		thrown = e;
	}
	thrown;
	"#,
        ) {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        assert_eq!(v.go_string(&mut rt).expect("s"), "testError");
    });
}
