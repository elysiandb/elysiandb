mod jsrun;

use goja::Value;
use goja::object::ObjExt;
use jsrun::{new_runtime, on_big_stack, run_error_string};

fn check(cases: &'static [(&'static str, &'static str)]) {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for (script, want) in cases {
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(v) => v.go_string(&mut rt).expect("go_string"),
                Err(e) => format!("!{}", e.error()),
            };
            if got != *want {
                failures.push(format!("{script}\n  got:  {got:?}\n  want: {want:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn test_native_constructor_frames_are_named() {
    check(&[
        (
            "var s; try { Number(Symbol()); } catch (e) { s = e.stack; } s",
            "TypeError: Cannot convert a Symbol value to a number\n\tat Number (native)\n\tat <eval>:1:20(5)\n",
        ),
        (
            "var s; try { new Date(Symbol()); } catch (e) { s = e.stack; } s",
            "TypeError: Cannot convert a Symbol value to a number\n\tat <eval>:1:14(5)\n",
        ),
        (
            "var s; try { BigInt(1.5); } catch (e) { s = e.stack; } s",
            "RangeError: Cannot convert 1.5 to a BigInt\n\tat BigInt (native)\n\tat <eval>:1:20(4)\n",
        ),
        (
            "var s; try { [1].map(function() { BigInt(0.5); }); } catch (e) { s = e.stack; } s",
            "RangeError: Cannot convert 0.5 to a BigInt\n\tat BigInt (native)\n\tat <eval>:1:41(3)\n\tat map (native)\n\tat <eval>:1:21(7)\n",
        ),
    ]);
}

fn check_out(cases: &'static [(&'static str, &'static str)]) {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for (script, want) in cases {
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(_) => {
                    let v = rt.get("out").expect("get").unwrap_or_default();
                    v.go_string(&mut rt).expect("go_string")
                }
                Err(e) => format!("!{}", e.error()),
            };
            if got != *want {
                failures.push(format!("{script}\n  got:  {got:?}\n  want: {want:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn test_resumed_async_functions_keep_their_callers() {
    check_out(&[
        (
            "var out; async function foo(x) { await bar(x); } async function bar(x) { await x; throw new Error(\"boom\"); } async function test() { try { await foo(1); } catch (e) { out = e.stack; } } test();",
            "Error: boom\n\tat bar (<eval>:1:89(6))\n\tat foo (<eval>:1:43(5))\n\tat test (<eval>:1:149(6))\n",
        ),
        (
            "var out; async function a() { await null; out = new Error(\"x\").stack; } async function b() { await a(); } async function c() { await b(); } c();",
            "Error: x\n\tat a (<eval>:1:49(7))\n\tat b (<eval>:1:101(4))\n\tat c (<eval>:1:135(4))\n",
        ),
        (
            "var out; async function a() { await null; out = new Error(\"x\").stack; } async function b() { await a(); } async function c() { b(); } c();",
            "Error: x\n\tat a (<eval>:1:49(7))\n\tat b (<eval>:1:101(4))\n",
        ),
        (
            "var out; async function a() { await null; out = new Error(\"x\").stack; } async function b() { var p = a(); p.then(function(){}); await p; } b();",
            "Error: x\n\tat a (<eval>:1:49(7))\n",
        ),
        (
            "var out; async function a() { out = new Error(\"x\").stack; } async function b() { await a(); } b();",
            "Error: x\n\tat a (<eval>:1:37(4))\n\tat b (<eval>:1:89(2))\n\tat <eval>:1:96(4)\n",
        ),
    ]);
}

#[test]
fn test_index_assignment_through_plain_prototypes() {
    check(&[
        (
            "var top = {}; Object.defineProperty(top, \"0\", {set: function(v){ this.seen = v; }}); var mid = Object.create(top); var a = []; Object.setPrototypeOf(a, mid); a[0] = 7; a.seen + \" \" + a.hasOwnProperty(0) + \" \" + a.length",
            "undefined true 1",
        ),
        (
            "var top = {}; Object.defineProperty(top, \"0\", {set: function(v){ this.seen = v; }}); var mid = Object.create(top); var o = Object.create(mid); o[0] = 7; o.seen + \" \" + o.hasOwnProperty(0)",
            "7 false",
        ),
        (
            "var top = {}; Object.defineProperty(top, \"0\", {value: 1, writable: false}); var mid = Object.create(top); var a = []; Object.setPrototypeOf(a, mid); a[0] = 7; a[0] + \" \" + a.hasOwnProperty(0)",
            "7 true",
        ),
        (
            "var top = {}; Object.defineProperty(top, \"0\", {set: function(v){ this.seen = v; }}); var mid = Object.create(top); var s = new String(\"ab\"); Object.setPrototypeOf(s, mid); s[5] = 7; s.seen + \" \" + s.hasOwnProperty(5)",
            "undefined true",
        ),
        (
            "var called = false; Object.defineProperty(Object.prototype, \"0\", {set: function(v){ called = true; }, configurable: true}); var a = []; a[0] = 1; called + \" \" + a.hasOwnProperty(0) + \" \" + a.length",
            "true false 0",
        ),
        (
            "var proto = {}; Object.defineProperty(proto, \"0\", {set: function(v){ this.seen = v; }}); var a = []; Object.setPrototypeOf(a, proto); a[0] = 7; a.seen + \" \" + a.hasOwnProperty(0)",
            "7 false",
        ),
    ]);
}

#[test]
fn test_big_int_is_not_an_integer_argument() {
    check(&[
        (
            "(5).toExponential(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toExponential (native)",
        ),
        (
            "(5).toFixed(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toFixed (native)",
        ),
        (
            "(5).toPrecision(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toPrecision (native)",
        ),
        (
            "(5).toString(2n)",
            "!TypeError: Cannot convert a BigInt value to a number at toString (native)",
        ),
        (
            "(5).toExponential(Object(1n))",
            "!TypeError: Cannot convert a BigInt value to a number at toExponential (native)",
        ),
        (
            "\"abc\".charAt(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at charAt (native)",
        ),
        (
            "\"abc\".substring(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at substring (native)",
        ),
        (
            "[1,2,3].slice(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at slice (native)",
        ),
        (
            "var a = [1,2]; a.length = 1n",
            "!TypeError: Cannot convert a BigInt value to a number at <eval>:1:18(10)",
        ),
    ]);
}

fn representation(v: Option<Value>, rt: &goja::Runtime) -> String {
    let Some(Value::Object(o)) = v else {
        return "notobj".to_string();
    };
    match &rt.obj(o).ext {
        ObjExt::Array(a) => format!(
            "dense(len={},cap={},length={},obj={},pvc={})",
            a.values.len(),
            a.values.capacity(),
            a.length,
            a.obj_count as i64,
            a.prop_value_count as i64
        ),
        ObjExt::SparseArray(a) => format!(
            "sparse(items={},length={},pvc={})",
            a.items.len(),
            a.length,
            a.prop_value_count as i64
        ),
        _ => "other".to_string(),
    }
}

#[test]
fn test_array_storage_grows_like_go_slices() {
    const CASES: &[(&str, &str)] = &[
        (
            r#"var a = [...Array(5000).keys()];"#,
            "dense(len=5000,cap=5120,length=5000,obj=5000,pvc=0)",
        ),
        (
            r#"var a = [1, 2, ...[3, 4, 5]];"#,
            "dense(len=5,cap=8,length=5,obj=5,pvc=0)",
        ),
        (
            r#"var a = [1, 2, 3]; a.push(4);"#,
            "dense(len=4,cap=6,length=4,obj=4,pvc=0)",
        ),
        (
            r#"var b = [...Array(600).keys()]; var a = [0, ...b, 1, 2];"#,
            "dense(len=603,cap=1023,length=603,obj=603,pvc=0)",
        ),
        (
            r#"function f(...r) { return r; } var a = f(1, 2, 3, 4, 5);"#,
            "dense(len=5,cap=5,length=5,obj=5,pvc=0)",
        ),
        (
            r#"function f(x, ...r) { return r; } var a = f(...Array(600).keys());"#,
            "dense(len=599,cap=599,length=599,obj=599,pvc=0)",
        ),
        (
            r#"function f(x, ...r) { var g = function() { return x; }; return r; } var a = f(...Array(600).keys());"#,
            "dense(len=599,cap=599,length=599,obj=599,pvc=0)",
        ),
        (
            r#"var [x, ...a] = Array(600).fill(1);"#,
            "dense(len=599,cap=1023,length=599,obj=599,pvc=0)",
        ),
        (
            r#"var o = {}; for (var i = 0; i < 600; i++) { o["k" + i] = i; } var a = Object.keys(o);"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var o = {}; for (var i = 0; i < 600; i++) { o["k" + i] = i; } var a = Object.getOwnPropertyNames(o);"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var o = {}; for (var i = 0; i < 600; i++) { o["k" + i] = i; } var a = Object.values(o);"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var o = {}; for (var i = 0; i < 600; i++) { o["k" + i] = i; } var a = Object.entries(o);"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var o = []; for (var i = 0; i < 600; i++) { o[i] = i; } o.x = 1; var a = Object.keys(o);"#,
            "dense(len=601,cap=1023,length=601,obj=601,pvc=0)",
        ),
        (
            r#"var o = {}; for (var i = 0; i < 600; i++) { o[Symbol()] = i; } var a = Object.getOwnPropertySymbols(o);"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var a = "x,".repeat(600).split(",");"#,
            "dense(len=601,cap=601,length=601,obj=601,pvc=0)",
        ),
        (
            r#"var a = "x1".repeat(600).split(/\d/);"#,
            "dense(len=601,cap=1023,length=601,obj=601,pvc=0)",
        ),
        (
            r#"var a = "x1y".repeat(600).split(/(\d)/);"#,
            "dense(len=1201,cap=1535,length=1201,obj=1201,pvc=0)",
        ),
        (
            r#"var a = new AggregateError(Array(600).fill(0)).errors;"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var a; Promise.all(Array(600).fill(0)).then(function(v) { a = v; });"#,
            "dense(len=600,cap=1023,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var a; Promise.allSettled(Array(300).fill(0)).then(function(v) { a = v; });"#,
            "dense(len=300,cap=303,length=300,obj=300,pvc=0)",
        ),
        (
            r#"var a; Promise.any(Array(300).fill(0).map(function(){ return Promise.reject(1); })).catch(function(e) { a = e.errors; });"#,
            "dense(len=300,cap=303,length=300,obj=300,pvc=0)",
        ),
        (
            r#"var a = "x1y".repeat(600).match(/\d/g);"#,
            "dense(len=600,cap=600,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var a = /(a)(b)?/.exec("ab");"#,
            "dense(len=3,cap=3,length=3,obj=3,pvc=0)",
        ),
        (
            r#"var a = Array.from(new Set(Array(600).keys()));"#,
            "dense(len=600,cap=1024,length=600,obj=600,pvc=0)",
        ),
        (
            r#"var a = [...new Map([[1, 2]]).entries()][0];"#,
            "dense(len=2,cap=2,length=2,obj=2,pvc=0)",
        ),
        (
            r#"var re = /\d/; re[Symbol.split] = RegExp.prototype[Symbol.split]; var C = function() { return /\d/y; }; re.constructor = {}; re.constructor[Symbol.species] = C; var a = re[Symbol.split]("a1".repeat(600));"#,
            "dense(len=601,cap=1023,length=601,obj=601,pvc=0)",
        ),
        (
            r#"var a = Array.prototype.concat.call([], ...Array(10).fill([1, 2, 3]));"#,
            "dense(len=30,cap=32,length=30,obj=30,pvc=0)",
        ),
        (
            r#"var a = "x".repeat(300).split("");"#,
            "dense(len=300,cap=300,length=300,obj=300,pvc=0)",
        ),
    ];
    on_big_stack(|| {
        let mut failures = Vec::new();
        for (script, want) in CASES {
            let mut rt = new_runtime();
            if let Err(e) = rt.run_string(script) {
                panic!("{script}: {}", run_error_string(&e));
            }
            let v = rt.get("a").expect("get");
            let got = representation(v, &rt);
            if got != *want {
                failures.push(format!(
                    "{script}
  got:  {got}
  want: {want}"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{}",
            failures.join(
                "
"
            )
        );
    });
}

#[test]
fn test_shared_helpers_keep_builtin_behaviour() {
    check(&[
        (r#"var s = new Set([3, 1, 3, 2]); [...s].join()"#, "3,1,2"),
        (
            r#"var seen = []; class S extends Set { add(v) { seen.push(v); return super.add(v); } } var arr = [1, 2, 3]; new S(arr); seen.join()"#,
            "1,2,3",
        ),
        (
            r#"var a = {}, b = {}; var w = new WeakSet([a, b]); w.has(a) && w.has(b)"#,
            "true",
        ),
        (
            r#"var seen = 0; class W extends WeakSet { add(v) { seen++; return super.add(v); } } new W([{}, {}, {}]); seen"#,
            "3",
        ),
        (
            r#"new WeakSet([1])"#,
            "!TypeError: Value is not an object: 1 at <eval>:1:1(4)",
        ),
        (
            r#"Number(Object(12345678901234567890n))"#,
            "-6101065172474983000",
        ),
        (r#"Number(5n) + new Number(7n)"#, "12"),
        (r#"BigInt(Object(3)) + BigInt("0x10")"#, "19"),
        (
            r#"BigInt({valueOf() { return 2.5; }})"#,
            "!RangeError: Cannot convert 2.5 to a BigInt at BigInt (native)",
        ),
        (
            r#"var C = function(executor) { return new Promise(executor); }; Promise.resolve.call(C, 3) instanceof Promise"#,
            "true",
        ),
        (
            r#"Promise.resolve.call({}, 1)"#,
            "!TypeError: Value is not a constructor at call (native)",
        ),
        (
            r#"var it = [1, 2][Symbol.iterator](); [...it].length + Object.prototype.toString.call([].entries())"#,
            "2[object Array Iterator]",
        ),
        (
            r#"[...new Map([[1, "a"]]).keys(), ...new Map([[1, "a"]]).values(), ...new Set([5]).entries()].join("|")"#,
            "1|a|5,5",
        ),
    ]);
}

#[test]
fn test_string_keys_and_lengths_on_hot_paths() {
    check(&[
        (
            r#""x".length + "," + "abcdef".length + "," + "abc".charCodeAt(1) + "," + "été".length"#,
            "1,6,98,3",
        ),
        (
            r#"var s = "abc"; s.length = 7; s.length + "," + "zz".length"#,
            "3,2",
        ),
        (
            r#"({a: 1}).hasOwnProperty("a") + "," + ({a: 1}).hasOwnProperty("b") + "," + ({"é": 1}).hasOwnProperty("é")"#,
            "true,false,true",
        ),
        (
            r#"("b" < "a") + "," + ("a" < "é") + "," + ("é" < "a") + "," + ["b", "a", "é", "aa", "A"].sort().join()"#,
            "false,true,false,A,a,aa,b,é",
        ),
        (
            r#"var a = []; a[0] = 1; a[3] = 2; a[-1] = 3; a[4294967294] = 4; a[4294967295] = 5; Object.keys(a).join() + "|" + a.length"#,
            "0,3,4294967294,-1,4294967295|4294967295",
        ),
        (
            r#"var a = []; for (var i = 0; i < 12; i++) { a[i] = i * 2; } a.join() + "|" + a.length"#,
            "0,2,4,6,8,10,12,14,16,18,20,22|12",
        ),
        (
            r#"var o = Object.create(Array.prototype); o[2] = 1; o[-7] = 2; Object.keys(o).join()"#,
            "2,-7",
        ),
        (
            r#"var called = []; Object.defineProperty(Object.prototype, "5", {set: function(v) { called.push(v); }, configurable: true}); var a = []; a[5] = 9; called.join() + "|" + a.hasOwnProperty(5) + "|" + a.length"#,
            "9|false|0",
        ),
    ]);
}
