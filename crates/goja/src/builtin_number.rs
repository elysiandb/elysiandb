use crate::ftoa::{
    MODE_EXPONENTIAL, MODE_FIXED, MODE_PRECISION, MODE_STANDARD, MODE_STANDARD_EXPONENTIAL,
    f_to_base_str,
};
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_NUMBER, FunctionCall, ObjExt};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::{
    STRING_INFINITY, STRING_NAN, STRING_NEG_INFINITY, ascii_string, new_string_value,
};
use crate::string_ascii::AsciiString;
use crate::value::{MAX_INT, Value, f_to_str, int_to_value, value_prop};
use gocompat::math::floor;
use std::sync::OnceLock;

fn ascii_value(b: &[u8]) -> Value {
    Value::String(crate::string::JsString::Ascii(AsciiString::new(b)))
}

impl Runtime {
    pub fn to_number(&mut self, v: &Value) -> JsResult<Value> {
        match v {
            Value::Float(_) | Value::Int(_) => return Ok(v.clone()),
            Value::Object(o) => {
                if let ObjExt::Primitive(p) = &self.obj(*o).ext {
                    let p = p.clone();
                    return self.to_number(&p);
                }
                if let Some(v) = crate::host::goreflect::reflect_number_value(self, *o) {
                    return Ok(v);
                }
                if Some(*o) == self.global.number_prototype {
                    return Ok(Value::Int(0));
                }
            }
            _ => {}
        }
        let s = v.go_string(self)?;
        Err(Throw::Value(
            self.new_type_error(&format!("Value is not a number: {s}")),
        ))
    }

    pub fn numberproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.to_number(call.this)
    }

    pub fn numberproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut num_val = None;
        match call.this {
            Value::Float(_) | Value::Int(_) => num_val = Some(call.this.clone()),
            Value::Object(t) => {
                if let ObjExt::Primitive(p) = &self.obj(*t).ext {
                    let p = p.clone();
                    num_val = Some(self.to_number(&p)?);
                }
                if let Some(r) = crate::host::goreflect::reflect_number_to_string(self, *t) {
                    match r {
                        Ok(s) => return Ok(Value::String(new_string_value(s.as_bytes()))),
                        Err(v) => num_val = Some(v),
                    }
                }
                if Some(*t) == self.global.number_prototype {
                    return Ok(Value::String(ascii_string("0")));
                }
            }
            _ => {}
        }
        let Some(num_val) = num_val else {
            return Err(Throw::Value(self.new_type_error("Value is not a number")));
        };
        let arg = call.argument(0);
        let radix = if !arg.is_undefined() {
            arg.to_integer(self)?
        } else {
            10
        };

        if !(2..=36).contains(&radix) {
            let e = self.get_range_error();
            return Err(Throw::Value(self.new_error(
                e,
                "toString() radix argument must be between 2 and 36",
            )));
        }

        let num = num_val.prim_to_float();

        if num.is_nan() {
            return Ok(Value::String(STRING_NAN.clone()));
        }

        if num == f64::INFINITY {
            return Ok(Value::String(STRING_INFINITY.clone()));
        }

        if num == f64::NEG_INFINITY {
            return Ok(Value::String(STRING_NEG_INFINITY.clone()));
        }

        if radix == 10 {
            return Ok(ascii_value(&f_to_str(num, MODE_STANDARD, 0)));
        }

        Ok(ascii_value(f_to_base_str(num, radix).as_bytes()))
    }

    pub fn numberproto_to_fixed(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let num = self.to_number(call.this)?.prim_to_float();
        let prec = call.argument(0).to_integer(self)?;

        if !(0..=100).contains(&prec) {
            let e = self.get_range_error();
            return Err(Throw::Value(
                self.new_error(e, "toFixed() precision must be between 0 and 100"),
            ));
        }
        if num.is_nan() {
            return Ok(Value::String(STRING_NAN.clone()));
        }
        Ok(ascii_value(&f_to_str(num, MODE_FIXED, prec)))
    }

    pub fn numberproto_to_exponential(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let num = self.to_number(call.this)?.prim_to_float();
        let prec_val = call.argument(0);
        let prec = if prec_val.is_undefined() {
            return Ok(ascii_value(&f_to_str(num, MODE_STANDARD_EXPONENTIAL, 0)));
        } else {
            prec_val.to_integer(self)?
        };

        if num.is_nan() {
            return Ok(Value::String(STRING_NAN.clone()));
        }
        if num == f64::INFINITY {
            return Ok(Value::String(STRING_INFINITY.clone()));
        }
        if num == f64::NEG_INFINITY {
            return Ok(Value::String(STRING_NEG_INFINITY.clone()));
        }

        if !(0..=100).contains(&prec) {
            let e = self.get_range_error();
            return Err(Throw::Value(self.new_error(
                e,
                "toExponential() precision must be between 0 and 100",
            )));
        }

        Ok(ascii_value(&f_to_str(num, MODE_EXPONENTIAL, prec + 1)))
    }

    pub fn numberproto_to_precision(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let num_val = self.to_number(call.this)?;
        let prec_val = call.argument(0);
        if prec_val.is_undefined() {
            return Ok(Value::String(num_val.prim_to_string()));
        }
        let num = num_val.prim_to_float();
        let prec = prec_val.to_integer(self)?;

        if num.is_nan() {
            return Ok(Value::String(STRING_NAN.clone()));
        }
        if num == f64::INFINITY {
            return Ok(Value::String(STRING_INFINITY.clone()));
        }
        if num == f64::NEG_INFINITY {
            return Ok(Value::String(STRING_NEG_INFINITY.clone()));
        }
        if !(1..=100).contains(&prec) {
            let e = self.get_range_error();
            return Err(Throw::Value(
                self.new_error(e, "toPrecision() precision must be between 1 and 100"),
            ));
        }

        Ok(ascii_value(&f_to_str(num, MODE_PRECISION, prec)))
    }

    pub fn number_is_finite(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(match call.arg(0) {
            Value::Int(_) => Value::Bool(true),
            Value::Float(f) => self.to_boolean(!f.is_infinite() && !f.is_nan()),
            _ => Value::Bool(false),
        })
    }

    pub fn number_is_integer(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(match call.arg(0) {
            Value::Int(_) => Value::Bool(true),
            Value::Float(f) => self.to_boolean(!f.is_nan() && !f.is_infinite() && floor(*f) == *f),
            _ => Value::Bool(false),
        })
    }

    pub fn number_is_nan(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Float(f) = call.arg(0)
            && f.is_nan()
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn number_is_safe_integer(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.arg(0);
        if let Value::Int(i) = arg
            && *i >= -(MAX_INT - 1)
            && *i < MAX_INT
        {
            return Ok(Value::Bool(true));
        }
        if let Value::Float(f) = arg
            && *f == 0.0
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn builtin_number(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Some(t) = call.arguments.first() {
            match t {
                Value::Object(o) => {
                    let prim_value = o.to_primitive_number(self)?;
                    if let Value::BigInt(bigint) = &prim_value {
                        return Ok(int_to_value(bigint.int64()));
                    }
                    prim_value.to_number(self)
                }
                Value::BigInt(t) => Ok(int_to_value(t.int64())),
                t => t.to_number(self),
            }
        } else {
            Ok(Value::Int(0))
        }
    }

    pub fn builtin_new_number(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let v = if let Some(t) = args.first() {
            match t {
                Value::Object(o) => {
                    let prim_value = o.to_primitive_number(self)?;
                    if let Value::BigInt(bigint) = &prim_value {
                        int_to_value(bigint.int64())
                    } else {
                        prim_value.to_number(self)?
                    }
                }
                Value::BigInt(t) => int_to_value(t.int64()),
                t => t.to_number(self)?,
            }
        } else {
            int_to_value(0)
        };
        Ok(self.new_primitive_object(v, proto, CLASS_NUMBER))
    }
}

fn create_number_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_number()), true, false, true)
    });

    t.put_str("toExponential", |r| {
        r.method_prop(|r, c| r.numberproto_to_exponential(c), "toExponential", 1)
    });
    t.put_str("toFixed", |r| {
        r.method_prop(|r, c| r.numberproto_to_fixed(c), "toFixed", 1)
    });
    t.put_str("toLocaleString", |r| {
        r.method_prop(|r, c| r.numberproto_to_string(c), "toLocaleString", 0)
    });
    t.put_str("toPrecision", |r| {
        r.method_prop(|r, c| r.numberproto_to_precision(c), "toPrecision", 1)
    });
    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.numberproto_to_string(c), "toString", 1)
    });
    t.put_str("valueOf", |r| {
        r.method_prop(|r, c| r.numberproto_value_of(c), "valueOf", 0)
    });

    t
}

pub fn get_number_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_number_proto_template)
}

fn create_number_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t.put_str("length", |_| {
        value_prop(int_to_value(1), false, false, true)
    });
    t.put_str("name", |_| {
        value_prop(Value::String(ascii_string("Number")), false, false, true)
    });

    t.put_str("prototype", |r| {
        value_prop(Value::Object(r.get_number_prototype()), false, false, false)
    });

    t.put_str("EPSILON", |_| {
        value_prop(Value::Float(f64::EPSILON), false, false, false)
    });
    t.put_str("isFinite", |r| {
        r.method_prop(|r, c| r.number_is_finite(c), "isFinite", 1)
    });
    t.put_str("isInteger", |r| {
        r.method_prop(|r, c| r.number_is_integer(c), "isInteger", 1)
    });
    t.put_str("isNaN", |r| {
        r.method_prop(|r, c| r.number_is_nan(c), "isNaN", 1)
    });
    t.put_str("isSafeInteger", |r| {
        r.method_prop(|r, c| r.number_is_safe_integer(c), "isSafeInteger", 1)
    });
    t.put_str("MAX_SAFE_INTEGER", |_| {
        value_prop(Value::Int(MAX_INT - 1), false, false, false)
    });
    t.put_str("MIN_SAFE_INTEGER", |_| {
        value_prop(Value::Int(-(MAX_INT - 1)), false, false, false)
    });
    t.put_str("MIN_VALUE", |_| {
        value_prop(Value::Float(f64::from_bits(1)), false, false, false)
    });
    t.put_str("MAX_VALUE", |_| {
        value_prop(Value::Float(f64::MAX), false, false, false)
    });
    t.put_str("NaN", |_| {
        value_prop(Value::Float(f64::NAN), false, false, false)
    });
    t.put_str("NEGATIVE_INFINITY", |_| {
        value_prop(Value::Float(f64::NEG_INFINITY), false, false, false)
    });
    t.put_str("parseFloat", |r| {
        value_prop(Value::Object(r.get_parse_float()), true, false, true)
    });
    t.put_str("parseInt", |r| {
        value_prop(Value::Object(r.get_parse_int()), true, false, true)
    });
    t.put_str("POSITIVE_INFINITY", |_| {
        value_prop(Value::Float(f64::INFINITY), false, false, false)
    });

    t
}

pub fn get_number_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_number_template)
}

impl Runtime {
    pub fn get_number(&mut self) -> ObjRef {
        if let Some(r) = self.global.number {
            return r;
        }
        let ret = self.reserve_object();
        self.global.number = Some(ret);
        let proto = self.get_number_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_number(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_templated_func_object(
            get_number_template(),
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_number(c))),
            ctor,
        );
        ret
    }

    pub fn get_number_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.number_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.number_prototype = Some(ret);
        self.new_templated_object(get_number_proto_template(), Some(ret));
        self.obj_mut(ret).base.class = CLASS_NUMBER;
        ret
    }

    pub fn get_parse_float(&mut self) -> ObjRef {
        if let Some(r) = self.global.parse_float {
            return r;
        }
        let ret = self.new_native_func(
            crate::func::NativeFn::Fn(|r, c| r.builtin_parse_float(c)),
            "parseFloat",
            1,
        );
        self.global.parse_float = Some(ret);
        ret
    }

    pub fn get_parse_int(&mut self) -> ObjRef {
        if let Some(r) = self.global.parse_int {
            return r;
        }
        let ret = self.new_native_func(
            crate::func::NativeFn::Fn(|r, c| r.builtin_parse_int(c)),
            "parseInt",
            2,
        );
        self.global.parse_int = Some(ret);
        ret
    }
}
