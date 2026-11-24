mod jsrun;

use goja::ipow::ipow;

#[test]
fn test_i_pow() {
    assert_eq!(ipow(-9223372036854775808, 1), -9223372036854775808);
    assert_eq!(ipow(9223372036854775807, 1), 9223372036854775807);
    assert_eq!(ipow(-9223372036854775807, 1), -9223372036854775807);
    assert_eq!(ipow(9223372036854775807, 0), 1);
    assert_eq!(ipow(-9223372036854775807, 0), 1);
    assert_eq!(ipow(-9223372036854775808, 0), 1);
}

#[test]
fn test_promise_jobs_run_in_order_while_collecting() {
    let h = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let mut rt = goja::Runtime::new();
            rt.set_gc_stress(true);
            let src = r#"
	var s = "";
	var p = Promise.resolve(1);
	p.then(function(v) {
		for (var i = 0; i < 50; i++) {
			var o = {n: i};
		}
		s += "a" + v;
	});
	p.then(function(v) {
		var q = {v: v};
		s += "b" + q.v;
	});
	"#;
            if let Err(e) = rt.run_string(src) {
                panic!("{e}");
            }
            let v = rt.run_string("s").unwrap_or_else(|e| panic!("{e}"));
            let s = v.to_string(&mut rt).expect("to_string").to_rust_string();
            assert_eq!(s, "a1b1");
        })
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

#[cfg(target_arch = "aarch64")]
#[test]
fn test_exponentiation_follows_go_math() {
    let h = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/golden/gomath/arm64.cases");
            let data = std::fs::read_to_string(&p).expect("golden");
            let mut rt = goja::Runtime::new();
            let mut checked = 0;
            for line in data.lines().filter(|l| l.starts_with("pow ")) {
                let f: Vec<&str> = line.split(' ').collect();
                let x = f64::from_bits(u64::from_str_radix(f[1], 16).expect("hex"));
                let y = f64::from_bits(u64::from_str_radix(f[2], 16).expect("hex"));
                let want = f64::from_bits(u64::from_str_radix(f[3], 16).expect("hex"));
                rt.set("x", goja::Value::Float(x)).expect("set x");
                rt.set("y", goja::Value::Float(y)).expect("set y");
                let v = rt.run_string("x ** y").unwrap_or_else(|e| panic!("{e}"));
                let got = v.to_float(&mut rt).expect("to_float");
                assert!(
                    got.to_bits() == want.to_bits() || got.is_nan() && want.is_nan(),
                    "{x:?} ** {y:?} = {got:?}, want {want:?}"
                );
                checked += 1;
            }
            assert!(checked > 1000);
        })
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn test_promise_all() {
    const SCRIPT: &str = r#"
var p1 = new Promise(function() {});
var p2 = new Promise(function() {});
var p3 = new Promise(function() {});
var callCount = 0;
var currentThis = p1;
var nextThis = p2;
var afterNextThis = p3;

p1.then = p2.then = p3.then = function(a, b) {
  assert.sameValue(typeof a, 'function', 'type of first argument');
  assert.sameValue(
    a.length,
    1,
    'ES6 25.4.1.3.2: The length property of a promise resolve function is 1.'
  );
  assert.sameValue(typeof b, 'function', 'type of second argument');
  assert.sameValue(
    b.length,
    1,
    'ES6 25.4.1.3.1: The length property of a promise reject function is 1.'
  );
  assert.sameValue(arguments.length, 2, '"then"" invoked with two arguments');
  assert.sameValue(this, currentThis, '"this" value');

  currentThis = nextThis;
  nextThis = afterNextThis;
  afterNextThis = null;

  callCount += 1;
};

Promise.all([p1, p2, p3]);

assert.sameValue(callCount, 3, '"then"" invoked once for every iterated value');
	"#;
    jsrun::test_script_with_test_lib(SCRIPT, goja::Value::Undefined);
}

#[test]
fn test_async_func_throw() {
    const SCRIPT: &str = r#"
	class TestError extends Error {
	}

	async function f() {
		throw new TestError();
	}

	async function f1() {
		try {
			await f();
		} catch (e) {
			assert.sameValue(e.constructor.name, TestError.name);
			return;
		}
		throw new Error("No exception was thrown");
	}
	await f1();
	return undefined;
	"#;
    jsrun::test_async_func_with_test_lib(SCRIPT, goja::Value::Undefined);
}

fn test_async_func_with_test_lib_x(src: &'static str, expected: goja::Value) {
    jsrun::on_big_stack(move || {
        let mut rt = jsrun::new_runtime();
        for p in [jsrun::test_lib(), jsrun::test_lib_x()] {
            if let Err(e) = rt.run_program(&p) {
                panic!("{}", jsrun::run_error_string(&e));
            }
        }
        jsrun::rt_test_async_func(&mut rt, src, &expected);
    });
}

#[test]
#[ignore = "requires the caller frames of resumed async functions in the VM"]
fn test_async_stacktrace() {
    const SCRIPT: &str = r#"
	let ex;
	async function foo(x) {
	  await bar(x);
	}

	async function bar(x) {
	  await x;
	  throw new Error("Let's have a look...");
	}

	try {
		await foo(1);
	} catch (e) {
		assertStack(e, [
			["test.js", "bar", 9, 10],
			["test.js", "foo", 4, 13],
			["test.js", "test", 13, 12],
		]);
	}
	"#;
    test_async_func_with_test_lib_x(SCRIPT, goja::Value::Undefined);
}

#[test]
fn test_await_in_parameters() {
    let res = goja::compile(
        "",
        b"
	async function g() {
	    async function inner(a = 1 + await 1) {
	    }
	}
	",
        false,
    );
    assert!(res.is_err(), "Expected error");
}
