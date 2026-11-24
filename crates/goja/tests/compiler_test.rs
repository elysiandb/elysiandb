use goja::{compile, must_compile};
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
