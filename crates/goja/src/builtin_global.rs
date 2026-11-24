use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_GLOBAL, FunctionCall};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime};
use crate::string::ascii_string;
use crate::value::{Value, value_prop};
use std::sync::OnceLock;

impl Runtime {
    pub fn builtin_is_nan(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_isNaN")
    }

    pub fn builtin_parse_int(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_parseInt")
    }

    pub fn builtin_parse_float(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_parseFloat")
    }

    pub fn builtin_is_finite(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_isFinite")
    }

    pub fn builtin_decode_uri(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_decodeURI")
    }

    pub fn builtin_decode_uri_component(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_decodeURIComponent")
    }

    pub fn builtin_encode_uri(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_encodeURI")
    }

    pub fn builtin_encode_uri_component(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_encodeURIComponent")
    }

    pub fn builtin_escape(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_escape")
    }

    pub fn builtin_unescape(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        todo!("builtin_unescape")
    }

    pub fn get_eval(&mut self) -> ObjRef {
        if let Some(r) = self.global.eval {
            return r;
        }
        let ret = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_eval(c)), "eval", 1);
        self.global.eval = Some(ret);
        ret
    }
}

fn prop(o: ObjRef) -> Value {
    value_prop(Value::Object(o), true, false, true)
}

fn create_global_object_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t.put_str("Object", |r| prop(r.get_object()));
    t.put_str("Function", |r| prop(r.get_function()));
    t.put_str("Array", |r| prop(r.get_array()));
    t.put_str("String", |r| prop(r.get_string()));
    t.put_str("Number", |r| prop(r.get_number()));
    t.put_str("BigInt", |r| prop(r.get_big_int()));
    t.put_str("RegExp", |r| prop(r.get_reg_exp()));
    t.put_str("Date", |r| prop(r.get_date()));
    t.put_str("Boolean", |r| prop(r.get_boolean()));
    t.put_str("Proxy", |r| prop(r.get_proxy()));
    t.put_str("Reflect", |r| prop(r.get_reflect()));
    t.put_str("Error", |r| prop(r.get_error()));
    t.put_str("AggregateError", |r| prop(r.get_aggregate_error()));
    t.put_str("TypeError", |r| prop(r.get_type_error()));
    t.put_str("ReferenceError", |r| prop(r.get_reference_error()));
    t.put_str("SyntaxError", |r| prop(r.get_syntax_error()));
    t.put_str("RangeError", |r| prop(r.get_range_error()));
    t.put_str("EvalError", |r| prop(r.get_eval_error()));
    t.put_str("URIError", |r| prop(r.get_uri_error()));
    t.put_str("eval", |r| prop(r.get_eval()));
    t.put_str("Math", |r| prop(r.get_math()));
    t.put_str("JSON", |r| prop(r.get_json()));
    crate::builtin_typedarrays::add_typed_arrays(&mut t);
    t.put_str("Symbol", |r| prop(r.get_symbol()));
    t.put_str("WeakSet", |r| prop(r.get_weak_set()));
    t.put_str("WeakMap", |r| prop(r.get_weak_map()));
    t.put_str("Map", |r| prop(r.get_map()));
    t.put_str("Set", |r| prop(r.get_set()));
    t.put_str("Promise", |r| prop(r.get_promise()));
    t.put_str("globalThis", |r| prop(r.global_object));
    t.put_str("NaN", |_| {
        value_prop(Value::Float(f64::NAN), false, false, false)
    });
    t.put_str("undefined", |_| {
        value_prop(Value::Undefined, false, false, false)
    });
    t.put_str("Infinity", |_| {
        value_prop(Value::Float(f64::INFINITY), false, false, false)
    });
    t.put_str("isNaN", |r| {
        r.method_prop(|r, c| r.builtin_is_nan(c), "isNaN", 1)
    });
    t.put_str("parseInt", |r| prop(r.get_parse_int()));
    t.put_str("parseFloat", |r| prop(r.get_parse_float()));
    t.put_str("isFinite", |r| {
        r.method_prop(|r, c| r.builtin_is_finite(c), "isFinite", 1)
    });
    t.put_str("decodeURI", |r| {
        r.method_prop(|r, c| r.builtin_decode_uri(c), "decodeURI", 1)
    });
    t.put_str("decodeURIComponent", |r| {
        r.method_prop(
            |r, c| r.builtin_decode_uri_component(c),
            "decodeURIComponent",
            1,
        )
    });
    t.put_str("encodeURI", |r| {
        r.method_prop(|r, c| r.builtin_encode_uri(c), "encodeURI", 1)
    });
    t.put_str("encodeURIComponent", |r| {
        r.method_prop(
            |r, c| r.builtin_encode_uri_component(c),
            "encodeURIComponent",
            1,
        )
    });
    t.put_str("escape", |r| {
        r.method_prop(|r, c| r.builtin_escape(c), "escape", 1)
    });
    t.put_str("unescape", |r| {
        r.method_prop(|r, c| r.builtin_unescape(c), "unescape", 1)
    });
    t.put_sym(SYM_TO_STRING_TAG, |_| {
        value_prop(
            Value::String(ascii_string(CLASS_GLOBAL)),
            false,
            false,
            true,
        )
    });
    t
}

pub fn get_global_object_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_global_object_template)
}
