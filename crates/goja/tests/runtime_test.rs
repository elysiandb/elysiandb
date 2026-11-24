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
