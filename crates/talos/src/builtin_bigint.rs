use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::deps::big;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_OBJECT, FunctionCall, ObjExt};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::ascii_string;
use crate::string_ascii::{ParseErr, string_to_int};
use crate::value::{Value, int_to_value, new_big_int, value_prop};
use std::sync::{Arc, OnceLock};

pub(crate) fn string_to_big_int(s: &[u8]) -> Option<big::Int> {
    match string_to_int(s) {
        Ok(n) => Some(big::Int::new_int(n)),
        Err(ParseErr::Range) => Some(big::Int::set_string(s, 0).unwrap_or_default()),
        Err(ParseErr::Syntax) if s == b"-0" => Some(big::Int::new()),
        Err(ParseErr::Syntax) => None,
    }
}

pub fn to_big_int(rt: &mut Runtime, value: &Value) -> JsResult<Arc<big::Int>> {
    match value {
        Value::BigInt(prim) => Ok(prim.clone()),
        Value::String(prim) => match string_to_big_int(&prim.to_trimmed_utf8()) {
            Some(bigint) => Ok(Arc::new(bigint)),
            None => {
                let e = rt.get_syntax_error();
                Err(Throw::Value(rt.new_error(
                    e,
                    &format!("Cannot convert {} to a BigInt", prim.to_rust_string()),
                )))
            }
        },
        Value::Bool(prim) => Ok(Arc::new(big::Int::new_int(i64::from(*prim)))),
        Value::Symbol(_) => Err(Throw::Value(
            rt.new_type_error("Cannot convert Symbol to a BigInt"),
        )),
        Value::Object(prim) => {
            let p = prim.to_primitive_number(rt)?;
            to_big_int(rt, &p)
        }
        prim => {
            let s = prim.go_string(rt)?;
            Err(Throw::Value(
                rt.new_type_error(&format!("Cannot convert {s} to a BigInt")),
            ))
        }
    }
}

pub fn number_to_big_int(rt: &mut Runtime, v: &Value) -> JsResult<Arc<big::Int>> {
    match rt.vm_to_numeric(v)? {
        Value::BigInt(v) => Ok(v),
        Value::Int(v) => Ok(Arc::new(big::Int::new_int(v))),
        Value::Float(f) => {
            if f.is_infinite() || f.is_nan() {
                let s = Value::Float(f).go_string(rt)?;
                return Err(Throw::Value(
                    rt.new_range_error(&format!("Cannot convert {s} to a BigInt")),
                ));
            }
            if big::float_is_int(f)
                && let (Some(n), _) = big::float_int(f)
            {
                return Ok(Arc::new(n));
            }
            let s = Value::Float(f).go_string(rt)?;
            Err(Throw::Value(rt.new_range_error(&format!(
                "Cannot convert {s} to a BigInt"
            ))))
        }
        Value::Object(v) => {
            let prim = v.to_primitive_number(rt)?;
            match prim {
                Value::Int(_) | Value::Float(_) => number_to_big_int(rt, &prim),
                _ => to_big_int(rt, &prim),
            }
        }
        v => {
            let s = v.go_string(rt)?;
            Err(Throw::Value(
                rt.new_type_error(&format!("Cannot convert {s} to a BigInt")),
            ))
        }
    }
}

impl Runtime {
    pub fn this_big_int_value(&mut self, value: &Value) -> JsResult<Value> {
        match value {
            Value::BigInt(_) => return Ok(value.clone()),
            Value::Object(t) => {
                if let ObjExt::Primitive(p) = &self.obj(*t).ext {
                    let p = p.clone();
                    return self.this_big_int_value(&p);
                }
            }
            _ => {}
        }
        Err(Throw::Value(
            self.new_type_error("requires that 'this' be a BigInt"),
        ))
    }

    pub fn bigintproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.this_big_int_value(call.this)
    }

    pub fn bigintproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let x = match self.this_big_int_value(call.this)? {
            Value::BigInt(x) => x,
            _ => unreachable!("this_big_int_value returns a BigInt"),
        };
        let radix = call.argument(0);
        let radix_mv = if radix.is_undefined() {
            10
        } else {
            let radix_mv = radix.to_integer(self)?;
            if !(2..=36).contains(&radix_mv) {
                let e = self.get_range_error();
                return Err(Throw::Value(
                    self.new_error(e, "radix must be an integer between 2 and 36"),
                ));
            }
            radix_mv
        };

        Ok(Value::String(ascii_string(&x.text(radix_mv as u32))))
    }

    pub fn bigint_as_int_n(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if call.arguments.len() < 2 {
            return Err(Throw::Value(
                self.new_type_error("Cannot convert undefined to a BigInt"),
            ));
        }
        let n = call.argument(0).to_number(self)?;
        let bits = self.to_index(&n)?;
        if bits < 0 {
            return Err(Throw::Value(self.new_type_error(
                "Invalid value: not (convertible to) a safe integer",
            )));
        }
        let bigint = to_big_int(self, &call.argument(1))?;

        let two_to_bits = big::Int::new_int(1).lsh(bits as u64);
        let m = bigint.modulo(&two_to_bits);
        if bits > 0 && m.cmp(&big::Int::new_int(1).lsh((bits - 1) as u64)) >= 0 {
            Ok(new_big_int(m.sub(&two_to_bits)))
        } else {
            Ok(new_big_int(m))
        }
    }

    pub fn bigint_as_uint_n(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if call.arguments.len() < 2 {
            return Err(Throw::Value(
                self.new_type_error("Cannot convert undefined to a BigInt"),
            ));
        }
        let n = call.argument(0).to_number(self)?;
        let bits = self.to_index(&n)?;
        if bits < 0 {
            return Err(Throw::Value(self.new_type_error(
                "Invalid value: not (convertible to) a safe integer",
            )));
        }
        let bigint = to_big_int(self, &call.argument(1))?;
        let ret = bigint.modulo(&big::Int::new_int(1).lsh(bits as u64));
        Ok(new_big_int(ret))
    }
}

pub fn get_big_int_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_int_template)
}

fn create_big_int_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));

    t.put_str("name", |_| {
        value_prop(Value::String(ascii_string("BigInt")), false, false, true)
    });
    t.put_str("length", |_| {
        value_prop(int_to_value(1), false, false, true)
    });
    t.put_str("prototype", |r| {
        value_prop(
            Value::Object(r.get_big_int_prototype()),
            false,
            false,
            false,
        )
    });

    t.put_str("asIntN", |r| {
        r.method_prop(|r, c| r.bigint_as_int_n(c), "asIntN", 2)
    });
    t.put_str("asUintN", |r| {
        r.method_prop(|r, c| r.bigint_as_uint_n(c), "asUintN", 2)
    });

    t
}

impl Runtime {
    pub fn builtin_big_int(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if !call.arguments.is_empty() {
            let v = call.arg(0);
            return match v {
                Value::BigInt(_) | Value::Int(_) | Value::Float(_) | Value::Object(_) => {
                    Ok(Value::BigInt(number_to_big_int(self, v)?))
                }
                _ => Ok(Value::BigInt(to_big_int(self, v)?)),
            };
        }
        Ok(new_big_int(big::Int::new_int(0)))
    }

    pub fn builtin_new_big_int(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        if new_target.is_some() {
            return Err(Throw::Value(
                self.new_type_error("BigInt is not a constructor"),
            ));
        }
        let v = if let Some(a) = args.first() {
            Value::BigInt(number_to_big_int(self, a)?)
        } else {
            new_big_int(big::Int::new_int(0))
        };
        Ok(self.new_primitive_object(v, new_target, CLASS_OBJECT))
    }

    pub fn get_big_int(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int = Some(ret);
        let proto = self.get_big_int_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_big_int(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_templated_func_object(
            get_big_int_template(),
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_big_int(c))),
            ctor,
        );
        ret
    }
}

fn create_big_int_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("length", |_| {
        value_prop(int_to_value(0), false, false, true)
    });
    t.put_str("name", |_| {
        value_prop(Value::String(ascii_string("BigInt")), false, false, true)
    });
    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_big_int()), true, false, true)
    });

    t.put_str("toLocaleString", |r| {
        r.method_prop(|r, c| r.bigintproto_to_string(c), "toLocaleString", 0)
    });
    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.bigintproto_to_string(c), "toString", 0)
    });
    t.put_str("valueOf", |r| {
        r.method_prop(|r, c| r.bigintproto_value_of(c), "valueOf", 0)
    });
    t.put_sym(SYM_TO_STRING_TAG, |_| {
        value_prop(Value::String(ascii_string("BigInt")), false, false, true)
    });

    t
}

pub fn get_big_int_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_int_proto_template)
}

impl Runtime {
    pub fn get_big_int_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int_prototype = Some(ret);
        self.new_templated_object(get_big_int_proto_template(), Some(ret));
        self.obj_mut(ret).base.class = CLASS_OBJECT;
        ret
    }
}
