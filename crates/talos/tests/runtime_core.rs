use talos::{Runtime, Throw, Value};

fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new()
        .name("talos-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

fn get(rt: &mut Runtime, o: talos::ObjRef, name: &str) -> Value {
    match o.get_str(rt, &name.into(), None) {
        Ok(Some(v)) => v,
        Ok(None) => Value::Undefined,
        Err(_) => panic!("get {name} failed"),
    }
}

fn obj(v: &Value) -> talos::ObjRef {
    match v {
        Value::Object(o) => *o,
        _ => panic!("not an object"),
    }
}

#[test]
fn boolean_template_native_call_and_type_error() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let g = rt.global_object();
        let b = obj(&get(&mut rt, g, "Boolean"));
        let call = b.assert_callable(&rt).expect("callable");
        let r = call
            .call(&mut rt, Value::Undefined, &[Value::Int(1)])
            .expect("call");
        assert!(matches!(r, Value::Bool(true)));

        let ctor = b.assert_constructor(&rt).expect("constructor");
        let o = ctor
            .construct(&mut rt, &[Value::Int(0)], Some(b))
            .expect("construct");
        let value_of = obj(&get(&mut rt, o, "valueOf"));
        let r = value_of
            .assert_callable(&rt)
            .expect("callable")
            .call(&mut rt, Value::Object(o), &[])
            .expect("valueOf");
        assert!(matches!(r, Value::Bool(false)));

        let proto = obj(&get(&mut rt, b, "prototype"));
        let to_string = obj(&get(&mut rt, proto, "toString"));
        let err = match to_string.assert_callable(&rt).expect("callable").call(
            &mut rt,
            Value::Int(1),
            &[],
        ) {
            Err(Throw::Value(v)) => v,
            _ => panic!("expected a thrown TypeError"),
        };
        let s = err.to_string(&mut rt).expect("to_string").to_rust_string();
        assert_eq!(
            s,
            "TypeError: Method Boolean.prototype.toString is called on incompatible receiver"
        );
        let type_error = obj(&get(&mut rt, g, "TypeError"));
        assert!(rt.instance_of(&err, type_error).expect("instanceof"));
    });
}

#[test]
fn collected_objects_are_released_and_roots_survive() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let g = rt.global_object();
        let kept = rt.new_object();
        g.set_own_str(&mut rt, &"kept".into(), Value::Object(kept), true)
            .expect("set");
        kept.set_own_str(&mut rt, &"x".into(), Value::Int(42), true)
            .expect("set");
        let before = rt.heap.live_objects();
        for _ in 0..10_000 {
            rt.new_object();
        }
        rt.handles.clear();
        rt.collect_garbage();
        let after = rt.heap.live_objects();
        assert!(after <= before, "{after} > {before}");
        let kept2 = obj(&get(&mut rt, g, "kept"));
        assert_eq!(kept2, kept);
        assert!(matches!(get(&mut rt, kept2, "x"), Value::Int(42)));
    });
}
