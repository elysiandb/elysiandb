mod jsrun;

use jsrun::{new_runtime, on_big_stack, rt_test_script_with_test_lib, run_error_string};
use std::cell::RefCell;
use std::rc::Rc;
use talos::host::{DocValue, HostFn, SliceHeader};
use talos::{Runtime, Throw, Value};

fn ints(v: &[i64]) -> Vec<DocValue> {
    v.iter().map(|i| DocValue::Int64(*i)).collect()
}

fn slice(items: Vec<DocValue>) -> DocValue {
    DocValue::new_slice(items)
}

fn slice_ptr(items: Vec<DocValue>) -> Rc<RefCell<SliceHeader>> {
    Rc::new(RefCell::new(SliceHeader::new(items)))
}

fn run(rt: &mut Runtime, src: &str) -> Value {
    match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

fn header_ints(p: &Rc<RefCell<SliceHeader>>) -> Vec<i64> {
    p.borrow()
        .to_vec()
        .iter()
        .map(|v| match v {
            DocValue::Int64(i) => *i,
            _ => panic!("not an int"),
        })
        .collect()
}

#[test]
fn go_slice_basic() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("a", &slice(ints(&[1, 2, 3, 4]))).expect("set");
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
fn go_slice_in() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("a", &slice(ints(&[1, 2, 3, 4]))).expect("set");
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
fn go_slice_expand() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(ints(&[1, 2, 3, 4]));
        rt.set_value("a", &DocValue::SlicePtr(a.clone()))
            .expect("set");
        let v = run(
            &mut rt,
            r#"
	var l = a.length;
	for (var i = 0; i < l; i++) {
		a[l + i] = a[i] * 2;
	}

	var sum = 0;
	for (var i = 0; i < a.length; i++) {
		sum += a[i];
	}
	sum;
	"#,
        );
        let sum: i64 = header_ints(&a).iter().sum();
        assert_eq!(v.to_integer(&mut rt).expect("int"), sum);
    });
}

#[test]
fn go_slice_proto_method() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("a", &slice(ints(&[1, 2, 3, 4]))).expect("set");
        let v = run(&mut rt, r#"a.join(",")"#);
        assert_eq!(v.go_string(&mut rt).expect("s"), "1,2,3,4");
    });
}

#[test]
fn go_slice_set_length() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(ints(&[1, 2, 3, 4]));
        rt.set_value("a", &DocValue::SlicePtr(a)).expect("set");
        run(
            &mut rt,
            r#"
	'use strict';
	a.length = 3;
	if (a.length !== 3) {
		throw new Error("length="+a.length);
	}
	if (a[3] !== undefined) {
		throw new Error("a[3](1)="+a[3]);
	}
	a.length = 5;
	if (a.length !== 5) {
		throw new Error("length="+a.length);
	}
	if (a[3] !== null) {
		throw new Error("a[3](2)="+a[3]);
	}
	if (a[4] !== null) {
		throw new Error("a[4]="+a[4]);
	}
	"#,
        );
    });
}

#[test]
fn go_slice_proto() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &DocValue::SlicePtr(a)).expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
	var proto = [,2,,4];
	Object.setPrototypeOf(a, proto);
	assert.sameValue(a[1], null, "a[1]");
	assert.sameValue(a[3], 4, "a[3]");
	var desc = Object.getOwnPropertyDescriptor(a, "1");
	assert.sameValue(desc.value, null, "desc.value");
	assert(desc.writable, "writable");
	assert(desc.enumerable, "enumerable");
	assert(!desc.configurable, "configurable");
	var v5;
	Object.defineProperty(proto, "5", {
		set: function(v) {
			v5 = v;
		}
	});
	a[5] = "test";
	assert.sameValue(v5, "test", "v5");
	"#,
            &Value::Undefined,
        );
    });
}

#[test]
fn go_slice_proto_proto() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &DocValue::SlicePtr(a)).expect("set");
        rt.set_value("proto", &slice(ints(&[1, 2, 3, 4])))
            .expect("set");
        run(
            &mut rt,
            r#"
	"use strict";
	var protoproto = Object.create(null);
	Object.defineProperty(protoproto, "3", {
		value: 42
	});
	Object.setPrototypeOf(proto, protoproto);
	Object.setPrototypeOf(a, proto);
	a[3] = 11;
	if (a[3] !== 11) {
		throw new Error("a[3]=" + a[3]);
	}
	"#,
        );
    });
}

#[test]
fn go_slice_delete() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &a).expect("set");
        let v = run(&mut rt, "delete a[0] && delete a[1] && delete a[3];");
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn go_slice_pop() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &DocValue::SlicePtr(a)).expect("set");
        let v = run(&mut rt, "a.pop()");
        assert!(v.same_as(&Value::Int(3)));
    });
}

#[test]
fn go_slice_pop_no_ptr() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &a).expect("set");
        let v = run(&mut rt, "a.pop()");
        assert!(v.same_as(&Value::Int(3)));
    });
}

#[test]
fn go_slice_shift() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = slice_ptr(vec![DocValue::Int64(1), DocValue::Nil, DocValue::Int64(3)]);
        rt.set_value("a", &DocValue::SlicePtr(a)).expect("set");
        let v = run(&mut rt, "a.shift()");
        assert!(v.same_as(&Value::Int(1)));
    });
}

#[test]
fn go_slice_length_property() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("s", &slice(ints(&[2, 3, 4]))).expect("set");
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
fn go_slice_sort() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let s = slice_ptr(ints(&[4, 2, 3]));
        rt.set_value("s", &DocValue::SlicePtr(s.clone()))
            .expect("set");
        run(&mut rt, "s.sort()");
        assert_eq!(s.borrow().len, 3);
        assert_eq!(header_ints(&s), vec![2, 3, 4]);
    });
}

#[test]
fn go_slice_to_string() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let s = slice_ptr(ints(&[4, 2, 3]));
        rt.set_value("s", &DocValue::SlicePtr(s)).expect("set");
        let res = run(&mut rt, "`${s}`");
        match &rt.export(&res).expect("export") {
            DocValue::String(s) => assert_eq!(&**s, "4,2,3"),
            _ => panic!("not a string"),
        }
    });
}

#[test]
fn go_slice_external_len_update() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let data = slice_ptr(ints(&[1]));
        rt.set_value("data", &DocValue::SlicePtr(data.clone()))
            .expect("set");
        let d2 = data.clone();
        let append = HostFn::new("append", move |rt, call| {
            let a = rt.export(call.arg(0))?;
            let DocValue::SlicePtr(p) = &a else {
                return Err(Throw::Value(rt.new_type_error("a != data")));
            };
            if !Rc::ptr_eq(p, &d2) {
                return Err(Throw::Value(rt.new_type_error("a != data")));
            }
            let v = call.arg(1).to_integer(rt)?;
            let mut h = p.borrow_mut();
            let mut items = h.to_vec();
            items.push(DocValue::Int64(v));
            *h = SliceHeader::decoded(items);
            Ok(Value::Undefined)
        });
        rt.set_value(
            "append",
            &DocValue::Func(talos::host::FuncRef::Native(append)),
        )
        .expect("set");
        rt_test_script_with_test_lib(
            &mut rt,
            r#"
		assert.sameValue(data.length, 1);

        // modify with js
        data.push(1);
		assert.sameValue(data.length, 2);

        // modify with go
        append(data, 2);
		assert.sameValue(data.length, 3);
    "#,
            &Value::Undefined,
        );
    });
}
