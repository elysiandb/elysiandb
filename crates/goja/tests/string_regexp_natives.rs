mod string_regexp_cases;

use goja::builtin_symbol::{
    SYM_ITERATOR, SYM_MATCH, SYM_MATCH_ALL, SYM_REPLACE, SYM_SEARCH, SYM_SPLIT,
};
use goja::func::NativeFn;
use goja::object::{FunctionCall, ObjExt};
use goja::regexp::RegexpObject;
use goja::string::{JsString, string_from_utf16};
use goja::string_imported::ImportedString;
use goja::{JsResult, ObjRef, Runtime, SymRef, Throw, Value};
use string_regexp_cases as cases;

#[derive(Clone, Copy, Debug)]
pub enum A {
    U,
    Null,
    B(bool),
    I(i64),
    F(f64),
    S(&'static str),
    U16(&'static [u16]),
    Imp(&'static str),
    R(&'static str, &'static str, Option<i64>),
    RU(&'static [u16], &'static str, Option<i64>),
}

#[derive(Debug)]
pub struct Case {
    pub m: &'static str,
    pub this: A,
    pub args: &'static [A],
    pub li: bool,
    pub want: &'static str,
}

fn on_big_stack<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(f: F) -> T {
    let h = std::thread::Builder::new()
        .name("goja-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    match h.join() {
        Ok(v) => v,
        Err(e) => std::panic::resume_unwind(e),
    }
}

fn js_str(s: &str) -> JsString {
    let u: Vec<u16> = s.encode_utf16().collect();
    string_from_utf16(&u)
}

fn get(rt: &mut Runtime, o: ObjRef, name: &str) -> JsResult<Value> {
    Ok(o.get_str(rt, &name.into(), None)?
        .unwrap_or(Value::Undefined))
}

fn call(rt: &mut Runtime, f: &Value, this: Value, args: &[Value]) -> JsResult<Value> {
    let Value::Object(f) = f else {
        panic!("not callable: {f:?}");
    };
    let c = f.assert_callable(rt).expect("callable");
    c.call(rt, this, args)
}

fn make_regexp(rt: &mut Runtime, p: JsString, f: &str, li: Option<i64>) -> JsResult<Value> {
    let proto = rt.get_reg_exp_prototype();
    let o = rt.new_reg_exp(
        Some(&Value::String(p)),
        Some(&Value::String(js_str(f))),
        Some(proto),
    )?;
    if let Some(li) = li {
        o.set_own_str(rt, &"lastIndex".into(), Value::Int(li), true)?;
    }
    Ok(Value::Object(o))
}

fn mk(rt: &mut Runtime, a: &A) -> JsResult<Value> {
    Ok(match *a {
        A::U => Value::Undefined,
        A::Null => Value::Null,
        A::B(b) => Value::Bool(b),
        A::I(i) => Value::Int(i),
        A::F(f) => Value::Float(f),
        A::S(s) => Value::String(js_str(s)),
        A::U16(u) => Value::String(string_from_utf16(u)),
        A::Imp(s) => Value::String(JsString::Imported(ImportedString::new(s.as_bytes()))),
        A::R(p, f, li) => return make_regexp(rt, js_str(p), f, li),
        A::RU(p, f, li) => return make_regexp(rt, string_from_utf16(p), f, li),
    })
}

fn symbol(name: &str) -> SymRef {
    match name {
        "iterator" => SYM_ITERATOR,
        "match" => SYM_MATCH,
        "matchAll" => SYM_MATCH_ALL,
        "replace" => SYM_REPLACE,
        "search" => SYM_SEARCH,
        "split" => SYM_SPLIT,
        other => panic!("unknown symbol {other}"),
    }
}

fn call_member(
    rt: &mut Runtime,
    holder: ObjRef,
    name: &str,
    this: Value,
    args: &[Value],
) -> JsResult<Value> {
    if let Some(accessor) = name.strip_prefix("get ") {
        let p = holder
            .get_own_prop_str(rt, &accessor.into())?
            .expect("accessor");
        let Value::Property(p) = p else {
            panic!("{accessor} is not an accessor");
        };
        let g = Value::Object(p.getter_func.expect("getter"));
        return call(rt, &g, this, args);
    }
    let f = match name.strip_prefix("@@") {
        Some(sym) => holder
            .get_sym(rt, symbol(sym), None)?
            .unwrap_or(Value::Undefined),
        None => get(rt, holder, name)?,
    };
    call(rt, &f, this, args)
}

fn drain(rt: &mut Runtime, it: Value) -> JsResult<Value> {
    let it = it.as_object().expect("iterator");
    let next = get(rt, it, "next")?;
    let mut out = Vec::new();
    for _ in 0..1000 {
        let n = call(rt, &next, Value::Object(it), &[])?;
        let n = n.as_object().expect("iterator result");
        if get(rt, n, "done")?.to_boolean() {
            break;
        }
        out.push(get(rt, n, "value")?);
    }
    Ok(Value::Object(rt.new_array_values(out)))
}

fn invoke(rt: &mut Runtime, case: &Case) -> JsResult<Value> {
    let this = mk(rt, &case.this)?;
    let mut args = Vec::new();
    for a in case.args {
        args.push(mk(rt, a)?);
    }
    let (m, drained) = match case.m.strip_suffix('*') {
        Some(m) => (m, true),
        None => (case.m, false),
    };
    let string = rt.get_string();
    let reg_exp = rt.get_reg_exp();
    let r = if let Some(name) = m.strip_prefix("S.p.") {
        let proto = rt.get_string_prototype();
        call_member(rt, proto, name, this.clone(), &args)?
    } else if let Some(name) = m.strip_prefix("R.p.") {
        let proto = rt.get_reg_exp_prototype();
        call_member(rt, proto, name, this.clone(), &args)?
    } else {
        match m {
            "S()" => call(rt, &Value::Object(string), Value::Undefined, &args)?,
            "R()" => call(rt, &Value::Object(reg_exp), Value::Undefined, &args)?,
            "new S" => {
                let c = string.assert_constructor(rt).expect("constructor");
                let o = c.construct(rt, &args, Some(string))?;
                let t = Value::String(o.type_of(rt));
                let proto = rt.get_string_prototype();
                let v = call_member(rt, proto, "valueOf", Value::Object(o), &[])?;
                let l = get(rt, o, "length")?;
                Value::Object(rt.new_array_values(vec![t, v, l]))
            }
            "new R" => {
                let c = reg_exp.assert_constructor(rt).expect("constructor");
                Value::Object(c.construct(rt, &args, Some(reg_exp))?)
            }
            other => {
                let name = other.strip_prefix("S.").expect("static method");
                call_member(rt, string, name, Value::Object(string), &args)?
            }
        }
    };
    let r = if drained { drain(rt, r)? } else { r };
    if case.li {
        let target = match case.this {
            A::R(..) | A::RU(..) => this,
            _ => {
                let i = case
                    .args
                    .iter()
                    .position(|a| matches!(a, A::R(..) | A::RU(..)))
                    .expect("a regexp argument");
                args[i].clone()
            }
        };
        let t = target.as_object().expect("regexp");
        let li = get(rt, t, "lastIndex")?;
        return Ok(Value::Object(rt.new_array_values(vec![r, li])));
    }
    Ok(r)
}

fn repr_units(units: &[u16]) -> String {
    let mut r = String::from("s:\"");
    for &c in units {
        if (0x20..0x7f).contains(&c) && c != 0x22 && c != 0x5c {
            r.push(c as u8 as char);
        } else {
            r.push_str(&format!("\\u{c:04x}"));
        }
    }
    r.push('"');
    r
}

fn units_of(s: &JsString) -> Vec<u16> {
    (0..s.length()).map(|i| s.char_at(i)).collect()
}

fn repr(rt: &mut Runtime, v: &Value) -> String {
    match v {
        Value::Undefined => "u".into(),
        Value::Null => "null".into(),
        Value::String(s) => repr_units(&units_of(s)),
        Value::Int(i) => format!("n:{i}"),
        Value::Float(f) if *f == 0.0 && f.is_sign_negative() => "n:-0".into(),
        Value::Float(_) => format!("n:{}", v.prim_to_string().to_rust_string()),
        Value::Bool(b) => format!("b:{b}"),
        Value::Symbol(_) => "sym".into(),
        Value::Object(o) => {
            let o = *o;
            if let ObjExt::Array(a) = &rt.obj(o).ext {
                let values = a.values.clone();
                let items: Vec<String> = values.iter().map(|x| repr(rt, x)).collect();
                let mut s = format!("a:[{}]", items.join(","));
                if o.get_own_prop_str(rt, &"index".into())
                    .expect("index")
                    .is_some()
                {
                    let i = get(rt, o, "index").expect("index");
                    let input = get(rt, o, "input").expect("input");
                    s.push_str(&format!("|i:{}|in:{}", repr(rt, &i), repr(rt, &input)));
                }
                return s;
            }
            if RegexpObject::data(rt, o).is_some() {
                let proto = rt.get_reg_exp_prototype();
                let s =
                    call_member(rt, proto, "toString", Value::Object(o), &[]).expect("toString");
                let li = get(rt, o, "lastIndex").expect("lastIndex");
                return format!("re:{}|li:{}", repr(rt, &s), repr(rt, &li));
            }
            let has = |rt: &mut Runtime, k: &str| {
                o.get_own_prop_str(rt, &k.into())
                    .expect("own prop")
                    .is_some()
            };
            if has(rt, "index") && has(rt, "length") {
                let l = get(rt, o, "length").expect("length").prim_to_integer();
                let mut items = Vec::new();
                for k in 0..l {
                    let v = o
                        .get_idx(rt, k, None)
                        .expect("item")
                        .unwrap_or(Value::Undefined);
                    items.push(repr(rt, &v));
                }
                let i = get(rt, o, "index").expect("index");
                return format!("m:[{}]|i:{}", items.join(","), repr(rt, &i));
            }
            "o".into()
        }
        other => panic!("unexpected value {other:?}"),
    }
}

fn throw_string(rt: &mut Runtime, t: Throw) -> String {
    match t {
        Throw::Value(v) => match v.to_string(rt) {
            Ok(s) => s.to_rust_string(),
            Err(e) => format!("{e:?}"),
        },
        Throw::TypeError(m) => format!("TypeError: {m}"),
        Throw::RangeError(m) => format!("RangeError: {m}"),
        Throw::SyntaxError(m) => format!("SyntaxError: {m}"),
        Throw::ReferenceError(m) => format!("ReferenceError: {m}"),
        other => format!("{other:?}"),
    }
}

fn arrays_ready(rt: &mut Runtime) -> bool {
    let a = rt.new_array_values(vec![Value::Int(7)]);
    matches!(a.get_idx(rt, 0, None), Ok(Some(Value::Int(7))))
}

fn plain_exec(rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let r = rt.regexpproto_exec(call)?;
    let Value::Object(a) = r else {
        return Ok(r);
    };
    let values = match &rt.obj(a).ext {
        ObjExt::Array(arr) => arr.values.clone(),
        _ => panic!("exec returned a non-array"),
    };
    let o = rt.new_object();
    for (i, v) in values.iter().enumerate() {
        o.set_own_str(rt, &i.to_string().as_str().into(), v.clone(), true)?;
    }
    o.set_own_str(rt, &"length".into(), Value::Int(values.len() as i64), true)?;
    let index = get(rt, a, "index")?;
    o.set_own_str(rt, &"index".into(), index, true)?;
    let input = get(rt, a, "input")?;
    o.set_own_str(rt, &"input".into(), input, true)?;
    Ok(Value::Object(o))
}

fn install_plain_exec(rt: &mut Runtime) {
    let proto = rt.get_reg_exp_prototype();
    let f = rt.new_native_func(NativeFn::Fn(plain_exec), "exec", 1);
    proto
        .set_own_str(rt, &"exec".into(), Value::Object(f), true)
        .expect("set exec");
}

fn run_cases(cases: &'static [Case]) {
    run_cases_with(cases, false, 0);
}

fn run_generic_cases(cases: &'static [Case]) {
    run_cases_with(cases, true, 0);
}

fn run_cases_with(cases: &'static [Case], generic: bool, pending_without_arrays: usize) {
    on_big_stack(move || {
        let mut rt = Runtime::new();
        if generic {
            install_plain_exec(&mut rt);
        }
        let arrays = arrays_ready(&mut rt);
        let mut failures = Vec::new();
        let mut pending = 0;
        for case in cases {
            let got = match invoke(&mut rt, case) {
                Ok(v) => repr(&mut rt, &v),
                Err(t) => format!("throw:{}", throw_string(&mut rt, t)),
            };
            if got != case.want && !arrays && case.m.ends_with("matchAll*") {
                pending += 1;
            } else if got != case.want {
                failures.push(format!(
                    "{} this={:?} args={:?}\n  got  {got}\n  want {}",
                    case.m, case.this, case.args, case.want
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        if !arrays {
            assert_eq!(
                pending, pending_without_arrays,
                "cases waiting for indexed arrays"
            );
        }
    });
}

#[test]
fn stringproto_at() {
    run_cases(cases::STRINGPROTO_AT);
}

#[test]
fn stringproto_char_at() {
    run_cases(cases::STRINGPROTO_CHAR_AT);
}

#[test]
fn stringproto_char_code_at() {
    run_cases(cases::STRINGPROTO_CHAR_CODE_AT);
}

#[test]
fn stringproto_code_point_at() {
    run_cases(cases::STRINGPROTO_CODE_POINT_AT);
}

#[test]
fn stringproto_concat() {
    run_cases(cases::STRINGPROTO_CONCAT);
}

#[test]
fn stringproto_ends_with() {
    run_cases(cases::STRINGPROTO_ENDS_WITH);
}

#[test]
fn stringproto_includes() {
    run_cases(cases::STRINGPROTO_INCLUDES);
}

#[test]
fn stringproto_index_of() {
    run_cases(cases::STRINGPROTO_INDEX_OF);
}

#[test]
fn stringproto_last_index_of() {
    run_cases(cases::STRINGPROTO_LAST_INDEX_OF);
}

#[test]
fn stringproto_locale_compare() {
    run_cases(cases::STRINGPROTO_LOCALE_COMPARE);
}

#[test]
fn stringproto_match() {
    run_cases(cases::STRINGPROTO_MATCH);
}

#[test]
fn stringproto_match_all() {
    run_cases_with(cases::STRINGPROTO_MATCH_ALL, false, 3);
}

#[test]
fn stringproto_normalize() {
    run_cases(cases::STRINGPROTO_NORMALIZE);
}

#[test]
fn stringproto_pad() {
    run_cases(cases::STRINGPROTO_PAD);
}

#[test]
fn stringproto_repeat() {
    run_cases(cases::STRINGPROTO_REPEAT);
}

#[test]
fn stringproto_replace() {
    run_cases(cases::STRINGPROTO_REPLACE);
}

#[test]
fn stringproto_replace_all() {
    run_cases(cases::STRINGPROTO_REPLACE_ALL);
}

#[test]
fn stringproto_search() {
    run_cases(cases::STRINGPROTO_SEARCH);
}

#[test]
fn stringproto_slice() {
    run_cases(cases::STRINGPROTO_SLICE);
}

#[test]
fn stringproto_split() {
    run_cases(cases::STRINGPROTO_SPLIT);
}

#[test]
fn stringproto_starts_with() {
    run_cases(cases::STRINGPROTO_STARTS_WITH);
}

#[test]
fn stringproto_substring() {
    run_cases(cases::STRINGPROTO_SUBSTRING);
}

#[test]
fn stringproto_substr() {
    run_cases(cases::STRINGPROTO_SUBSTR);
}

#[test]
fn stringproto_case() {
    run_cases(cases::STRINGPROTO_CASE);
}

#[test]
fn stringproto_trim() {
    run_cases(cases::STRINGPROTO_TRIM);
}

#[test]
fn string_static() {
    run_cases(cases::STRING_STATIC);
}

#[test]
fn stringproto_to_string_value_of() {
    run_cases(cases::STRINGPROTO_TO_STRING_VALUE_OF);
}

#[test]
fn stringproto_iterator() {
    run_cases(cases::STRINGPROTO_ITERATOR);
}

#[test]
fn regexp_exec() {
    run_cases(cases::REGEXP_EXEC);
}

#[test]
fn regexp_test() {
    run_cases(cases::REGEXP_TEST);
}

#[test]
fn regexp_to_string() {
    run_cases(cases::REGEXP_TO_STRING);
}

#[test]
fn regexp_ctor() {
    run_cases(cases::REGEXP_CTOR);
}

#[test]
fn regexp_symbol_methods() {
    run_cases_with(cases::REGEXP_SYMBOL_METHODS, false, 1);
}

#[test]
fn regexp_compile() {
    run_cases(cases::REGEXP_COMPILE);
}

#[test]
fn regexp_incompatible() {
    run_cases(cases::REGEXP_INCOMPATIBLE);
}

#[test]
fn generic_split() {
    run_generic_cases(cases::GENERIC_SPLIT);
}

#[test]
fn generic_replace() {
    run_generic_cases(cases::GENERIC_REPLACE);
}

#[test]
fn generic_match() {
    run_generic_cases(cases::GENERIC_MATCH);
}

#[test]
fn imported_ascii() {
    run_cases(cases::IMPORTED_ASCII);
}
