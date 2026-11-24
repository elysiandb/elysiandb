mod jsrun;

use gocompat::time::Duration;
use goja::host::goreflect::arg_int64;
use goja::host::gotime::duration_value;
use goja::host::{DocValue, FuncRef, HostFn};
use goja::{Throw, Value};
use jsrun::{new_runtime, on_big_stack, run_error_string};

#[test]
fn go_reflect_preserve_type() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let expect = i64::MAX;
        let make = HostFn::new("make", move |rt, _call| {
            rt.to_value(&duration_value(expect))
        });
        let handle = HostFn::new("handle", move |rt, call| {
            let d = arg_int64(rt, &call, 0)?;
            if Duration(d).to_string() != Duration(expect).to_string() {
                return Err(Throw::TypeError(format!("expect {expect}, but get {d}")));
            }
            Ok(Value::Undefined)
        });
        rt.set_value("make", &DocValue::Func(FuncRef::Native(make)))
            .expect("make");
        rt.set_value("handle", &DocValue::Func(FuncRef::Native(handle)))
            .expect("handle");
        if let Err(e) = rt.run_string(
            r#"
	var d=make()
	handle(d)
	"#,
        ) {
            panic!("{}", run_error_string(&e));
        }
    });
}
