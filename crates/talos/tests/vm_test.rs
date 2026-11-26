mod jsrun;
mod vm_test_cases;

use jsrun::*;
use std::sync::Arc;
use talos::file::File;
use talos::string::ascii_string;
use talos::vm::{BindGlobal, Instruction};
use talos::{Program, Value};

fn run(src: &str) -> Result<Value, String> {
    let mut rt = new_runtime();
    rt.run_string(src).map_err(|e| run_error_string(&e))
}

#[test]
fn test_vm1() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let prg = Program::from_code(
            vec![
                Instruction::BindGlobal(Box::new(BindGlobal {
                    vars: vec!["v".into()],
                    ..BindGlobal::default()
                })),
                Instruction::NewObject,
                Instruction::SetGlobal("v".into()),
                Instruction::LoadVal(Value::String(ascii_string("test"))),
                Instruction::LoadVal(Value::Int(3)),
                Instruction::LoadVal(Value::Int(2)),
                Instruction::Add,
                Instruction::SetElem,
                Instruction::Pop,
                Instruction::LoadDynamic("v".into()),
            ],
            Some(Arc::new(File::new("dummy", b"", 1))),
        );
        rt.vm.prg = Some(Arc::new(prg));
        rt.vm_run().expect("run");
        let rv = rt.vm.pop();
        match rv {
            Value::Object(o) => {
                let v = o
                    .get_str(&mut rt, &"test".into(), None)
                    .expect("get")
                    .unwrap_or_default();
                let v = v.to_integer(&mut rt).expect("to_integer");
                assert_eq!(v, 5, "Unexpected property value: {v}");
            }
            other => panic!("Unexpected result: {other:?}"),
        }
    });
}

#[test]
fn test_eval_var() {
    test_script(
        r#"
	function test() {
		var a;
		return eval("var a = 'yes'; var z = 'no'; a;") === "yes" && a === "yes";
	}
	test();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_resolve_mixed_stack1() {
    test_script(
        r#"
	function test(arg) {
		var a = 1;
		var scope = {};
		(function() {return arg})(); // move arguments to stash
		with (scope) {
			a++; // resolveMixedStack1 here
			return a + arg;
		}
	}
	test(40);
	"#,
        Value::Int(42),
    );
}

#[test]
fn smoke_run_string_add() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let v = rt.run_string("1+2").expect("run");
        assert!(v.same_as(&Value::Int(3)), "{v:?}");
    });
}

#[test]
fn smoke_functions() {
    on_big_stack(|| {
        let r =
            run("function fib(n) { if (n < 2) return n; return fib(n - 2) + fib(n - 1); } fib(20)");
        assert!(r.as_ref().unwrap().same_as(&Value::Int(6765)), "{r:?}");
        let r = run("var o = {a: 1}; o.b = 2; o.a + o.b");
        assert!(r.as_ref().unwrap().same_as(&Value::Int(3)), "{r:?}");
        let r = run("try { undefinedVar } catch (e) { e.message }");
        assert!(
            r.as_ref()
                .unwrap()
                .same_as(&str_value("undefinedVar is not defined")),
            "{r:?}"
        );
        let r = run("var x; x.y");
        assert_eq!(
            r.unwrap_err(),
            "TypeError: Cannot read property 'y' of undefined\n\tat <eval>:1:10(2)\n"
        );
        let r = run(
            "(function() { var c = 0; function inc() { c++; return c; } inc(); inc(); return inc(); })()",
        );
        assert!(r.as_ref().unwrap().same_as(&Value::Int(3)), "{r:?}");
        let r = run("let s = ''; for (var k in {a: 1, b: 2}) { s += k; } s");
        assert!(r.as_ref().unwrap().same_as(&str_value("ab")), "{r:?}");
    });
}

fn interrupt_after(rt: &talos::Runtime, ms: u64) -> std::thread::JoinHandle<()> {
    let h = rt.interrupt_handle();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        h.interrupt("halt");
    })
}

#[test]
fn interrupt_skips_catch_and_finally() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let t = interrupt_after(&rt, 50);
        let r = rt.run_string(
            "var i = 0; var caught = 'no'; try { for (;;) { i++; } } catch (e) { caught = 'catch'; } finally { caught = 'finally'; }",
        );
        t.join().expect("join");
        match r {
            Err(talos::RunError::Interrupted(e)) => {
                assert!(e.error().starts_with("halt at <eval>:1:"), "{}", e.error());
                assert_eq!(e.text, "halt");
            }
            Err(e) => panic!("unexpected error {}", run_error_string(&e)),
            Ok(v) => panic!("unexpected result {v:?}"),
        }
        assert_eq!(rt.vm.sp, 0);
        assert!(rt.vm.call_stack.is_empty());
        let v = rt.run_string("caught").expect("run after interrupt");
        assert!(v.same_as(&str_value("no")), "{v:?}");
    });
}

#[test]
fn interrupt_inside_native_callback() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let t = interrupt_after(&rt, 50);
        let r = rt.run_string(
            "function f() { for (;;) {} } var r = 'no'; try { f.call(null); } catch (e) { r = 'catch'; } r",
        );
        t.join().expect("join");
        match r {
            Err(talos::RunError::Interrupted(e)) => {
                let s = e.string();
                assert!(s.starts_with("halt\n\tat f (<eval>:1:"), "{s}");
                assert!(s.contains("\tat call (native)\n"), "{s}");
            }
            Err(e) => panic!("unexpected error {}", run_error_string(&e)),
            Ok(v) => panic!("unexpected result {v:?}"),
        }
        assert_eq!(rt.vm.sp, 0);
        let v = rt.run_string("r").expect("run after interrupt");
        assert!(v.same_as(&str_value("no")), "{v:?}");
    });
}

#[test]
fn stack_overflow_is_uncatchable() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        rt.set_max_call_stack_size(100);
        let r = rt.run_string(
            "var c = 'no'; function f() { f(); } try { f(); } catch (e) { c = 'catch'; } finally { c = 'finally'; }",
        );
        match r {
            Err(talos::RunError::StackOverflow(e)) => {
                assert!(e.exception.stack.len() > 100, "{}", e.exception.stack.len());
            }
            Err(e) => panic!("unexpected error {}", run_error_string(&e)),
            Ok(v) => panic!("unexpected result {v:?}"),
        }
        let v = rt.run_string("c").expect("run after overflow");
        assert!(v.same_as(&str_value("no")), "{v:?}");
    });
}

#[test]
fn native_frame_stays_visible_in_stack() {
    on_big_stack(|| {
        let r = run("function f() { null.x } function g() { f.call(); } g()");
        assert_eq!(
            r.unwrap_err(),
            "TypeError: Cannot read property 'x' of undefined\n\tat f (<eval>:1:21(2))\n\tat call (native)\n\tat g (<eval>:1:46(3))\n\tat <eval>:1:53(4)\n"
        );
    });
}

#[test]
fn error_messages_keep_the_failure_of_a_formatted_value() {
    const SCRIPT: &str = r#"function Boom(m) { this.message = m; }
Boom.prototype.toString = function () { return "Boom: " + this.message; };
var key = { toString: function () { throw new Boom("key"); } };
var twice = { toString: function () { throw { toString: function () { throw new Boom("twice"); } }; } };
var plain = { toString: function () { throw 42; } };
function message(f) {
  try {
    f();
  } catch (e) {
    return e.constructor.name + ": " + e.message;
  }
  return "no error";
}
[
  message(function () { var b = null; b[key]; }),
  message(function () { var b; b[key](); }),
  message(function () { var b = null; b[twice]; }),
  message(function () { var b = null; b[plain]; }),
  message(function () { [1].filter(key); }),
  message(function () { [1].reduce(key); }),
  message(function () { [1].reduceRight(key); }),
  message(function () { Array.from([1], key); }),
  message(function () { [{ toLocaleString: 1, toString: key.toString }].toLocaleString(); }),
  message(function () { Date.prototype[Symbol.toPrimitive].call({}, key); }),
  message(function () { Number.prototype.valueOf.call(key); }),
  message(function () { Object.setPrototypeOf(Object.preventExtensions({ toString: key.toString }), {}); }),
  message(function () {
    var t = {};
    Object.defineProperty(t, "x", { value: key });
    new Proxy(t, { get: function () { return 1; } }).x;
  }),
  message(function () { [1].every(key); }),
].join("\n");
"#;
    const EXPECTED: &str = r#"TypeError: Cannot read property '%!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4)))' of undefined
TypeError: Cannot read property '%!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4)))' of undefined
Boom: twice
TypeError: Cannot read property '%!s(PANIC=String method: 42 at toString (<eval>:5:39(2)))' of undefined
TypeError: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not a function
TypeError: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not a function
TypeError: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not a function
TypeError: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not a function
TypeError: Property 'toLocaleString' of object %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not a function
TypeError: Invalid hint: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4)))
TypeError: Value is not a number: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4)))
TypeError: %!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4))) is not extensible
TypeError: 'get' on proxy: property 'x' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected '%!s(PANIC=String method: Boom: key at toString (<eval>:3:37(4)))' but got '{}')
Boom: key"#;
    on_big_stack(|| {
        let mut rt = new_runtime();
        let v = rt.run_string(SCRIPT).expect("run");
        let got = v.go_string(&mut rt).expect("string");
        assert_eq!(got, EXPECTED);
    });
}

fn oracle_type_name(v: &Value) -> &'static str {
    match v {
        Value::Undefined => "talos.valueUndefined",
        Value::Null => "talos.valueNull",
        Value::Bool(_) => "talos.valueBool",
        Value::Int(_) => "talos.valueInt",
        Value::Float(_) => "talos.valueFloat",
        Value::String(talos::JsString::Ascii(_)) => "talos.asciiString",
        Value::String(talos::JsString::Unicode(_)) => "talos.unicodeString",
        Value::String(talos::JsString::Imported(_)) => "*talos.importedString",
        Value::BigInt(_) => "*talos.valueBigInt",
        Value::Symbol(_) => "*talos.Symbol",
        Value::Object(_) => "*talos.Object",
        _ => "other",
    }
}

fn oracle_line(rt: &mut talos::Runtime, r: Result<Value, talos::RunError>) -> String {
    use gocompat::strconv::quote;
    match r {
        Ok(v) => {
            let s = v
                .go_string(rt)
                .unwrap_or_else(|_| "<string error>".to_string());
            format!("RESULT {} {}", oracle_type_name(&v), quote(s.as_bytes()))
        }
        Err(talos::RunError::Exception(e)) => format!(
            "EXCEPTION {}\nERROR {}",
            quote(e.string().as_bytes()),
            quote(e.error().as_bytes())
        ),
        Err(e) => format!("ERR {}", quote(e.error().as_bytes())),
    }
}

#[test]
fn baseline_oracle_cases() {
    on_big_stack(|| {
        let mut failures = Vec::new();
        for (src, expected) in vm_test_cases::ORACLE_CASES {
            let mut rt = new_runtime();
            let r = rt.run_script("", src);
            let got = oracle_line(&mut rt, r);
            if got != *expected {
                failures.push(format!("{src}\n  expected: {expected}\n  got: {got}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}
