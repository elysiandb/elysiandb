mod jsrun;

use goja::Value;
use goja::{compile, must_compile};
use jsrun::*;
use std::collections::HashMap;

fn expect_error(src: &str, strict: bool) {
    if compile("", src.as_bytes(), strict).is_ok() {
        panic!("expected error");
    }
}

fn expect_ok(src: &str, strict: bool) {
    if let Err(e) = compile("", src.as_bytes(), strict) {
        panic!("{e}");
    }
}

#[test]
fn test_try_illegal_continue_with_finally_override() {
    expect_error(
        "
	L: {
		while (Math.random() > 0.5) {
			try {
				continue L;
			} finally {
				break;
			}
		}
	}
	",
        false,
    );
}

#[test]
fn test_try_illegal_continue_with_finally_override_no_label() {
    expect_error(
        "
	L: {
		try {
			continue;
		} finally {
			break L;
		}
	}
	",
        false,
    );
}

#[test]
fn test_try_illegal_continue_with_finally_override_dummy() {
    expect_error(
        "
	L: {
		while (false) {
			try {
				continue L;
			} finally {
				break;
			}
		}
	}
	",
        false,
    );
}

#[test]
fn test_for_in_let_with_initializer() {
    expect_error("for (let x = 3 in {}) { }", false);
}

#[test]
fn test_for_of_let_let() {
    expect_error(
        "
	for (let let of [23]) {
	}
",
        false,
    );
}

#[test]
fn test_let_postfix_asi() {
    expect_error(
        "
	let
	++
	",
        false,
    );
}

#[test]
fn test_falsy_loop_break() {
    must_compile(
        "",
        b"
	while(false) {
	  	break;
	}
	for(;false;) {
		break;
	}
	undefined;
	",
        false,
    );
}

#[test]
fn test_dummy_compile() {
    expect_error(
        "
	'use strict';

	for (;false;) {
		eval = 1;
	}
	",
        false,
    );
}

#[test]
fn test_dummy_compile_for_update() {
    expect_error(
        "
	'use strict';

	for (;false;eval=1) {
	}
	",
        false,
    );
}

#[test]
fn test_escaped_object_property_keys() {
    expect_ok(
        "
	var obj = {
		w\\u0069th: 42
	};
	var obj = {
		with() {42}
	};
	",
        false,
    );
}

#[test]
fn test_escaped_keywords() {
    expect_error("r\\u0065turn;", false);
}

#[test]
fn test_escaped_let() {
    expect_ok(
        "
this.let = 0;

l\\u0065t // ASI
a;

// If the parser treated the previous escaped \"let\" as a lexical declaration,
// this variable declaration will result an early syntax error.
var a;
",
        false,
    );
}

#[test]
fn test_strict_let() {
    expect_error(
        "
	var let = 1;
	",
        true,
    );
}

#[test]
fn test_let_let() {
    expect_error(
        "
	let let = 1;
	",
        false,
    );
}

#[test]
fn test_let_asi() {
    expect_ok(
        "
	while (false) let // ASI
	x = 1;
	",
        false,
    );
}

#[test]
fn test_let_asi1() {
    expect_ok(
        "
	let
	x = 1;
	",
        true,
    );
}

#[test]
fn test_let_no_asi() {
    expect_ok(
        "
	function f() {}let
x = 1;
	",
        true,
    );
}

#[test]
fn test_let_no_asi1() {
    expect_error(
        "
let
let = 1;
	",
        false,
    );
}

#[test]
fn test_let_array_with_newline() {
    expect_error(
        "
    with ({}) let
    [a] = 0;
	",
        false,
    );
}

#[test]
fn test_var_block_conflict() {
    expect_error(
        "
	let x;
	{
		if (false) {
			var x;
		}
	}
	",
        false,
    );
}

#[test]
fn test_lexical_strict_names() {
    expect_error("let eval = 1;", true);
}

#[test]
fn test_obj_literal_shorthand_with_initializer() {
    expect_error(
        "
	o = {a=1};
	",
        false,
    );
}

#[test]
fn test_obj_literal_shorthand_let_string_lit() {
    expect_error(
        "
	o = {\"let\"};
	",
        false,
    );
}

#[test]
fn test_binding_pattern_rest_trailing_comma() {
    expect_error(
        "
	const [a, b, ...rest,] = [];
	",
        false,
    );
}

#[test]
fn test_assign_pattern_rest_trailing_comma() {
    expect_error(
        "
	([a, b, ...rest,] = []);
	",
        false,
    );
}

#[test]
fn test_arrow_use_strict() {
    expect_ok("(a) => {'use strict';}", false);
    expect_error("(a=0) => {'use strict';}", false);
}

#[test]
fn test_src_location() {
    let prg = must_compile(
        "test.js",
        b"
f();
var x = 1;
let y = 1;
let [z1, z2] = [0, 0];

var [z3, z4] = [0, 0];
	",
        false,
    );
    let mut lines_of_interest: HashMap<i64, &str> = HashMap::from([
        (3, "var"),
        (4, "let"),
        (5, "destruct let"),
        (7, "destruct var"),
    ]);
    let src = prg.src().expect("source file");
    for i in 0..prg.code().len() {
        let loc = src.position(prg.source_offset(i as isize) as i64);
        lines_of_interest.remove(&loc.line);
        if lines_of_interest.is_empty() {
            break;
        }
    }
    if let Some(v) = lines_of_interest.values().next() {
        panic!("no {v} line");
    }
}

#[test]
fn test_bad_object_key() {
    expect_error("({!:0})", false);
}

#[test]
fn test_async_func() {
    const SCRIPT: &str = r#"
	async (x = true, y) => {};
	async x => {};
	let passed = false;
	async function f() {
		return true;
	}
	async function f1(arg = true) {
		passed = await f();
	}
	await f1();
	return passed;
	"#;
    jsrun::test_async_func(SCRIPT, Value::Bool(true));
}

#[test]
fn test_object_literal_computed_method_keys() {
    expect_ok(
        "
		({
		    [\"__proto__\"]() {},
		    [\"__proto__\"]() {}
		})
	",
        false,
    );
    expect_ok(
        "
		({
		    get [\"__proto__\"]() {},
		    get [\"__proto__\"]() {}
		})
	",
        false,
    );
}

#[test]
fn test_function_body_class_decl() {
    expect_ok(
        "
	function as(requiredArgument = {}) {
		class something { }
	};
	",
        false,
    );
}

#[test]
fn test_empty_program() {
    test_script(
        r#"
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_result_empty_block() {
    test_script(
        r#"
	undefined;
	{}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_result_var_decl() {
    test_script(
        r#"
	7; var x = 1;
	"#,
        Value::Int(7),
    );
}

#[test]
fn test_result_lex_decl() {
    test_script(
        r#"
	7; {let x = 1};
	"#,
        Value::Int(7),
    );
}

#[test]
fn test_result_lex_decl_break() {
    test_script(
        r#"
	L:{ 7; {let x = 1; break L;}};
	"#,
        Value::Int(7),
    );
}

#[test]
fn test_result_lex_decl_nested() {
    test_script(
        r#"
	7; {let x = (function() { return eval("8; {let y = 9}")})()};
	"#,
        Value::Int(7),
    );
}

#[test]
fn test_error_proto() {
    test_script(
        r#"
	var e = new TypeError();
	e.name;
	"#,
        str_value("TypeError"),
    );
}

#[test]
fn test_this1() {
    test_script(
        r#"
	function independent() {
		return this.prop;
	}
	var o = {};
	o.b = {g: independent, prop: 42};

	o.b.g();
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_this2() {
    test_script(
        r#"
var o = {
  prop: 37,
  f: function() {
    return this.prop;
  }
};

o.f();
"#,
        Value::Int(37),
    );
}

#[test]
fn test_this_strict() {
    test_script(
        r#"
	"use strict";

	Object.defineProperty(Object.prototype, "x", { get: function () { return this; } });

	(5).x === 5;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Number builtin"]
fn test_this_no_strict() {
    test_script(
        r#"
	Object.defineProperty(Object.prototype, "x", { get: function () { return this; } });

	(5).x == 5;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_nested_func_var_resolution() {
    test_script(
        r#"
	(function outer() {
		var v = 42;
		function inner() {
			return v;
		}
		return inner();
	})();
"#,
        Value::Int(42),
    );
}

#[test]
fn test_nested_func_var_resolution1() {
    test_script(
        r#"
	function outer(argOuter) {
		var called = 0;
	  var inner = function(argInner) {
		if (arguments.length !== 1) {
			throw new Error();
		}
		called++;
		if (argOuter !== 1) {
			throw new Error("argOuter");
		}
		if (argInner !== 2) {
			throw new Error("argInner");
		}
	  };
		inner(2);
	}
	outer(1);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_call_fewer_args_closure_no_args() {
    test_script(
        r#"
	var x;
	function A(a, b, c) {
		var y = a;
		x = function() { return " " + y };
		return String(a) + " " + String(b) + " " + String(c);
	}

	A(1, 2) + x();
"#,
        str_value("1 2 undefined 1"),
    );
}

#[test]
fn test_call_fewer_args_closure_args() {
    test_script(
        r#"
	var x;
	function A(a, b, c) {
		var y = b;
		x = function() { return " " + a + " " + y };
		return String(a) + " " + String(b) + " " + String(c);
	}

	A(1, 2) + x();
"#,
        str_value("1 2 undefined 1 2"),
    );
}

#[test]
fn test_call_less_args_dynamic_local_var() {
    test_script(
        r#"
	function f(param) {
		var a = 42;
		if (false) {
			eval("");
		}
		return a;
	}
	f();
"#,
        Value::Int(42),
    );
}

#[test]
fn test_native_call() {
    test_script(
        r#"
	var o = Object(1);
	Object.defineProperty(o, "test", {value: 42});
	o.test;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_js_call() {
    test_script(
        r#"
	function getter() {
		return this.x;
	}
	var o = Object(1);
	o.x = 42;
	Object.defineProperty(o, "test", {get: getter});
	o.test;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_loop1() {
    test_script(
        r#"
	function A() {
    		var x = 1;
    		for (var i = 0; i < 1; i++) {
        		var x = 2;
    		}
    		return x;
	}

	A();
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_loop_break() {
    test_script(
        r#"
	function A() {
    		var x = 1;
    		for (var i = 0; i < 1; i++) {
        		break;
        		var x = 2;
    		}
    		return x;
	}

	A();
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_for_loop_optional_expr() {
    test_script(
        r#"
	function A() {
    		var x = 1;
    		for (;;) {
        		break;
        		var x = 2;
    		}
    		return x;
	}

	A();
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_block_break() {
    test_script(
        r#"
	var rv = 0;
	B1: {
		rv = 1;
		B2: {
			rv = 2;
			break B1;
		}
		rv = 3;
	}
	rv;
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_try() {
    test_script(
        r#"
	function A() {
		var x = 1;
		try {
			x = 2;
		} catch(e) {
			x = 3;
		} finally {
			x = 4;
		}
		return x;
	}

	A();
	"#,
        Value::Int(4),
    );
}

#[test]
fn test_try_optional_catch_binding() {
    test_script(
        r#"
	try {
		throw null;
	} catch {
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_catch() {
    test_script(
        r#"
	function A() {
		var x;
		try {
			throw 4;
		} catch(e) {
			x = e;
		}
		return x;
	}

	A();
	"#,
        Value::Int(4),
    );
}

#[test]
fn test_try_catch_direct_eval() {
    test_script(
        r#"
	function A() {
		var x;
		try {
			throw 4;
		} catch(e) {
			eval("x = e");
		}
		return x;
	}

	A();
	"#,
        Value::Int(4),
    );
}

#[test]
fn test_try_exception_in_catch() {
    test_script(
        r#"
	function A() {
		var x;
		try {
			throw 4;
		} catch(e) {
			throw 5;
		}
		return x;
	}

	var rv;
	try {
		A();
	} catch (e) {
		rv = e;
	}
	rv;
	"#,
        Value::Int(5),
    );
}

#[test]
fn test_try_continue_in_catch() {
    test_script(
        r#"
	var c3 = 0, fin3 = 0;
	while (c3 < 2) {
		try {
			throw "ex1";
		} catch(er1) {
			c3 += 1;
			continue;
		} finally {
			fin3 = 1;
		}
		fin3 = 0;
	}

	fin3;
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_continue_in_with() {
    test_script(
        r#"
	var x;
	var o = {x: 0};
	for (var i = 0; i < 2; i++) {
		with(o) {
			x = i;
			if (i === 0) {
				continue;
			}
		}
		break;
	}
	x;
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_continue_in_finally() {
    test_script(
        r#"
	var c3 = 0, fin3 = 0;
	while (c3 < 2) {
  		try {
    			throw "ex1";
  		} catch(er1) {
    			c3 += 1;
  		} finally {
    			fin3 = 1;
    			continue;
  		}
  		fin3 = 0;
	}

	fin3;
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_try_break_finally_continue() {
    test_script(
        r#"
	for (var i = 0; i < 3; i++) {
	  try {
		break;
	  } finally {
		continue;
	  }
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_break_finally_continue_with_result() {
    test_script(
        r#"
	for (var i = 0; i < 3; i++) {
	  try {
		true;
		break;
	  } finally {
		continue;
	  }
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_break_finally_continue_with_result1() {
    test_script(
        r#"
	for (var i = 0; i < 3; i++) {
	  try {
		true;
		break;
	  } finally {
		var x = 1;
		continue;
	  }
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_break_finally_continue_with_result_nested() {
    test_script(
        r#"
LOOP:
	for (var i = 0; i < 3; i++) {
	  try {
		if (true) {
			false; break;
		}
	  } finally {
		if (true) {
			true; continue;
		}
	  }
	}
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_try_break_outer_finally_continue() {
    test_script(
        r#"
	let iCount = 0, jCount = 0;
	OUTER: for (let i = 0; i < 1; i++) {
		iCount++;
		for (let j = 0; j < 2; j++) {
			jCount++;
			try {
				break OUTER;
			} finally {
				continue;
			}
		}
	}
	""+iCount+jCount;
	"#,
        str_value("12"),
    );
}

#[test]
fn test_try_no_result() {
    test_script(
        r#"
	true;
    L:
    try {
        break L;
    } finally {
    }
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_catch_lexical_env() {
    test_script(
        r#"
	function F() {
		try {
			throw 1;
		} catch (e) {
			var x = e;
		}
		return x;
	}

	F();
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_throw_type() {
    test_script(
        r#"
	function Exception(message) {
		this.message = message;
	}


	function A() {
		try {
			throw new Exception("boo!");
		} catch(e) {
			return e;
		}
	}
	var thrown = A();
	thrown !== null && typeof thrown === "object" && thrown.constructor === Exception;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_throw_constructor_name() {
    test_script(
        r#"
	function Exception(message) {
		this.message = message;
	}


	function A() {
		try {
			throw new Exception("boo!");
		} catch(e) {
			return e;
		}
	}
	A().constructor.name;
	"#,
        str_value("Exception"),
    );
}

#[test]
fn test_throw_native_constructor_name() {
    test_script(
        r#"


	function A() {
		try {
			throw new TypeError();
		} catch(e) {
			return e;
		}
	}
	A().constructor.name;
	"#,
        str_value("TypeError"),
    );
}

#[test]
fn test_empty_try_no_catch() {
    test_script(
        r#"
	var called = false;
	try {
	} finally {
		called = true;
	}
	called;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_try_return_from_catch() {
    test_script(
        r#"
	function f(o) {
		var x = 42;

		function innerf(o) {
			try {
				throw o;
			} catch (e) {
				return x;
			}
		}

		return innerf(o);
	}
	f({});
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_try_completion_result() {
    test_script(
        r#"
	99; do { -99; try { 39 } catch (e) { -1 } finally { break; -2 }; } while (false);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_if_else() {
    test_script(
        r#"
	var rv;
	if (rv === undefined) {
		rv = "passed";
	} else {
		rv = "failed";
	}
	rv;
	"#,
        str_value("passed"),
    );
}

#[test]
fn test_if_else_ret_val() {
    test_script(
        r#"
	var x;
	if (x === undefined) {
		"passed";
	} else {
		"failed";
	}
	"#,
        str_value("passed"),
    );
}

#[test]
fn test_while_return_value() {
    test_script(
        r#"
	var x = 0;
	while(true) {
		x = 1;
		break;
	}
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_if_else_label() {
    test_script(
        r#"
	var x = 0;
	abc: if (true) {
		x = 1;
		break abc;
	}
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_if_multiple_labels() {
    test_script(
        r#"
	var x = 0;
	xyz:abc: if (true) {
		break xyz;
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_break_out_of_try() {
    test_script(
        r#"
	function A() {
		var x = 1;
		B: {
			try {
				x = 2;
			} catch(e) {
				x = 3;
			} finally {
				break B;
				x = 4;
			}
		}
		return x;
	}

	A();
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_return_out_of_try_nested() {
    test_script(
        r#"
	function A() {
		function nested() {
			try {
				return 1;
			} catch(e) {
				return 2;
			}
		}
		return nested();
	}

	A();
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_return_out_of_try_with_finally() {
    test_script(
        r#"
	function test() {
		try {
			return 'Hello, world!';
		} finally {
			const dummy = 'unexpected';
		}
	}
	test();
	"#,
        str_value("Hello, world!"),
    );
}

#[test]
fn test_continue_loop() {
    test_script(
        r#"
	function A() {
		var r = 0;
		for (var i = 0; i < 5; i++) {
			if (i > 1) {
				continue;
			}
			r++;
		}
		return r;
	}

	A();
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_continue_out_of_try() {
    test_script(
        r#"
	function A() {
		var r = 0;
		for (var i = 0; i < 5; i++) {
			try {
				if (i > 1) {
					continue;
				}
			} catch(e) {
				return 99;
			}
			r++;
		}
		return r;
	}

	A();
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_this_in_catch() {
    test_script(
        r#"
	function O() {
		try {
			f();
		} catch (e) {
			this.value = e.toString();
		}
	}

	function f() {
		throw "ex";
	}

	var o = new O();
	o.value;
	"#,
        str_value("ex"),
    );
}

#[test]
fn test_nested_try() {
    test_script(
        r#"
	var ex;
	try {
  		throw "ex1";
	} catch (er1) {
  		try {
    			throw "ex2";
  		} catch (er1) {
			ex = er1;
		}
	}
	ex;
	"#,
        str_value("ex2"),
    );
}

#[test]
fn test_nested_try_in_stashless_func() {
    test_script(
        r#"
	function f() {
		var ex1, ex2;
		try {
			throw "ex1";
		} catch (er1) {
			try {
				throw "ex2";
			} catch (er1) {
				ex2 = er1;
			}
			ex1 = er1;
		}
		return ex1 == "ex1" && ex2 == "ex2";
	}
	f();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_lexical_decl() {
    test_script(
        r#"
	eval("let x = true; x;");
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_in_catch_in_stashless_func() {
    test_script(
        r#"
	function f() {
		var ex;
		try {
			throw "ex1";
		} catch (er1) {
			eval("ex = er1");
		}
		return ex;
	}
	f();
	"#,
        str_value("ex1"),
    );
}

#[test]
fn test_catch_closure_in_stashless_func() {
    test_script(
        r#"
	function f() {
		var ex;
		try {
			throw "ex1";
		} catch (er1) {
			return function() {
				return er1;
			}
		}
	}
	f()();
	"#,
        str_value("ex1"),
    );
}

#[test]
fn test_catch_var_not_used_in_stashless_func() {
    test_script(
        r#"
	function f() {
		var ex;
		try {
			throw "ex1";
		} catch (er1) {
			ex = "ok";
		}
		return ex;
	}
	f();
	"#,
        str_value("ok"),
    );
}

#[test]
fn test_new() {
    test_script(
        r#"
	function O() {
		this.x = 42;
	}

	new O().x;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_string_constructor() {
    test_script(
        r#"
	function F() {
		return String(33) + " " + String("cows");
	}

	F();
	"#,
        str_value("33 cows"),
    );
}

#[test]
fn test_error() {
    test_script(
        r#"
	function F() {
		return new Error("test");
	}

	var e = F();
	e.message == "test" && e.name == "Error";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_type_error() {
    test_script(
        r#"
	function F() {
		return new TypeError("test");
	}

	var e = F();
	e.message == "test" && e.name == "TypeError";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_to_string() {
    test_script(
        r#"
	var o = {x: 42};
	o.toString = function() {
		return String(this.x);
	}

	var o1 = {};
	o.toString() + " ### " + o1.toString();
	"#,
        str_value("42 ### [object Object]"),
    );
}

#[test]
fn test_eval_order() {
    test_script(
        r#"
	var o = {f: function() {return 42}, x: 0};
	var trace = "";

	function F1() {
	    trace += "First!";
	    return o;
	}

	function F2() {
	    trace += "Second!";
	    return "f";
	}

	function F3() {
	    trace += "Third!";
	}

	var rv = F1()[F2()](F3());
	rv += trace;
	rv;
	"#,
        str_value("42First!Second!Third!"),
    );
}

#[test]
fn test_postfix_inc_bracket() {
    test_script(
        r#"
	var o = {x: 42};
	var trace = "";

	function F1() {
	    trace += "First!";
	    return o;
	}

	function F2() {
	    trace += "Second!";
	    return "x";
	}


	var rv = F1()[F2()]++;
	rv + trace + o.x;
	"#,
        str_value("42First!Second!43"),
    );
}

#[test]
fn test_postfix_inc_dot() {
    test_script(
        r#"
	var o = {x: 42};
	var trace = "";

	function F1() {
	    trace += "First!";
	    return o;
	}

	var rv = F1().x++;
	rv + trace + o.x;
	"#,
        str_value("42First!43"),
    );
}

#[test]
fn test_prefix_inc_bracket() {
    test_script(
        r#"
	var o = {x: 42};
	var trace = "";

	function F1() {
	    trace += "First!";
	    return o;
	}

	function F2() {
	    trace += "Second!";
	    return "x";
	}


	var rv = ++F1()[F2()];
	rv + trace + o.x;
	"#,
        str_value("43First!Second!43"),
    );
}

#[test]
fn test_prefix_inc_dot() {
    test_script(
        r#"
	var o = {x: 42};
	var trace = "";

	function F1() {
	    trace += "First!";
	    return o;
	}

	var rv = ++F1().x;
	rv + trace + o.x;
	"#,
        str_value("43First!43"),
    );
}

#[test]
fn test_post_dec_obj() {
    test_script(
        r#"
	var object = {valueOf: function() {return 1}};
	var y = object--;
	var ok = false;
	if (y === 1) {
		ok = true;
	}
	ok;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Number builtin"]
fn test_prop_acc1() {
    test_script(
        r#"
	1..toString()
	"#,
        str_value("1"),
    );
}

#[test]
fn test_eval_direct() {
    test_script(
        r#"
	var rv = false;
    function foo(){ rv = true; }

    var o = { };
    function f() {
        try {
	        eval("o.bar( foo() );");
		} catch (e) {
		}
    }
    f();
	rv;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_ret() {
    test_script(
        r#"
	eval("for (var i = 0; i < 3; i++) {i}")
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_eval_function_decl() {
    test_script(
        r#"
	eval("function F() {}")
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_function_expr() {
    test_script(
        r#"
	eval("(function F() {return 42;})")()
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_eval_direct_scope() {
    test_script(
        r#"
	var __10_4_2_1_3 = "str";
	function testcase() {
		var __10_4_2_1_3 = "str1";
		try {
			throw "error";
		} catch (e) {
			var __10_4_2_1_3 = "str2";
			return eval("__10_4_2_1_3");
		}
	}
	testcase();
	"#,
        str_value("str2"),
    );
}

#[test]
fn test_eval_direct_scope1() {
    test_script(
        r#"
	'use strict';
	var __10_4_2_1_5 = "str";
	function testcase() {
				var __10_4_2_1_5 = "str1";
				var r = eval("\
							  var __10_4_2_1_5 = \'str2\'; \
							  eval(\"\'str2\' === __10_4_2_1_5\")\
							");
				return r;
		}
	testcase();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_direct_create_binding() {
    test_script(
        r#"
	function f() {
		eval("var x = true");
		return x;
	}
	var res = f();
	var thrown = false;
	try {
		x;
	} catch(e) {
		if (e instanceof ReferenceError) {
			thrown = true;
		} else {
			throw e;
		}
	}
	res && thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_direct_create_binding1() {
    test_script_with_test_lib(
        r#"
	function f() {
		eval("let x = 1; var y = 2; function f1() {return x};");
		assert.throws(ReferenceError, function() { x });
		return ""+y+f1();
	}
	f();
	"#,
        str_value("21"),
    );
}

#[test]
fn test_eval_direct_create_binding3() {
    test_script_with_test_lib(
        r#"
	function f() {
		let x;
		try {
			eval("var y=1, x=2");
		} catch(e) {}
		return y;
	}
	assert.throws(ReferenceError, f);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_global_strict() {
    test_script(
        r#"
	'use strict';
	var evalStr =
	'for (var x in this) {\n'+
	'  if ( x === \'Math\' ) {\n'+
	'  }\n'+
	'}\n';

	eval(evalStr);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_empty_strict() {
    test_script(
        r#"
	'use strict';
	eval("");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_func_decl() {
    test_script(
        r#"
	'use strict';
	var funcA = eval("function __funcA(__arg){return __arg;}; __funcA");
	typeof funcA;
	"#,
        str_value("function"),
    );
}

#[test]
fn test_get_after_set() {
    test_script(
        r#"
	function f() {
		var x = 1;
		return x;
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_for_loop_ret() {
    test_script(
        r#"
	for (var i = 0; i < 20; i++) { if (i > 2) {break;} else { i }}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_for_loop_ret1() {
    test_script(
        r#"
	for (var i = 0; i < 20; i++) { if (i > 2) {42;; {L:{break;}}} else { i }}
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_for_in_loop_ret() {
    test_script(
        r#"
	var o = [1, 2, 3, 4];
	for (var i in o) { if (i > 2) {break;} else { i }}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_for_in_loop_ret1() {
    test_script(
        r#"
	var o = {};
	o.x = 1;
	o.y = 2;
	for (var i in o) {
		true;
	}

	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_do_while_loop_ret() {
    test_script(
        r#"
	var i = 0;
	do {
		if (i > 2) {
			break;
		} else {
			i;
		}
	} while (i++ < 20);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_do_while_continue_ret() {
    test_script(
        r#"
	var i = 0;
	do {
		if (i > 2) {
			true;
			continue;
		} else {
			i;
		}
	} while (i++ < 20);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_while_loop_ret() {
    test_script(
        r#"
	var i; while (i < 20) { if (i > 2) {break;} else { i++ }}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_loop_ret1() {
    test_script(
        r#"
	for (var i = 0; i < 20; i++) { }
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_instanceof() {
    test_script(
        r#"
	var rv;
	try {
		true();
	} catch (e) {
		rv = e instanceof TypeError;
	}
	rv;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_strict_assign() {
    test_script(
        r#"
	'use strict';
	var rv;
	var called = false;
	function F() {
		called = true;
		return 1;
	}
	try {
		x = F();
	} catch (e) {
		rv = e instanceof ReferenceError;
	}
	rv + " " + called;
	"#,
        str_value("true true"),
    );
}

#[test]
fn test_strict_scope() {
    test_script(
        r#"
	var rv;
	var called = false;
	function F() {
		'use strict';
		x = 1;
	}
	try {
		F();
	} catch (e) {
		rv = e instanceof ReferenceError;
	}
	x = 1;
	rv + " " + x;
	"#,
        str_value("true 1"),
    );
}

#[test]
fn test_string_obj() {
    test_script(
        r#"
	var s = new String("test");
	s[0] + s[2] + s[1];
	"#,
        str_value("tse"),
    );
}

#[test]
fn test_string_primitive() {
    test_script(
        r#"
	var s = "test";
	s[0] + s[2] + s[1];
	"#,
        str_value("tse"),
    );
}

#[test]
fn test_call_global_object() {
    test_script(
        r#"
	var rv;
	try {
		this();
	} catch (e) {
		rv = e instanceof TypeError
	}
	rv;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_func_length() {
    test_script(
        r#"
	function F(x, y) {

	}
	F.length
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_native_func_length() {
    test_script(
        r#"
	eval.length + Object.defineProperty.length + String.length
	"#,
        Value::Int(5),
    );
}

#[test]
fn test_arguments() {
    test_script(
        r#"
	function F() {
		return arguments.length + " " + arguments[1];
	}

	F(1,2,3)
	"#,
        str_value("3 2"),
    );
}

#[test]
fn test_arguments_put() {
    test_script(
        r#"
	function F(x, y) {
		arguments[0] -= arguments[1];
		return x;
	}

	F(5, 2)
	"#,
        Value::Int(3),
    );
}

#[test]
fn test_arguments_put_strict() {
    test_script(
        r#"
	function F(x, y) {
		'use strict';
		arguments[0] -= arguments[1];
		return x;
	}

	F(5, 2)
	"#,
        Value::Int(5),
    );
}

#[test]
fn test_arguments_extra() {
    test_script(
        r#"
	function F(x, y) {
		return arguments[2];
	}

	F(1, 2, 42)
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_arguments_exist() {
    test_script(
        r#"
	function F(x, arguments) {
		return arguments;
	}

	F(1, 42)
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_arguments_delete() {
    test_script(
        r#"
	function f(x) {
		delete arguments[0];
		arguments[0] = 42;
		return x;
	}
	f(1)
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_arguments_in_eval() {
    test_script(
        r#"
	function f() {
		return eval("arguments");
	}
	f(1)[0];
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_arguments_redeclare_in_eval() {
    test_script_with_test_lib(
        r#"
	assert.sameValue("arguments" in this, false, "No global 'arguments' binding");

	function f(p = eval("var arguments = 'param'"), arguments) {}
	assert.throws(SyntaxError, f);

	assert.sameValue("arguments" in this, false, "No global 'arguments' binding");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_arguments_redeclare_arrow() {
    test_script_with_test_lib(
        r#"
	const oldArguments = globalThis.arguments;
	let count = 0;
	const f = (p = eval("var arguments = 'param'"), q = () => arguments) => {
	  var arguments = "local";
	  assert.sameValue(arguments, "local", "arguments");
	  assert.sameValue(q(), "param", "q");
	  count++;
	}
	f();
	assert.sameValue(count, 1);
	assert.sameValue(globalThis.arguments, oldArguments, "globalThis.arguments unchanged");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_param_with_def() {
    test_script(
        r#"
	function f(param = 0) {
		eval("var param = 1");
		return param;
	}
	f();
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_arguments_redefined_as_let_dyn() {
    test_script(
        r#"
	function f() {
		let arguments;
		eval(""); // force dynamic scope
		return arguments;
	}
	
	f(1,2);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_with() {
    test_script(
        r#"
	var b = 1;
	var o = {a: 41};
	with(o) {
		a += b;
	}
	o.a;

	"#,
        Value::Int(42),
    );
}

#[test]
fn test_with_in_func() {
    test_script(
        r#"
	function F() {
		var b = 1;
		var c = 0;
		var o = {a: 40, c: 1};
		with(o) {
			a += b + c;
		}
		return o.a;
	}

	F();
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_assign_non_extendable() {
    test_script(
        r#"
	'use strict';

	function F() {
    		this.x = 1;
	}

	var o = new F();
	Object.preventExtensions(o);
	o.x = 42;
	o.x;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_assign_non_extendable1() {
    test_script(
        r#"
	'use strict';

	function F() {
	}

	var o = new F();
	var rv;

	Object.preventExtensions(o);
	try {
		o.x = 42;
	} catch (e) {
		rv = e.constructor === TypeError;
	}

	rv += " " + o.x;
	rv;
	"#,
        str_value("true undefined"),
    );
}

#[test]
fn test_assign_strict() {
    test_script(
        r#"
	'use strict';

	try {
		eval("eval = 42");
	} catch(e) {
		var rv = e instanceof SyntaxError
	}
	rv;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_illegal_argment_name() {
    test_script(
        r#"
	'use strict';

	try {
		eval("function F(eval) {}");
	} catch (e) {
		var rv = e instanceof SyntaxError
	}
	rv;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_function() {
    test_script(
        r#"

	var f0 = Function("");
	var f1 = Function("return ' one'");
	var f2 = Function("arg", "return ' ' + arg");
	f0() + f1() + f2("two");
	"#,
        str_value("undefined one two"),
    );
}

#[test]
fn test_function1() {
    test_script(
        r#"

	var f = function f1(count) {
		if (count == 0) {
			return true;
		}
		return f1(count-1);
	}

	f(1);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_function2() {
    test_script(
        r#"
	var trace = "";
	function f(count) {
    		trace += "f("+count+")";
    		if (count == 0) {
        		return;
    		}
    		return f(count-1);
	}

	function f1() {
    		trace += "f1";
	}

	var f2 = f;
	f = f1;
	f2(1);
	trace;

	"#,
        str_value("f(1)f1"),
    );
}

#[test]
fn test_function_to_string() {
    test_script(
        r#"

	Function("arg1", "arg2", "return 42").toString();
	"#,
        str_value("function anonymous(arg1,arg2\n) {\nreturn 42\n}"),
    );
}

#[test]
fn test_object_literal() {
    test_script(
        r#"
	var getterCalled = false;
	var setterCalled = false;

	var o = {get x() {getterCalled = true}, set x(_) {setterCalled = true}};

	o.x;
	o.x = 42;

	getterCalled && setterCalled;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_const() {
    test_script(
        r#"

	var v1 = true && true;
	var v2 = 1/(-1 * 0);
	var v3 = 1 == 2 || v1;
	var v4 = true && false
	v1 === true && v2 === -Infinity && v3 === v1 && v4 === false;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_const_while() {
    test_script(
        r#"
	var c = 0;
	while (2 + 2 === 4) {
		if (++c > 9) {
			break;
		}
	}
	c === 10;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_const_while_throw() {
    test_script(
        r#"
	var thrown = false;
	try {
		while ('s' in true) {
			break;
		}
	} catch (e) {
		thrown = e instanceof TypeError
	}
	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_dup_params() {
    test_script(
        r#"
	function F(x, y, x) {
		return x;
	}

	F(1, 2);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_use_unsupplied_param() {
    test_script(
        r#"
	function getMessage(message) {
		if (message === undefined) {
			message = '';
		}
		message += " 123 456";
		return message;
	}

	getMessage();
	"#,
        str_value(" 123 456"),
    );
}

#[test]
fn test_for_in_loop() {
    test_script(
        r#"
	function Proto() {}
	Proto.prototype.x = 42;
	var o = new Proto();
	o.y = 44;
	o.x = 45;
	var hasX = false;
	var hasY = false;

	for (var i in o) {
    		switch(i) {
    		case "x":
        		if (hasX) {
            			throw new Error("Already has X");
        		}
        		hasX = true;
        		break;
    		case "y":
        		if (hasY) {
            			throw new Error("Already has Y");
        		}
        		hasY = true;
        		break;
    		}
	}

	hasX && hasY;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_while_loop_result() {
    test_script(
        r#"
	while(false);

	"#,
        Value::Undefined,
    );
}

#[test]
fn test_empty_switch() {
    test_script(
        r#"
	switch(1){}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_empty_do_while() {
    test_script(
        r#"
	do {} while(false)
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_switch() {
    test_script(
        r#"
	function F(x) {
		var i = 0;
		switch (x) {
		case 0:
			i++;
		case 1:
			i++;
		default:
			i++;
		case 2:
			i++;
			break;
		case 3:
			i++;
		}
		return i;
	}

	F(0) + F(1) + F(2) + F(4);

	"#,
        Value::Int(10),
    );
}

#[test]
fn test_switch_def_first() {
    test_script(
        r#"
	function F(x) {
		var i = 0;
		switch (x) {
		default:
			i++;
		case 0:
			i++;
		case 1:
			i++;
		case 2:
			i++;
			break;
		case 3:
			i++;
		}
		return i;
	}

	F(0) + F(1) + F(2) + F(4);

	"#,
        Value::Int(10),
    );
}

#[test]
fn test_switch_result() {
    test_script(
        r#"
	var x = 2;

	switch (x) {
	case 0:
		"zero";
	case 1:
		"one";
	case 2:
		"two";
		break;
	case 3:
		"three";
	default:
		"default";
	}
	"#,
        str_value("two"),
    );
}

#[test]
fn test_switch_result1() {
    test_script(
        r#"
	var x = 0;
	switch (x) { case 0: "two"; case 1: break}
	"#,
        str_value("two"),
    );
}

#[test]
fn test_switch_result2() {
    test_script(
        r#"
	6; switch ("a") { case "a": 7; case "b": }
	"#,
        Value::Int(7),
    );
}

#[test]
fn test_switch_result_jump_into_empty_eval() {
    test_script(
        r#"
	function t(x) {
		return eval("switch(x) { case 1: 2; break; case 2: let x = 1; case 3: x+2; break; case 4: default: 9}");
	}
	""+t(2)+t();
	"#,
        str_value("39"),
    );
}

#[test]
fn test_switch_result_jump_into_empty() {
    test_script(
        r#"
	switch(2) { case 1: 2; break; case 2: let x = 1; case 3: x+2; case 4: {let y = 2}; break; default: 9};
	"#,
        Value::Int(3),
    );
}

#[test]
fn test_switch_lexical() {
    test_script(
        r#"
	switch (true) { case true: let x = 1; }
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_switch_break_outer() {
    test_script(
        r#"
	LOOP:
	for (let i = 0; i < 10; i++) {
		switch (i) {
		case 0:
			continue;
		case 1:
			let x = 1;
			continue;
		case 2:
			try {
				x++;
			} catch (e) {
				if (e instanceof ReferenceError) {
					break LOOP;
				}
			}
			throw new Error("Exception was not thrown");
		}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_if_break_result() {
    test_script(
        r#"
	L: {if (true) {42;} break L;}
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_switch_no_match() {
    test_script(
        r#"
	var result;
	var x;
	switch (x) {
	case 0:
		result = "2";
		break;
	}

	result;

	"#,
        Value::Undefined,
    );
}

#[test]
fn test_switch_no_match_no_default() {
    test_script(
        r#"
		switch (1) {
		case 0:
		}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_switch_no_match_no_default_no_result() {
    test_script(
        r#"
		switch (1) {
		case 0:
		}
		42;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_switch_no_match_no_default_no_result_match() {
    test_script(
        r#"
		switch (1) {
		case 1:
		}
		42;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_empty_switch_no_result() {
    test_script(
        r#"
		switch (1) {}
		42;
	"#,
        Value::Int(42),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_get_own_property_names() {
    test_script(
        r#"
	var o = {
		prop1: 42,
		prop2: "test"
	}

	var hasProp1 = false;
	var hasProp2 = false;

	var names = Object.getOwnPropertyNames(o);
	for (var i in names) {
		var p = names[i];
		switch(p) {
		case "prop1":
			hasProp1 = true;
			break;
		case "prop2":
			hasProp2 = true;
			break;
		}
	}

	hasProp1 && hasProp2;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_array_literal() {
    test_script(
        r#"

	var f1Called = false;
	var f2Called = false;
	var f3Called = false;
	var errorThrown = false;

	function F1() {
		f1Called = true;
	}

	function F2() {
		f2Called = true;
	}

	function F3() {
		f3Called = true;
	}


	try {
		var a = [F1(), x(F3()), F2()];
	} catch(e) {
		if (e instanceof ReferenceError) {
			errorThrown = true;
		} else {
			throw e;
		}
	}

	f1Called && !f2Called && f3Called && errorThrown && a === undefined;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_jump_out_of_return() {
    test_script(
        r#"
	function f() {
		var a;
		if (a == 0) {
			return true;
		}
	}

	f();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_switch_jump_out_of_return() {
    test_script(
        r#"
	function f(x) {
		switch(x) {
		case 0:
			break;
		default:
			return x;
		}
	}

	f(0);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_set_to_read_only_property_strict_bracket() {
    test_script(
        r#"
	'use strict';

	var o = {};
	var thrown = false;
	Object.defineProperty(o, "test", {value: 42, configurable: true});
	try {
		o["test"] = 43;
	} catch (e) {
		thrown = e instanceof TypeError;
	}

	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_set_to_read_only_property_strict_dot() {
    test_script(
        r#"
	'use strict';

	var o = {};
	var thrown = false;
	Object.defineProperty(o, "test", {value: 42, configurable: true});
	try {
		o.test = 43;
	} catch (e) {
		thrown = e instanceof TypeError;
	}

	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_delete_non_configurable_property_strict_bracket() {
    test_script(
        r#"
	'use strict';

	var o = {};
	var thrown = false;
	Object.defineProperty(o, "test", {value: 42});
	try {
		delete o["test"];
	} catch (e) {
		thrown = e instanceof TypeError;
	}

	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_delete_non_configurable_property_strict_dot() {
    test_script(
        r#"
	'use strict';

	var o = {};
	var thrown = false;
	Object.defineProperty(o, "test", {value: 42});
	try {
		delete o.test;
	} catch (e) {
		thrown = e instanceof TypeError;
	}

	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_compound1() {
    test_script(
        r#"
	var x = 0;
  	var scope = {x: 1};
    	var f;
  	with (scope) {
    		f = function() {
        		x *= (delete scope.x, 2);
    		}
  	}
	f();

	scope.x === 2 && x === 0;

	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_compound2() {
    test_script(
        r#"

var x;
x = "x";
x ^= "1";

	"#,
        Value::Int(1),
    );
}

#[test]
fn test_return_undefined() {
    test_script(
        r#"
	function f() {
    		return x;
	}

	var thrown = false;
	try {
		f();
	} catch (e) {
		thrown = e instanceof ReferenceError;
	}

	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_for_break() {
    test_script(
        r#"
	var supreme, count;
	supreme = 5;
	var __evaluated =  eval("for(count=0;;) {if (count===supreme)break;else count++; }");
    	if (__evaluated !== void 0) {
        	throw new Error('#1: __evaluated === 4. Actual:  __evaluated ==='+ __evaluated  );
    	}

	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Number builtin"]
fn test_large_number_literal() {
    test_script(
        r#"
	var x = 0x800000000000000000000;
	x.toString();
	"#,
        str_value("9.671406556917033e+24"),
    );
}

#[test]
fn test_inc_delete() {
    test_script(
        r#"
	var o = {x: 1};
	o.x += (delete o.x, 1);
	o.x;
	"#,
        Value::Int(2),
    );
}

#[test]
fn test_compound_assign_ref_error() {
    test_script(
        r#"
	var thrown = false;
	try {
		a *= 1;
	} catch (e) {
		if (e instanceof ReferenceError) {
			thrown = true;
		} else {
			throw e;
		}
	}
	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_object_literal_proto() {
    test_script(
        r#"
	var o = {
		__proto__: null,
		test: 42
	}

	Object.getPrototypeOf(o);
	"#,
        Value::Null,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_of_array() {
    test_script_with_test_lib(
        r#"
	var array = [0, 'a', true, false, null, /* hole */, undefined, NaN];
	var i = 0;
	
	for (var value of array) {
	  assert.sameValue(value, array[i], 'element at index ' + i);
	  i++;
	}
	
	assert.sameValue(i, 8, 'Visits all elements');
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_for_of_return() {
    test_script_with_test_lib(
        r#"
	var callCount = 0;
	var iterationCount = 0;
	var iterable = {};
	var x = {
	  set attr(_) {
		throw new Test262Error();
	  }
	};
	
	iterable[Symbol.iterator] = function() {
	  return {
		next: function() {
		  return { done: false, value: 0 };
		},
		return: function() {
		  callCount += 1;
		}
	  }
	};
	
	assert.throws(Test262Error, function() {
	  for (x.attr of iterable) {
		iterationCount += 1;
	  }
	});
	
	assert.sameValue(iterationCount, 0, 'The loop body is not evaluated');
	assert.sameValue(callCount, 1, 'Iterator is closed');
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_for_of_return1() {
    test_script_with_test_lib(
        r#"
	var iterable = {};
	var iterationCount = 0;

	iterable[Symbol.iterator] = function() {
	  return {
		next: function() {
		  return { done: false, value: null };
		},
		get return() {
		  throw new Test262Error();
		}
	  };
	};

	assert.throws(Test262Error, function() {
	  for (var x of iterable) {
		iterationCount += 1;
		break;
	  }
	});

	assert.sameValue(iterationCount, 1, 'The loop body is evaluated');
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_of_let() {
    test_script(
        r#"
	var iterCount = 0;
	function f() {}
	for (var let of [23]) {
		f(let);
		if (let != 23) {
			throw new Error("");
		}
		iterCount += 1;
	}

	iterCount;
"#,
        Value::Int(1),
    );
}

#[test]
fn test_for_head_let() {
    test_script(
        r#"
	for (let = 0; let < 2; let++);
"#,
        Value::Undefined,
    );
}

#[test]
fn test_lhs_let() {
    test_script(
        r#"
	let = 1;
	let;
	"#,
        Value::Int(1),
    );
}

#[test]
fn test_iterator_return_normal() {
    test_script_with_test_lib(
        r#"
	var iterable = {};
	var iterationCount = 0;

	iterable[Symbol.iterator] = function() {
	  return {
		next: function() {
		  return { done: ++iterationCount > 2, value: null };
		},
		get return() {
		  throw new Test262Error();
		}
	  };
	};

	for (var x of iterable) {
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_iterator_return_error_nested() {
    test_script(
        r#"
	var returnCalled = {};
	function iter(id) {
		return function() {
			var count = 0;
			return {
				next: function () {
					return {
						value: null,
						done: ++count > 2
					};
				},
				return: function () {
					returnCalled[id] = true;
					throw new Error(id);
				}
			};
		}
	}
	var iterable1 = {};
	iterable1[Symbol.iterator] = iter("1");
	var iterable2 = {};
	iterable2[Symbol.iterator] = iter("2");

	try {
		for (var i of iterable1) {
			for (var j of iterable2) {
				break;
			}
		}
		throw new Error("no exception was thrown");
	} catch (e) {
		if (e.message !== "2") {
			throw e;
		}
	}
	if (!returnCalled["1"]) {
		throw new Error("no return 1");
	}
	if (!returnCalled["2"]) {
		throw new Error("no return 2");
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_return_from_for_in_loop() {
    test_script(
        r#"
	(function f() {
		for (var i in {a: 1}) {
			return true;
		}
	})();
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_return_from_for_of_loop() {
    test_script(
        r#"
	(function f() {
		for (var i of [1]) {
			return true;
		}
	})();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_if_stack_leaks() {
    test_script(
        r#"
	var t = 0;
	if (t === 0) {
		t;
	}
	"#,
        Value::Int(0),
    );
}

#[test]
fn test_with_callee() {
    test_script(
        r#"
	function O() {
		var that = this;
		this.m = function() {
			return this === that;
		}
	}
	with(new O()) {
		m();
	}
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_with_scope() {
    test_script(
        r#"
	function f(o) {
		var x = 42;

		function innerf(o) {
			with (o) {
				return x;
			}
		}

		return innerf(o);
	}
	f({});
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_eval_callee() {
    test_script(
        r#"
	(function () {
		'use strict';
		var v = function() {
			return this === undefined;
		};
		return eval('v()');
	})();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_binding_delete_var() {
    test_script(
        r#"
	(function () {
		eval("var x = 1");
		return x === 1 && delete x;
	})();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_eval_binding_delete_func() {
    test_script(
        r#"
	(function () {
		eval("function x(){}");
		return typeof x === "function" && delete x;
	})();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_delete_global_lexical() {
    test_script(
        r#"
	let x;
	delete x;
	"#,
        Value::Bool(false),
    );
}

#[test]
fn test_delete_global_eval() {
    test_script(
        r#"
	eval("var x");
	delete x;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_try_result_empty() {
    test_script(
        r#"
	1; try { } finally { }
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_result_empty_catch() {
    test_script(
        r#"
	1; try { throw null } catch(e) { }
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_result_empty_continue_loop() {
    test_script(
        r#"
	for (var i = 0; i < 2; i++) { try {throw null;} catch(e) {continue;} 'bad'}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_empty_catch_stack_leak() {
    test_script(
        r#"
	(function() {
		var f;
		// Make sure the outer function is not stashless.
		(function() {
			f++;
		})();
		try {
			throw new Error();
		} catch(e) {}
	})();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_try_throw_empty_catch() {
    test_script(
        r#"
	try {
		throw new Error();
	}
	catch (e) {}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_falsy_loop_break_with_result() {
    test_script(
        r#"
	while(false) {
	  break;
	}
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_object_literal_with_numeric_keys() {
    test_script(
        r#"
	var o = {1e3: true};
	var keys = Object.keys(o);
	var o1 = {get 1e3() {return true;}};
	var keys1 = Object.keys(o1);
	var o2 = {1e21: true};
	var keys2 = Object.keys(o2);
	let o3 = {0(){return true}};
	keys.length === 1 && keys[0] === "1000" && 
	keys1.length === 1 && keys1[0] === "1000" && o1[1e3] === true &&
	keys2.length === 1 && keys2[0] === "1e+21" && o3[0]();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_object_literal_func_props() {
    test_script_with_test_lib(
        r#"
	(function() {
		'use strict';
		var o = {
			eval: function() {return 1;},
			arguments() {return 2;},
			test: function test1() {}
		}
		assert.sameValue(o.eval.name, "eval");
		assert.sameValue(o.arguments.name, "arguments");
		assert.sameValue(o.eval(), 1);
		assert.sameValue(o.arguments(), 2);
		assert.sameValue(o.test.name, "test1");
	})();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_func_name() {
    test_script(
        r#"
	var method = 1;
	var o = {
		method: function() {
			return method;
		},
		method1: function method() {
			return method;
		}
	}
	o.method() === 1 && o.method1() === o.method1;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_func_name_assign() {
    test_script(
        r#"
	var f = function() {};
	var f1;
	f1 = function() {};
	let f2 = function() {};

	f.name === "f" && f1.name === "f1" && f2.name === "f2";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_lexical_decl_global() {
    test_script(
        r#"
	if (true) {
		let it = "be";
		if (it !== "be") {
			throw new Error(it);
		}
	}
	let thrown = false;
	try {
		it;
	} catch(e) {
		if (e instanceof ReferenceError) {
			thrown = true;
		}
	}
	thrown;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_lexical_decl_function() {
    test_script(
        r#"
	function f() {
		if (true) {
			let it = "be";
			if (it !== "be") {
				throw new Error(it);
			}
		}
		let thrown = false;
		try {
			it;
		} catch(e) {
			if (e instanceof ReferenceError) {
				thrown = true;
			}
		}
		return thrown;
	}
	f();
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_lexical_dynamic_scope() {
    test_script(
        r#"
	const global = 1;
	function f() {
		const func = global + 1;
		function inner() {
			function assertThrows(fn) {
				let thrown = false;
				try {
					fn();
				} catch (e) {
					if (e instanceof TypeError) {
						thrown = true;
					} else {
						throw e;
					}
				}
				if (!thrown) {
					throw new Error("Did not throw");
				}
			}

			assertThrows(function() {
				func++;
			});
			assertThrows(function() {
				global++;
			});

			assertThrows(function() {
				eval("func++");
			});
			assertThrows(function() {
				eval("global++");
			});

			return eval("func + 1");
		}
		return inner();
	}
	f();
	"#,
        Value::Int(3),
    );
}

#[test]
fn test_lexical_dynamic_scope1() {
    test_script(
        r#"
	(function() {
		const x = 1 * 4;
		return (function() {
			eval("");
			return x;
		})();
	})();
	"#,
        Value::Int(4),
    );
}

#[test]
fn test_lexical_dynamic_scope2() {
    test_script(
        r#"
	(function() {
		const x = 1 + 3;
		var y = 2 * 2;
		eval("");
		return x;
	})();
	"#,
        Value::Int(4),
    );
}

#[test]
fn test_non_strict_let() {
    test_script(
        r#"
	var let = 1;
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_dynamic_uninited_var_access() {
    test_script(
        r#"
	function f() {
		var x;
		return eval("x");
	}
	f();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_lexical_for_loop_no_closure() {
    test_script(
        r#"
	let sum = 0;
	for (let i = 0; i < 3; i++) {
		sum += i;
	}
	sum;
	"#,
        Value::Int(3),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_lexical_for_loop_closure() {
    test_script(
        r#"
	var f = [];
	for (let i = 0; i < 3; i++) {
		f.push(function() {
			return i;
		});
	}
	f.length === 3 && f[0]() === 0 && f[1]() === 1 && f[2]() === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_lexical_for_loop_closure_in_next() {
    test_script(
        r#"
	const a = [];
	for (let i = 0; i < 5; a.push(function () { return i; }), ++i) { }
	let res = "";
	for (let k = 0; k < 5; ++k) {
		res += ""+a[k]();
	}
	res;
	"#,
        str_value("12345"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_var_for_loop() {
    test_script(
        r#"
	var f = [];
	for (var i = 0, j = 0; i < 3; i++) {
		f.push(function() {
			return i;
		});
	}
	f.length === 3 && f[0]() === 3 && f[1]() === 3 && f[2]() === 3;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_lexical_for_of_loop() {
    test_script(
        r#"
	var f = [];
	for (let i of [0, 1, 2]) {
		f.push(function() {
			return i;
		});
	}
	f.length === 3 && f[0]() === 0 && f[1]() === 1 && f[2]() === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_lexical_for_of_loop_cont_break() {
    test_script(
        r#"
	const f = [];
	for (let i of [0, 1, 2, 3, 4, 5]) {
		if (i % 2) continue;
		f.push(function() {
			return i;
		});
		if (i > 2) break;
	}
	let res = "";
	f.forEach(function(item) {res += item()});
	f.length === 3 && res === "024";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_var_block_conflict_eval() {
    test_script_with_test_lib(
        r#"
	assert.throws(SyntaxError, function() {
		let x;
		{
			if (true) {
				eval("var x");
			}
		}
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_var_block_no_conflict() {
    test_script(
        r#"
	function f() {
		let x;
		function ff() {
			{
				var x = 3;
			}
		}
		ff();
	}
	f();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_var_block_no_conflict_eval() {
    test_script(
        r#"
	function f() {
		let x;
		function ff() {
			{
				eval("var x = 3");
			}
		}
		ff();
	}
	f();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_var_decl_correct_scope() {
    test_script(
        r#"
	function f() {
		{
			let z;
			eval("var x = 3");
		}
		return x;
	}
	f();
	"#,
        Value::Int(3),
    );
}

#[test]
fn test_lexical_catch() {
    test_script(
        r#"
	try {
		throw null;
	} catch (e) {
		let x = 1;
		function f() {}
		e;
	}
	"#,
        Value::Null,
    );
}

#[test]
fn test_arguments_lexical_decl() {
    test_script(
        r#"
	function f1() {
		let arguments;
		return arguments;
	}
	f1(42);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_arguments_lexical_decl_assign() {
    test_script_with_test_lib(
        r#"
	function f1() {
		let arguments = arguments;
		return a;
	}
	assert.throws(ReferenceError, function() {
		f1(42);
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_lexical_const_modify_from_eval() {
    test_script_with_test_lib(
        r#"
	const x = 1;
	function f() {
		eval("x = 2");
	}
	assert.throws(TypeError, function() {
		f();
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_arg_access_from_dynamic_stash() {
    test_script(
        r#"
	function f(arg) {
		function test() {
			eval("");
			return a;
		}
		return arg;
	}
	f(true);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_load_mixed_lex() {
    test_script(
        r#"
	function f() {
		let a = 1;
		{
			function inner() {
				eval("var a = true");
				return a;
			}
			return inner();
		}
	}
	f();
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Reflect builtin"]
fn test_object_literal_spread() {
    test_script_with_test_lib_x(
        r#"
	let src = {prop1: 1};
	Object.defineProperty(src, "prop2", {value: 2, configurable: true});
	Object.defineProperty(src, "prop3", {value: 3, enumerable: true, configurable: true});
	let target = {prop4: 4, ...src};
	assert(deepEqual(target, {prop1: 1, prop3: 3, prop4: 4}));
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_literal_spread() {
    test_script_with_test_lib(
        r#"
	let a1 = [1, 2];
	let a2 = [3, 4];
	let a = [...a1, 0, ...a2, 1];
	assert(compareArray(a, [1, 2, 0, 3, 4, 1]));
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_object_assignment_pattern() {
    test_script_with_test_lib(
        r#"
	let a, b, c;
	({a, b, c=3} = {a: 1, b: 2});
	assert.sameValue(a, 1, "a");
	assert.sameValue(b, 2, "b");
	assert.sameValue(c, 3, "c");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_object_assignment_pattern_no_dyn() {
    test_script_with_test_lib(
        r#"
	(function() {
		let a, b, c;
		({a, b, c=3} = {a: 1, b: 2});
		assert.sameValue(a, 1, "a");
		assert.sameValue(b, 2, "b");
		assert.sameValue(c, 3, "c");
	})();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_object_assignment_pattern_nested() {
    test_script_with_test_lib(
        r#"
	let a, b, c, d;
	({a, b, c: {d} = 3} = {a: 1, b: 2, c: {d: 4}});
	assert.sameValue(a, 1, "a");
	assert.sameValue(b, 2, "b");
	assert.sameValue(c, undefined, "c");
	assert.sameValue(d, 4, "d");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_object_assignment_pattern_eval_order() {
    test_script(
        r#"
	let trace = "";
	let target_obj = {};

	function src() {
	    trace += "src(),";
		return {
			get a() {
				trace += "get a,";
				return "a";
			}
		}
	}
	
	function prop1() {
		trace += "prop1(),"
		return {
			toString: function() {
				trace += "prop1-to-string(),";
				return "a";
			}
		}
	}
	
	function prop2() {
		trace += "prop2(),";
		return {
			toString: function() {
				trace += "prop2-to-string(),";
				return "b";
			}
		}
	}
	
	function target() {
		trace += "target(),"
		return target_obj;
	}
	
	let a, b;
	
	({[prop1()]: target().a, [prop2()]: b} = src());
	if (target_obj.a !== "a") {
		throw new Error("target_obj.a="+target_obj.a);
	}
	trace;
	"#,
        str_value("src(),prop1(),prop1-to-string(),target(),get a,prop2(),prop2-to-string(),"),
    );
}

#[test]
fn test_array_assignment_pattern_eval_order() {
    test_script(
        r#"
	let trace = "";

	let src_arr = {
		[Symbol.iterator]: function() {
			let done = false;
			return {
				next: function() {
					trace += "next,";
					if (!done) {
						done = true;
						return {value: 0};
					}
					return {done: true};
				},
				return: function() {
					trace += "return,";
				}
			}
		}
	}

	function src() {
		trace += "src(),";
		return src_arr;
	}

	let tgt = {
		get a() {
			trace += "get a,";
			return "a";
		},
		get b() {
			trace += "get b,";
			return "b";
		}
	}

	function target() {
		trace += "target(),";
		return tgt;
	}

	function default_a() {
		trace += "default a,";
		return "def_a";
	}

	function default_b() {
		trace += "default b,";
		return "def_b";
	}

	([target().a = default_a(), target().b = default_b()] = src());
	trace;
	"#,
        str_value("src(),target(),next,target(),next,default b,"),
    );
}

#[test]
#[ignore = "requires the Reflect builtin"]
fn test_object_assign_pattern_rest() {
    test_script_with_test_lib_x(
        r#"
	let a, b, c, d;
	({a, b, c, ...d} = {a: 1, b: 2, d: 4});
	assert.sameValue(a, 1, "a");
	assert.sameValue(b, 2, "b");
	assert.sameValue(c, undefined, "c");
	assert(deepEqual(d, {d: 4}), "d");
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Reflect builtin"]
fn test_object_bind_pattern() {
    test_script_with_test_lib_x(
        r#"
	let {a, b, c, ...d} = {a: 1, b: 2, d: 4};
	assert.sameValue(a, 1, "a");
	assert.sameValue(b, 2, "b");
	assert.sameValue(c, undefined, "c");
	assert(deepEqual(d, {d: 4}), "d");

	var { x: y, } = { x: 23 };
	
	assert.sameValue(y, 23);
	
	assert.throws(ReferenceError, function() {
	  x;
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_obj_literal_computed_keys() {
    test_script(
        r#"
	let o = {
		get [Symbol.toString]() {
		}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_obj_literal_computed_keys_eval_order() {
    test_script(
        r#"
	let trace = [];
	function key() {
		trace.push("key");
		return {
			toString: function() {
				trace.push("key-toString");
				return "key";
			}
		}
	}
	function val() {
		trace.push("val");
		return "val";
	}
	
	const _ = {
		[key()]: val(),
	}
	
	trace.join(",");
	"#,
        str_value("key,key-toString,val"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_assign_pattern() {
    test_script(
        r#"
	let a, b;
	([a, b] = [1, 2]);
	a === 1 && b === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_assign_pattern1() {
    test_script(
        r#"
	let a, b;
	([a = 3, b = 2] = [1]);
	a === 1 && b === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_assign_pattern_lhs() {
    test_script(
        r#"
	let a = {};
	[ a.b, a['c'] = 2 ] = [1];
	a.b === 1 && a.c === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_assign_pattern_elision() {
    test_script(
        r#"
	let a, b;
	([a,, b] = [1, 4, 2]);
	a === 1 && b === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_assign_pattern_rest_pattern() {
    test_script(
        r#"
	let a, b, z;
	[ z, ...[a, b] ] = [0, 1, 2];
	z === 0 && a === 1 && b === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_binding_pattern() {
    test_script(
        r#"
	let [a, b] = [1, 2];
	a === 1 && b === 2;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_object_pattern_shorthand_init() {
    test_script(
        r#"
	[...{ x = 1 }] = [];
	x;
	"#,
        Value::Int(1),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_array_binding_pattern_rest_pattern() {
    test_script(
        r#"
	const [a, b, ...[c, d]] = [1, 2, 3, 4];
	a === 1 && b === 2 && c === 3 && d === 4;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_var_pattern() {
    test_script(
        r#"
	var o = {a: 1};
	var trace = "";
	for (var [key, value] of Object.entries(o)) {
		trace += key+":"+value;
	}
	trace;
	"#,
        str_value("a:1"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_lex_pattern() {
    test_script(
        r#"
	var o = {a: 1};
	var trace = "";
	for (const [key, value] of Object.entries(o)) {
		trace += key+":"+value;
	}
	trace;
	"#,
        str_value("a:1"),
    );
}

#[test]
fn test_func_param_initializer_simple() {
    test_script(
        r#"
	function f(a = 1) {
		return a;
	}
	""+f()+f(2);
	"#,
        str_value("12"),
    );
}

#[test]
fn test_func_param_object_pattern_simple() {
    test_script(
        r#"
	function f({a, b} = {a: 1, b: 2}) {
		return "" + a + b;
	}
	""+f()+" "+f({a: 3, b: 4});
	"#,
        str_value("12 34"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_func_param_rest_stack_simple() {
    test_script(
        r#"
	function f(arg1, ...rest) {
		return rest;
	}
	let ar = f(1, 2, 3);
	ar.join(",");
	"#,
        str_value("2,3"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_func_param_rest_stash_simple() {
    test_script(
        r#"
	function f(arg1, ...rest) {
		eval("true");
		return rest;
	}
	let ar = f(1, 2, 3);
	ar.join(",");
	"#,
        str_value("2,3"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_rest_args_not_in_stash() {
    test_script(
        r#"
	function f(...rest) {
		() => rest;
		return rest.length;
	}
	f(1,2);
	"#,
        Value::Int(2),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_rest_args_in_stash() {
    test_script(
        r#"
	function f(first, ...rest) {
		() => first;
		() => rest;
		return rest.length;
	}
	f(1,2);
	"#,
        Value::Int(1),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_rest_args_in_stash_fwd_ref() {
    test_script(
        r#"
	function f(first = eval(), ...rest) {
		() => first;
		() => rest;
		return rest.length === 1 && rest[0] === 2;
	}
	f(1,2);
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_func_param_rest_pattern() {
    test_script(
        r#"
	function f(arg1, ...{0: rest1, 1: rest2}) {
		return ""+arg1+" "+rest1+" "+rest2;
	}
	f(1, 2, 3);
	"#,
        str_value("1 2 3"),
    );
}

#[test]
fn test_func_param_forward_ref() {
    test_script(
        r#"
	function f(a = b + 1, b) {
		return ""+a+" "+b;
	}
	f(1, 2);
	"#,
        str_value("1 2"),
    );
}

#[test]
fn test_func_param_forward_ref_missing() {
    test_script_with_test_lib(
        r#"
	function f(a = b + 1, b) {
		return ""+a+" "+b;
	}
	assert.throws(ReferenceError, function() {
		f();
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_func_param_inner_ref() {
    test_script_with_test_lib(
        r#"
	function f(a = inner) {
		var inner = 42;
		return a;
	}
	assert.throws(ReferenceError, function() {
		f();
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_func_param_inner_ref_eval() {
    test_script_with_test_lib(
        r#"
	function f(a = eval("inner")) {
		var inner = 42;
		return a;
	}
	assert.throws(ReferenceError, function() {
		f();
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_func_param_callee_name() {
    test_script(
        r#"
	function f(a = f) {
		var f;
		return f;
	}
	typeof f();
	"#,
        str_value("undefined"),
    );
}

#[test]
fn test_func_param_var_copy() {
    test_script(
        r#"
	function f(a = f) {
		var a;
		return a;
	}
	typeof f();
	"#,
        str_value("function"),
    );
}

#[test]
fn test_func_param_scope() {
    test_script(
        r#"
	var x = 'outside';
	var probe1, probe2;
	
	function f(
		_ = probe1 = function() { return x; },
		__ = (eval('var x = "inside";'), probe2 = function() { return x; })
	) {
	}
	f();
	probe1()+" "+probe2();
	"#,
        str_value("inside inside"),
    );
}

#[test]
fn test_def_params_stack_ptr() {
    test_script(
        r#"
	function A() {};
	A.B = function () {};
	function D(message = '') {
	  var C = A.B;
	  C([1,2,3]);
	};
	
	D();
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_nested_variadic_calls() {
    test_script(
        r#"
	function f() {
		return Array.prototype.join.call(arguments, ",");
	}
	f(...[1], "a", f(...[2]));
	"#,
        str_value("1,a,2"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_variadic_new() {
    test_script(
        r#"
	function C() {
		this.res = Array.prototype.join.call(arguments, ",");
	}
	var c = new C(...[1], "a", new C(...[2]).res);
	c.res;
	"#,
        str_value("1,a,2"),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_variadic_use_stack_vars() {
    test_script(
        r#"
	function A(message) { return message; }
	function B(...args){
			return A(...args);
	}
	B("C");
	"#,
        str_value("C"),
    );
}

#[test]
fn test_catch_param_pattern() {
    test_script(
        r#"
	function f() {
		let x = 3;
		try {
			throw {a: 1, b: 2};
		} catch ({a, b, c = x}) {
			let x = 99;
			return ""+a+" "+b+" "+c;
		}
	}
	f();
	"#,
        str_value("1 2 3"),
    );
}

#[test]
fn test_arrow_boxed_this() {
    test_script(
        r#"
	var context;
	fn = function() {
		return (arg) => { var local; context = this; };
	};
	
	fn()();
	context === this;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_parameter_override() {
    test_script(
        r#"
	function f(arg) {
		var arg = arg || "default"
		return arg
	}
	f()
	"#,
        str_value("default"),
    );
}

#[test]
fn test_eval_in_iter_scope() {
    test_script(
        r#"
	for (let a = 0; a < 1; a++) {
		eval("a");
	}
	"#,
        Value::Int(0),
    );
}

#[test]
fn test_duplicate_global_func() {
    test_script(
        r#"
	function a(){}
	function b(){ return "b" }
	function c(){ return "c" }
	function a(){}
	b();
	"#,
        str_value("b"),
    );
}

#[test]
fn test_duplicate_func() {
    test_script(
        r#"
	function f() {
		function a(){}
		function b(){ return "b" }
		function c(){ return "c" }
		function a(){}
		return b();
	}
	f();
	"#,
        str_value("b"),
    );
}

#[test]
fn test_assign_before_init() {
    test_script_with_test_lib(
        r#"
	assert.throws(ReferenceError, () => {
		a = 1;
		let a;
	});

	assert.throws(ReferenceError, () => {
	    ({a, b} = {a: 1, b: 2});
	    let a, b;
	});

	assert.throws(ReferenceError, () => {
		(function() {
			eval("");
			({a} = {a: 1});
		})();
		let a;
	});

	assert.throws(ReferenceError, () => {
	    const ctx = {x: 1};
	    function t() {
	        delete ctx.x;
	        return 42;
	    }
	    with(ctx) {
	        (function() {
	            'use strict';
	            ({x} = {x: t()});
	        })();
	    }
	    return ctx.x;
	});

	assert.throws(ReferenceError, () => {
	    const ctx = {x: 1};
	    function t() {
	        delete ctx.x;
	        return 42;
	    }
	    with(ctx) {
	        (function() {
	            'use strict';
				const src = {};
				Object.defineProperty(src, "x", {
					get() {
						return t();
					}
				});
	            ({x} = src);
	        })();
	    }
	    return ctx.x;
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_opt_chain_callee() {
    test_script_with_test_lib(
        r#"
	var a;
	assert.sameValue(a?.(true), undefined);
	a = null;
	assert.sameValue(a?.(), undefined);
	var o = {n: null};
	assert.sameValue(o.m?.(true), undefined);
	assert.sameValue(o.n?.(true), undefined);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_object_literal_super() {
    test_script(
        r#"
	const proto = {
		m() {
			return 40;
		}
	}
	const o = {
		m() {
			return super.m() + 2;
		}
	}
	o.__proto__ = proto;
	o.m();
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_class_capture_this_in_field_init() {
    test_script(
        r#"
	let capture;

	class C {
		a = () => this
	}

	let c = new C();
	c.a() === c;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_use_this_in_field_init() {
    test_script(
        r#"
	let capture;

	class C {
		a = this
	}

	let c = new C();
	c.a === c;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_capture_this_in_static_field_init() {
    test_script(
        r#"
	let capture;

	class C {
		static a = (capture = () => this, 0)
	}

	let c = new C();
	capture() === C;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_dyn_capture_this_in_static_field_init() {
    test_script(
        r#"
	class C {
		static a = eval("this")
	}

	C.a === C;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Math builtin"]
fn test_compile_class() {
    test_script(
        r#"
	class C extends Error {
		a = true;
		b = 1;
		["b".toUpperCase()] = 2
		static A = Math.random() < 1
		constructor(message = "My Error") {
			super(message);
		}
		static M() {
		}
		static M1() {
		}
		m() {
			//return C.a;
		}
		m1() {
			return true;
		}
		static {
			this.supername = super.name;
		}
	}
	let c = new C();
	c.a === true && c.b === 1 && c.B === 2 && c.m1() && C.A && C.supername === "Error";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_super_in_eval() {
    test_script(
        r#"
	class C extends Error {
		constructor() {
			eval("super()");
		}
		m() {
			return eval("super.name");
		}
	}
	let c = new C();
	c.m() === "Error";
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_super_ref_dot() {
    test_script_with_test_lib(
        r#"
	let thisGet, thisSet;
	class P {
		_p = 0
	    get p() {
			thisGet = this;
	        return this._p;
	    }
		set p(v) {
			thisSet = this;
			this._p = v;
		}
	}

	class C extends P {
	    g() {
	        return super.p;
	    }
		s(v) {
			super.p = v;
		}

		inc() {
			super.p++;
		}
		incR() {
			return super.p++;
		}

		inc1() {
			++super.p;
		}

		inc1R() {
			return ++super.p;
		}
		unary() {
			return +super.p;
		}
		pattern() {
			[super.p] = [9];
		}
	}

	let o = new C();
	assert.sameValue(o.g(), 0, "get value");
	assert.sameValue(thisGet, o, "get this");
	o.s(1);
	assert.sameValue(o._p, 1, "set value");
	assert.sameValue(thisSet, o, "set this");

	thisGet = undefined;
	thisSet = undefined;
	o.inc();
	assert.sameValue(o._p, 2, "inc value");
	assert.sameValue(thisGet, o, "inc thisGet");
	assert.sameValue(thisSet, o, "inc thisSet");

	thisGet = undefined;
	thisSet = undefined;
	assert.sameValue(o.incR(), 2, "incR result");
	assert.sameValue(o._p, 3, "incR value");
	assert.sameValue(thisGet, o, "incR thisGet");
	assert.sameValue(thisSet, o, "incR thisSet");

	thisGet = undefined;
	thisSet = undefined;
	o.inc1();
	assert.sameValue(o._p, 4, "inc1 value");
	assert.sameValue(thisGet, o, "inc1 thisGet");
	assert.sameValue(thisSet, o, "inc1 thisSet");

	thisGet = undefined;
	thisSet = undefined;
	assert.sameValue(o.inc1R(), 5, "inc1R result");
	assert.sameValue(o._p, 5, "inc1R value");
	assert.sameValue(thisGet, o, "inc1R thisGet");
	assert.sameValue(thisSet, o, "inc1R thisSet");

	assert.sameValue(o.unary(), 5, "unary");

	o.pattern();
	assert.sameValue(o._p, 9, "pattern");
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_private_ref_dot() {
    test_script_with_test_lib(
        r#"
	class C {
		#p = 0;
	    g() {
	        return this.#p;
	    }
		s(v) {
			this.#p = v;
		}

		inc() {
			this.#p++;
		}
		incR() {
			return this.#p++;
		}

		inc1() {
			++this.#p;
		}

		inc1R() {
			return ++this.#p;
		}
		pattern() {
			[this.#p] = [9];
		}
	}

	let o = new C();
	assert.sameValue(o.g(), 0, "get value");
	o.s(1);
	assert.sameValue(o.g(), 1, "set value");

	o.inc();
	assert.sameValue(o.g(), 2, "inc value");

	assert.sameValue(o.incR(), 2, "incR result");
	assert.sameValue(o.g(), 3, "incR value");

	o.inc1();
	assert.sameValue(o.g(), 4, "inc1 value");

	assert.sameValue(o.inc1R(), 5, "inc1R result");
	assert.sameValue(o.g(), 5, "inc1R value");

	o.pattern();
	assert.sameValue(o.g(), 9, "pattern");
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_private_ref_dot_eval() {
    test_script_with_test_lib(
        r#"
	class C {
		#p = 0;
	    g() {
	        return eval("this.#p");
	    }
		s(v) {
			eval("this.#p = v");
		}

		incR() {
			return eval("this.#p++");
		}

		inc1R() {
			return eval("++this.#p");
		}

		pattern() {
			eval("[this.#p] = [9]");
		}
	}

	let o = new C();
	assert.sameValue(o.g(), 0, "get value");
	o.s(1);
	assert.sameValue(o.g(), 1, "set value");

	assert.sameValue(o.incR(), 1, "incR result");
	assert.sameValue(o.g(), 2, "incR value");

	assert.sameValue(o.inc1R(), 3, "inc1R result");
	assert.sameValue(o.g(), 3, "inc1R value");

	o.pattern();
	assert.sameValue(o.g(), 9, "pattern");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_super_ref_dot_callee() {
    test_script(
        r#"
	class P {
	    get p() {
	        return function() {
	            return this;
	        };
	    }
	}

	class C extends P {
	    m() {
	        return super.p();
	    }
	}

	let o = new C();
	o.m() === o;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_super_ref_bracket() {
    test_script_with_test_lib(
        r#"
	let PROP = "p";
	let thisGet, thisSet;
	class P {
		_p = 0
	    get p() {
			thisGet = this;
	        return this._p;
	    }
		set p(v) {
			thisSet = this;
			this._p = v;
		}
	}

	class C extends P {
	    g() {
	        return super[PROP];
	    }
		s(v) {
			super[PROP] = v;
		}

		inc() {
			super[PROP]++;
		}
		incR() {
			return super[PROP]++;
		}

		inc1() {
			++super[PROP];
		}

		inc1R() {
			return ++super[PROP];
		}
		pattern() {
			[super[PROP]] = [9];
		}
	}

	let o = new C();
	assert.sameValue(o.g(), 0, "get value");
	assert.sameValue(thisGet, o, "get this");
	o.s(1);
	assert.sameValue(o._p, 1, "set value");
	assert.sameValue(thisSet, o, "set this");

	thisGet = undefined;
	thisSet = undefined;
	o.inc();
	assert.sameValue(o._p, 2, "inc value");
	assert.sameValue(thisGet, o, "inc thisGet");
	assert.sameValue(thisSet, o, "inc thisSet");

	thisGet = undefined;
	thisSet = undefined;
	assert.sameValue(o.incR(), 2, "incR result");
	assert.sameValue(o._p, 3, "incR value");
	assert.sameValue(thisGet, o, "incR thisGet");
	assert.sameValue(thisSet, o, "incR thisSet");

	thisGet = undefined;
	thisSet = undefined;
	o.inc1();
	assert.sameValue(o._p, 4, "inc1 value");
	assert.sameValue(thisGet, o, "inc1 thisGet");
	assert.sameValue(thisSet, o, "inc1 thisSet");

	thisGet = undefined;
	thisSet = undefined;
	assert.sameValue(o.inc1R(), 5, "inc1R result");
	assert.sameValue(o._p, 5, "inc1R value");
	assert.sameValue(thisGet, o, "inc1R thisGet");
	assert.sameValue(thisSet, o, "inc1R thisSet");

	o.pattern();
	assert.sameValue(o._p, 9, "pattern");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_super_ref_bracket_eval_order() {
    test_script_with_test_lib(
        r#"
	let keyCallCount = 0;

	function key() {
		keyCallCount++;
		C.prototype.__proto__ = null;
	    return "k";
	}

	class C {
	    constructor() {
	        super[key()]++;
	    }
	}

	assert.throws(TypeError, () => new C());
	assert.sameValue(keyCallCount, 1);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_super_ref_bracket_callee() {
    test_script(
        r#"
	let PROP = "p";
	class P {
	    get p() {
	        return function() {
	            return this;
	        };
	    }
	}

	class C extends P {
	    m() {
	        return super[PROP]();
	    }
	}

	let o = new C();
	o.m() === o;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_super_base_in_ctor() {
    test_script(
        r#"
	let result;
	class Derived extends Object {
	    constructor() {
	        super();
	        result = super.constructor === Object;
	    }
	}
	new Derived();
	result;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_named_eval() {
    test_script(
        r#"
	const C = class {
	}

	C.name === "C";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_non_derived() {
    test_script(
        r#"
	function initF() {
	}
	class C {
	    f = initF()
	}
	let c = new C();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_expr() {
    test_script(
        r#"
	typeof Object.getOwnPropertyDescriptor(class {get f() {}}.prototype, "f").get === "function";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_super_in_heritage() {
    test_script(
        r#"
	class P {
	    a() {
	        return Error;
	    }
	}

	class C extends P {
	    m() {
	        class Inner extends super.a() {
	        }
	        return new Inner();
	    }
	}

	new C().m() instanceof Error;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_super_in_heritage_expr() {
    test_script(
        r#"
	class P {
	    a() {
	        return Error;
	    }
	}

	class C extends P {
	    m() {
			function f(cls) {
				return new cls();
			}
	        return f(class Inner extends super.a() {
	        })
	    }
	}

	new C().m() instanceof Error;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_refer_to_binding() {
    test_script(
        r#"
	const CC = class C {
		static T = 40
	    f = C.T + 2
	}
	let c = new CC();
	c.f;
	"#,
        Value::Int(42),
    );
}

#[test]
fn test_class_refer_to_binding_in_static_decl() {
    test_script(
        r#"
	class C {
		static T = C.name
	}
	C.T;
	"#,
        str_value("C"),
    );
}

#[test]
fn test_class_refer_to_binding_in_static_eval() {
    test_script(
        r#"
	const CC = class C {
		static T = eval("C.name")
	}
	CC.T;
	"#,
        str_value("C"),
    );
}

#[test]
fn test_class_refer_to_binding_from_heritage() {
    test_script_with_test_lib(
        r#"
	assert.throws(ReferenceError, () => {
		class C extends C {
		}
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_capture_super_call_in_arrow_func() {
    test_script(
        r#"
	let f;
	class C extends class {} {
		constructor() {
			f = () => super();
			f();
		}
	}
	let c = new C();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_capture_super_call_in_nested_arrow_func() {
    test_script(
        r#"
	let f;
	class P {
	}
	class C extends P {
		constructor() {
			f = () => () => super();
			f()();
		}
	}
	new C() instanceof P;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_this_in_eval() {
    test_script_with_test_lib(
        r#"
	assert.sameValue(eval("this"), this, "global");

	let o = {
		f() {
			return eval("this");
		}
	}
	assert.sameValue(o.f(), o, "obj literal");
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_static_as_binding_target() {
    test_script(
        r#"
	let [static] = [];
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_eval_in_static_field_init() {
    test_script(
        r#"
	var C = class {
		static f = 'test';
		static g = this.f + '262';
		static h = eval('this.g') + 'test';
	}
	C.f === "test" && C.g === "test262" && C.h === "test262test";
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_class_private_elem_in_eval() {
    test_script_with_test_lib(
        r#"
	let f1, f2;

	class C extends (f1 = o => eval("o.#a"), Object) {
	    static #a = 42;
	    static {
	        f2 = o => eval("o.#a");
			assert.sameValue(C.#a, 42);
			assert.sameValue((() => C.#a)(), 42);
	    }
	}

	assert.throws(SyntaxError, () => f1(C));
	assert.sameValue(f2(C), 42);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_private_elem_in_indirect_eval() {
    test_script_with_test_lib(
        r#"
	let f1, f2;

	class C extends (f1 = o => (0, eval)("o.#a"), Object) {
	    static #a = 42;
	    static {
	        f2 = o => (0, eval)("o.#a");
			assert.throws(SyntaxError, () => (0, eval)("C.#a"));
	    }
	}

	assert.throws(SyntaxError, () => f1(C));
	assert.throws(SyntaxError, () => f2(C));
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_private_elem_in_function() {
    test_script_with_test_lib(
        r#"
	assert.throws(SyntaxError, () => {
		class C {
		    static #a = 42;
		    static {
		        Function("o", "return o.#a");
		    }
		}
	});
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_private_elements_decl() {
    test_script(
        r#"
	class C {
		#a = 42;
		get #b() {}
		set #b(_) {}
		get c() {
			return this.#a;
		}
		#m() {
			return this.#a;
		}
		static getter(inst) {
			return inst.#m();
		}
	}
	let c = new C();
	c.c + C.getter(c);
	"#,
        Value::Int(84),
    );
}

#[test]
fn test_private_in() {
    test_script(
        r#"
	class C {
		#a = 42;
		static check(inst) {
			return #a in inst;
		}
	}
	let c = new C();
	C.check(c);
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_delete_prop_of_non_object() {
    test_script(
        r#"
	delete 'Test262'[100] && delete 'Test262'.a && delete 'Test262'['@'];
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_keywords_as_labels() {
    test_script(
        r#"
	let: for (let i = 0; i < 2; i++) {
		if (i === 0) continue let;
		break let;
	}

	\u006Cet: for (let i = 0; i < 2; i++) {
		if (i === 0) continue \u006Cet;
		break \u006Cet;
	}

	yield: for (let i = 0; i < 2; i++) {
		if (i === 0) continue yield;
		break yield;
	}

	yi\u0065ld: for (let i = 0; i < 2; i++) {
		if (i === 0) continue yi\u0065ld;
		break yi\u0065ld;
	}
"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_this_resolution_with_arg() {
    test_script(
        r#"
	let capture;
	function f(arg) {
		capture = () => this; // move 'this' to stash
		return [this, arg];
	}
	const _this = {};
	const arg = {};
	const [_this1, arg1] = f.call(_this, arg);
	_this1 === _this && arg1 === arg;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_this_resolution_arg_in_stash() {
    test_script(
        r#"
	let capture;
	function f(arg) {
		capture = () => this + arg; // move 'this' and arguments to stash
		return [this, arg];
	}
	const _this = {};
	const arg = {};
	const [_this1, arg1] = f.call(_this, arg);
	_this1 === _this && arg1 === arg;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_this_resolution_with_stack_var() {
    test_script(
        r#"
	let capture;
	function f(arg) {
		const _ = 1; // a stack variable
		capture = () => this + arg; // move 'this' and arguments to stash
		return [this, arg];
	}
	const _this = {};
	const arg = {};
	const [_this1, arg1] = f.call(_this, arg);
	_this1 === _this && arg1 === arg;
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_in_loop_continue() {
    test_script(
        r#"
	var globalSink;
	(function() {
	    const data = [{disabled: true}, {}];
		function dummy() {}
	    function f1() {}

	    function f() {
			dummy(); // move dummy to stash (so that f1 is at index 1)
	        for (const d of data) {
	            if (d.disabled) continue;
	            globalSink = () => d; // move d to stash
	            f1();
	        }
	    }

	    f();
	})();
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_for_in_loop_continue_outer() {
    test_script(
        r#"
	var globalSink;
	(function() {
	    const data = [{disabled: true}, {}];
		function dummy1() {}
	    function f1() {}

	    function f() {
			dummy1();
			let counter = 0;
			OUTER: for (let i = 0; i < 1; i++) {
		        for (const d of data) {
		            if (d.disabled) continue OUTER;
		            globalSink = () => d;
		        }
				counter++;
			}
			f1();
			if (counter !== 0) {
				throw new Error(counter);
			}
	    }

	    f();
	})();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_lexical_decl_in_switch() {
    test_script(
        r#"
	switch(0) {
	    case 1:
	        if (false) b = 3;
	    case 2:
	        const c = 1;
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_field_special() {
    test_script(
        r#"
	class C {
		get;
		set;
		async;
		static;
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_method_special() {
    test_script(
        r#"
	class C {
		get() {}
		set() {}
		async() {}
		static() {}
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_class_method_num_literal() {
    test_script(
        r#"
	class C {
		0() {
			return true;
		}
	}
	new C()[0]();
	"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Math builtin"]
fn test_generator_func() {
    test_script_with_test_lib(
        r#"
	let trace = "";
	function defParam() {
		trace += "1";
		return "def";
	}
	function* g(param = defParam()) {
		const THREE = 3;
		trace += "2";
		assert.sameValue(Math.floor(yield 1), THREE);
		return 42;
	}
	let iter = g();
	assert.sameValue(trace, "1");

	let next = iter.next();
	assert.sameValue(next.value, 1);
	assert.sameValue(next.done, false);

	next = iter.next(Math.PI);
	assert.sameValue(next.value, 42);
	assert.sameValue(next.done, true);

	assert.sameValue(trace, "12");
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_generator_methods() {
    test_script_with_test_lib(
        r#"
	class C {
		*g(param) {
			yield 1;
			yield 2;
		}
	}
	let c = new C();
	let iter = c.g();
	let res = iter.next();
	assert.sameValue(res.value, 1);
	assert.sameValue(res.done, false);

	res = iter.next();
	assert.sameValue(res.value, 2);
	assert.sameValue(res.done, false);

	res = iter.next();
	assert.sameValue(res.value, undefined);
	assert.sameValue(res.done, true);
	"#,
        Value::Undefined,
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_nested_destruct_array() {
    test_script_with_test_lib(
        r#"
	var [
		[ h = 0 ] = [ 0 ]
	] = [];
	assert.sameValue(h, 0);

	var [
		[ h1 = 1 ] = []
	] = [];
	assert.sameValue(h1, 1);

	var [
		[ h2 = 1 ] = []
	] = [ [ 2 ] ];
	assert.sameValue(h2, 2);
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_this_in_stash() {
    test_script(
        r#"
	function f() {
		globalThis.x = () => this; // move 'this' to stash
		
		{
            try {
                throw new Error("boo!");
            } catch (e) {
                if (e.message !== 'boo!') {
					throw new Error("unexpected exception value");
				}
            }
        }
	}

	function f1() {
		globalThis.x = () => this; // move 'this' to stash
		var v; // introduce a stack variable

		{
            try {
                throw new Error("boo!");
            } catch (e) {
                if (e.message !== 'boo!') {
					throw new Error("unexpected exception value");
				}
            }
        }
	}

	f();
	f1();
"#,
        Value::Undefined,
    );
}

#[test]
fn test_this_in_stash_ctor() {
    test_script(
        r#"
	class C extends Object {
		constructor() {
			super();
			globalThis.x = () => this; // move 'this' to stash
			{
				try {
					throw new Error("boo!");
				} catch (e) {
					if (e.message !== 'boo!') {
						throw new Error("unexpected exception value");
					}
				}
			}	
		}
	}

	class C1 extends Object {
		constructor() {
			super();
			globalThis.x = () => this; // move 'this' to stash
			var v; // introduce a stack variable
			{
				try {
					throw new Error("boo!");
				} catch (e) {
					if (e.message !== 'boo!') {
						throw new Error("unexpected exception value");
					}
				}
			}	
		}
	}

	new C();
	new C1();
	undefined;
"#,
        Value::Undefined,
    );
}

#[test]
fn test_wrong_this_rest() {
    test_script(
        r#"
	class mine {
		constructor (arg) {
			this.field = arg
		}
		f(...argument) {
			if (this.field != "something") {
				throw "wrong " + this.field + " Object.keys:" + Object.keys(this)
			}
			let s = () => {
				for (const arg of argument) arg.call()
				return this.field
			}
		}
	}
	const a = new mine("something")
	a.f({"SomeKey": "here"})
"#,
        Value::Undefined,
    );
}

#[test]
fn test_wrong_this_extra_arg() {
    test_script(
        r#"
	class mine {
		constructor (arg) {
			this.field = arg
		}
		f() {
			if (this.field != "something") {
				throw "wrong " + this.field + " Object.keys:" + Object.keys(this)
			}
			globalThis.x = () => this.field;
		}
	}
	const a = new mine("something")
	a.f({"SomeKey": "here"}) // pass extra arg
"#,
        Value::Undefined,
    );
}

#[test]
fn test_wrong_this_forward_ref() {
    test_script(
        r#"
	const expectedThis = {};
	function f(a = b + 1, b) { // forward ref argument to make sure args remain on stack
		if (this != expectedThis) {
			throw new Error("unexpected 'this'");
		}
		eval("true"); // make the scope dynamic which copies args to stash
	}

	f.call(expectedThis, 1, 2);
"#,
        Value::Undefined,
    );
}

#[test]
fn test_call_fewer_args() {
    test_script(
        r#"
function A(a, b, c) {
	return String(a) + " " + String(b) + " " + String(c);
}

A(1, 2);
"#,
        str_value("1 2 undefined"),
    );
}

#[test]
fn test_call_more_args() {
    test_script(
        r#"
function A(a, b) {
	var c = 4;
	return a - b + c;
}

A(1, 2, 3);
"#,
        Value::Int(3),
    );
}

#[test]
fn test_call_more_args_dynamic() {
    test_script(
        r#"
function A(a, b) {
	var c = 4;
	if (false) {
		eval("");
	}
	return a - b + c;
}

A(1, 2, 3);
"#,
        Value::Int(3),
    );
}

#[test]
fn test_call_less_args_dynamic() {
    test_script(
        r#"
function A(a, b, c) {
	// Make it stashful
	function B() {
		return a;
	}
	return String(a) + " " + String(b) + " " + String(c);
}

A(1, 2);
"#,
        str_value("1 2 undefined"),
    );
}

#[test]
fn test_delete_arguments() {
    let res = compile(
        "test.js",
        br#"
	'use strict';

	function f() {
		delete arguments;
	}

	"#,
        false,
    );
    assert!(res.is_err(), "Expected syntax error");
}

#[test]
fn test_empty_code_error() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        match rt.run_string("i") {
            Ok(_) => panic!("Expected an error"),
            Err(e) => {
                let e = e.error();
                assert_eq!(
                    e, "ReferenceError: i is not defined at <eval>:1:1(0)",
                    "Unexpected error: '{e}'"
                );
            }
        }
    });
}

#[test]
fn test_assign_after_stack_expand() {
    test_script(
        r#"
	function f() {
		let sum = 0;
		for (let i = 0; i < arguments.length; i++) {
			sum += arguments[i];
		}
		return sum;
	}
	function testAssignment() {
	  var x = 0;
	  var scope = {};

	  with (scope) {
		x = (scope.x = f(0, 0, 0, 0, 0, 0, 1, 1), 1);
	  }

	  if (scope.x !== 2) {
		throw new Error('#1: scope.x === 2. Actual: ' + (scope.x));
	  }
	  if (x !== 1) {
		throw new Error('#2: x === 1. Actual: ' + (x));
	  }
	}
	testAssignment();
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_template_literals() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        if let Err(e) = rt.run_string("const a = 1, b = 'b';") {
            panic!("{}", run_error_string(&e));
        }
        let mut f = |template: &str, expected: &str| {
            let res = match rt.run_string(template) {
                Ok(v) => v,
                Err(e) => panic!("{}", run_error_string(&e)),
            };
            let actual = res.go_string(&mut rt).unwrap_or_default();
            assert_eq!(
                actual, expected,
                "Expected: {expected:?}, actual: {actual:?}"
            );
        };
        f("``", "");
        f("`test`", "test");
        f("`a=${a},b=${b}`", "a=1,b=b");
        f("`${a},b=${b}$`", "1,b=b$");
        f("`a=${a},b=${b}$`", "a=1,b=b$");
    });
}

#[test]
fn test_tagged_template() {
    test_script(
        r#"
		let res;
		const o = {
			tmpl() {
				res = this;
				return () => {};
			}
		}
		o.tmpl()`test`;
		res === o;
		"#,
        Value::Bool(true),
    );
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_src_locations() {
    test_script_with_test_lib_x(
        r#"
	let i = {
		valueOf() {
			throw new Error();
		}
	};
	try {
		i++;
	} catch(e) {
		assertStack(e, [["test.js", "valueOf", 4, 10],
						["test.js", "", 8, 3]
						]);
	}

	Object.defineProperty(globalThis, "x", {
		get() {
			throw new Error();
		},
		set() {
			throw new Error();
		}
	});

	try {
		x;
	} catch(e) {
		assertStack(e, [["test.js", "get", 17, 10],
						["test.js", "", 25, 3]
						]);
	}

	try {
		x++;
	} catch(e) {
		assertStack(e, [["test.js", "get", 17, 10],
						["test.js", "", 33, 3]
						]);
	}

	try {
		x = 2;
	} catch(e) {
		assertStack(e, [["test.js", "set", 20, 10],
						["test.js", "", 41, 3]
						]);
	}

	try {
		+i;
	} catch(e) {
		assertStack(e, [["test.js", "valueOf", 4, 10],
						["test.js", "", 49, 4]
						]);
	}

	try {
		let n;
		n.field = {
	        "key1": "test",
	        "key2": {},
		}
	} catch(e) {
		assertStack(e, [["test.js", "", 58, 5]
						]);
	}
	"#,
        Value::Undefined,
    );
}

#[test]
fn test_src_location_throw_literal() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let err = rt.run_string(
            r#"
	const z = 1;
	throw "";
	"#,
        );
        match err {
            Err(goja::RunError::Exception(ex)) => {
                let pos = ex.stack[0].position();
                assert_eq!(pos.line, 3, "{pos:?}");
            }
            Err(e) => panic!("{}", run_error_string(&e)),
            Ok(v) => panic!("unexpected result {v:?}"),
        }
    });
}

fn constant_folding_values(prg: &goja::Program) {
    let mut values = std::collections::HashSet::new();
    for ins in prg.code() {
        if let goja::vm::Instruction::LoadVal(v) = ins {
            values.insert(v.prim_string().to_string());
        }
    }
    assert_eq!(values.len(), 1, "values: {values:?}");
}

fn constant_folding_f(src: &'static str, result: Value) {
    on_big_stack(move || {
        let prg = must_compile("test.js", src.as_bytes(), false);
        constant_folding_values(&prg);
        let mut rt = new_runtime();
        test_prg(&mut rt, &prg, &result);
    });
}

fn constant_folding_ff(src: &'static str, result: Value) {
    on_big_stack(move || {
        let prg = must_compile("test.js", src.as_bytes(), false);
        match &prg.code()[0] {
            goja::vm::Instruction::NewFunc(fl) => constant_folding_values(&fl.prg),
            other => panic!("unexpected {}", other.go_type_name()),
        }
        let mut rt = new_runtime();
        test_prg(&mut rt, &prg, &result);
    });
}

#[test]
fn test_constant_folding() {
    constant_folding_f("const x = 1 + 2; x", Value::Int(3));
    constant_folding_f("var x = 1 + 2; x", Value::Int(3));
    constant_folding_f("x = 1 + 2; x", Value::Int(3));
    constant_folding_f("const {x = 1 + 2} = {}; x", Value::Int(3));
    constant_folding_f("var o = {x: 1 + 2}; o.x", Value::Int(3));
    constant_folding_ff("function f(arg = 1 + 2) {return arg}; f()", Value::Int(3));
    constant_folding_ff("function f() {return 1 + 2}; f()", Value::Int(3));
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_constant_folding_array_subtests() {
    constant_folding_f("const [x = 1 + 2] = []; x", Value::Int(3));
    constant_folding_f("var a = [3, 3, 3, 1 + 2]; a[3]", Value::Int(3));
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_string_interning() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let res = rt
            .run_string(
                r#"
	const str1 = "Test";
	function f() {
		return "Test";
	}
	[str1, f()];
	"#,
            )
            .unwrap_or_else(|e| panic!("{}", run_error_string(&e)));
        let Value::Object(o) = res else {
            panic!("not an object");
        };
        let s1 = o.get_str(&mut rt, &"0".into(), None).ok().flatten();
        let s2 = o.get_str(&mut rt, &"1".into(), None).ok().flatten();
        match (s1, s2) {
            (
                Some(Value::String(goja::string::JsString::Ascii(a))),
                Some(Value::String(goja::string::JsString::Ascii(b))),
            ) => assert!(
                std::ptr::eq(a.as_bytes().as_ptr(), b.as_bytes().as_ptr()),
                "not interned"
            ),
            other => panic!("unexpected {other:?}"),
        }
    });
}
