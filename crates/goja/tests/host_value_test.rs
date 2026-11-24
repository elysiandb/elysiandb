mod jsrun;
mod tzchild;

use gocompat::json as gojson;
use goja::export::ExportType;
use goja::host::{DocMap, DocValue, HostFn, SliceHeader, TypedSlice, json};
use goja::{Runtime, Value};
use jsrun::{new_runtime, on_big_stack, run_error_string};
use std::rc::Rc;
use tzchild::{Zone, run_in_zone};

const INPUT: &str = r#"{"id":"x","price":12,"ratio":0.5,"tags":["a","b"],"meta":{"k":"v","n":1},"nul":null,"s":"str","b":true,"big":12345678901234567890}"#;

fn decode(s: &str) -> DocValue {
    json::from_json(&gojson::unmarshal_value(s.as_bytes()).expect("json"))
}

fn ctx_with(rt: &mut Runtime, entity: &DocValue) {
    let mut ctx = DocMap::new();
    ctx.insert("entity".to_string(), entity.clone());
    rt.set_value("ctx", &DocValue::new_map(ctx)).expect("ctx");
}

fn run(rt: &mut Runtime, src: &str) -> Value {
    match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

fn run_err(rt: &mut Runtime, src: &str) -> String {
    match rt.run_string(src) {
        Ok(v) => panic!("no error: {v:?}"),
        Err(e) => run_error_string(&e),
    }
}

fn out(v: &DocValue) -> String {
    String::from_utf8(json::marshal(v).expect("marshal")).expect("utf8")
}

fn field(e: &DocValue, k: &str) -> String {
    let m = e.as_map().expect("map").borrow();
    out(m.get(k).expect("field"))
}

fn script(src: &str) -> (Runtime, DocValue, Value) {
    let mut rt = new_runtime();
    let e = decode(INPUT);
    ctx_with(&mut rt, &e);
    let v = run(&mut rt, src);
    (rt, e, v)
}

fn s(rt: &mut Runtime, v: &Value) -> String {
    v.go_string(rt).expect("string")
}

#[test]
fn reads_rewrap_and_writes_store_exports() {
    on_big_stack(|| {
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity;
            var o = {a: 1}; e.o = o; o.b = 2;
            e.m2 = e.meta; e.m2.q = 1;
            e.meta.k = "w";
            [e.meta === e.meta, e.tags === e.tags, ctx.entity === ctx.entity, e.o.b === undefined, e === e].join()"#,
        );
        assert_eq!(s(&mut rt, &v), "false,false,false,true,true");
        assert_eq!(field(&e, "o"), r#"{"a":1}"#);
        assert_eq!(field(&e, "meta"), r#"{"k":"w","n":1,"q":1}"#);
        assert_eq!(field(&e, "m2"), r#"{"k":"w","n":1,"q":1}"#);
    });
}

#[test]
fn slices_share_elements_and_own_their_length() {
    on_big_stack(|| {
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity;
            e.tags.push("c");
            var t = e.tags; t.push("c");
            e.arr = [1]; e.arr.push(2);
            [e.tags.length, t.length, e.arr.length].join()"#,
        );
        assert_eq!(s(&mut rt, &v), "2,3,1");
        assert_eq!(field(&e, "tags"), r#"["a","b"]"#);
        assert_eq!(field(&e, "arr"), "[1]");
        let (_, e, _) = script("ctx.entity.tags.pop()");
        assert_eq!(field(&e, "tags"), r#"["a",null]"#);
        let (_, e, _) = script("ctx.entity.tags.splice(0,1)");
        assert_eq!(field(&e, "tags"), r#"["b",null]"#);
        let (_, e, _) = script("ctx.entity.tags.length = 0");
        assert_eq!(field(&e, "tags"), "[null,null]");
        let (_, e, _) = script(r#"ctx.entity.tags[0] = "z"; ctx.entity.tags.reverse()"#);
        assert_eq!(field(&e, "tags"), r#"["b","z"]"#);
    });
}

#[test]
fn spare_capacity_is_shared_like_go_append() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let e = decode(r#"{"t2":["a","b"],"t3":["a","b","c"]}"#);
        ctx_with(&mut rt, &e);
        let v = run(
            &mut rt,
            r#"var e = ctx.entity;
            var a = e.t3, b = e.t3; a.push(1); b.push(2);
            var c = e.t2, d = e.t2; c.push(1); d.push(2);
            var f = e.t3, g = e.t3; f.push(9); g.length = 4;
            [a[3], b[3], c[2], d[2], e.t3.length, f[3], g[3]].join()"#,
        );
        assert_eq!(s(&mut rt, &v), ",,1,2,3,,");
    });
}

#[test]
fn decoded_capacities_follow_go_append() {
    let caps: Vec<usize> = [0usize, 1, 2, 3, 4, 5, 8, 9, 16, 17]
        .iter()
        .map(|n| SliceHeader::decoded(vec![DocValue::Nil; *n]).cap())
        .collect();
    assert_eq!(caps, vec![0, 1, 2, 4, 4, 8, 8, 16, 16, 32]);
    let mut v: Vec<u8> = Vec::new();
    for _ in 0..1000 {
        goja::array::go_append(&mut v, 0u8);
    }
    assert_eq!(
        SliceHeader::decoded(vec![DocValue::Nil; 1000]).cap(),
        v.capacity()
    );
}

#[test]
fn host_property_rules() {
    on_big_stack(|| {
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity; var r = [];
            try { Object.defineProperty(e, "g", {get: function () { return 1 }}) } catch (x) { r.push(String(x)) }
            try { Object.freeze(e) } catch (x) { r.push(String(x)) }
            try { Object.defineProperty(e, "c", {value: 1, configurable: true}) } catch (x) { r.push(String(x)) }
            var d = Object.getOwnPropertyDescriptor(e, "price");
            r.push([d.value, d.writable, d.enumerable, d.configurable, Object.isExtensible(e), Object.isExtensible(ctx.entity), "s" in e, e.hasOwnProperty("s")].join());
            r.push(Object.keys(e).join());
            var k = []; for (var x in e) k.push(x); r.push(k.join());
            e = ctx.entity;
            e[1] = "one"; e[undefined] = 1; e[null] = 2; e[1.5] = 3;
            var sym = Symbol("q"); e[sym] = 1; r.push(String(Object.getOwnPropertySymbols(ctx.entity).length));
            delete e.s;
            r.join("|")"#,
        );
        assert_eq!(
            s(&mut rt, &v),
            "TypeError: Host objects do not support accessor properties|TypeError: Host object field b cannot be made read-only|TypeError: Host object field c cannot be made configurable|12,true,true,true,false,true,true,true|b,big,id,meta,nul,price,ratio,s,tags|b,big,id,meta,nul,price,ratio,s,tags|0"
        );
        let m = e.as_map().expect("map").borrow();
        for k in ["1", "undefined", "null", "1.5"] {
            assert!(m.contains_key(k), "{k}");
        }
        assert!(!m.contains_key("s"));
    });
}

#[test]
fn define_without_value_panics_like_go() {
    let r = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let (_rt, _e, _v) =
                script(r#"Object.defineProperty(ctx.entity, "k", {enumerable: true})"#);
        })
        .expect("spawn")
        .join();
    let err = r.expect_err("a panic");
    let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert_eq!(msg, goja::host::gomap::NIL_DESCRIPTOR_VALUE_PANIC);
}

#[test]
fn values_written_are_exported() {
    run_in_zone("values_written_are_exported", Zone::Env("UTC"), || {
        on_big_stack(values_written_are_exported_in_utc)
    });
}

fn values_written_are_exported_in_utc() {
    {
        let (_rt, e, _) = script(
            r#"var e = ctx.entity;
            e.i = 1; e.f = 1.5; e.negz = -0; e.e21 = 1e21; e.small = 1e-7; e.sum = 0.1 + 0.2;
            e.v = undefined; e.a = [1, "a", null, undefined, , 2.5];
            e.m = new Map([["a", 1]]); e.st = new Set([1, 2]); e.bi = 10n; e.big2 = 123456789012345678901234567890n;
            e.nb = new Number(3); e.bo = new Boolean(false); e.str = new String("q");
            e.sym = Symbol(); e.sym2 = Symbol.iterator; e.err = new Error("x"); e.re = /x/;
            e.args = (function () { return arguments })(1, 2);
            e.d = new Date(0); e.bad = new Date(NaN);
            e.h = "<a&b> "; e.p = Promise.resolve(1);
            e.cls = new (class { constructor() { this.z = 1; this.a = 2 } })()"#,
        );
        let want = [
            ("i", "1"),
            ("f", "1.5"),
            ("negz", "-0"),
            ("e21", "1e+21"),
            ("small", "1e-7"),
            ("sum", "0.30000000000000004"),
            ("v", "null"),
            ("a", r#"[1,"a",null,null,null,2.5]"#),
            ("m", r#"[["a",1]]"#),
            ("st", "[1,2]"),
            ("bi", "10"),
            ("big2", "123456789012345678901234567890"),
            ("nb", "3"),
            ("bo", "false"),
            ("str", "{}"),
            ("sym", r#""""#),
            ("sym2", r#""Symbol.iterator""#),
            ("err", "{}"),
            ("re", "{}"),
            ("args", "[1,2]"),
            ("d", r#""1970-01-01T00:00:00Z""#),
            ("bad", "null"),
            ("h", "\"\\u003ca\\u0026b\\u003e\\u2028\""),
            ("p", "{}"),
            ("cls", r#"{"a":2,"z":1}"#),
        ];
        for (k, v) in want {
            assert_eq!(field(&e, k), v, "{k}");
        }
    }
}

#[test]
fn encoding_errors_follow_the_catalog() {
    on_big_stack(|| {
        for (src, msg) in [
            (
                "ctx.entity.n = NaN",
                "cannot encode JSON: NaN is not a valid JSON number",
            ),
            (
                "ctx.entity.n = -Infinity",
                "cannot encode JSON: -Infinity is not a valid JSON number",
            ),
            (
                "ctx.entity.f = function () {}",
                "cannot encode JSON: a function is not a valid JSON value",
            ),
            (
                "var o = {}; o.o = o; ctx.entity.c = o",
                "cannot encode JSON: the value contains a cycle",
            ),
        ] {
            let (_rt, e, _) = script(src);
            assert_eq!(json::marshal(&e).expect_err(src), msg);
        }
    });
}

#[test]
fn export_circular() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let o = rt.new_object();
        o.set_own_str(&mut rt, &"o".into(), Value::Object(o), true)
            .expect("set");
        let v = rt.export(&Value::Object(o)).expect("export");
        let m = v.as_map().expect("map");
        let inner = m
            .borrow()
            .get("o")
            .and_then(|x| x.as_map().cloned())
            .expect("inner");
        assert!(Rc::ptr_eq(m, &inner));
        let res = run(&mut rt, "var a = []; a[0] = a;");
        let v = rt.export(&res).expect("export");
        let DocValue::Slice(h) = &v else {
            panic!("not a slice");
        };
        let DocValue::Slice(h0) = &h.to_vec()[0] else {
            panic!("not a slice");
        };
        assert!(Rc::ptr_eq(&h.backing, &h0.backing));
        m.borrow_mut().clear();
        h.backing.borrow_mut().clear();
    });
}

#[test]
fn export_wrapped_map() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let mut m = DocMap::new();
        m.insert("test".to_string(), DocValue::string("failed"));
        let m = DocValue::new_map(m);
        let v = rt.to_value(&m).expect("to_value");
        let exported = rt.export(&v).expect("export");
        exported
            .as_map()
            .expect("map")
            .borrow_mut()
            .insert("test".to_string(), DocValue::string("passed"));
        assert_eq!(field(&m, "test"), r#""passed""#);
    });
}

#[test]
fn to_value_nil() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        assert!(rt.to_value(&DocValue::Nil).expect("nil").is_null());
        let empty = DocValue::new_slice(Vec::new());
        assert!(rt.to_value(&empty).expect("slice").is_object());
    });
}

#[test]
fn to_value_float() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("f64", &DocValue::Float64(123.0)).expect("set");
        rt.set_value("f32", &DocValue::Float64(f64::from(321f32)))
            .expect("set");
        let v = run(&mut rt, "f64 === 123 && f32 === 321");
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn to_value_nil_value() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("a", &DocValue::Nil).expect("set");
        let v = run(&mut rt, r#"""+a;"#);
        assert_eq!(s(&mut rt, &v), "null");
    });
}

#[test]
fn to_value_nil_big_int() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("n", &DocValue::BigInt(Default::default()))
            .expect("set");
        let v = run(&mut rt, "n === 0n");
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn class_reexport() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_value("m", &DocValue::new_map(DocMap::new()))
            .expect("set");
        let v = run(
            &mut rt,
            r#"
	class C {
		constructor() {
			this.test = true;
		}
	}
	m.C = C;
	const c = new m.C()
	c instanceof C && c.test;
	"#,
        );
        assert!(matches!(v, Value::Bool(true)));
    });
}

#[test]
fn date_export_type() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let v = run(&mut rt, "new Date()");
        assert_eq!(rt.export_type(&v), ExportType::Time);
    });
}

#[test]
fn big_int_export_to() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let v = run(&mut rt, "BigInt(Number.MAX_SAFE_INTEGER + 10);");
        assert_eq!(rt.export_type(&v), ExportType::BigInt);
        let ten = DocValue::BigInt(std::sync::Arc::new(goja::deps::big::Int::new_int(10)));
        let v = rt.to_value(&ten).expect("to_value");
        match &rt.export(&v).expect("export") {
            DocValue::BigInt(b) => assert_eq!(b.string(), "10"),
            _ => panic!("not a bigint"),
        }
    });
}

#[test]
fn promise_export() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let p = run(&mut rt, "Promise.resolve(1)");
        assert_eq!(rt.export_type(&p), ExportType::Promise);
        let e = rt.export(&p).expect("export");
        let back = rt.to_value(&e).expect("to_value");
        assert!(back.same_as(&p));
        let mut other = new_runtime();
        let err = other.to_value(&e).expect_err("transition");
        let err = other.throw_to_run_error(err).error();
        assert!(
            err.starts_with("TypeError: Illegal runtime transition of a Promise"),
            "{err}"
        );
    });
}

#[test]
fn functions_are_exported_and_reread() {
    on_big_stack(|| {
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity;
            e.f = function named(a) { return this.k + a };
            e.g = function (a) { return a * 2 };
            var f = e.f, g = e.g;
            [typeof f, f.length, f.name, g.name, f.call({k: 1}, 2), g(4), String(f), f === e.f].join()"#,
        );
        assert_eq!(
            s(&mut rt, &v),
            "function,0,named,,3,8,function named() { [native code] },false"
        );
        assert!(matches!(
            e.as_map().expect("map").borrow().get("f"),
            Some(DocValue::Func(_))
        ));
    });
}

#[test]
fn time_values_are_go_reflect_objects() {
    run_in_zone(
        "time_values_are_go_reflect_objects",
        Zone::Env("UTC"),
        || on_big_stack(time_values_in_utc),
    );
}

fn time_values_in_utc() {
    {
        let (mut rt, _e, v) = script(
            r#"var e = ctx.entity; e.d = new Date(1500); var d = e.d;
            var r = [Object.getOwnPropertyNames(d).length, typeof d.Year, d.Year(), d.UnixMilli(),
                d instanceof Date, Object.getPrototypeOf(d) === Object.prototype, d.Add.length, d.Year.name];
            r.push(String(d.Month()), d.Month() + 1, String(d.Sub(new Date(0))), d.Sub(new Date(0)) * 1);
            r.push(d.Format("2006-01-02"), d.GoString(), d.In({}).String(), String(d), String(d.Weekday()), d.Weekday() * 1, String(d.Location()));
            var c = d.Clock(); r.push(Array.isArray(c), c.join("-"));
            var dt = d.Date(); r.push(String(dt[1]));
            r.push(d.Add("x").UnixMilli(), d.Format({}));
            try { d.In(5) } catch (x) { r.push(String(x)) }
            try { d.After(5) } catch (x) { r.push(String(x)) }
            r.push(d.MarshalJSON().length, d.Equal(e.d), d == e.d);
            var u = e.d; u.UnmarshalJSON('"2020-01-02T03:04:05Z"'); r.push(u.Year(), e.d.Year());
            try { u.UnmarshalJSON('"x"') } catch (x) { r.push(String(x), u.IsZero()) }
            r.join("|")"#,
        );
        assert_eq!(
            s(&mut rt, &v),
            "47|function|1970|1500|false|true|1|Year|January|2|1.5s|1500000000|1970-01-01|time.Date(1970, time.January, 1, 0, 0, 1, 500000000, time.Local)|1970-01-01 00:00:01.5 +0000 UTC|1970-01-01 00:00:01.5 +0000 UTC|Thursday|4|UTC|true|0-0-1|January|1500|[object Object]|TypeError: could not convert function call parameter 0: could not convert 5 to an object|TypeError: could not convert function call parameter 0: could not convert 5 to an object|24|true|true|2020|1970|Error: \"x\" is not a valid RFC 3339 time|true"
        );
    }
}

#[test]
fn map_pairs_reread() {
    on_big_stack(|| {
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity; e.x = new Map([["a", 1], ["b", 2]]); var x = e.x;
            var p0 = x[0]; var r = [Object.getOwnPropertyNames(x).join(), x.length, Array.isArray(p0), p0 === x[0], p0.join()];
            p0[1] = 5; r.push(String(e.x[0][1]));
            x[1] = ["c", 3]; r.push(e.x[1].join());
            r.join("|")"#,
        );
        assert_eq!(s(&mut rt, &v), "0,1|2|true|true|a,1|5|c,3");
        assert_eq!(field(&e, "x"), r#"[["a",5],["c",3]]"#);
        let (mut rt, e, v) = script(
            r#"var e = ctx.entity; e.x = new Map([["a", 1]]); var x = e.x; e.y = x;
            x[0][1] = 7; var old = x[0]; x[0] = ["z", 9];
            [old.join(), x[0].join()].join("|")"#,
        );
        assert_eq!(s(&mut rt, &v), "a,7|z,9");
        assert_eq!(field(&e, "y"), r#"[["z",9]]"#);
    });
}

#[test]
fn native_host_functions() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let mut ctx = DocMap::new();
        ctx.insert(
            "count".to_string(),
            DocValue::Func(goja::host::FuncRef::Native(HostFn::new(
                "count",
                |rt, call| {
                    if call.arguments.len() < 2 {
                        let v = Value::String(goja::export::string_from_go(
                            b"count(entity, params) expected",
                        ));
                        return Err(goja::Throw::Value(v));
                    }
                    let _ = rt;
                    Ok(Value::Int(3))
                },
            ))),
        );
        rt.set_value("ctx", &DocValue::new_map(ctx)).expect("ctx");
        let v = run(
            &mut rt,
            r#"var r = [ctx.count("a", {}), ctx.count === ctx.count, ctx.count.length, String(ctx.count)];
            try { ctx.count("a") } catch (e) { r.push(typeof e, e) }
            r.join("|")"#,
        );
        assert_eq!(
            s(&mut rt, &v),
            "3|false|0|function count() { [native code] }|string|count(entity, params) expected"
        );
        let err = run_err(&mut rt, r#"ctx.count("a")"#);
        assert!(err.starts_with("count(entity, params) expected"), "{err}");
        assert!(err.contains("at count (native)"), "{err}");
    });
}

#[test]
fn go_errors_are_plain_errors() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let e = rt.new_go_error("server busy");
        rt.set("e", e).expect("set");
        let v = run(
            &mut rt,
            r#"[String(e), e instanceof Error, e.value, typeof GoError].join()"#,
        );
        assert_eq!(s(&mut rt, &v), "Error: server busy,true,,undefined");
    });
}

#[test]
fn global_get_set_use_lexical_bindings() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        run(&mut rt, "let a = 1; const b = 2; var c = 3;");
        assert!(
            rt.get_var("a")
                .expect("a")
                .expect("a")
                .same_as(&Value::Int(1))
        );
        rt.set_var("a", Value::Int(5)).expect("set a");
        let err = rt.set_var("b", Value::Int(6)).expect_err("set b");
        assert!(
            matches!(err, goja::Throw::TypeError(m) if m == "Assignment to constant variable.")
        );
        rt.set_var("d", Value::Int(7)).expect("set d");
        let v = run(&mut rt, "[a, b, c, d].join()");
        assert_eq!(s(&mut rt, &v), "5,2,3,7");
    });
}

#[test]
fn format_v_follows_fmt() {
    let v = decode(r#"{"m":{"b":[1,"x",null],"a":1.5e6},"f":1e-5,"n":0.0001,"i":1000000}"#);
    assert_eq!(
        json::format_v(&v),
        "map[f:1e-05 i:1e+06 m:map[a:1.5e+06 b:[1 x <nil>]] n:0.0001]"
    );
    assert_eq!(json::format_v(&DocValue::Int64(1_000_000)), "1000000");
    assert_eq!(
        json::format_v(&DocValue::Float64(1_500_000.5)),
        "1.5000005e+06"
    );
    assert_eq!(json::format_v(&DocValue::Float64(f64::INFINITY)), "+Inf");
    assert_eq!(json::format_v(&DocValue::Bool(true)), "true");
}

fn nested_json(depth: usize) -> String {
    let mut s = String::new();
    for _ in 0..depth {
        s.push_str(r#"{"a":["#);
    }
    s.push('1');
    for _ in 0..depth {
        s.push_str("]}");
    }
    s
}

#[test]
fn deep_documents_use_no_recursion() {
    let h = std::thread::Builder::new()
        .stack_size(512 << 10)
        .spawn(|| {
            let depth = 9990;
            let doc = decode(&nested_json(depth / 2));
            let copied = doc.deep_copy();
            let text = String::from_utf8(json::marshal(&copied).expect("marshal")).expect("utf8");
            assert_eq!(text.len(), nested_json(depth / 2).len());
            assert!(json::format_v(&doc).starts_with("map[a:[map[a:["));
            let mut rt = new_runtime();
            let mut ctx = DocMap::new();
            ctx.insert("entity".to_string(), doc.clone());
            rt.set_value("ctx", &DocValue::new_map(ctx)).expect("ctx");
            let v = run(
                &mut rt,
                r#"var e = ctx.entity, n = 0, c = e;
                while (typeof c === "object") { c = c.a !== undefined ? c.a : c[0]; n++ }
                var o = {}, p = o; for (var i = 0; i < 9990; i++) { p.x = {}; p = p.x }
                e.deep = o; var arr = [], q = arr; for (var j = 0; j < 9990; j++) { q[0] = []; q = q[0] }
                e.deepa = arr; n"#,
            );
            assert_eq!(v.to_integer(&mut rt).expect("n"), depth as i64);
            let text = json::marshal(&doc).expect("marshal");
            assert!(text.len() > 2 * 9990 * 5);
            drop(rt);
            drop(doc);
            drop(copied);
        })
        .expect("spawn");
    h.join().expect("small stack");
}

#[test]
fn export_keeps_objects_alive_during_getters() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_gc_stress(true);
        let e = decode(INPUT);
        ctx_with(&mut rt, &e);
        run(
            &mut rt,
            r#"var holder = {o: {}};
            var t = {get a() { delete holder.o; var j = []; for (var i = 0; i < 50; i++) j.push({i: i}); return {x: 1} }, b: {c: [1, {d: 2}]}};
            holder.o = t;
            ctx.entity.t = holder.o;
            var m = new Map([[{k: 1}, {get v() { var j = []; for (var i = 0; i < 50; i++) j.push([i]); return 3 }}]]);
            ctx.entity.m = m;"#,
        );
        assert_eq!(field(&e, "t"), r#"{"a":{"x":1},"b":{"c":[1,{"d":2}]}}"#);
        assert_eq!(field(&e, "m"), r#"[[{"k":1},{"v":3}]]"#);
    });
}

#[test]
fn typed_slices_reread_as_arrays() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let b = DocValue::Bytes(TypedSlice::new(vec![1u8, 2]));
        let f = DocValue::Float64s(TypedSlice::new(vec![1.5f64]));
        let mut m = DocMap::new();
        m.insert("b".to_string(), b);
        m.insert("f".to_string(), f);
        let m = DocValue::new_map(m);
        rt.set_value("o", &m).expect("set");
        let v = run(
            &mut rt,
            r#"var x = o.b; x[0] = 9; x[2] = 300;
            [Object.getOwnPropertyNames(x).join(), x[0], x.length, Array.isArray(x), o.b[0], o.f[0]].join("|")"#,
        );
        assert_eq!(s(&mut rt, &v), "0,1,2|9|3|true|9|1.5");
        assert_eq!(field(&m, "b"), r#""CQI=""#);
    });
}

#[test]
fn released_graphs_free_cycles() {
    on_big_stack(|| {
        let (rt, e, _) =
            script("var o = {}; o.o = o; var a = []; a[0] = a; a[1] = o; ctx.entity.c = a");
        let weak = {
            let m = e.as_map().expect("map").borrow();
            match m.get("c") {
                Some(DocValue::Slice(h)) => Rc::downgrade(&h.backing),
                _ => panic!("not a slice"),
            }
        };
        e.release_graph();
        drop(e);
        drop(rt);
        assert!(weak.upgrade().is_none());
    });
}

struct Holder {
    v: std::cell::RefCell<Option<Value>>,
}

static HOLDER_METHODS: &[goja::host::GoMethod] = &[goja::host::GoMethod {
    name: "Get",
    num_in: 0,
}];

impl goja::host::DynamicObject for Holder {
    fn get(&self, _rt: &mut Runtime, key: &str) -> goja::JsResult<Option<Value>> {
        Ok((key == "v").then(|| self.v.borrow().clone()).flatten())
    }

    fn set(&self, _rt: &mut Runtime, key: &str, val: Value) -> goja::JsResult<bool> {
        if key == "v" {
            *self.v.borrow_mut() = Some(val);
        }
        Ok(true)
    }

    fn has(&self, _rt: &mut Runtime, key: &str) -> goja::JsResult<bool> {
        Ok(key == "v")
    }

    fn delete(&self, _rt: &mut Runtime, _key: &str) -> goja::JsResult<bool> {
        Ok(true)
    }

    fn keys(&self, _rt: &mut Runtime) -> goja::JsResult<Vec<String>> {
        Ok(vec!["v".to_string()])
    }

    fn export(self: Rc<Self>) -> DocValue {
        DocValue::Host(self)
    }

    fn trace(&self, t: &mut goja::heap::Tracer) {
        if let Some(v) = &*self.v.borrow() {
            t.value(v);
        }
    }
}

impl goja::host::GoMethods for Holder {
    fn methods(&self) -> &'static [goja::host::GoMethod] {
        HOLDER_METHODS
    }

    fn call(
        &self,
        _rt: &mut Runtime,
        _idx: usize,
        _call: goja::FunctionCall<'_>,
    ) -> goja::JsResult<Value> {
        Ok(self.v.borrow().clone().unwrap_or_default())
    }

    fn trace(&self, t: &mut goja::heap::Tracer) {
        if let Some(v) = &*self.v.borrow() {
            t.value(v);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn exported_host_values_stay_traced() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let h = rt.new_dynamic_object(Rc::new(Holder {
            v: std::cell::RefCell::new(None),
        }));
        rt.set("h", Value::Object(h)).expect("h");
        rt.set_value("m", &DocValue::new_map(DocMap::new()))
            .expect("m");
        run(&mut rt, "h.v = {k: 1}; m.x = h; h = null;");
        for _ in 0..3 {
            run(
                &mut rt,
                "var junk = []; for (var i = 0; i < 5000; i++) junk.push({i: i}); junk = null;",
            );
            rt.handles.clear();
            rt.collect_garbage();
        }
        let v = run(
            &mut rt,
            "var x = m.x; [Object.getOwnPropertyNames(x).join(), x.Get().k].join()",
        );
        assert_eq!(s(&mut rt, &v), "Get,1");
    });
}

struct Busy;

#[test]
fn go_errors_keep_their_cause() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let f = HostFn::new("query", |rt, _call| {
            let e = rt.new_go_error_with("server busy", Rc::new(Busy));
            Err(goja::Throw::Value(e))
        });
        rt.set_value("f", &DocValue::Func(goja::host::FuncRef::Native(f)))
            .expect("f");
        for src in ["f()", "try { f() } catch (e) { throw e }"] {
            let err = rt.run_string(src).expect_err(src);
            assert!(err.error().starts_with("Error: server busy"), "{src}");
            let cause = rt.run_error_cause(&err).expect("cause");
            assert!(cause.downcast_ref::<Busy>().is_some(), "{src}");
        }
        let err = rt
            .run_string(r#"throw new Error("server busy")"#)
            .expect_err("plain");
        assert!(rt.run_error_cause(&err).is_none());
    });
}

#[test]
fn reset_shells_reject_previous_handles() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let v = run(&mut rt, "(function named() { return 1 })");
        let f = rt.export(&v).expect("export");
        rt.host.reset();
        let g = rt.to_value(&f).expect("to_value");
        rt.set("g", g).expect("g");
        let err = run_err(&mut rt, "g()");
        assert!(
            err.starts_with("TypeError: Illegal runtime transition of an Object"),
            "{err}"
        );
    });
}

#[test]
fn pair_index_write_panics_like_go() {
    let r = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let (_rt, _e, _v) =
                script(r#"ctx.entity.x = new Map([["a", 1]]); var p = ctx.entity.x[0]; p[5] = 1"#);
        })
        .expect("spawn")
        .join();
    let err = r.expect_err("a panic");
    let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert_eq!(msg, goja::host::goarray_reflect::ARRAY_INDEX_PANIC);
}
