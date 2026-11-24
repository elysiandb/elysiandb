#![allow(dead_code)]

use goja::runtime::RunError;
use goja::{Program, Runtime, Value, must_compile};
use std::sync::{Arc, OnceLock};

pub const TESTLIB: &str = r#"
function $ERROR(message) {
	throw new Error(message);
}

function Test262Error(message) {
  this.message = message || "";
}

Test262Error.prototype.toString = function () {
  return "Test262Error: " + this.message;
};

Test262Error.thrower = (message) => {
  throw new Test262Error(message);
};

function assert(mustBeTrue, message) {
    if (mustBeTrue === true) {
        return;
    }

    if (message === undefined) {
        message = 'Expected true but got ' + String(mustBeTrue);
    }
    $ERROR(message);
}

assert._isSameValue = function (a, b) {
    if (a === b) {
        // Handle +/-0 vs. -/+0
        return a !== 0 || 1 / a === 1 / b;
    }

    // Handle NaN vs. NaN
    return a !== a && b !== b;
};

assert.sameValue = function (actual, expected, message) {
    if (assert._isSameValue(actual, expected)) {
        return;
    }

    if (message === undefined) {
        message = '';
    } else {
        message += ' ';
    }

    message += 'Expected SameValue(«' + String(actual) + '», «' + String(expected) + '») to be true';

    $ERROR(message);
};

assert.throws = function (expectedErrorConstructor, func, message) {
  if (typeof func !== "function") {
    $ERROR('assert.throws requires two arguments: the error constructor ' +
      'and a function to run');
    return;
  }
  if (message === undefined) {
    message = '';
  } else {
    message += ' ';
  }

  try {
    func();
  } catch (thrown) {
    if (typeof thrown !== 'object' || thrown === null) {
      message += 'Thrown value was not an object!';
      $ERROR(message);
    } else if (thrown.constructor !== expectedErrorConstructor) {
      message += 'Expected a ' + expectedErrorConstructor.name + ' but got a ' + thrown.constructor.name;
      $ERROR(message);
    }
    return;
  }

  message += 'Expected a ' + expectedErrorConstructor.name + ' to be thrown but no exception was thrown at all';
  $ERROR(message);
};

function compareArray(a, b) {
  if (b.length !== a.length) {
    return false;
  }

  for (var i = 0; i < a.length; i++) {
    if (b[i] !== a[i]) {
      return false;
    }
  }
  return true;
}
"#;

pub const TESTLIBX: &str = r#"
	function looksNative(fn) {
		return /native code/.test(Function.prototype.toString.call(fn));
	}

	function deepEqual(a, b) {
		if (typeof a === "object") {
			if (typeof b === "object") {
				if (a === b) {
					return true;
				}
				if (Reflect.getPrototypeOf(a) !== Reflect.getPrototypeOf(b)) {
					return false;
				}
				var keysA = Object.keys(a);
				var keysB = Object.keys(b);
				if (keysA.length !== keysB.length) {
					return false;
				}
				if (!compareArray(keysA.sort(), keysB.sort())) {
					return false;
				}
				for (var i = 0; i < keysA.length; i++) {
					var key = keysA[i];
					if (!deepEqual(a[key], b[key])) {
						return false;
					}
				}
				return true;
			} else {
				return false;
			}
		}
		return assert._isSameValue(a, b);
	}

	function assertStack(e, expected) {
		const lines = e.stack.split('\n');
		assert.sameValue(lines.length, expected.length + 2, "Stack lengths mismatch");
		let lnum = 1;
		for (const [file, func, line, col] of expected) {
			const expLine = func === "" ?
				"\tat " + file + ":" + line + ":" + col + "(" :
				"\tat " + func + " (" + file + ":" + line + ":" + col + "(";
			assert.sameValue(lines[lnum].substring(0, expLine.length), expLine, "line " + lnum);
			lnum++;
		}
	}
"#;

pub fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new()
        .name("goja-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

pub fn gc_stress() -> bool {
    std::env::var("GOJA_GC_STRESS").is_ok_and(|v| v == "1")
}

pub fn new_runtime() -> Runtime {
    let mut rt = Runtime::new();
    if gc_stress() {
        rt.set_gc_stress(true);
    }
    rt
}

pub fn test_lib() -> Arc<Program> {
    static P: OnceLock<Arc<Program>> = OnceLock::new();
    P.get_or_init(|| must_compile("testlib.js", TESTLIB.as_bytes(), false))
        .clone()
}

pub fn test_lib_x() -> Arc<Program> {
    static P: OnceLock<Arc<Program>> = OnceLock::new();
    P.get_or_init(|| must_compile("testlibx.js", TESTLIBX.as_bytes(), false))
        .clone()
}

pub fn run_error_string(e: &RunError) -> String {
    match e {
        RunError::Exception(ex) => ex.string(),
        other => other.error(),
    }
}

pub fn test_prg(rt: &mut Runtime, p: &Arc<Program>, expected: &Value) {
    let v = rt.run_program(p);
    let v = match v {
        Ok(v) => v,
        Err(e) => panic!("Exception: {}", run_error_string(&e)),
    };
    if !v.same_as(expected) {
        panic!("Result: {v:?}, expected: {expected:?}");
    }
    assert_eq!(rt.vm.sp, 0, "sp: {}", rt.vm.sp);
    assert!(
        rt.vm.iter_stack.is_empty(),
        "iter stack is not empty: {}",
        rt.vm.iter_stack.len()
    );
}

pub fn rt_test_script(rt: &mut Runtime, script: &str, expected: &Value) {
    let p = must_compile("test.js", script.as_bytes(), false);
    test_prg(rt, &p, expected);
}

pub fn rt_test_script_with_test_lib(rt: &mut Runtime, script: &str, expected: &Value) {
    if let Err(e) = rt.run_program(&test_lib()) {
        panic!("{}", run_error_string(&e));
    }
    rt_test_script(rt, script, expected);
}

pub fn rt_test_script_with_test_lib_x(rt: &mut Runtime, script: &str, expected: &Value) {
    if let Err(e) = rt.run_program(&test_lib()) {
        panic!("{}", run_error_string(&e));
    }
    if let Err(e) = rt.run_program(&test_lib_x()) {
        panic!("{}", run_error_string(&e));
    }
    rt_test_script(rt, script, expected);
}

pub fn test_script(script: &'static str, expected: Value) {
    on_big_stack(move || {
        let mut rt = new_runtime();
        rt_test_script(&mut rt, script, &expected);
    });
}

pub fn test_script_with_test_lib(script: &'static str, expected: Value) {
    on_big_stack(move || {
        let mut rt = new_runtime();
        rt_test_script_with_test_lib(&mut rt, script, &expected);
    });
}

pub fn test_script_with_test_lib_x(script: &'static str, expected: Value) {
    on_big_stack(move || {
        let mut rt = new_runtime();
        rt_test_script_with_test_lib_x(&mut rt, script, &expected);
    });
}

pub fn int(i: i64) -> Value {
    Value::Int(i)
}

pub fn str_value(s: &str) -> Value {
    Value::String(goja::string::new_string_value(s.as_bytes()))
}
