use std::rc::Rc;
use std::sync::Arc;
use talos::builtin_regexp::{compile_regexp, is_regexp};
use talos::builtin_string::is_whitespace_char;
use talos::builtin_symbol::{SYM_MATCH, SYM_SPECIES, SYM_SPLIT, SYM_TO_STRING_TAG};
use talos::func::{NativeCtor, NativeFn};
use talos::object::{Flag, FunctionCall, ObjExt, PropertyDescriptor};
use talos::parser::WHITESPACE_CHARS;
use talos::regexp::{RegexpObject, RegexpPattern};
use talos::string::{JsString, new_string_value};
use talos::{JsResult, ObjRef, Runtime, Throw, Value};

fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new()
        .name("talos-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

fn js(s: &str) -> Value {
    Value::String(new_string_value(s.as_bytes()))
}

fn text(rt: &mut Runtime, v: &Value) -> String {
    v.to_string(rt).expect("to_string").to_rust_string()
}

fn get(rt: &mut Runtime, o: ObjRef, name: &str) -> Value {
    o.get_str(rt, &name.into(), None)
        .expect("get")
        .unwrap_or(Value::Undefined)
}

fn regexp(rt: &mut Runtime, pattern: &str, flags: &str) -> ObjRef {
    let proto = rt.get_reg_exp_prototype();
    rt.new_reg_exp(Some(&js(pattern)), Some(&js(flags)), Some(proto))
        .expect("regexp")
}

fn method(rt: &mut Runtime, holder: ObjRef, name: &str) -> Value {
    get(rt, holder, name)
}

fn call(rt: &mut Runtime, f: &Value, this: Value, args: &[Value]) -> JsResult<Value> {
    let f = f.as_object().expect("function");
    f.assert_callable(rt)
        .expect("callable")
        .call(rt, this, args)
}

fn thrown(rt: &mut Runtime, r: JsResult<Value>) -> String {
    match r {
        Ok(v) => panic!("expected a throw, got {v:?}"),
        Err(Throw::Value(v)) => text(rt, &v),
        Err(Throw::TypeError(m)) => format!("TypeError: {m}"),
        Err(other) => format!("{other:?}"),
    }
}

#[test]
fn regexp_standard_flag_follows_deoptimisation() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let re = regexp(&mut rt, "a", "g");
        assert_eq!(rt.check_std_regexp(re), Some(re));
        re.set_own_str(&mut rt, &"foo".into(), Value::Int(1), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), Some(re));
        re.set_own_sym(&mut rt, SYM_TO_STRING_TAG, js("x"), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), Some(re));
        re.set_own_str(&mut rt, &"exec".into(), Value::Int(1), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        re.set_own_sym(&mut rt, SYM_SPLIT, Value::Int(1), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        re.define_own_property_str(
            &mut rt,
            &"lastIndex".into(),
            PropertyDescriptor {
                value: Some(Value::Int(3)),
                ..PropertyDescriptor::default()
            },
            true,
        )
        .expect("define");
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        let proto = re.proto(&mut rt).expect("proto");
        assert!(re.set_proto(&mut rt, proto, true).expect("set proto"));
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        assert!(
            re.delete_str(&mut rt, &"missing".into(), true)
                .expect("delete")
        );
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        let other = rt.new_object();
        re.set_proto(&mut rt, Some(other), true).expect("set proto");
        assert_eq!(rt.check_std_regexp(re), None);

        let re = regexp(&mut rt, "a", "g");
        let proto = rt.get_reg_exp_prototype();
        proto
            .set_own_str(&mut rt, &"lastIndex".into(), Value::Int(1), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), Some(re));
        proto
            .set_own_str(&mut rt, &"exec".into(), Value::Int(1), true)
            .expect("set");
        assert_eq!(rt.check_std_regexp(re), None);
        let fresh = regexp(&mut rt, "b", "");
        assert_eq!(rt.check_std_regexp(fresh), None);
    });
}

#[test]
fn regexp_last_index_is_writable_hidden_and_permanent() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let re = regexp(&mut rt, "a", "");
        let p = re
            .get_own_prop_str(&mut rt, &"lastIndex".into())
            .expect("prop")
            .expect("lastIndex");
        let Value::Property(p) = p else {
            panic!("lastIndex is not a property");
        };
        assert!(p.writable && !p.enumerable && !p.configurable && !p.accessor);
        assert!(matches!(p.value, Value::Int(0)));
        let keys = re.keys(&mut rt, true, Vec::new()).expect("keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(re.class_name(&rt), "RegExp");
    });
}

#[test]
fn regexp_clone_keeps_the_prototype_and_the_pattern() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let p = rt.new_object();
        let re = rt
            .new_reg_exp(Some(&js("a+")), Some(&js("gi")), Some(p))
            .expect("regexp");
        let proto = rt.get_reg_exp_prototype();
        let copy = rt
            .new_reg_exp(Some(&Value::Object(re)), None, Some(proto))
            .expect("copy");
        assert_ne!(copy, re);
        assert_eq!(copy.proto(&mut rt).expect("proto"), Some(p));
        let a = RegexpObject::pattern_of(&rt, re);
        let b = RegexpObject::pattern_of(&rt, copy);
        assert!(Rc::ptr_eq(&a, &b));
        let other = rt
            .new_reg_exp(Some(&Value::Object(re)), Some(&js("m")), Some(proto))
            .expect("other");
        assert_eq!(other.proto(&mut rt).expect("proto"), Some(proto));
        let src = RegexpObject::source_of(&rt, other);
        assert_eq!(src.to_rust_string(), "a+");
        assert!(RegexpObject::pattern_of(&rt, other).borrow().multiline);
    });
}

#[test]
fn regexp_call_returns_the_same_object_for_a_plain_regexp() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let re = regexp(&mut rt, "a", "g");
        let ctor = Value::Object(rt.get_reg_exp());
        let r = call(&mut rt, &ctor, Value::Undefined, &[Value::Object(re)]).expect("call");
        assert!(matches!(r, Value::Object(o) if o == re));
        let r = call(
            &mut rt,
            &ctor,
            Value::Undefined,
            &[Value::Object(re), Value::Undefined],
        )
        .expect("call");
        assert!(matches!(r, Value::Object(o) if o == re));
        let r = call(
            &mut rt,
            &ctor,
            Value::Undefined,
            &[Value::Object(re), js("y")],
        )
        .expect("call");
        assert!(matches!(r, Value::Object(o) if o != re));
        re.set_own_str(&mut rt, &"constructor".into(), Value::Int(1), true)
            .expect("set");
        let r = call(&mut rt, &ctor, Value::Undefined, &[Value::Object(re)]).expect("call");
        assert!(matches!(r, Value::Object(o) if o != re));
    });
}

#[test]
fn regexp_prototype_getters_on_the_prototype_itself() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let proto = rt.get_reg_exp_prototype();
        let p = Value::Object(proto);
        for name in [
            "global",
            "multiline",
            "dotAll",
            "ignoreCase",
            "unicode",
            "sticky",
        ] {
            assert!(get(&mut rt, proto, name).is_undefined(), "{name}");
        }
        let source = get(&mut rt, proto, "source");
        assert_eq!(text(&mut rt, &source), "(?:)");
        let flags = get(&mut rt, proto, "flags");
        assert_eq!(text(&mut rt, &flags), "");
        let to_string = method(&mut rt, proto, "toString");
        let s = call(&mut rt, &to_string, p.clone(), &[]).expect("toString");
        assert_eq!(text(&mut rt, &s), "/(?:)/");
        let o = Value::Object(rt.new_object());
        let exec = method(&mut rt, proto, "exec");
        let r = call(&mut rt, &exec, o, &[js("a")]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Method RegExp.prototype.exec called on incompatible receiver [object Object]"
        );
        let r = call(&mut rt, &exec, p, &[js("a")]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Method RegExp.prototype.exec called on incompatible receiver [object Object]"
        );
    });
}

#[test]
fn string_constructor_and_receivers() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let string = rt.get_string();
        let sym = rt.new_symbol(Some(new_string_value(b"x")));
        let r = call(
            &mut rt,
            &Value::Object(string),
            Value::Undefined,
            &[Value::Symbol(sym)],
        )
        .expect("String(sym)");
        assert_eq!(text(&mut rt, &r), "Symbol(x)");
        let ctor = string.assert_constructor(&rt).expect("constructor");
        let err = ctor
            .construct(&mut rt, &[Value::Symbol(sym)], Some(string))
            .map(Value::Object);
        assert_eq!(
            thrown(&mut rt, err),
            "TypeError: Cannot convert a Symbol value to a string"
        );
        let proto = rt.get_string_prototype();
        assert_eq!(proto.class_name(&rt), "String");
        let to_string = method(&mut rt, proto, "toString");
        let r = call(&mut rt, &to_string, Value::Object(proto), &[]).expect("toString");
        assert_eq!(text(&mut rt, &r), "");
        let o = Value::Object(rt.new_object());
        let value_of = method(&mut rt, proto, "valueOf");
        let r = call(&mut rt, &value_of, o, &[]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: String.prototype.valueOf is called on incompatible receiver"
        );
        let other_proto = rt.new_object();
        let s = rt
            .builtin_new_string(&[js("ab")], Some(other_proto))
            .expect("new string");
        assert_eq!(s.proto(&mut rt).expect("proto"), Some(other_proto));
        let l = get(&mut rt, s, "length");
        assert!(matches!(l, Value::Int(2)));
        let single = rt.get_string_singleton().expect("singleton");
        assert_eq!(rt.get_string_singleton().expect("singleton"), single);
        assert!(matches!(&rt.obj(single).ext, ObjExt::StringObj(_)));
    });
}

#[test]
fn string_raw_reads_the_raw_segments() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let raw = rt.new_object();
        for (i, s) in ["a", "b", "c"].iter().enumerate() {
            raw.set_own_str(&mut rt, &i.to_string().as_str().into(), js(s), true)
                .expect("set");
        }
        raw.set_own_str(&mut rt, &"length".into(), Value::Int(3), true)
            .expect("set");
        let cooked = rt.new_object();
        cooked
            .set_own_str(&mut rt, &"raw".into(), Value::Object(raw), true)
            .expect("set");
        let string = rt.get_string();
        let f = method(&mut rt, string, "raw");
        let r = call(
            &mut rt,
            &f,
            Value::Undefined,
            &[
                Value::Object(cooked),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
            ],
        )
        .expect("raw");
        assert_eq!(text(&mut rt, &r), "a1b2c");
        let r = call(
            &mut rt,
            &f,
            Value::Undefined,
            &[Value::Object(cooked), Value::Int(1)],
        )
        .expect("raw");
        assert_eq!(text(&mut rt, &r), "a1bc");
        raw.set_own_str(&mut rt, &"length".into(), Value::Int(0), true)
            .expect("set");
        let r = call(&mut rt, &f, Value::Undefined, &[Value::Object(cooked)]).expect("raw");
        assert_eq!(text(&mut rt, &r), "");
        let r = call(&mut rt, &f, Value::Undefined, &[]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Cannot convert undefined or null to object"
        );
    });
}

#[test]
fn iterators_reject_foreign_receivers() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let o = Value::Object(rt.new_object());
        let sip = rt.get_string_iterator_prototype();
        let next = method(&mut rt, sip, "next");
        let r = call(&mut rt, &next, o.clone(), &[]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Method String Iterator.prototype.next called on incompatible receiver [object Object]"
        );
        let r = call(&mut rt, &next, Value::Object(sip), &[]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Method String Iterator.prototype.next called on incompatible receiver [object String Iterator]"
        );
        let rip = rt.get_reg_exp_string_iterator_prototype();
        let next = method(&mut rt, rip, "next");
        let r = call(&mut rt, &next, o, &[]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: Method RegExp String Iterator.prototype.next called on incompatible receiver [object Object]"
        );
        let r = call(&mut rt, &next, js("x"), &[]);
        assert_eq!(thrown(&mut rt, r), "TypeError: Value is not an object: x");
    });
}

#[test]
fn symbol_match_drives_is_regexp() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let o = rt.new_object();
        assert!(!is_regexp(&mut rt, &Value::Object(o)).expect("is_regexp"));
        o.set_own_sym(&mut rt, SYM_MATCH, Value::Int(1), true)
            .expect("set");
        assert!(is_regexp(&mut rt, &Value::Object(o)).expect("is_regexp"));
        let proto = rt.get_string_prototype();
        let includes = method(&mut rt, proto, "includes");
        let r = call(&mut rt, &includes, js("abc"), &[Value::Object(o)]);
        assert_eq!(
            thrown(&mut rt, r),
            "TypeError: First argument to String.prototype.includes must not be a regular expression"
        );
        let re = regexp(&mut rt, "a", "");
        assert!(is_regexp(&mut rt, &Value::Object(re)).expect("is_regexp"));
        re.set_own_sym(&mut rt, SYM_MATCH, Value::Bool(false), true)
            .expect("set");
        assert!(!is_regexp(&mut rt, &Value::Object(re)).expect("is_regexp"));
        let r = call(&mut rt, &includes, js("x/a/y"), &[Value::Object(re)]).expect("includes");
        assert!(matches!(r, Value::Bool(true)));
    });
}

fn prefix_match(rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let this = call.this.as_object().expect("this");
    let p = get(rt, this, "p").to_string(rt)?.to_rust_string();
    let s = call.argument(0).to_string(rt)?.to_rust_string();
    Ok(Value::Bool(s.starts_with(&p)))
}

#[test]
fn string_match_sym_natively() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let f = rt.new_native_func(NativeFn::Fn(prefix_match), "match", 1);
        let proto = rt.new_object();
        proto
            .set_own_sym(&mut rt, SYM_MATCH, Value::Object(f), true)
            .expect("set");
        let prefix = |rt: &mut Runtime, p: &str| {
            let o = rt.create_object(Some(proto));
            o.set_own_str(rt, &"p".into(), js(p), true).expect("set");
            Value::Object(o)
        };
        let prefix1 = prefix(&mut rt, "abc");
        let prefix2 = prefix(&mut rt, "def");
        let sp = rt.get_string_prototype();
        let m = method(&mut rt, sp, "match");
        for (s, p, want) in [
            ("abc123", &prefix1, true),
            ("abc123", &prefix2, false),
            ("def123", &prefix1, false),
            ("def123", &prefix2, true),
        ] {
            let r = call(&mut rt, &m, js(s), std::slice::from_ref(p)).expect("match");
            assert!(matches!(r, Value::Bool(b) if b == want), "{s}");
        }
    });
}

fn throw_passed(_rt: &mut Runtime, _args: &[Value], _nt: Option<ObjRef>) -> JsResult<ObjRef> {
    Err(Throw::Value(Value::String(new_string_value(b"passed"))))
}

#[test]
fn regexp_override_species_natively() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let species = rt.new_native_func_and_construct(
            None,
            None,
            Some(NativeCtor::Fn(throw_passed)),
            None,
            "",
            Value::Int(0),
        );
        let re_ctor = rt.get_reg_exp();
        re_ctor
            .define_own_property_sym(
                &mut rt,
                SYM_SPECIES,
                PropertyDescriptor {
                    value: Some(Value::Object(species)),
                    configurable: Flag::True,
                    ..PropertyDescriptor::default()
                },
                true,
            )
            .expect("define");
        let re = regexp(&mut rt, "a", "");
        let sp = rt.get_string_prototype();
        let split = method(&mut rt, sp, "split");
        let r = call(&mut rt, &split, js("ab"), &[Value::Object(re)]);
        assert_eq!(thrown(&mut rt, r), "passed");
    });
}

#[test]
fn regexp_consecutive_match_cache_natively() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        for flags in ["g", "gu"] {
            let re = regexp(&mut rt, "t(e)(st(\\d?))", flags);
            let target = new_string_value(b"test1test2");
            let mut indexes = Vec::new();
            loop {
                let m = RegexpObject::exec(&mut rt, re, &target).expect("exec");
                let Value::Object(m) = m else {
                    break;
                };
                let values = match &rt.obj(m).ext {
                    ObjExt::Array(a) => a.values.clone(),
                    _ => panic!("not an array"),
                };
                let values: Vec<String> = values.iter().map(|v| text(&mut rt, v)).collect();
                let i = get(&mut rt, m, "index");
                indexes.push((values, text(&mut rt, &i)));
            }
            assert_eq!(
                indexes,
                vec![
                    (
                        vec!["test1".into(), "e".into(), "st1".into(), "1".into()],
                        "0".into()
                    ),
                    (
                        vec!["test2".into(), "e".into(), "st2".into(), "2".into()],
                        "5".into()
                    ),
                ],
                "{flags}"
            );
            re.set_own_str(&mut rt, &"lastIndex".into(), Value::Int(0), true)
                .expect("set");
            let m = RegexpObject::exec(&mut rt, re, &new_string_value(b" test5")).expect("exec");
            let m = m.as_object().expect("match");
            let i = get(&mut rt, m, "index");
            assert_eq!(text(&mut rt, &i), "1", "{flags}");
            let pattern = RegexpObject::pattern_of(&rt, re);
            assert!(pattern.borrow().regexp2_wrapper.is_some());
        }
    });
}

#[test]
fn regexp2_cache_is_kept_only_for_global_or_sticky_matches() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let target = new_string_value("aé(a)é".as_bytes());
        let re = regexp(&mut rt, "(?=a)a", "g");
        assert!(RegexpObject::test(&mut rt, re, &target).expect("test"));
        let cache = |rt: &Runtime, re| {
            let p = RegexpObject::pattern_of(rt, re);
            let p = p.borrow();
            p.regexp2_wrapper
                .as_ref()
                .expect("regexp2")
                .cache
                .as_ref()
                .map(|c| (c.target.to_rust_string(), c.pos_map.is_some()))
        };
        assert_eq!(cache(&rt, re), Some(("aé(a)é".to_string(), false)));
        assert!(RegexpObject::test(&mut rt, re, &target).expect("test"));
        assert_eq!(cache(&rt, re), Some(("aé(a)é".to_string(), false)));
        assert!(!RegexpObject::test(&mut rt, re, &target).expect("test"));
        assert_eq!(cache(&rt, re), None);
        let re = regexp(&mut rt, "(?=a)a", "gu");
        assert!(RegexpObject::test(&mut rt, re, &target).expect("test"));
        assert_eq!(cache(&rt, re), Some(("aé(a)é".to_string(), true)));
        let re = regexp(&mut rt, "(?=a)a", "");
        assert!(RegexpObject::test(&mut rt, re, &target).expect("test"));
        assert_eq!(cache(&rt, re), None);
    });
}

#[test]
fn literal_patterns_are_shareable_and_cloned_per_object() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RegexpPattern>();
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let shared = Arc::new(compile_regexp(b"(?<!-)\\d+", b"g").expect("compile"));
        let proto = rt.get_reg_exp_prototype();
        let a = rt.new_reg_expp(
            (*shared).clone(),
            new_string_value(b"(?<!-)\\d+"),
            Some(proto),
        );
        let b = rt.new_reg_expp(
            (*shared).clone(),
            new_string_value(b"(?<!-)\\d+"),
            Some(proto),
        );
        let s = new_string_value(b"-12 34");
        let ra = RegexpObject::exec(&mut rt, a, &s).expect("exec");
        let rb = RegexpObject::exec(&mut rt, b, &s).expect("exec");
        let ma = ra.as_object().expect("match");
        let mb = rb.as_object().expect("match");
        let ia = get(&mut rt, ma, "index");
        let ib = get(&mut rt, mb, "index");
        assert_eq!(text(&mut rt, &ia), "2");
        assert_eq!(text(&mut rt, &ib), "2");
        assert!(
            shared
                .regexp2_wrapper
                .as_ref()
                .expect("regexp2")
                .cache
                .is_none()
        );
        let pa = RegexpObject::pattern_of(&rt, a);
        assert!(
            pa.borrow()
                .regexp2_wrapper
                .as_ref()
                .expect("regexp2")
                .cache
                .is_some()
        );
    });
}

#[test]
fn whitespace_set_matches_the_parser() {
    let set: Vec<i32> = WHITESPACE_CHARS.chars().map(|c| c as i32).collect();
    for r in 0..0x11000 {
        assert_eq!(is_whitespace_char(r), set.contains(&r), "{r:#x}");
    }
}

#[test]
fn unicode_repeat_drops_the_first_code_unit() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let sp = rt.get_string_prototype();
        let f = method(&mut rt, sp, "repeat");
        let r = call(&mut rt, &f, js("éa"), &[Value::Int(1)]).expect("repeat");
        let Value::String(s) = &r else {
            panic!("not a string");
        };
        assert!(matches!(s, JsString::Unicode(_)));
        assert_eq!(s.to_rust_string(), "a");
        assert!(!r.strict_equals(&js("a")));
    });
}

fn expected_pad(s: &str, p: &str, n: usize, start: bool) -> String {
    if n == 0 {
        return s.to_string();
    }
    let p = if p.is_empty() { " " } else { p };
    let ss: Vec<u16> = s.encode_utf16().collect();
    let mut b = String::new();
    let mut n = n as isize - ss.len() as isize;
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
fn imported_strings_follow_the_test_imported_string_tables() {
    on_big_stack(|| {
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
        let index_of: [[i64; 8]; 8] = [
            [0, -1, -1, -1, -1, 5, -1, -1],
            [-1, 0, -1, -1, -1, 8, 0, -1],
            [-1, -1, 0, -1, 6, -1, -1, 9],
            [-1, -1, -1, 0, 5, -1, 0, 8],
            [-1, -1, -1, -1, 0, -1, -1, 3],
            [-1, -1, -1, -1, -1, 0, -1, -1],
            [-1, -1, -1, -1, -1, -1, 0, -1],
            [-1, -1, -1, -1, -1, -1, -1, 0],
        ];
        let last_index_of: [[i64; 8]; 8] = [
            [0, -1, -1, -1, -1, 5, -1, -1],
            [-1, 0, -1, -1, -1, 8, 4, -1],
            [-1, -1, 0, -1, 6, -1, -1, 9],
            [-1, -1, -1, 0, 23, -1, 0, 26],
            [-1, -1, -1, -1, 0, -1, -1, 3],
            [-1, -1, -1, -1, -1, 0, -1, -1],
            [-1, -1, -1, -1, -1, -1, 0, -1],
            [-1, -1, -1, -1, -1, -1, -1, 0],
        ];
        let mut rt = Runtime::new();
        let sp = rt.get_string_prototype();
        let index_of_f = method(&mut rt, sp, "indexOf");
        let last_index_of_f = method(&mut rt, sp, "lastIndexOf");
        let pad_start = method(&mut rt, sp, "padStart");
        let pad_end = method(&mut rt, sp, "padEnd");
        let replace = method(&mut rt, sp, "replace");
        let variants = |s: &str| {
            [
                js(s),
                Value::String(JsString::Imported(
                    talos::string_imported::ImportedString::new(s.as_bytes()),
                )),
            ]
        };
        for (i, a) in strs.iter().enumerate() {
            for (j, b) in strs.iter().enumerate() {
                for aa in variants(a) {
                    for bb in variants(b) {
                        let ctx = format!("{a:?} {b:?} {aa:?} {bb:?}");
                        let r = call(&mut rt, &index_of_f, aa.clone(), std::slice::from_ref(&bb))
                            .expect("indexOf");
                        assert!(
                            matches!(r, Value::Int(x) if x == index_of[i][j]),
                            "indexOf {ctx}"
                        );
                        let r = call(
                            &mut rt,
                            &last_index_of_f,
                            aa.clone(),
                            std::slice::from_ref(&bb),
                        )
                        .expect("lastIndexOf");
                        assert!(
                            matches!(r, Value::Int(x) if x == last_index_of[i][j]),
                            "lastIndexOf {ctx}"
                        );
                        let r = call(
                            &mut rt,
                            &pad_start,
                            aa.clone(),
                            &[Value::Int(32), bb.clone()],
                        )
                        .expect("padStart");
                        assert_eq!(
                            text(&mut rt, &r),
                            expected_pad(a, b, 32, true),
                            "padStart {ctx}"
                        );
                        let r = call(&mut rt, &pad_end, aa.clone(), &[Value::Int(32), bb.clone()])
                            .expect("padEnd");
                        assert_eq!(
                            text(&mut rt, &r),
                            expected_pad(a, b, 32, false),
                            "padEnd {ctx}"
                        );
                        let r = call(&mut rt, &replace, aa.clone(), &[bb.clone(), js("")])
                            .expect("replace");
                        assert_eq!(text(&mut rt, &r), a.replacen(b, "", 1), "replace {ctx}");
                    }
                }
            }
        }
    });
}

#[test]
fn trim_of_an_imported_string_gives_a_native_string() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let sp = rt.get_string_prototype();
        for name in ["trim", "trimStart", "trimEnd"] {
            let f = method(&mut rt, sp, name);
            let imported = Value::String(JsString::Imported(
                talos::string_imported::ImportedString::new(b"ab"),
            ));
            let r = call(&mut rt, &f, imported, &[]).expect("trim");
            assert!(
                matches!(&r, Value::String(JsString::Ascii(a)) if a.as_bytes() == b"ab"),
                "{name}"
            );
        }
    });
}
