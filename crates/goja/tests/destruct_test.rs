mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn rest_excludes_used_keys() {
    test_script(
        r#"var { a, ...rest } = { a: 1, b: 2, c: 3 }; var s = ""; for (var k in rest) s += k + rest[k]; s + ("a" in rest)"#,
        str_value("b2c3false"),
    );
}

#[test]
fn getters_run_once_in_source_order() {
    test_script(
        r#"var log = ""; var src = { get a() { log += "a"; return 1; }, get b() { log += "b"; return 2; } }; var { a, ...rest } = src; log + rest.b + log"#,
        str_value("ab2ab"),
    );
}

#[test]
fn symbol_key_is_recorded() {
    test_script(
        r#"var sym = Symbol("s"); var { [sym]: v, ...rest } = { [sym]: 1, x: 2 }; v + "," + (sym in rest) + "," + rest.x"#,
        str_value("1,false,2"),
    );
}

#[test]
fn unused_symbol_is_copied() {
    test_script(
        r#"var sym = Symbol("s"); var { x, ...rest } = { [sym]: 1, x: 2 }; rest[sym] + "," + ("x" in rest)"#,
        str_value("1,false"),
    );
}

#[test]
fn string_source_indexes() {
    test_script(
        r#"var { 0: first, ...rest } = "xyz"; var s = first; for (var k in rest) s += k + rest[k]; s"#,
        str_value("x1y2z"),
    );
}

#[test]
fn primitive_source_rest_is_plain_object() {
    test_script(
        r#"var { valueOf, ...rest } = 5; typeof valueOf + "," + (Object.getPrototypeOf(rest) === Object.prototype)"#,
        str_value("function,true"),
    );
}

#[test]
fn inherited_properties_are_not_copied() {
    test_script(
        r#"var p = { inherited: 1 }; var o = Object.create(p); o.own = 2; var { ...rest } = o; ("inherited" in rest) + "," + rest.own"#,
        str_value("false,2"),
    );
}

#[test]
fn non_enumerable_properties_are_not_copied() {
    test_script(
        r#"var o = {}; Object.defineProperty(o, "hidden", { value: 1, enumerable: false }); o.vis = 2; var { ...rest } = o; ("hidden" in rest) + "," + rest.vis"#,
        str_value("false,2"),
    );
}

#[test]
fn rest_without_remaining_keys_is_empty() {
    test_script(
        r#"var { a, b, ...rest } = { a: 1, b: 2 }; var n = 0; for (var k in rest) n++; n"#,
        Value::Int(0),
    );
}

#[test]
fn computed_key_is_recorded() {
    test_script(
        r#"var key = "dyn"; var { [key]: d, ...rest } = { dyn: 1, other: 2 }; d + "," + ("dyn" in rest) + "," + rest.other"#,
        str_value("1,false,2"),
    );
}

#[test]
fn nested_pattern_key_is_recorded() {
    test_script(
        r#"var { a: { x }, ...rest } = { a: { x: 9 }, b: 1 }; x + "," + ("a" in rest) + "," + rest.b"#,
        str_value("9,false,1"),
    );
}

#[test]
fn rest_in_parameters() {
    test_script(
        r#"function f({ a, ...rest }) { var s = ""; for (var k in rest) s += k; return a + s; } f({ a: "A", b: 1, c: 2 })"#,
        str_value("Abc"),
    );
}

#[test]
fn null_source_throws() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let r = rt.run_string(r#"var { a, ...rest } = null"#);
        match r {
            Err(e) => assert_eq!(
                run_error_string(&e),
                "TypeError: Value is not object coercible\n\tat <eval>:1:1(2)\n"
            ),
            Ok(v) => panic!("unexpected result {v:?}"),
        }
    });
}

#[test]
fn rest_runs_enumerable_getters() {
    test_script(
        r#"var log = ""; var o = { a: 1 }; Object.defineProperty(o, "b", { get() { log += "B"; return 2; }, enumerable: true }); var { ...rest } = o; log + rest.b"#,
        str_value("B2"),
    );
}

#[test]
fn default_does_not_record_missing_key() {
    test_script(
        r#"var { x = 5, ...rest } = { y: 1 }; x + "," + rest.y + "," + ("x" in rest)"#,
        str_value("5,1,false"),
    );
}
