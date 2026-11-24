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
