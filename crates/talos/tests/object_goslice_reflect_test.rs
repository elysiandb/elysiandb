mod jsrun;

use jsrun::{new_runtime, on_big_stack, run_error_string};
use talos::host::{DocValue, TypedSlice};
use talos::{Runtime, Value};

fn run(rt: &mut Runtime, src: &str) -> Value {
    match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

#[test]
fn go_slice_reflect_basic() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = DocValue::Int64s(TypedSlice::new(vec![1, 2, 3, 4]));
        rt.set_value("a", &a).expect("set");
        let v = run(
            &mut rt,
            r#"
	var sum = 0;
	for (var i = 0; i < a.length; i++) {
		sum += a[i];
	}
	sum;
	"#,
        );
        assert_eq!(v.to_integer(&mut rt).expect("int"), 10);
    });
}

#[test]
fn go_slice_reflect_in() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = DocValue::Int64s(TypedSlice::new(vec![1, 2, 3, 4]));
        rt.set_value("a", &a).expect("set");
        let v = run(
            &mut rt,
            r#"
	var idx = "";
	for (var i in a) {
		idx += i;
	}
	idx;
	"#,
        );
        assert_eq!(v.go_string(&mut rt).expect("s"), "0123");
    });
}

#[test]
fn go_slice_reflect_set() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let s = TypedSlice::new(vec![1i8, 2, 3, 4]);
        rt.set_value("a", &DocValue::Int8s(s.clone())).expect("set");
        run(
            &mut rt,
            r#"
	a[0] = 33;
	a[1] = 333;
	a[2] = "42";
	a[3] = {};
	a[4] = 0;
	"#,
        );
        assert_eq!(s.to_vec(), vec![33, 77, 42, 0]);
    });
}

#[test]
fn go_slice_reflect_proto_method() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = DocValue::Int8s(TypedSlice::new(vec![1, 2, 3, 4]));
        rt.set_value("a", &a).expect("set");
        let v = run(&mut rt, r#"a.join(",")"#);
        assert_eq!(v.go_string(&mut rt).expect("s"), "1,2,3,4");
    });
}

#[test]
fn go_slice_reflect_length_property() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let s = DocValue::Int64s(TypedSlice::new(vec![2, 3, 4]));
        rt.set_value("s", &s).expect("set");
        run(
            &mut rt,
            r#"
	if (!s.hasOwnProperty("length")) {
		throw new Error("hasOwnProperty() returned false");
	}
	let desc = Object.getOwnPropertyDescriptor(s, "length");
	if (desc.value !== 3 || !desc.writable || desc.enumerable || desc.configurable) {
		throw new Error("incorrect property descriptor: " + JSON.stringify(desc));
	}
	"#,
        );
    });
}

#[test]
fn go_slice_reflect_export_after_grow() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("a", &DocValue::Int64s(TypedSlice::new(vec![1])))
            .expect("set");
        let v = run(
            &mut rt,
            r#"
		a.push(2);
		a;
	"#,
        );
        match &rt.export(&v).expect("export") {
            DocValue::Int64s(a) => assert_eq!(a.to_vec(), vec![1, 2]),
            _ => panic!("wrong type"),
        }
    });
}

#[test]
fn go_slice_reflect111() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("v", &DocValue::Int32s(TypedSlice::new(vec![1, 2])))
            .expect("set");
        let v = run(
            &mut rt,
            r#"
	const tmp = v[0];
	v[0] = v[1];
	v[1] = tmp;
	"" + v[0] + v[1];
"#,
        );
        assert_eq!(v.go_string(&mut rt).expect("s"), "21");
    });
}
