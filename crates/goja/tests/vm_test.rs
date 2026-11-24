mod jsrun;

use goja::file::File;
use goja::string::ascii_string;
use goja::vm::{BindGlobal, Instruction};
use goja::{Program, Value};
use jsrun::*;
use std::sync::Arc;

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
