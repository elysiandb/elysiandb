mod jsrun;

use jsrun::*;
use talos::string::{StringBuilder, string_from_utf16};
use talos::string_imported::ImportedString;
use talos::{JsString, Runtime, Value};

#[test]
fn test_string_oob_properties() {
    const SCRIPT: &str = r#"
	var string = new String("str");

	string[4] = 1;
	string[4];
	"#;
    test_script(SCRIPT, int(1));
}

fn to_value(s: &str) -> Value {
    if s.len() <= 16 {
        if talos::unistring::scan(s.as_bytes()).is_some() {
            let i = ImportedString::new(s.as_bytes());
            i.unicode();
            return Value::String(JsString::Imported(i));
        }
        return Value::String(talos::string::ascii_string(s));
    }
    Value::String(JsString::Imported(ImportedString::new(s.as_bytes())))
}

enum Expected {
    Int(i64),
    Bool(bool),
    Str(String),
}

fn check(rt: &mut Runtime, res: &Value, expected: &Expected, what: &str) {
    match (expected, res) {
        (Expected::Int(e), Value::Int(i)) => assert_eq!(e, i, "{what}"),
        (Expected::Bool(e), Value::Bool(b)) => assert_eq!(e, b, "{what}"),
        (Expected::Str(e), Value::String(s)) => {
            let _ = &rt;
            assert_eq!(e, &s.to_rust_string(), "{what}")
        }
        _ => panic!("{what}: unexpected result kind"),
    }
}

fn compile_fn(rt: &mut Runtime, src: &str) -> talos::object::CallFn {
    let v = match rt.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    let Value::Object(o) = v else {
        panic!("not a function")
    };
    o.assert_callable(rt).expect("callable")
}

fn test_unary_op(rt: &mut Runtime, a: &str, expr: &str, result: Expected) {
    let f = compile_fn(rt, &format!("a => {expr}"));
    for aa in [
        Value::String(talos::string::new_string_value(a.as_bytes())),
        to_value(a),
    ] {
        let res = match f.call(rt, Value::Undefined, &[aa]) {
            Ok(v) => v,
            Err(e) => panic!("{expr}: {e:?}"),
        };
        check(rt, &res, &result, &format!("{expr}, a:{a}"));
    }
}

fn test_binary_op(rt: &mut Runtime, a: &str, b: &str, expr: &str, result: Expected) {
    let f = compile_fn(rt, &format!("(a, b) => {expr}"));
    for aa in [
        Value::String(talos::string::new_string_value(a.as_bytes())),
        to_value(a),
    ] {
        for bb in [
            Value::String(talos::string::new_string_value(b.as_bytes())),
            to_value(b),
        ] {
            let res = match f.call(rt, Value::Undefined, &[aa.clone(), bb]) {
                Ok(v) => v,
                Err(e) => panic!("{expr}: {e:?}"),
            };
            check(rt, &res, &result, &format!("{expr}, a:{a}, b:{b}"));
        }
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn pad(s: &str, p: &str, n: usize, start: bool) -> String {
    if n == 0 {
        return s.to_string();
    }
    let p = if p.is_empty() { " " } else { p };
    let mut b = String::new();
    let ss = utf16_len(s);
    let mut n = n as isize - ss as isize;
    if !start {
        b.push_str(s);
    }
    if n > 0 {
        let pp: Vec<u16> = p.encode_utf16().collect();
        while n > 0 {
            if n > pp.len() as isize {
                b.push_str(p);
                n -= pp.len() as isize;
            } else {
                b.push_str(&String::from_utf16_lossy(&pp[..n as usize]));
                n = 0;
            }
        }
    }
    if start {
        b.push_str(s);
    }
    b
}

#[test]
fn test_imported_string() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let strs = [
            "shortAscii",
            "longlongAscii1234567890123456789",
            "short юникод",
            "long юникод 1234567890 юникод \u{1F600}",
            "юникод",
            "Ascii",
            "long",
            "код",
        ];
        let index_of_results: [[i64; 8]; 8] = [
            [0, -1, -1, -1, -1, 5, -1, -1],
            [-1, 0, -1, -1, -1, 8, 0, -1],
            [-1, -1, 0, -1, 6, -1, -1, 9],
            [-1, -1, -1, 0, 5, -1, 0, 8],
            [-1, -1, -1, -1, 0, -1, -1, 3],
            [-1, -1, -1, -1, -1, 0, -1, -1],
            [-1, -1, -1, -1, -1, -1, 0, -1],
            [-1, -1, -1, -1, -1, -1, -1, 0],
        ];
        let last_index_of_results: [[i64; 8]; 8] = [
            [0, -1, -1, -1, -1, 5, -1, -1],
            [-1, 0, -1, -1, -1, 8, 4, -1],
            [-1, -1, 0, -1, 6, -1, -1, 9],
            [-1, -1, -1, 0, 23, -1, 0, 26],
            [-1, -1, -1, -1, 0, -1, -1, 3],
            [-1, -1, -1, -1, -1, 0, -1, -1],
            [-1, -1, -1, -1, -1, -1, 0, -1],
            [-1, -1, -1, -1, -1, -1, -1, 0],
        ];
        for (i, a) in strs.iter().enumerate() {
            test_unary_op(&mut vm, a, "a.length", Expected::Int(utf16_len(a) as i64));
            for (j, b) in strs.iter().enumerate() {
                test_binary_op(&mut vm, a, b, "a === b", Expected::Bool(a == b));
                test_binary_op(&mut vm, a, b, "a == b", Expected::Bool(a == b));
                test_binary_op(&mut vm, a, b, "a + b", Expected::Str(format!("{a}{b}")));
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a > b",
                    Expected::Bool(a.as_bytes() > b.as_bytes()),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "`A${a}B${b}C`",
                    Expected::Str(format!("A{a}B{b}C")),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a.indexOf(b)",
                    Expected::Int(index_of_results[i][j]),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a.lastIndexOf(b)",
                    Expected::Int(last_index_of_results[i][j]),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a.padStart(32, b)",
                    Expected::Str(pad(a, b, 32, true)),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a.padEnd(32, b)",
                    Expected::Str(pad(a, b, 32, false)),
                );
                test_binary_op(
                    &mut vm,
                    a,
                    b,
                    "a.replace(b, '')",
                    Expected::Str(a.replacen(b, "", 1)),
                );
            }
        }
    });
}

#[test]
fn test_string_from_utf16() {
    let s = string_from_utf16(&[]);
    assert!(s.length() == 0 && s.same_as(&Value::String(talos::string::ascii_string(""))));
    let s = string_from_utf16(&[0xD800]);
    assert!(s.length() == 1 && s.char_at(0) == 0xD800);
    let s = string_from_utf16(&[u16::from(b'A'), u16::from(b'B')]);
    assert!(s.same_as(&Value::String(talos::string::ascii_string("AB"))));
}

#[test]
fn test_string_builder() {
    let mut sb = StringBuilder::new();
    sb.write_utf8_string("Head".as_bytes());
    sb.write_utf8_string("1ábc".as_bytes());
    assert_eq!(sb.string().to_rust_string(), "Head1ábc");
}
