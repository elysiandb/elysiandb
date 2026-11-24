use goja::builtin_symbol::{SYM_ITERATOR, SYM_SPECIES, SYM_TO_STRING_TAG};
use goja::func::{NativeCtor, NativeFn};
use goja::object::{Flag, FunctionCall, ObjExt, PropertyDescriptor};
use goja::string::ascii_string;
use goja::value::int_to_value;
use goja::{JsResult, ObjRef, Runtime, Throw, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Out = Rc<RefCell<Vec<String>>>;

const MAP_BASIC: &[&str] = &[
    "boolean:true",
    "string:a=number:6:u",
    "number:1=number:2:u",
    "obj=number:3:u",
    "number:NaN=number:4:u",
    "number:0=number:7:u",
    "number:5",
    "number:7",
    "number:7",
    "boolean:true",
    "number:4",
    "number:3",
    "undefined:undefined",
    "boolean:false",
    "boolean:true",
    "boolean:false",
    "number:4",
    "boolean:true",
    "boolean:true",
    "boolean:true",
    "boolean:true",
    "[object Map]",
    "[object Map Iterator]",
    "boolean:true",
];

const MAP_CTOR: &[&str] = &[
    "number:3",
    "string:v1",
    "undefined:undefined",
    "boolean:true",
    "boolean:true",
    "string:v3",
    "number:0",
    "number:0",
    "string:a/number:1,string:b/undefined:undefined",
    "number:2",
    "boolean:true",
    "number:1",
    "TypeError: Map.set in missing",
    "no error",
    "TypeError: 1 is not a method",
    "TypeError: object is not iterable",
    "TypeError: Value is not an object: 1",
    "boolean:true",
    "boolean:true",
    "get [Symbol.species]",
    "undefined:undefined",
];

const MAP_ERRORS: &[&str] = &[
    "TypeError: Constructor Map requires 'new'",
    "TypeError: Method Map.prototype.get called on incompatible receiver [object Object]",
    "TypeError: Method Map.prototype.set called on incompatible receiver [object Set]",
    "TypeError: Value is not an object: 1",
    "TypeError: object is not a function %!s(MISSING)",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not an object: undefined",
    "TypeError: Method get Map.prototype.size called on incompatible receiver [object Object]",
    "TypeError: Object is not a Map",
    "TypeError: Object is not a Map",
    "TypeError: Method Map Iterator.prototype.next called on incompatible receiver [object Object]",
    "TypeError: Method Map Iterator.prototype.next called on incompatible receiver [object Set Iterator]",
    "TypeError: Method Map.prototype.clear called on incompatible receiver [object WeakMap]",
    "TypeError: Method Map.prototype.has called on incompatible receiver [object Custom]",
    "TypeError: Value is not an object: null",
    "TypeError: Value is not an object: undefined",
];

const MAP_ITERATION: &[&str] = &[
    "aA",
    "cC",
    "dD",
    "eE",
    "done",
    "done",
    "number:1",
    "number:0",
    "number:4",
    "boolean:true",
    "string:w",
    "a1",
    "c3",
    "d4",
    "z0",
    "number:1",
];

const SET_BASIC: &[&str] = &[
    "boolean:true",
    "string:a=string:a:u",
    "number:1=number:1:u",
    "obj=obj:u",
    "number:NaN=number:NaN:u",
    "number:0=number:0:u",
    "number:5",
    "boolean:true",
    "boolean:true",
    "boolean:true",
    "boolean:true",
    "boolean:false",
    "boolean:true",
    "boolean:false",
    "number:4",
    "string:a",
    "string:a",
    "boolean:true",
    "boolean:true",
    "[object Set]",
    "[object Set Iterator]",
    "boolean:true",
    "boolean:true",
    "boolean:true",
    "boolean:true",
];

const SET_CTOR: &[&str] = &[
    "number:3",
    "string:x",
    "string:y",
    "number:0",
    "number:3",
    "number:1,number:2,number:2,number:3,string:p,string:q",
    "number:3",
    "number:2",
    "boolean:true",
    "TypeError: Set.add in missing",
    "TypeError: 1 is not a method",
    "no error",
    "TypeError: object is not iterable",
    "number:0",
    "number:0",
    "number:3",
];

const SET_ERRORS: &[&str] = &[
    "TypeError: Constructor Set requires 'new'",
    "TypeError: Method Set.prototype.add called on incompatible receiver [object Map]",
    "TypeError: Method Set.prototype.has called on incompatible receiver [object Object]",
    "TypeError: object is not a function %!s(MISSING)",
    "TypeError: Value is not an object: 1",
    "TypeError: Method get Set.prototype.size called on incompatible receiver [object Map]",
    "TypeError: Object is not a Set",
    "TypeError: Value is not an object: 1",
    "TypeError: Method Set Iterator.prototype.next called on incompatible receiver [object Map Iterator]",
    "TypeError: Value is not an object: null",
    "TypeError: Value is not an object: undefined",
    "TypeError: Method Set.prototype.forEach called on incompatible receiver [object WeakSet]",
];

const SET_ITERATION: &[&str] = &[
    "a",
    "c",
    "d",
    "e",
    "done",
    "done",
    "gg",
    "boolean:true",
    "number:1",
    "number:3",
    "number:4",
    "number:9",
    "number:1",
];

const WEAKMAP: &[&str] = &[
    "boolean:true",
    "string:v1",
    "boolean:true",
    "boolean:false",
    "undefined:undefined",
    "boolean:true",
    "undefined:undefined",
    "boolean:false",
    "undefined:undefined",
    "undefined:undefined",
    "boolean:false",
    "boolean:false",
    "boolean:false",
    "string:v1",
    "string:other",
    "boolean:true",
    "boolean:false",
    "string:other",
    "boolean:false",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not an object: s",
    "TypeError: Constructor WeakMap requires 'new'",
    "TypeError: Method WeakMap.prototype.get called on incompatible receiver [object Object]",
    "TypeError: Method WeakMap.prototype.set called on incompatible receiver [object WeakSet]",
    "TypeError: Value is not an object: 1",
    "string:a",
    "boolean:true",
    "undefined:undefined",
    "TypeError: Value is not an object: 5",
    "TypeError: Value is not an object: undefined",
    "number:1/number:2,undefined:undefined/undefined:undefined",
    "TypeError: WeakMap.set in missing",
    "[object WeakMap]",
];

const WEAKSET: &[&str] = &[
    "boolean:true",
    "boolean:true",
    "boolean:false",
    "boolean:false",
    "boolean:false",
    "string:wm",
    "boolean:false",
    "boolean:false",
    "boolean:true",
    "boolean:false",
    "string:wm",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not an object: null",
    "TypeError: Constructor WeakSet requires 'new'",
    "TypeError: Method WeakSet.prototype.add called on incompatible receiver [object WeakMap]",
    "TypeError: Method WeakSet.prototype.has called on incompatible receiver [object Object]",
    "boolean:true",
    "boolean:true",
    "TypeError: Value is not an object: 3",
    "boolean:true",
    "TypeError: Value is not an object: z",
    "obj,string:z,number:7",
    "TypeError: WeakSet.add in missing",
    "[object WeakSet]",
];

const PROMISE_ORDER: &[&str] = &[
    "sync",
    "a1",
    "b1",
    "then called",
    "p2 2",
    "bad getter",
    "c2",
    "p1 T",
    "throwing inthen",
    "late first",
];

const PROMISE_REJECT: &[&str] = &[
    "then:undefined:X",
    "number:7",
    "sync",
    "caught boom",
    "rej 7",
    "TypeError: Promise self-resolution",
    "kept 1",
    "chained R",
];

const PROMISE_COMBINATORS: &[&str] = &[
    "TypeError: Value is not a constructor",
    "TypeError: Value is not an object: 1",
    "sync",
    "all empty 0",
    "all bad TypeError: object is not iterable",
    "any empty 0",
    "all rejected no",
    "settled fulfilled:1:status|value,rejected:x:status|reason",
    "any AggregateError e1,e2 true",
    "any ok 5",
    "race 5",
    "all 1,2,3",
];

const PROMISE_FINALLY: &[&str] = &[
    "boolean:true",
    "TypeError: Value is not an object: 1",
    "sync",
    "f1 0",
    "f2",
    "c3 x",
    "nf 4",
    "nf2 6",
    "after 1",
    "caught 2",
];

const PROMISE_CAPABILITY: &[&str] = &[
    "C called",
    "res 5",
    "r1 c",
    "C called",
    "rej 6",
    "r2 c",
    "boolean:true",
    "boolean:false",
    "boolean:true",
    "C called",
    "t c",
    "TypeError: resolve is already set",
    "TypeError: reject is already set",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not callable: [object Object]",
    "TypeError: Value is not a constructor",
    "TypeError: Value is not an object: 1",
    "boolean:true",
    "number:1",
    "number:2",
    "[object Promise]",
    "sync",
    "then 2",
    "res tv",
];

const PROMISE_ERRORS: &[&str] = &[
    "TypeError: Constructor Promise requires 'new'",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not callable: [object Object]",
    "TypeError: Method Promise.prototype.then called on incompatible receiver [object Object]",
    "TypeError: Value is not an object: 1",
    "TypeError: Cannot convert undefined or null to object",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not callable: [object Object]",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not an object: 1",
    "TypeError: Value is not an object: undefined",
    "TypeError: Value is not a constructor",
    "TypeError: Value is not an object: null",
];

fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new()
        .name("goja-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

fn stress_env() -> bool {
    std::env::var("GOJA_GC_STRESS").is_ok_and(|v| v == "1")
}

fn new_rt() -> Runtime {
    let mut rt = Runtime::new();
    if stress_env() {
        rt.set_gc_stress(true);
    }
    rt
}

fn gc_point(rt: &mut Runtime) {
    if rt.heap.stress {
        rt.collect_garbage();
    }
}

fn keep(rt: &mut Runtime, v: &Value) {
    if let Value::Object(_) = v {
        rt.extra_roots.push(v.clone());
    }
}

fn s(x: &str) -> Value {
    Value::String(ascii_string(x))
}

fn push(out: &Out, x: impl Into<String>) {
    out.borrow_mut().push(x.into());
}

fn check(out: &Out, want: &[&str]) {
    let got = out.borrow().clone();
    assert_eq!(got, want);
}

fn obj_of(v: &Value) -> ObjRef {
    match v {
        Value::Object(o) => *o,
        other => panic!("not an object: {other:?}"),
    }
}

fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Undefined => "undefined",
        Value::Null | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Int(_) | Value::Float(_) => "number",
        Value::String(_) => "string",
        Value::Symbol(_) => "symbol",
        Value::BigInt(_) => "bigint",
        other => panic!("unexpected {other:?}"),
    }
}

fn str_of(rt: &mut Runtime, v: &Value) -> String {
    v.to_string(rt).expect("to_string").to_rust_string()
}

fn show(rt: &mut Runtime, v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Object(_) => "obj".to_string(),
        Value::Float(f) if *f == 0.0 && f.is_sign_negative() => "-0".to_string(),
        _ => format!("{}:{}", type_of(v), str_of(rt, v)),
    }
}

fn thrown(rt: &mut Runtime, r: JsResult<Value>) -> String {
    match r {
        Ok(_) => "no error".to_string(),
        Err(t) => {
            let ex = rt.vm_exception_from_throw(t);
            str_of(rt, &ex.val)
        }
    }
}

fn scoped(rt: &mut Runtime, f: impl FnOnce(&mut Runtime) -> JsResult<Value>) -> JsResult<Value> {
    let mark = rt.handle_scope_enter();
    let r = f(rt);
    rt.handle_scope_leave(mark);
    if let Ok(v) = &r {
        keep(rt, v);
    }
    r
}

fn call(rt: &mut Runtime, f: &Value, this: &Value, args: &[Value]) -> JsResult<Value> {
    let c = obj_of(f).assert_callable(rt).expect("callable");
    scoped(rt, |rt| c.call(rt, this.clone(), args))
}

fn get(rt: &mut Runtime, v: &Value, name: &str) -> Value {
    let o = obj_of(v);
    let r = o.get_str(rt, &name.into(), None).expect("get");
    let r = r.unwrap_or(Value::Undefined);
    keep(rt, &r);
    r
}

fn get_sym(rt: &mut Runtime, v: &Value, sym: goja::SymRef) -> Value {
    let o = obj_of(v);
    let r = o.get_sym(rt, sym, None).expect("get");
    let r = r.unwrap_or(Value::Undefined);
    keep(rt, &r);
    r
}

fn set(rt: &mut Runtime, v: &Value, name: &str, val: Value) {
    obj_of(v)
        .set_own_str(rt, &name.into(), val, true)
        .expect("set");
}

fn invoke(rt: &mut Runtime, v: &Value, name: &str, args: &[Value]) -> JsResult<Value> {
    let m = get(rt, v, name);
    call(rt, &m, v, args)
}

fn inv(rt: &mut Runtime, v: &Value, name: &str, args: &[Value]) -> Value {
    match invoke(rt, v, name, args) {
        Ok(r) => r,
        Err(t) => {
            let m = thrown(rt, Err(t));
            panic!("{name} threw {m}")
        }
    }
}

fn construct(rt: &mut Runtime, c: &Value, args: &[Value], nt: Option<&Value>) -> JsResult<Value> {
    let co = obj_of(c);
    let ctor = co.assert_constructor(rt).expect("constructor");
    let nt = nt.map(obj_of).unwrap_or(co);
    scoped(rt, |rt| {
        Ok(Value::Object(ctor.construct(rt, args, Some(nt))?))
    })
}

fn new(rt: &mut Runtime, c: &Value, args: &[Value]) -> Value {
    construct(rt, c, args, None).expect("construct")
}

fn global(rt: &mut Runtime, name: &str) -> Value {
    let g = Value::Object(rt.global_object());
    get(rt, &g, name)
}

fn new_obj(rt: &mut Runtime) -> Value {
    let o = Value::Object(rt.new_object());
    keep(rt, &o);
    o
}

fn native(
    rt: &mut Runtime,
    len: i64,
    f: impl Fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value> + 'static,
) -> Value {
    let o = rt.new_native_func(
        NativeFn::closure_with(Vec::new(), move |rt, _d, call| {
            gc_point(rt);
            f(rt, call)
        }),
        "",
        len,
    );
    let v = Value::Object(o);
    keep(rt, &v);
    v
}

fn native_ctor(
    rt: &mut Runtime,
    f: impl Fn(&mut Runtime, &[Value]) -> JsResult<Value> + 'static,
) -> Value {
    let o = rt.new_native_func_and_construct(
        None,
        None,
        Some(NativeCtor::closure_with(
            Vec::new(),
            move |rt, _d, args, _nt| {
                gc_point(rt);
                Ok(obj_of(&f(rt, args)?))
            },
        )),
        None,
        "",
        int_to_value(1),
    );
    let v = Value::Object(o);
    keep(rt, &v);
    v
}

fn throw_str(x: &str) -> Throw {
    Throw::Value(s(x))
}

fn getter(rt: &mut Runtime, v: &Value, name: &str, g: Value) {
    obj_of(v)
        .define_own_property_str(
            rt,
            &name.into(),
            PropertyDescriptor {
                getter: Some(g),
                configurable: Flag::True,
                enumerable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        )
        .expect("define");
}

fn pair(rt: &mut Runtime, k: Option<Value>, v: Option<Value>) -> Value {
    let o = new_obj(rt);
    if let Some(k) = k {
        set(rt, &o, "0", k);
    }
    if let Some(v) = v {
        set(rt, &o, "1", v);
    }
    o
}

fn iterable(rt: &mut Runtime, items: Vec<Value>) -> Value {
    let idx = Rc::new(Cell::new(0usize));
    let next = rt.new_native_func(
        NativeFn::closure_with(items, move |rt, d, _call| {
            gc_point(rt);
            let r = rt.new_object();
            let i = idx.get();
            if i < d.len() {
                idx.set(i + 1);
                r.set_own_str(rt, &"value".into(), d[i].clone(), true)?;
                r.set_own_str(rt, &"done".into(), Value::Bool(false), true)?;
            } else {
                r.set_own_str(rt, &"value".into(), Value::Undefined, true)?;
                r.set_own_str(rt, &"done".into(), Value::Bool(true), true)?;
            }
            Ok(Value::Object(r))
        }),
        "next",
        0,
    );
    let it = rt.new_object();
    it.set_own_str(rt, &"next".into(), Value::Object(next), true)
        .expect("set");
    let iter_fn = rt.new_native_func(
        NativeFn::closure_with(vec![Value::Object(it)], |_rt, d, _call| Ok(d[0].clone())),
        "",
        0,
    );
    let o = rt.new_object();
    o.set_own_sym(rt, SYM_ITERATOR, Value::Object(iter_fn), true)
        .expect("set");
    let o = Value::Object(o);
    keep(rt, &o);
    o
}

fn array_values(rt: &Runtime, v: &Value) -> Vec<Value> {
    match &rt.obj(obj_of(v)).ext {
        ObjExt::Array(a) => a.values.clone(),
        _ => panic!("not an array"),
    }
}

fn join(rt: &mut Runtime, values: &[Value], sep: &str) -> String {
    let mut parts = Vec::new();
    for v in values {
        parts.push(str_of(rt, v));
    }
    parts.join(sep)
}

fn std_array(rt: &mut Runtime, values: Vec<Value>) -> Value {
    let a = rt.new_array_values(values);
    let values_fn = rt.get_array_values();
    a.set_own_sym(rt, SYM_ITERATOR, Value::Object(values_fn), true)
        .expect("set");
    let a = Value::Object(a);
    keep(rt, &a);
    a
}

fn to_string_tag(rt: &mut Runtime, v: &Value) -> String {
    let r = rt
        .objectproto_to_string(FunctionCall::new(v, &[]))
        .expect("toString");
    str_of(rt, &r)
}

fn drain(rt: &mut Runtime) {
    while !rt.job_queue.is_empty() {
        let job = rt.job_queue.remove(0);
        if let Err(t) = job.run(rt) {
            let m = thrown(rt, Err(t));
            panic!("job failed: {m}");
        }
    }
}

fn proto_method(rt: &mut Runtime, ctor: &Value, name: &str) -> Value {
    let p = get(rt, ctor, "prototype");
    get(rt, &p, name)
}

fn accessor_getter(rt: &mut Runtime, v: &Value, name: &str) -> Value {
    match obj_of(v)
        .get_own_prop_str(rt, &name.into())
        .expect("own prop")
    {
        Some(Value::Property(p)) => Value::Object(p.getter_func.expect("getter")),
        other => panic!("{name} is not an accessor: {other:?}"),
    }
}

fn subclass_new_target(rt: &mut Runtime, parent: &Value) -> (Value, Value) {
    let parent_proto = get(rt, parent, "prototype");
    let p = Value::Object(rt.create_object(Some(obj_of(&parent_proto))));
    keep(rt, &p);
    let nt = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
    set(rt, &nt, "prototype", p.clone());
    (nt, p)
}

#[test]
fn map_insertion_order_negative_zero_and_nan_keys() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let map = global(rt, "Map");
        let o = new_obj(rt);
        let m = new(rt, &map, &[]);
        let r = inv(rt, &m, "set", &[s("a"), Value::Int(1)]);
        push(&out, show(rt, &Value::Bool(r.same_as(&m))));
        for (k, v) in [
            (Value::Int(1), 2),
            (o.clone(), 3),
            (Value::Float(f64::NAN), 4),
            (Value::Float(-0.0), 5),
            (s("a"), 6),
            (Value::Int(0), 7),
        ] {
            inv(rt, &m, "set", &[k, Value::Int(v)]);
        }
        let (o2, m2) = (out.clone(), m.clone());
        let cb = native(rt, 3, move |rt, call| {
            let k = call.argument(1);
            let v = call.argument(0);
            let line = format!(
                "{}={}{}{}",
                show(rt, &k),
                show(rt, &v),
                if call.argument(2).same_as(&m2) {
                    ""
                } else {
                    "!"
                },
                if call.this.is_undefined() { ":u" } else { ":t" }
            );
            push(&o2, line);
            Ok(Value::Undefined)
        });
        inv(rt, &m, "forEach", &[cb]);
        let mut vals = vec![get(rt, &m, "size")];
        vals.push(inv(rt, &m, "get", &[Value::Int(0)]));
        vals.push(inv(rt, &m, "get", &[Value::Float(-0.0)]));
        vals.push(inv(rt, &m, "has", &[Value::Float(f64::NAN)]));
        vals.push(inv(rt, &m, "get", &[Value::Float(f64::NAN)]));
        vals.push(inv(rt, &m, "get", std::slice::from_ref(&o)));
        vals.push(inv(rt, &m, "get", &[s("b")]));
        let other = new_obj(rt);
        vals.push(inv(rt, &m, "has", &[other]));
        vals.push(inv(rt, &m, "delete", &[Value::Float(f64::NAN)]));
        vals.push(inv(rt, &m, "delete", &[Value::Float(f64::NAN)]));
        vals.push(get(rt, &m, "size"));
        for v in &vals {
            push(&out, show(rt, v));
        }
        let tag = new_obj(rt);
        let (o3, t3) = (out.clone(), tag.clone());
        let cb2 = native(rt, 0, move |rt, call| {
            let b = Value::Bool(call.this.same_as(&t3));
            push(&o3, show(rt, &b));
            Ok(Value::Undefined)
        });
        inv(rt, &m, "forEach", &[cb2, tag]);
        let it = inv(rt, &m, "entries", &[]);
        push(&out, to_string_tag(rt, &m));
        push(&out, to_string_tag(rt, &it));
        let keys = inv(rt, &m, "keys", &[]);
        let f = get_sym(rt, &keys, SYM_ITERATOR);
        let same = call(rt, &f, &keys, &[]).expect("iterator");
        push(&out, show(rt, &Value::Bool(same.same_as(&keys))));
        check(&out, MAP_BASIC);
    });
}

fn iter_step(rt: &mut Runtime, it: &Value, out: &Out, pair_value: bool) {
    let r = inv(rt, it, "next", &[]);
    if get(rt, &r, "done").to_boolean() {
        push(out, "done");
        return;
    }
    let v = get(rt, &r, "value");
    if pair_value {
        let vals = array_values(rt, &v);
        let line = format!("{}{}", str_of(rt, &vals[0]), str_of(rt, &vals[1]));
        push(out, line);
    } else {
        let line = str_of(rt, &v);
        push(out, line);
    }
}

#[test]
fn map_iteration_with_deletion_clear_and_foreach_mutation() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let map = global(rt, "Map");
        let m = new(rt, &map, &[]);
        for k in ["a", "b", "c", "d"] {
            inv(rt, &m, "set", &[s(k), s(&k.to_uppercase())]);
        }
        let it = inv(rt, &m, "entries", &[]);
        iter_step(rt, &it, &out, true);
        inv(rt, &m, "delete", &[s("b")]);
        iter_step(rt, &it, &out, true);
        inv(rt, &m, "delete", &[s("c")]);
        inv(rt, &m, "delete", &[s("a")]);
        inv(rt, &m, "set", &[s("e"), s("E")]);
        iter_step(rt, &it, &out, true);
        inv(rt, &m, "delete", &[s("d")]);
        iter_step(rt, &it, &out, true);
        iter_step(rt, &it, &out, true);
        inv(rt, &m, "set", &[s("f"), s("F")]);
        iter_step(rt, &it, &out, true);

        let m2 = new(rt, &map, &[]);
        inv(rt, &m2, "set", &[Value::Int(1), s("x")]);
        inv(rt, &m2, "set", &[Value::Int(2), s("y")]);
        inv(rt, &m2, "set", &[Value::Int(3), s("z")]);
        let it2 = inv(rt, &m2, "keys", &[]);
        let r = inv(rt, &it2, "next", &[]);
        let v = get(rt, &r, "value");
        push(&out, show(rt, &v));
        inv(rt, &m2, "clear", &[]);
        let size = get(rt, &m2, "size");
        push(&out, show(rt, &size));
        inv(rt, &m2, "set", &[Value::Int(4), s("w")]);
        let r = inv(rt, &it2, "next", &[]);
        let v = get(rt, &r, "value");
        push(&out, show(rt, &v));
        let r = inv(rt, &it2, "next", &[]);
        let d = get(rt, &r, "done");
        push(&out, show(rt, &d));
        let vit = inv(rt, &m2, "values", &[]);
        let r = inv(rt, &vit, "next", &[]);
        let v = get(rt, &r, "value");
        push(&out, show(rt, &v));

        let m3 = new(rt, &map, &[]);
        inv(rt, &m3, "set", &[s("a"), Value::Int(1)]);
        inv(rt, &m3, "set", &[s("b"), Value::Int(2)]);
        inv(rt, &m3, "set", &[s("c"), Value::Int(3)]);
        let (o2, mm) = (out.clone(), m3.clone());
        let cb = native(rt, 2, move |rt, call| {
            let k = str_of(rt, call.arg(1));
            let v = str_of(rt, call.arg(0));
            push(&o2, format!("{k}{v}"));
            if k == "a" {
                inv(rt, &mm, "delete", &[s("b")]);
                inv(rt, &mm, "set", &[s("d"), Value::Int(4)]);
            }
            if k == "c" {
                inv(rt, &mm, "delete", &[s("a")]);
                inv(rt, &mm, "set", &[s("a"), Value::Int(5)]);
            }
            if k == "d" {
                inv(rt, &mm, "clear", &[]);
                inv(rt, &mm, "set", &[s("z"), Value::Int(0)]);
            }
            Ok(Value::Undefined)
        });
        inv(rt, &m3, "forEach", &[cb]);
        let size = get(rt, &m3, "size");
        push(&out, show(rt, &size));
        check(&out, MAP_ITERATION);
    });
}

#[test]
fn map_constructor_adder_and_species() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let map = global(rt, "Map");
        let p1 = pair(rt, Some(s("k1")), Some(s("v1")));
        let p2 = pair(rt, Some(s("k2")), None);
        let p3 = pair(rt, None, Some(s("v3")));
        let items = iterable(rt, vec![p1, p2, p3]);
        let m = new(rt, &map, &[items]);
        let mut vals = vec![get(rt, &m, "size")];
        vals.push(inv(rt, &m, "get", &[s("k1")]));
        vals.push(inv(rt, &m, "get", &[s("k2")]));
        vals.push(inv(rt, &m, "has", &[s("k2")]));
        vals.push(inv(rt, &m, "has", &[Value::Undefined]));
        vals.push(inv(rt, &m, "get", &[Value::Undefined]));
        let e1 = new(rt, &map, &[Value::Undefined]);
        vals.push(get(rt, &e1, "size"));
        let e2 = new(rt, &map, &[Value::Null]);
        vals.push(get(rt, &e2, "size"));
        for v in &vals {
            push(&out, show(rt, v));
        }

        let log = Out::default();
        let (nt, p) = subclass_new_target(rt, &map);
        let map_set = proto_method(rt, &map, "set");
        let l2 = log.clone();
        let adder = native(rt, 2, move |rt, call| {
            let line = format!("{}/{}", show(rt, call.arg(0)), show(rt, call.arg(1)));
            push(&l2, line);
            call_value(rt, &map_set, call.this, call.arguments)
        });
        set(rt, &p, "set", adder);
        let a = pair(rt, Some(s("a")), Some(Value::Int(1)));
        let b = pair(rt, Some(s("b")), None);
        let items = iterable(rt, vec![a, b]);
        let m2 = construct(rt, &map, &[items], Some(&nt)).expect("construct");
        push(&out, log.borrow().join(","));
        let size = get(rt, &m2, "size");
        push(&out, show(rt, &size));
        let proto = obj_of(&m2).proto(rt).expect("proto").map(Value::Object);
        push(
            &out,
            show(rt, &Value::Bool(proto.is_some_and(|x| x.same_as(&p)))),
        );
        let va = inv(rt, &m2, "get", &[s("a")]);
        push(&out, show(rt, &va));

        let (nt2, p2) = subclass_new_target(rt, &map);
        set(rt, &p2, "set", Value::Undefined);
        let items = iterable(rt, vec![]);
        let r = construct(rt, &map, &[items], Some(&nt2));
        push(&out, thrown(rt, r));
        let r = construct(rt, &map, &[Value::Undefined], Some(&nt2));
        push(&out, thrown(rt, r));
        set(rt, &p2, "set", Value::Int(1));
        let r = construct(rt, &map, &[Value::Int(5)], Some(&nt2));
        push(&out, thrown(rt, r));
        let r = construct(rt, &map, &[Value::Int(5)], None);
        push(&out, thrown(rt, r));
        let items = iterable(rt, vec![Value::Int(1)]);
        let r = construct(rt, &map, &[items], None);
        push(&out, thrown(rt, r));

        let g = species_getter(rt, &map);
        let x = new_obj(rt);
        let r = call(rt, &g, &x, &[]).expect("species");
        push(&out, show(rt, &Value::Bool(r.same_as(&x))));
        let sp = get_sym(rt, &map, SYM_SPECIES);
        push(&out, show(rt, &Value::Bool(sp.same_as(&map))));
        let name = get(rt, &g, "name");
        push(&out, str_of(rt, &name));
        let setter = match obj_of(&map).get_own_prop_sym(rt, SYM_SPECIES).expect("own") {
            Some(Value::Property(p)) => p.setter_func.map(Value::Object),
            _ => panic!("species"),
        };
        let setter = setter.unwrap_or(Value::Undefined);
        push(&out, show(rt, &setter));
        check(&out, MAP_CTOR);
    });
}

fn call_value(rt: &mut Runtime, f: &Value, this: &Value, args: &[Value]) -> JsResult<Value> {
    call(rt, f, this, args)
}

fn species_getter(rt: &mut Runtime, c: &Value) -> Value {
    match obj_of(c).get_own_prop_sym(rt, SYM_SPECIES).expect("own") {
        Some(Value::Property(p)) => {
            let g = Value::Object(p.getter_func.expect("getter"));
            keep(rt, &g);
            g
        }
        other => panic!("species is {other:?}"),
    }
}

#[test]
fn map_error_messages() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let map = global(rt, "Map");
        let set_c = global(rt, "Set");
        let weak_map = global(rt, "WeakMap");
        let r = call(rt, &map, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "get");
        let o = new_obj(rt);
        let r = call(rt, &f, &o, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "set");
        let st = new(rt, &set_c, &[]);
        let r = call(rt, &f, &st, &[Value::Int(1), Value::Int(1)]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "get");
        let r = call(rt, &f, &Value::Int(1), &[]);
        push(&out, thrown(rt, r));
        let m = new(rt, &map, &[]);
        let o = new_obj(rt);
        let r = invoke(rt, &m, "forEach", &[o]);
        push(&out, thrown(rt, r));
        let r = invoke(rt, &m, "forEach", &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let r = invoke(rt, &m, "forEach", &[]);
        push(&out, thrown(rt, r));
        let mp = get(rt, &map, "prototype");
        let g = accessor_getter(rt, &mp, "size");
        let o = new_obj(rt);
        let r = call(rt, &g, &o, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "entries");
        let o = new_obj(rt);
        let r = call(rt, &f, &o, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "keys");
        let st = new(rt, &set_c, &[]);
        let r = call(rt, &f, &st, &[]);
        push(&out, thrown(rt, r));
        let it = inv(rt, &m, "keys", &[]);
        let next = get(rt, &it, "next");
        let o = new_obj(rt);
        let r = call(rt, &next, &o, &[]);
        push(&out, thrown(rt, r));
        let st = new(rt, &set_c, &[]);
        let sit = inv(rt, &st, "values", &[]);
        let r = call(rt, &next, &sit, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "clear");
        let wm = new(rt, &weak_map, &[]);
        let r = call(rt, &f, &wm, &[]);
        push(&out, thrown(rt, r));
        let tagged = new_obj(rt);
        obj_of(&tagged)
            .set_own_sym(rt, SYM_TO_STRING_TAG, s("Custom"), true)
            .expect("set");
        let f = proto_method(rt, &map, "has");
        let r = call(rt, &f, &tagged, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "delete");
        let r = call(rt, &f, &Value::Null, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &map, "values");
        let r = call(rt, &f, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        check(&out, MAP_ERRORS);
    });
}

#[test]
fn set_insertion_order_entries_and_aliases() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let set_c = global(rt, "Set");
        let o = new_obj(rt);
        let st = new(rt, &set_c, &[]);
        let r = inv(rt, &st, "add", &[s("a")]);
        push(&out, show(rt, &Value::Bool(r.same_as(&st))));
        for k in [
            Value::Int(1),
            o.clone(),
            Value::Float(f64::NAN),
            Value::Float(-0.0),
            s("a"),
            Value::Int(0),
            Value::Float(f64::NAN),
        ] {
            inv(rt, &st, "add", &[k]);
        }
        let (o2, s2) = (out.clone(), st.clone());
        let cb = native(rt, 3, move |rt, call| {
            let line = format!(
                "{}={}{}{}",
                show(rt, call.arg(0)),
                show(rt, call.arg(1)),
                if call.argument(2).same_as(&s2) {
                    ""
                } else {
                    "!"
                },
                if call.this.is_undefined() { ":u" } else { ":t" }
            );
            push(&o2, line);
            Ok(Value::Undefined)
        });
        inv(rt, &st, "forEach", &[cb]);
        let mut vals = vec![get(rt, &st, "size")];
        vals.push(inv(rt, &st, "has", &[Value::Int(0)]));
        vals.push(inv(rt, &st, "has", &[Value::Float(-0.0)]));
        vals.push(inv(rt, &st, "has", &[Value::Float(f64::NAN)]));
        vals.push(inv(rt, &st, "has", std::slice::from_ref(&o)));
        let other = new_obj(rt);
        vals.push(inv(rt, &st, "has", &[other]));
        vals.push(inv(rt, &st, "delete", &[Value::Float(-0.0)]));
        vals.push(inv(rt, &st, "delete", &[Value::Int(0)]));
        vals.push(get(rt, &st, "size"));
        for v in &vals {
            push(&out, show(rt, v));
        }
        let it = inv(rt, &st, "entries", &[]);
        let r = inv(rt, &it, "next", &[]);
        let e = get(rt, &r, "value");
        let e = array_values(rt, &e);
        for v in &e {
            push(&out, show(rt, v));
        }
        let keys = proto_method(rt, &set_c, "keys");
        let values = proto_method(rt, &set_c, "values");
        let sp = get(rt, &set_c, "prototype");
        let iter = get_sym(rt, &sp, SYM_ITERATOR);
        push(&out, show(rt, &Value::Bool(keys.same_as(&values))));
        push(&out, show(rt, &Value::Bool(iter.same_as(&values))));
        push(&out, to_string_tag(rt, &st));
        let vit = inv(rt, &st, "values", &[]);
        push(&out, to_string_tag(rt, &vit));
        let tag = new_obj(rt);
        let (o3, t3) = (out.clone(), tag.clone());
        let cb2 = native(rt, 0, move |rt, call| {
            let b = Value::Bool(call.this.same_as(&t3));
            push(&o3, show(rt, &b));
            Ok(Value::Undefined)
        });
        inv(rt, &st, "forEach", &[cb2, tag]);
        check(&out, SET_BASIC);
    });
}

#[test]
fn set_iteration_with_deletion_clear_and_foreach_mutation() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let set_c = global(rt, "Set");
        let st = new(rt, &set_c, &[]);
        for k in ["a", "b", "c", "d"] {
            inv(rt, &st, "add", &[s(k)]);
        }
        let it = inv(rt, &st, "values", &[]);
        iter_step(rt, &it, &out, false);
        inv(rt, &st, "delete", &[s("b")]);
        iter_step(rt, &it, &out, false);
        inv(rt, &st, "delete", &[s("c")]);
        inv(rt, &st, "delete", &[s("a")]);
        inv(rt, &st, "add", &[s("e")]);
        iter_step(rt, &it, &out, false);
        inv(rt, &st, "delete", &[s("d")]);
        iter_step(rt, &it, &out, false);
        iter_step(rt, &it, &out, false);
        inv(rt, &st, "add", &[s("f")]);
        iter_step(rt, &it, &out, false);
        let it2 = inv(rt, &st, "entries", &[]);
        inv(rt, &st, "clear", &[]);
        inv(rt, &st, "add", &[s("g")]);
        iter_step(rt, &it2, &out, true);
        let r = inv(rt, &it2, "next", &[]);
        let d = get(rt, &r, "done");
        push(&out, show(rt, &d));

        let s3 = new(rt, &set_c, &[]);
        for v in 1..=3 {
            inv(rt, &s3, "add", &[Value::Int(v)]);
        }
        let (o2, ss) = (out.clone(), s3.clone());
        let cb = native(rt, 1, move |rt, call| {
            let v = call.argument(0);
            push(&o2, show(rt, &v));
            if matches!(v, Value::Int(1)) {
                inv(rt, &ss, "delete", &[Value::Int(2)]);
                inv(rt, &ss, "add", &[Value::Int(4)]);
            }
            if matches!(v, Value::Int(3)) {
                inv(rt, &ss, "delete", &[Value::Int(1)]);
                inv(rt, &ss, "add", &[Value::Int(1)]);
            }
            if matches!(v, Value::Int(4)) {
                inv(rt, &ss, "clear", &[]);
                inv(rt, &ss, "add", &[Value::Int(9)]);
            }
            Ok(Value::Undefined)
        });
        inv(rt, &s3, "forEach", &[cb]);
        let size = get(rt, &s3, "size");
        push(&out, show(rt, &size));
        check(&out, SET_ITERATION);
    });
}

#[test]
fn set_constructor_std_array_adder_and_species() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let set_c = global(rt, "Set");
        let items = iterable(
            rt,
            vec![s("x"), s("y"), s("x"), Value::Float(-0.0), Value::Int(0)],
        );
        let st = new(rt, &set_c, &[items]);
        let size = get(rt, &st, "size");
        push(&out, show(rt, &size));
        let o2 = out.clone();
        let cb = native(rt, 1, move |rt, call| {
            push(&o2, show(rt, call.arg(0)));
            Ok(Value::Undefined)
        });
        inv(rt, &st, "forEach", &[cb]);
        let arr = std_array(
            rt,
            vec![Value::Int(1), Value::Int(2), Value::Int(2), Value::Int(3)],
        );
        let s2 = new(rt, &set_c, std::slice::from_ref(&arr));
        let size = get(rt, &s2, "size");
        push(&out, show(rt, &size));
        let log = Out::default();
        let (nt, p) = subclass_new_target(rt, &set_c);
        let add = proto_method(rt, &set_c, "add");
        let l2 = log.clone();
        let adder = native(rt, 1, move |rt, call| {
            push(&l2, show(rt, call.arg(0)));
            call_value(rt, &add, call.this, call.arguments)
        });
        set(rt, &p, "add", adder);
        let s3 = construct(rt, &set_c, &[arr], Some(&nt)).expect("construct");
        let items = iterable(rt, vec![s("p"), s("q")]);
        let s4 = construct(rt, &set_c, &[items], Some(&nt)).expect("construct");
        push(&out, log.borrow().join(","));
        let size = get(rt, &s3, "size");
        push(&out, show(rt, &size));
        let size = get(rt, &s4, "size");
        push(&out, show(rt, &size));
        let proto = obj_of(&s4).proto(rt).expect("proto").map(Value::Object);
        push(
            &out,
            show(rt, &Value::Bool(proto.is_some_and(|x| x.same_as(&p)))),
        );
        let (nt2, p2) = subclass_new_target(rt, &set_c);
        set(rt, &p2, "add", Value::Undefined);
        let items = iterable(rt, vec![]);
        let r = construct(rt, &set_c, &[items], Some(&nt2));
        push(&out, thrown(rt, r));
        set(rt, &p2, "add", Value::Int(1));
        let items = iterable(rt, vec![]);
        let r = construct(rt, &set_c, &[items], Some(&nt2));
        push(&out, thrown(rt, r));
        let r = construct(rt, &set_c, &[Value::Null], Some(&nt2));
        push(&out, thrown(rt, r));
        let r = construct(rt, &set_c, &[Value::Int(5)], None);
        push(&out, thrown(rt, r));
        let e1 = new(rt, &set_c, &[Value::Null]);
        let size = get(rt, &e1, "size");
        push(&out, show(rt, &size));
        let e2 = new(rt, &set_c, &[Value::Undefined]);
        let size = get(rt, &e2, "size");
        push(&out, show(rt, &size));
        let g = species_getter(rt, &set_c);
        let r = call(rt, &g, &Value::Int(3), &[]).expect("species");
        push(&out, show(rt, &r));
        check(&out, SET_CTOR);
    });
}

#[test]
fn set_error_messages() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let set_c = global(rt, "Set");
        let map = global(rt, "Map");
        let weak_set = global(rt, "WeakSet");
        let r = call(rt, &set_c, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "add");
        let m = new(rt, &map, &[]);
        let r = call(rt, &f, &m, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "has");
        let o = new_obj(rt);
        let r = call(rt, &f, &o, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let st = new(rt, &set_c, &[]);
        let o = new_obj(rt);
        let r = invoke(rt, &st, "forEach", &[o]);
        push(&out, thrown(rt, r));
        let r = invoke(rt, &st, "forEach", &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let sp = get(rt, &set_c, "prototype");
        let g = accessor_getter(rt, &sp, "size");
        let r = call(rt, &g, &m, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "values");
        let r = call(rt, &f, &m, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "entries");
        let r = call(rt, &f, &Value::Int(1), &[]);
        push(&out, thrown(rt, r));
        let it = inv(rt, &st, "values", &[]);
        let next = get(rt, &it, "next");
        let mit = inv(rt, &m, "keys", &[]);
        let r = call(rt, &next, &mit, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "clear");
        let r = call(rt, &f, &Value::Null, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "delete");
        let r = call(rt, &f, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &set_c, "forEach");
        let ws = new(rt, &weak_set, &[]);
        let noop = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
        let r = call(rt, &f, &ws, &[noop]);
        push(&out, thrown(rt, r));
        check(&out, SET_ERRORS);
    });
}

#[test]
fn weak_map_semantics_and_errors() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let weak_map = global(rt, "WeakMap");
        let weak_set = global(rt, "WeakSet");
        let k1 = new_obj(rt);
        let k2 = new_obj(rt);
        let f = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
        let w = new(rt, &weak_map, &[]);
        let w2 = new(rt, &weak_map, &[]);
        let r = inv(rt, &w, "set", &[k1.clone(), s("v1")]);
        push(&out, show(rt, &Value::Bool(r.same_as(&w))));
        inv(rt, &w, "set", &[f.clone(), Value::Undefined]);
        let mut vals = vec![inv(rt, &w, "get", std::slice::from_ref(&k1))];
        vals.push(inv(rt, &w, "has", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w2, "has", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w2, "get", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w, "has", std::slice::from_ref(&f)));
        vals.push(inv(rt, &w, "get", std::slice::from_ref(&f)));
        vals.push(inv(rt, &w, "has", std::slice::from_ref(&k2)));
        vals.push(inv(rt, &w, "get", std::slice::from_ref(&k2)));
        vals.push(inv(rt, &w, "get", &[Value::Int(1)]));
        vals.push(inv(rt, &w, "has", &[s("x")]));
        vals.push(inv(rt, &w, "delete", &[Value::Int(1)]));
        vals.push(inv(rt, &w, "delete", std::slice::from_ref(&k2)));
        inv(rt, &w2, "set", &[k1.clone(), s("other")]);
        vals.push(inv(rt, &w, "get", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w2, "get", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w, "delete", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w, "has", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w2, "get", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w, "delete", std::slice::from_ref(&k1)));
        for v in &vals {
            push(&out, show(rt, v));
        }
        for k in [Value::Int(1), Value::Undefined, s("s")] {
            let r = invoke(rt, &w, "set", &[k, Value::Int(1)]);
            push(&out, thrown(rt, r));
        }
        let r = call(rt, &weak_map, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let g = proto_method(rt, &weak_map, "get");
        let o = new_obj(rt);
        let r = call(rt, &g, &o, std::slice::from_ref(&k1));
        push(&out, thrown(rt, r));
        let st = proto_method(rt, &weak_map, "set");
        let ws = new(rt, &weak_set, &[]);
        let r = call(rt, &st, &ws, &[k1.clone(), Value::Int(1)]);
        push(&out, thrown(rt, r));
        let h = proto_method(rt, &weak_map, "has");
        let r = call(rt, &h, &Value::Int(1), std::slice::from_ref(&k1));
        push(&out, thrown(rt, r));
        let a = pair(rt, Some(k1.clone()), Some(s("a")));
        let b = pair(rt, Some(k2.clone()), None);
        let items = iterable(rt, vec![a, b]);
        let w3 = new(rt, &weak_map, &[items]);
        let mut vals = vec![inv(rt, &w3, "get", std::slice::from_ref(&k1))];
        vals.push(inv(rt, &w3, "has", std::slice::from_ref(&k2)));
        vals.push(inv(rt, &w3, "get", std::slice::from_ref(&k2)));
        for v in &vals {
            push(&out, show(rt, v));
        }
        let a = pair(rt, Some(k1.clone()), Some(s("a")));
        let b = pair(rt, Some(Value::Int(5)), Some(s("x")));
        let items = iterable(rt, vec![a, b]);
        let r = construct(rt, &weak_map, &[items], None);
        push(&out, thrown(rt, r));
        let a = pair(rt, None, Some(s("x")));
        let items = iterable(rt, vec![a]);
        let r = construct(rt, &weak_map, &[items], None);
        push(&out, thrown(rt, r));
        let log = Out::default();
        let (nt, p) = subclass_new_target(rt, &weak_map);
        let l2 = log.clone();
        let adder = native(rt, 2, move |rt, call| {
            let line = format!("{}/{}", show(rt, call.arg(0)), show(rt, call.arg(1)));
            push(&l2, line);
            Ok(Value::Undefined)
        });
        set(rt, &p, "set", adder);
        let a = pair(rt, Some(Value::Int(1)), Some(Value::Int(2)));
        let b = new_obj(rt);
        let items = iterable(rt, vec![a, b]);
        construct(rt, &weak_map, &[items], Some(&nt)).expect("construct");
        push(&out, log.borrow().join(","));
        set(rt, &p, "set", Value::Null);
        let items = iterable(rt, vec![]);
        let r = construct(rt, &weak_map, &[items], Some(&nt));
        push(&out, thrown(rt, r));
        push(&out, to_string_tag(rt, &w));
        check(&out, WEAKMAP);
    });
}

#[test]
fn weak_set_semantics_and_errors() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let weak_map = global(rt, "WeakMap");
        let weak_set = global(rt, "WeakSet");
        let k1 = new_obj(rt);
        let k2 = new_obj(rt);
        let st = new(rt, &weak_set, &[]);
        let s2 = new(rt, &weak_set, &[]);
        let w = new(rt, &weak_map, &[]);
        let r = inv(rt, &st, "add", std::slice::from_ref(&k1));
        push(&out, show(rt, &Value::Bool(r.same_as(&st))));
        inv(rt, &w, "set", &[k1.clone(), s("wm")]);
        let mut vals = vec![inv(rt, &st, "has", std::slice::from_ref(&k1))];
        vals.push(inv(rt, &s2, "has", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &st, "has", std::slice::from_ref(&k2)));
        vals.push(inv(rt, &st, "has", &[Value::Int(1)]));
        vals.push(inv(rt, &w, "get", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &st, "delete", &[Value::Int(1)]));
        vals.push(inv(rt, &st, "delete", std::slice::from_ref(&k2)));
        vals.push(inv(rt, &st, "delete", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &st, "has", std::slice::from_ref(&k1)));
        vals.push(inv(rt, &w, "get", std::slice::from_ref(&k1)));
        for v in &vals {
            push(&out, show(rt, v));
        }
        let r = invoke(rt, &st, "add", &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let r = invoke(rt, &st, "add", &[Value::Null]);
        push(&out, thrown(rt, r));
        let r = call(rt, &weak_set, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &weak_set, "add");
        let r = call(rt, &f, &w, std::slice::from_ref(&k1));
        push(&out, thrown(rt, r));
        let f = proto_method(rt, &weak_set, "has");
        let o = new_obj(rt);
        let r = call(rt, &f, &o, std::slice::from_ref(&k1));
        push(&out, thrown(rt, r));
        let items = iterable(rt, vec![k1.clone(), k2.clone()]);
        let s3 = new(rt, &weak_set, &[items]);
        let a = inv(rt, &s3, "has", std::slice::from_ref(&k1));
        let b = inv(rt, &s3, "has", std::slice::from_ref(&k2));
        push(&out, show(rt, &a));
        push(&out, show(rt, &b));
        let items = iterable(rt, vec![k1.clone(), Value::Int(3)]);
        let r = construct(rt, &weak_set, &[items], None);
        push(&out, thrown(rt, r));
        let arr = std_array(rt, vec![k1.clone(), k2.clone()]);
        let s4 = new(rt, &weak_set, &[arr]);
        let b = inv(rt, &s4, "has", std::slice::from_ref(&k2));
        push(&out, show(rt, &b));
        let arr2 = std_array(rt, vec![k1.clone(), s("z")]);
        let r = construct(rt, &weak_set, std::slice::from_ref(&arr2), None);
        push(&out, thrown(rt, r));
        let log = Out::default();
        let (nt, p) = subclass_new_target(rt, &weak_set);
        let l2 = log.clone();
        let adder = native(rt, 1, move |rt, call| {
            push(&l2, show(rt, call.arg(0)));
            Ok(Value::Undefined)
        });
        set(rt, &p, "add", adder);
        construct(rt, &weak_set, &[arr2], Some(&nt)).expect("construct");
        let items = iterable(rt, vec![Value::Int(7)]);
        construct(rt, &weak_set, &[items], Some(&nt)).expect("construct");
        push(&out, log.borrow().join(","));
        set(rt, &p, "add", Value::Undefined);
        let items = iterable(rt, vec![]);
        let r = construct(rt, &weak_set, &[items], Some(&nt));
        push(&out, thrown(rt, r));
        push(&out, to_string_tag(rt, &st));
        check(&out, WEAKSET);
    });
}

fn capture(rt: &mut Runtime, idx: usize, slot: &Rc<RefCell<Option<Value>>>) -> Value {
    let slot = slot.clone();
    native(rt, 2, move |rt, call| {
        let v = call.argument(idx);
        keep(rt, &v);
        *slot.borrow_mut() = Some(v);
        Ok(Value::Undefined)
    })
}

fn logger(rt: &mut Runtime, out: &Out, prefix: &'static str) -> Value {
    let o = out.clone();
    native(rt, 1, move |rt, call| {
        let line = format!("{prefix}{}", str_of(rt, call.arg(0)));
        push(&o, line);
        Ok(Value::Undefined)
    })
}

fn resolver_executor(rt: &mut Runtime, v: Value) -> Value {
    keep(rt, &v);
    native(rt, 2, move |rt, c| {
        call(rt, c.arg(0), &Value::Undefined, std::slice::from_ref(&v))?;
        Ok(Value::Undefined)
    })
}

fn then_obj(
    rt: &mut Runtime,
    f: impl Fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value> + 'static,
) -> Value {
    let o = new_obj(rt);
    let t = native(rt, 2, f);
    set(rt, &o, "then", t);
    o
}

#[test]
fn promise_reaction_and_thenable_job_order() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let res = Rc::new(RefCell::new(None));
        let exec = capture(rt, 0, &res);
        let p = new(rt, &promise, &[exec]);
        let o2 = out.clone();
        let ha = native(rt, 1, move |rt, call| {
            let v = call.argument(0);
            push(&o2, format!("a{}", str_of(rt, &v)));
            Ok(match v {
                Value::Int(i) => Value::Int(i + 1),
                other => other,
            })
        });
        let t1 = inv(rt, &p, "then", &[ha]);
        let hc = logger(rt, &out, "c");
        inv(rt, &t1, "then", &[hc]);
        let hb = logger(rt, &out, "b");
        inv(rt, &p, "then", &[hb]);
        let res = res.borrow().clone().expect("resolve");
        call(rt, &res, &Value::Undefined, &[Value::Int(1)]).expect("res");
        call(rt, &res, &Value::Undefined, &[Value::Int(2)]).expect("res");

        let o3 = out.clone();
        let thenable = then_obj(rt, move |rt, c| {
            push(&o3, "then called");
            call(rt, c.arg(0), &Value::Undefined, &[s("T")])?;
            Ok(Value::Undefined)
        });
        let ex = resolver_executor(rt, thenable);
        let p1 = new(rt, &promise, &[ex]);
        let p2 = inv(rt, &promise, "resolve", &[Value::Int(2)]);
        let h = logger(rt, &out, "p1 ");
        inv(rt, &p1, "then", &[h]);
        let h = logger(rt, &out, "p2 ");
        inv(rt, &p2, "then", &[h]);

        let bad = new_obj(rt);
        let g = native(rt, 0, |_rt, _c| Err(throw_str("getter")));
        getter(rt, &bad, "then", g);
        let ex = resolver_executor(rt, bad);
        let pb = new(rt, &promise, &[ex]);
        let h = logger(rt, &out, "bad ");
        inv(rt, &pb, "catch", &[h]);

        let throwing = then_obj(rt, |_rt, _c| Err(throw_str("inthen")));
        let ex = resolver_executor(rt, throwing);
        let pt = new(rt, &promise, &[ex]);
        let h = logger(rt, &out, "throwing ");
        inv(rt, &pt, "catch", &[h]);

        let late = then_obj(rt, |rt, c| {
            call(rt, c.arg(0), &Value::Undefined, &[s("first")])?;
            call(rt, c.arg(1), &Value::Undefined, &[s("second")])?;
            Err(throw_str("ignored"))
        });
        let ex = resolver_executor(rt, late);
        let pl = new(rt, &promise, &[ex]);
        let h = logger(rt, &out, "late ");
        inv(rt, &pl, "then", &[h]);
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_ORDER);
    });
}

#[test]
fn promise_rejections_and_self_resolution() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let ex = native(rt, 2, |rt, _c| {
            let e = rt.get_error();
            let err = rt.builtin_new(e, &[s("boom")])?;
            Err(Throw::Value(Value::Object(err)))
        });
        let p = new(rt, &promise, &[ex]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, call| {
            let m = get(rt, call.arg(0), "message");
            push(&o2, format!("caught {}", str_of(rt, &m)));
            Ok(Value::Undefined)
        });
        inv(rt, &p, "catch", &[h]);
        let pr = inv(rt, &promise, "reject", &[Value::Int(7)]);
        let h = logger(rt, &out, "rej ");
        inv(rt, &pr, "then", &[Value::Null, h]);
        let rs = Rc::new(RefCell::new(None));
        let ex = capture(rt, 0, &rs);
        let ps = new(rt, &promise, &[ex]);
        let rsf = rs.borrow().clone().expect("resolve");
        call(rt, &rsf, &Value::Undefined, std::slice::from_ref(&ps)).expect("self");
        let h = logger(rt, &out, "");
        inv(rt, &ps, "catch", &[h]);
        let rj = Rc::new(RefCell::new(None));
        let ex = capture(rt, 1, &rj);
        let pr = new(rt, &promise, &[ex]);
        let never = logger(rt, &out, "never");
        let t = inv(rt, &pr, "then", &[never]);
        let h = logger(rt, &out, "chained ");
        inv(rt, &t, "then", &[Value::Null, h]);
        let rjf = rj.borrow().clone().expect("reject");
        call(rt, &rjf, &Value::Undefined, &[s("R")]).expect("reject");
        let ex = native(rt, 2, |rt, c| {
            call(rt, c.arg(0), &Value::Undefined, &[Value::Int(1)])?;
            Err(throw_str("after"))
        });
        let pk = new(rt, &promise, &[ex]);
        let h = logger(rt, &out, "kept ");
        inv(rt, &pk, "then", &[h]);
        let obj = then_obj(rt, |rt, c| {
            let b = str_of(rt, c.arg(1));
            Ok(s(&format!("then:{}:{}", type_of(c.arg(0)), b)))
        });
        let catch = proto_method(rt, &promise, "catch");
        let r = call(rt, &catch, &obj, &[s("X")]).expect("catch");
        push(&out, str_of(rt, &r));
        let g = species_getter(rt, &promise);
        let r = call(rt, &g, &Value::Int(7), &[]).expect("species");
        push(&out, show(rt, &r));
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_REJECT);
    });
}

#[test]
fn promise_combinators_all_all_settled_any_race() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let th = |rt: &mut Runtime, v: Value| {
            keep(rt, &v);
            then_obj(rt, move |rt, c| {
                call(rt, c.arg(0), &Value::Undefined, std::slice::from_ref(&v))?;
                Ok(Value::Undefined)
            })
        };
        let p2 = inv(rt, &promise, "resolve", &[Value::Int(2)]);
        let t3 = th(rt, Value::Int(3));
        let items = iterable(rt, vec![Value::Int(1), p2, t3]);
        let all = inv(rt, &promise, "all", &[items]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, c| {
            let vals = array_values(rt, c.arg(0));
            let line = format!("all {}", join(rt, &vals, ","));
            push(&o2, line);
            Ok(Value::Undefined)
        });
        inv(rt, &all, "then", &[h]);

        let r1 = inv(rt, &promise, "reject", &[s("no")]);
        let r2 = inv(rt, &promise, "reject", &[s("no2")]);
        let items = iterable(rt, vec![Value::Int(1), r1, r2]);
        let all = inv(rt, &promise, "all", &[items]);
        let h = logger(rt, &out, "all rejected ");
        inv(rt, &all, "then", &[Value::Null, h]);

        let items = iterable(rt, vec![]);
        let all = inv(rt, &promise, "all", &[items]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, c| {
            let n = array_values(rt, c.arg(0)).len();
            push(&o2, format!("all empty {n}"));
            Ok(Value::Undefined)
        });
        inv(rt, &all, "then", &[h]);

        let all = inv(rt, &promise, "all", &[Value::Int(5)]);
        let h = logger(rt, &out, "all bad ");
        inv(rt, &all, "then", &[Value::Null, h]);

        let rx = inv(rt, &promise, "reject", &[s("x")]);
        let items = iterable(rt, vec![Value::Int(1), rx]);
        let settled = inv(rt, &promise, "allSettled", &[items]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, c| {
            let vals = array_values(rt, c.arg(0));
            let mut parts = Vec::new();
            for o in &vals {
                let status = get(rt, o, "status");
                let status = str_of(rt, &status);
                let key = if status == "fulfilled" {
                    "value"
                } else {
                    "reason"
                };
                let v = get(rt, o, key);
                let v = str_of(rt, &v);
                let keys = obj_of(o).string_keys(rt, false, Vec::new())?;
                let keys = join(rt, &keys, "|");
                parts.push(format!("{status}:{v}:{keys}"));
            }
            push(&o2, format!("settled {}", parts.join(",")));
            Ok(Value::Undefined)
        });
        inv(rt, &settled, "then", &[h]);

        let e1 = inv(rt, &promise, "reject", &[s("e1")]);
        let e2 = inv(rt, &promise, "reject", &[s("e2")]);
        let items = iterable(rt, vec![e1, e2]);
        let any = inv(rt, &promise, "any", &[items]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, c| {
            let e = c.argument(0);
            let errors = get(rt, &e, "errors");
            let errors = array_values(rt, &errors);
            let ae = rt.get_aggregate_error();
            let inst = rt.instance_of(&e, ae)?;
            let line = format!("any {} {} {}", str_of(rt, &e), join(rt, &errors, ","), inst);
            push(&o2, line);
            Ok(Value::Undefined)
        });
        inv(rt, &any, "then", &[Value::Null, h]);

        let e1 = inv(rt, &promise, "reject", &[s("e1")]);
        let items = iterable(rt, vec![e1, Value::Int(5)]);
        let any = inv(rt, &promise, "any", &[items]);
        let h = logger(rt, &out, "any ok ");
        inv(rt, &any, "then", &[h]);

        let items = iterable(rt, vec![]);
        let any = inv(rt, &promise, "any", &[items]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, c| {
            let errors = get(rt, c.arg(0), "errors");
            let n = array_values(rt, &errors).len();
            push(&o2, format!("any empty {n}"));
            Ok(Value::Undefined)
        });
        inv(rt, &any, "then", &[Value::Null, h]);

        let noop = native(rt, 2, |_rt, _c| Ok(Value::Undefined));
        let pending = new(rt, &promise, &[noop]);
        let p5 = inv(rt, &promise, "resolve", &[Value::Int(5)]);
        let items = iterable(rt, vec![pending, p5, Value::Int(6)]);
        let race = inv(rt, &promise, "race", &[items]);
        let h = logger(rt, &out, "race ");
        inv(rt, &race, "then", &[h]);

        let items = iterable(rt, vec![]);
        let race = inv(rt, &promise, "race", &[items]);
        let h = logger(rt, &out, "race empty");
        inv(rt, &race, "then", &[h]);

        let all_f = get(rt, &promise, "all");
        let o = new_obj(rt);
        let items = iterable(rt, vec![]);
        let r = call(rt, &all_f, &o, &[items]);
        push(&out, thrown(rt, r));
        let items = iterable(rt, vec![]);
        let r = call(rt, &all_f, &Value::Int(1), &[items]);
        push(&out, thrown(rt, r));
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_COMBINATORS);
    });
}

#[test]
fn promise_finally_pass_through() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let p = inv(rt, &promise, "resolve", &[Value::Int(1)]);
        let o2 = out.clone();
        let f1 = native(rt, 0, move |_rt, c| {
            push(&o2, format!("f1 {}", c.arguments.len()));
            Ok(Value::Int(99))
        });
        let t = inv(rt, &p, "finally", &[f1]);
        let h = logger(rt, &out, "after ");
        inv(rt, &t, "then", &[h]);

        let p = inv(rt, &promise, "reject", &[Value::Int(2)]);
        let o2 = out.clone();
        let f2 = native(rt, 0, move |_rt, _c| {
            push(&o2, "f2");
            Ok(Value::Undefined)
        });
        let t = inv(rt, &p, "finally", &[f2]);
        let h = logger(rt, &out, "caught ");
        inv(rt, &t, "then", &[Value::Null, h]);

        let p = inv(rt, &promise, "resolve", &[Value::Int(3)]);
        let f3 = native(rt, 0, |_rt, _c| Err(throw_str("x")));
        let t = inv(rt, &p, "finally", &[f3]);
        let h = logger(rt, &out, "c3 ");
        inv(rt, &t, "then", &[Value::Null, h]);

        let p = inv(rt, &promise, "resolve", &[Value::Int(4)]);
        let t = inv(rt, &p, "finally", &[Value::Int(5)]);
        let h = logger(rt, &out, "nf ");
        inv(rt, &t, "then", &[h]);

        let p = inv(rt, &promise, "reject", &[Value::Int(6)]);
        let t = inv(rt, &p, "finally", &[]);
        let h = logger(rt, &out, "nf2 ");
        inv(rt, &t, "then", &[Value::Null, h]);

        let fp = inv(rt, &promise, "resolve", &[Value::Int(0)]);
        let noop = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
        let t = inv(rt, &fp, "finally", &[noop]);
        let pc = obj_of(&promise);
        let inst = rt.instance_of(&t, pc).expect("instanceof");
        push(&out, show(rt, &Value::Bool(inst)));
        let f = proto_method(rt, &promise, "finally");
        let r = call(rt, &f, &Value::Int(1), &[]);
        push(&out, thrown(rt, r));
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_FINALLY);
    });
}

fn capability_ctor(rt: &mut Runtime, out: &Out) -> Value {
    let o = out.clone();
    native_ctor(rt, move |rt, args| {
        push(&o, "C called");
        let res = logger(rt, &o, "res ");
        let rej = logger(rt, &o, "rej ");
        let executor = args.first().cloned().unwrap_or(Value::Undefined);
        call(rt, &executor, &Value::Undefined, &[res, rej])?;
        let this = new_obj(rt);
        set(rt, &this, "tag", s("c"));
        Ok(this)
    })
}

#[test]
fn promise_custom_capability_and_species() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let c = capability_ctor(rt, &out);
        let resolve = get(rt, &promise, "resolve");
        let reject = get(rt, &promise, "reject");
        let r1 = call(rt, &resolve, &c, &[Value::Int(5)]).expect("resolve");
        let tag = get(rt, &r1, "tag");
        push(&out, format!("r1 {}", str_of(rt, &tag)));
        let r2 = call(rt, &reject, &c, &[Value::Int(6)]).expect("reject");
        let tag = get(rt, &r2, "tag");
        push(&out, format!("r2 {}", str_of(rt, &tag)));
        let p = inv(rt, &promise, "resolve", &[Value::Int(1)]);
        let r = inv(rt, &promise, "resolve", std::slice::from_ref(&p));
        push(&out, show(rt, &Value::Bool(r.same_as(&p))));
        let q = inv(rt, &promise, "resolve", &[Value::Int(1)]);
        set(rt, &q, "constructor", c.clone());
        let r = inv(rt, &promise, "resolve", std::slice::from_ref(&q));
        push(&out, show(rt, &Value::Bool(r.same_as(&q))));
        let r = call(rt, &resolve, &c, std::slice::from_ref(&q)).expect("resolve");
        push(&out, show(rt, &Value::Bool(r.same_as(&q))));
        let sp = inv(rt, &promise, "resolve", &[Value::Int(2)]);
        let ctor = new_obj(rt);
        obj_of(&ctor)
            .set_own_sym(rt, SYM_SPECIES, c.clone(), true)
            .expect("set");
        set(rt, &sp, "constructor", ctor);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, call| {
            push(&o2, format!("then {}", str_of(rt, call.arg(0))));
            Ok(s("tv"))
        });
        let t = inv(rt, &sp, "then", &[h]);
        let tag = get(rt, &t, "tag");
        push(&out, format!("t {}", str_of(rt, &tag)));

        let twice = native_ctor(rt, |rt, args| {
            let e = args[0].clone();
            let a = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
            let b = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
            call(rt, &e, &Value::Undefined, &[a.clone(), b.clone()])?;
            call(rt, &e, &Value::Undefined, &[a, b])?;
            Ok(new_obj(rt))
        });
        let r = call(rt, &resolve, &twice, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let rej_twice = native_ctor(rt, |rt, args| {
            let e = args[0].clone();
            let a = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
            call(rt, &e, &Value::Undefined, &[Value::Undefined, a.clone()])?;
            call(rt, &e, &Value::Undefined, &[a, Value::Undefined])?;
            Ok(new_obj(rt))
        });
        let r = call(rt, &resolve, &rej_twice, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let never = native_ctor(rt, |rt, _args| Ok(new_obj(rt)));
        let r = call(rt, &resolve, &never, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let prim = native_ctor(rt, |rt, args| {
            call(
                rt,
                &args[0],
                &Value::Undefined,
                &[Value::Int(1), Value::Int(2)],
            )?;
            Ok(new_obj(rt))
        });
        let r = call(rt, &resolve, &prim, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let not_callable = native_ctor(rt, |rt, args| {
            let o = new_obj(rt);
            let a = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
            call(rt, &args[0], &Value::Undefined, &[o, a])?;
            Ok(new_obj(rt))
        });
        let r = call(rt, &resolve, &not_callable, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        let u = inv(rt, &promise, "resolve", &[Value::Int(3)]);
        let ctor = new_obj(rt);
        let bad_species = new_obj(rt);
        obj_of(&ctor)
            .set_own_sym(rt, SYM_SPECIES, bad_species, true)
            .expect("set");
        set(rt, &u, "constructor", ctor);
        let r = invoke(rt, &u, "then", &[]);
        push(&out, thrown(rt, r));
        let u2 = inv(rt, &promise, "resolve", &[Value::Int(3)]);
        set(rt, &u2, "constructor", Value::Int(1));
        let r = invoke(rt, &u2, "then", &[]);
        push(&out, thrown(rt, r));
        let g = inv(rt, &promise, "resolve", &[Value::Int(1)]);
        let gets = Rc::new(Cell::new(0i64));
        let (gc2, pc) = (gets.clone(), promise.clone());
        let ctor_getter = native(rt, 0, move |_rt, _c| {
            gc2.set(gc2.get() + 1);
            Ok(pc.clone())
        });
        getter(rt, &g, "constructor", ctor_getter);
        let r = inv(rt, &promise, "resolve", std::slice::from_ref(&g));
        push(&out, show(rt, &Value::Bool(r.same_as(&g))));
        push(&out, show(rt, &Value::Int(gets.get())));
        let noop = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
        inv(rt, &g, "then", &[noop]);
        push(&out, show(rt, &Value::Int(gets.get())));
        push(&out, to_string_tag(rt, &p));
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_CAPABILITY);
    });
}

#[test]
fn promise_error_messages() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let noop = native(rt, 0, |_rt, _c| Ok(Value::Undefined));
        let r = call(rt, &promise, &Value::Undefined, &[noop]);
        push(&out, thrown(rt, r));
        let r = construct(rt, &promise, &[], None);
        push(&out, thrown(rt, r));
        let r = construct(rt, &promise, &[Value::Int(1)], None);
        push(&out, thrown(rt, r));
        let o = new_obj(rt);
        let r = construct(rt, &promise, &[o], None);
        push(&out, thrown(rt, r));
        let then = proto_method(rt, &promise, "then");
        let o = new_obj(rt);
        let r = call(rt, &then, &o, &[]);
        push(&out, thrown(rt, r));
        let r = call(rt, &then, &Value::Int(1), &[]);
        push(&out, thrown(rt, r));
        let catch = proto_method(rt, &promise, "catch");
        let r = call(rt, &catch, &Value::Undefined, &[]);
        push(&out, thrown(rt, r));
        let o = new_obj(rt);
        let r = call(rt, &catch, &o, &[]);
        push(&out, thrown(rt, r));
        let o = new_obj(rt);
        set(rt, &o, "then", Value::Int(1));
        let r = call(rt, &catch, &o, &[]);
        push(&out, thrown(rt, r));
        let o = new_obj(rt);
        let inner = new_obj(rt);
        set(rt, &o, "then", inner);
        let r = call(rt, &catch, &o, &[]);
        push(&out, thrown(rt, r));
        let finally = proto_method(rt, &promise, "finally");
        let o = new_obj(rt);
        let r = call(rt, &finally, &o, &[]);
        push(&out, thrown(rt, r));
        let resolve = get(rt, &promise, "resolve");
        let r = call(rt, &resolve, &Value::Int(1), &[]);
        push(&out, thrown(rt, r));
        let race = get(rt, &promise, "race");
        let items = iterable(rt, vec![]);
        let r = call(rt, &race, &Value::Undefined, &[items]);
        push(&out, thrown(rt, r));
        let any = get(rt, &promise, "any");
        let o = new_obj(rt);
        let items = iterable(rt, vec![]);
        let r = call(rt, &any, &o, &[items]);
        push(&out, thrown(rt, r));
        let reject = get(rt, &promise, "reject");
        let r = call(rt, &reject, &Value::Null, &[Value::Int(1)]);
        push(&out, thrown(rt, r));
        check(&out, PROMISE_ERRORS);
    });
}

#[test]
fn promise_jobs_drained_by_runtime_leave_keep_order() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let res = Rc::new(RefCell::new(None));
        let exec = capture(rt, 0, &res);
        let p = new(rt, &promise, &[exec]);
        let o2 = out.clone();
        let ha = native(rt, 1, move |rt, call| {
            push(&o2, format!("a{}", str_of(rt, call.arg(0))));
            Ok(Value::Int(2))
        });
        let t1 = inv(rt, &p, "then", &[ha]);
        let hc = logger(rt, &out, "c");
        inv(rt, &t1, "then", &[hc]);
        let hb = logger(rt, &out, "b");
        inv(rt, &p, "then", &[hb]);
        let res = res.borrow().clone().expect("resolve");
        call(rt, &res, &Value::Undefined, &[Value::Int(1)]).expect("res");
        push(&out, "sync");
        rt.leave().expect("leave");
        check(&out, &["sync", "a1", "b1", "c2"]);
    });
}

fn unrooted_mark(rt: &Runtime) -> usize {
    rt.extra_roots.len()
}

fn collect_with_roots(rt: &mut Runtime, roots: &[Value]) {
    rt.handles.clear();
    for r in roots {
        rt.extra_roots.push(r.clone());
    }
    rt.collect_garbage();
}

#[test]
fn gc_map_and_set_trace_entries_and_iterators() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        rt.set_gc_stress(true);
        let map = global(rt, "Map");
        let set_c = global(rt, "Set");
        let mark = unrooted_mark(rt);
        let m = new(rt, &map, &[]);
        let st = new(rt, &set_c, &[]);
        for i in 0..20 {
            let k = Value::Object(rt.new_object());
            let v = Value::Object(rt.new_object());
            obj_of(&v)
                .set_own_str(rt, &"i".into(), Value::Int(i), true)
                .expect("set");
            inv(rt, &m, "set", &[k, v]);
            let sk = Value::Object(rt.new_object());
            obj_of(&sk)
                .set_own_str(rt, &"i".into(), Value::Int(i), true)
                .expect("set");
            inv(rt, &st, "add", &[sk]);
        }
        let it = inv(rt, &m, "entries", &[]);
        let sit = inv(rt, &st, "values", &[]);
        rt.extra_roots.truncate(mark);
        collect_with_roots(rt, &[it.clone(), sit.clone()]);
        let mut seen = 0;
        loop {
            let r = inv(rt, &it, "next", &[]);
            if get(rt, &r, "done").to_boolean() {
                break;
            }
            let pair = get(rt, &r, "value");
            let vals = array_values(rt, &pair);
            let i = get(rt, &vals[1], "i");
            assert!(matches!(i, Value::Int(n) if n == seen));
            seen += 1;
            rt.collect_garbage();
        }
        assert_eq!(seen, 20);
        let mut n = 0;
        loop {
            let r = inv(rt, &sit, "next", &[]);
            if get(rt, &r, "done").to_boolean() {
                break;
            }
            let k = get(rt, &r, "value");
            let i = get(rt, &k, "i");
            assert!(matches!(i, Value::Int(x) if x == n));
            n += 1;
            rt.collect_garbage();
        }
        assert_eq!(n, 20);
    });
}

#[test]
fn gc_foreach_callback_collection_keeps_entries() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        rt.set_gc_stress(true);
        let map = global(rt, "Map");
        let m = new(rt, &map, &[]);
        let mark = unrooted_mark(rt);
        for i in 0..5 {
            let v = Value::Object(rt.new_object());
            obj_of(&v)
                .set_own_str(rt, &"i".into(), Value::Int(i), true)
                .expect("set");
            inv(rt, &m, "set", &[Value::Int(i), v]);
        }
        rt.extra_roots.truncate(mark);
        let seen: Rc<RefCell<Vec<i64>>> = Rc::default();
        let s2 = seen.clone();
        let mm = m.clone();
        let cb = native(rt, 2, move |rt, call| {
            let i = get(rt, call.arg(0), "i");
            let Value::Int(i) = i else {
                panic!("bad entry value {i:?}");
            };
            s2.borrow_mut().push(i);
            if s2.borrow().len() == 2 {
                inv(rt, &mm, "delete", &[Value::Int(3)]);
                let v = Value::Object(rt.new_object());
                obj_of(&v)
                    .set_own_str(rt, &"i".into(), Value::Int(9), true)
                    .expect("set");
                inv(rt, &mm, "set", &[Value::Int(3), v]);
                rt.collect_garbage();
            }
            Ok(Value::Undefined)
        });
        rt.handles.clear();
        rt.collect_garbage();
        inv(rt, &m, "forEach", &[cb]);
        assert_eq!(*seen.borrow(), vec![0, 1, 2, 4, 9]);
    });
}

#[test]
fn gc_weak_map_value_lives_with_its_key() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        rt.set_gc_stress(true);
        let weak_map = global(rt, "WeakMap");
        let w = new(rt, &weak_map, &[]);
        let mark = unrooted_mark(rt);
        let key = new_obj(rt);
        let v = Value::Object(rt.new_object());
        obj_of(&v)
            .set_own_str(rt, &"x".into(), Value::Int(42), true)
            .expect("set");
        inv(rt, &w, "set", &[key.clone(), v.clone()]);
        rt.extra_roots.truncate(mark);
        collect_with_roots(rt, std::slice::from_ref(&key));
        let got = inv(rt, &w, "get", std::slice::from_ref(&key));
        assert!(got.same_as(&v));
        let x = get(rt, &got, "x");
        assert!(matches!(x, Value::Int(42)));
        rt.extra_roots.truncate(mark);
        rt.handles.clear();
        rt.collect_garbage();
        assert!(!rt.heap.is_live(obj_of(&key)));
        assert!(!rt.heap.is_live(obj_of(&v)));
        let has = inv(rt, &w, "has", &[Value::Int(1)]);
        assert!(matches!(has, Value::Bool(false)));
    });
}

#[test]
fn gc_weak_map_identity_survives_slot_reuse() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        let weak_map = global(rt, "WeakMap");
        let weak_set = global(rt, "WeakSet");
        let key = new_obj(rt);
        let mark = unrooted_mark(rt);
        for i in 0..200 {
            let w = new(rt, &weak_map, &[]);
            let has = inv(rt, &w, "has", std::slice::from_ref(&key));
            assert!(matches!(has, Value::Bool(false)), "weak map {i}");
            inv(rt, &w, "set", &[key.clone(), Value::Int(i)]);
            let ws = new(rt, &weak_set, &[]);
            let has = inv(rt, &ws, "has", std::slice::from_ref(&key));
            assert!(matches!(has, Value::Bool(false)), "weak set {i}");
            inv(rt, &ws, "add", std::slice::from_ref(&key));
            rt.extra_roots.truncate(mark);
            rt.handles.clear();
            rt.collect_garbage();
        }
    });
}

#[test]
fn gc_promise_reactions_capabilities_and_jobs_are_traced() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        rt.set_gc_stress(true);
        let out = Out::default();
        let promise = global(rt, "Promise");
        let res = Rc::new(RefCell::new(None));
        let exec = capture(rt, 0, &res);
        let p = new(rt, &promise, &[exec]);
        let mark = unrooted_mark(rt);
        let o2 = out.clone();
        let h1 = native(rt, 1, move |rt, call| {
            push(&o2, format!("h1 {}", str_of(rt, call.arg(0))));
            let fresh = Value::Object(rt.new_object());
            obj_of(&fresh)
                .set_own_str(rt, &"v".into(), Value::Int(7), true)
                .expect("set");
            rt.collect_garbage();
            Ok(fresh)
        });
        let t1 = inv(rt, &p, "then", &[h1]);
        let o3 = out.clone();
        let h2 = native(rt, 1, move |rt, call| {
            let v = get(rt, call.arg(0), "v");
            push(&o3, format!("h2 {}", str_of(rt, &v)));
            rt.collect_garbage();
            Ok(Value::Undefined)
        });
        inv(rt, &t1, "then", &[h2]);
        let o4 = out.clone();
        let h3 = native(rt, 1, move |rt, call| {
            push(&o4, format!("h3 {}", str_of(rt, call.arg(0))));
            Ok(Value::Undefined)
        });
        inv(rt, &p, "then", &[h3]);
        let resolve = res.borrow().clone().expect("resolve");
        rt.extra_roots.truncate(mark);
        let roots = [p.clone(), resolve.clone(), promise.clone()];
        collect_with_roots(rt, &roots);
        call(rt, &resolve, &Value::Undefined, &[s("x")]).expect("resolve");
        rt.extra_roots.truncate(mark);
        collect_with_roots(rt, &roots);
        drain(rt);
        check(&out, &["h1 x", "h3 x", "h2 7"]);
    });
}

#[test]
fn gc_promise_all_state_and_capability_executor_are_traced() {
    on_big_stack(|| {
        let rt = &mut Runtime::new();
        rt.set_gc_stress(true);
        let out = Out::default();
        let promise = global(rt, "Promise");
        let saved: Rc<RefCell<Vec<Value>>> = Rc::default();
        let mark = unrooted_mark(rt);
        let mut items = Vec::new();
        for _ in 0..3 {
            let sv = saved.clone();
            items.push(then_obj(rt, move |rt, c| {
                let f = c.argument(0);
                keep(rt, &f);
                sv.borrow_mut().push(f);
                Ok(Value::Undefined)
            }));
        }
        let it = iterable(rt, items);
        let all = inv(rt, &promise, "all", &[it]);
        let o2 = out.clone();
        let h = native(rt, 1, move |rt, call| {
            let vals = array_values(rt, call.arg(0));
            let mut parts = Vec::new();
            for v in &vals {
                let x = get(rt, v, "x");
                parts.push(str_of(rt, &x));
            }
            push(&o2, format!("all {}", parts.join(",")));
            Ok(Value::Undefined)
        });
        inv(rt, &all, "then", &[h]);
        let roots = vec![all.clone(), promise.clone()];
        rt.extra_roots.truncate(mark);
        collect_with_roots(rt, &roots);
        drain(rt);
        let mut fs = saved.borrow().clone();
        assert_eq!(fs.len(), 3);
        while let Some(f) = fs.pop() {
            let i = fs.len();
            let v = Value::Object(rt.new_object());
            obj_of(&v)
                .set_own_str(rt, &"x".into(), Value::Int(i as i64), true)
                .expect("set");
            let mut now = roots.clone();
            now.extend(fs.iter().cloned());
            now.push(f.clone());
            now.push(v.clone());
            rt.extra_roots.truncate(mark);
            collect_with_roots(rt, &now);
            call(rt, &f, &Value::Undefined, &[v]).expect("fulfill");
            drain(rt);
            now.truncate(roots.len() + fs.len());
            rt.extra_roots.truncate(mark);
            collect_with_roots(rt, &now);
        }
        check(&out, &["all 0,1,2"]);

        let o3 = out.clone();
        let c = native_ctor(rt, move |rt, args| {
            let scope = rt.handle_scope_enter();
            let o = o3.clone();
            let res = Value::Object(rt.new_native_func(
                NativeFn::closure_with(Vec::new(), move |rt, _d, call| {
                    push(&o, format!("res {}", str_of(rt, call.arg(0))));
                    Ok(Value::Undefined)
                }),
                "",
                1,
            ));
            let rej = Value::Object(rt.new_native_func(
                NativeFn::closure_with(Vec::new(), |_rt, _d, _call| Ok(Value::Undefined)),
                "",
                1,
            ));
            let executor = obj_of(&args[0]).assert_callable(rt).expect("callable");
            executor.call(rt, Value::Undefined, &[res, rej])?;
            rt.handle_scope_leave(scope);
            rt.collect_garbage();
            Ok(Value::Object(rt.new_object()))
        });
        let resolve = get(rt, &promise, "resolve");
        call(rt, &resolve, &c, &[s("ok")]).expect("capability");
        check(&out, &["all 0,1,2", "res ok"]);
    });
}

#[test]
fn adder_getter_runs_once_before_get_iterator() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        for (ctor, adder, msg) in [
            ("Map", "set", "TypeError: Map.set in missing"),
            ("Set", "add", "TypeError: Set.add in missing"),
            ("WeakMap", "set", "TypeError: WeakMap.set in missing"),
            ("WeakSet", "add", "TypeError: WeakSet.add in missing"),
        ] {
            let c = global(rt, ctor);
            let (nt, p) = subclass_new_target(rt, &c);
            let getter_called = Rc::new(Cell::new(0));
            let g2 = getter_called.clone();
            let g = native(rt, 0, move |_rt, _c| {
                g2.set(g2.get() + 1);
                Ok(Value::Null)
            });
            getter(rt, &p, adder, g);
            let iter_called = Rc::new(Cell::new(0));
            let i2 = iter_called.clone();
            let iterable = new_obj(rt);
            let f = native(rt, 0, move |rt, _c| {
                i2.set(i2.get() + 1);
                let o = new_obj(rt);
                set(rt, &o, "next", Value::Int(1));
                Ok(o)
            });
            obj_of(&iterable)
                .set_own_sym(rt, SYM_ITERATOR, f, true)
                .expect("set");
            let r = construct(rt, &c, &[iterable], Some(&nt));
            assert_eq!(thrown(rt, r), msg, "{ctor}");
            assert_eq!(getter_called.get(), 1, "{ctor}");
            assert_eq!(iter_called.get(), 0, "{ctor}");
        }
    });
}

const PROMISE_SETTLE_ONCE: &[&str] = &["sync", "rej first", "q q1"];

#[test]
fn promise_resolving_functions_settle_once() {
    on_big_stack(|| {
        let rt = &mut new_rt();
        let out = Out::default();
        let promise = global(rt, "Promise");
        let ex = native(rt, 2, |rt, c| {
            call(rt, c.arg(1), &Value::Undefined, &[s("first")])?;
            call(rt, c.arg(0), &Value::Undefined, &[s("second")])?;
            call(rt, c.arg(1), &Value::Undefined, &[s("third")])?;
            Ok(Value::Undefined)
        });
        let p = new(rt, &promise, &[ex]);
        let ful = logger(rt, &out, "ful ");
        let rej = logger(rt, &out, "rej ");
        inv(rt, &p, "then", &[ful, rej]);
        let ex = native(rt, 2, |rt, c| {
            call(rt, c.arg(1), &Value::Undefined, &[s("q1")])?;
            call(rt, c.arg(1), &Value::Undefined, &[s("q2")])?;
            Ok(Value::Undefined)
        });
        let q = new(rt, &promise, &[ex]);
        let h = logger(rt, &out, "q ");
        inv(rt, &q, "then", &[Value::Null, h]);
        push(&out, "sync");
        drain(rt);
        check(&out, PROMISE_SETTLE_ONCE);
    });
}
