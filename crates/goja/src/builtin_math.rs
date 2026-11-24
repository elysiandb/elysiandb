use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::heap::ObjRef;
use crate::ipow::ipow;
use crate::object::{CLASS_MATH, FunctionCall};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw, to_uint32};
use crate::string::ascii_string;
use crate::value::{Value, float_to_value, int_to_value, nil_safe, value_prop};
use gocompat::math as gomath;
use gocompat::math::{frexp, ldexp};
use std::cell::RefCell;
use std::sync::OnceLock;

pub(crate) fn go_mod(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() || x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let y = y.abs();
    let (yfr, yexp) = frexp(y);
    let mut r = if x < 0.0 { -x } else { x };
    while r >= y {
        let (rfr, mut rexp) = frexp(r);
        if rfr < yfr {
            rexp -= 1;
        }
        r -= ldexp(y, rexp - yexp);
    }
    if x < 0.0 {
        r = -r;
    }
    r
}

fn sqrt1_2() -> f64 {
    f64::from_bits(0x3fe6_a09e_667f_3bcd)
}

fn default_rand() -> f64 {
    thread_local! {
        static RNG: RefCell<Option<gocompat::rand::Rand>> = const { RefCell::new(None) };
    }
    RNG.with(|r| {
        let mut r = r.borrow_mut();
        let rng = r.get_or_insert_with(|| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as i64)
                .unwrap_or_default();
            let marker = 0u8;
            let addr = std::ptr::addr_of!(marker) as usize as i64;
            gocompat::rand::Rand::new(gocompat::rand::new_source(nanos ^ addr.rotate_left(17)))
        });
        rng.float64()
    })
}

fn to_float(r: &mut Runtime, v: &Value) -> JsResult<f64> {
    match v {
        Value::BigInt(_) => Err(Throw::Value(
            r.new_type_error("Cannot convert a BigInt value to a number"),
        )),
        Value::Object(o) => {
            let p = o.to_primitive_number(r)?;
            to_float(r, &p)
        }
        _ => v.to_float(r),
    }
}

fn arg_float(r: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<f64> {
    to_float(r, call.arg(i))
}

impl Runtime {
    pub fn math_abs(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::abs(arg_float(self, &call, 0)?)))
    }

    pub fn math_acos(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::acos(arg_float(self, &call, 0)?)))
    }

    pub fn math_acosh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::acosh(arg_float(self, &call, 0)?)))
    }

    pub fn math_asin(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::asin(arg_float(self, &call, 0)?)))
    }

    pub fn math_asinh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::asinh(arg_float(self, &call, 0)?)))
    }

    pub fn math_atan(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::atan(arg_float(self, &call, 0)?)))
    }

    pub fn math_atanh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::atanh(arg_float(self, &call, 0)?)))
    }

    pub fn math_atan2(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let y = arg_float(self, &call, 0)?;
        let x = arg_float(self, &call, 1)?;
        Ok(float_to_value(gomath::atan2(y, x)))
    }

    pub fn math_cbrt(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::cbrt(arg_float(self, &call, 0)?)))
    }

    pub fn math_ceil(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::ceil(arg_float(self, &call, 0)?)))
    }

    pub fn math_clz32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let v = to_uint32(self, call.arg(0))?;
        Ok(int_to_value(i64::from(v.leading_zeros())))
    }

    pub fn math_cos(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::cos(arg_float(self, &call, 0)?)))
    }

    pub fn math_cosh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::cosh(arg_float(self, &call, 0)?)))
    }

    pub fn math_exp(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::exp(arg_float(self, &call, 0)?)))
    }

    pub fn math_expm1(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::expm1(arg_float(self, &call, 0)?)))
    }

    pub fn math_floor(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::floor(arg_float(self, &call, 0)?)))
    }

    pub fn math_fround(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(f64::from(arg_float(self, &call, 0)? as f32)))
    }

    pub fn math_hypot(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut max: f64 = 0.0;
        let mut has_nan = false;
        let mut abs_values = Vec::with_capacity(call.arguments.len());
        for v in call.arguments {
            let arg = to_float(self, &nil_safe(Some(v.clone())))?;
            if arg.is_nan() {
                has_nan = true;
            } else {
                let abs = gomath::abs(arg);
                if abs > max {
                    max = abs;
                }
                abs_values.push(abs);
            }
        }
        if max == f64::INFINITY {
            return Ok(Value::Float(f64::INFINITY));
        }
        if has_nan {
            return Ok(Value::Float(f64::NAN));
        }
        if max == 0.0 {
            return Ok(Value::Int(0));
        }

        let mut sum: f64 = 0.0;
        let mut compensation: f64 = 0.0;
        for n in abs_values {
            let n = n / max;
            let summand = if cfg!(target_arch = "aarch64") {
                n.mul_add(n, -compensation)
            } else {
                n * n - compensation
            };
            let preliminary = sum + summand;
            compensation = (preliminary - sum) - summand;
            sum = preliminary;
        }
        Ok(float_to_value(gomath::sqrt(sum) * max))
    }

    pub fn math_imul(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let x = to_uint32(self, call.arg(0))?;
        let y = to_uint32(self, call.arg(1))?;
        Ok(int_to_value(i64::from(x.wrapping_mul(y) as i32)))
    }

    pub fn math_log(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::log(arg_float(self, &call, 0)?)))
    }

    pub fn math_log1p(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::log1p(arg_float(self, &call, 0)?)))
    }

    pub fn math_log10(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::log10(arg_float(self, &call, 0)?)))
    }

    pub fn math_log2(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::log2(arg_float(self, &call, 0)?)))
    }

    pub fn math_max(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut result = f64::NEG_INFINITY;
        let args = call.arguments;
        for (i, arg) in args.iter().enumerate() {
            let n = to_float(self, &nil_safe(Some(arg.clone())))?;
            if n.is_nan() {
                for arg in &args[i + 1..] {
                    to_float(self, &nil_safe(Some(arg.clone())))?;
                }
                return Ok(Value::Float(f64::NAN));
            }
            result = gomath::max(result, n);
        }
        Ok(float_to_value(result))
    }

    pub fn math_min(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut result = f64::INFINITY;
        let args = call.arguments;
        for (i, arg) in args.iter().enumerate() {
            let n = to_float(self, &nil_safe(Some(arg.clone())))?;
            if n.is_nan() {
                for arg in &args[i + 1..] {
                    to_float(self, &nil_safe(Some(arg.clone())))?;
                }
                return Ok(Value::Float(f64::NAN));
            }
            result = gomath::min(result, n);
        }
        Ok(float_to_value(result))
    }

    pub fn math_pow(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let x = call.argument(0);
        let y = call.argument(1);
        if let (Value::Int(xi), Value::Int(yi)) = (&x, &y)
            && *yi >= 0
        {
            if *yi == 0 {
                return Ok(int_to_value(1));
            }
            if *xi == 0 {
                return Ok(int_to_value(0));
            }
            let ip = ipow(*xi, *yi);
            if ip != 0 {
                return Ok(int_to_value(ip));
            }
        }
        let xf = to_float(self, &x)?;
        let yf = to_float(self, &y)?;
        Ok(pow_float(xf, yf))
    }

    pub fn math_random(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        let f = match &mut self.rand {
            Some(rand) => rand(),
            None => default_rand(),
        };
        Ok(float_to_value(f))
    }

    pub fn math_round(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let f = arg_float(self, &call, 0)?;
        if f.is_nan() {
            return Ok(Value::Float(f64::NAN));
        }

        if f == 0.0 && f.is_sign_negative() {
            return Ok(Value::Float(-0.0));
        }

        let t = gomath::trunc(f);

        if f >= 0.0 {
            if f - t >= 0.5 {
                return Ok(float_to_value(t + 1.0));
            }
        } else if t - f > 0.5 {
            return Ok(float_to_value(t - 1.0));
        }

        Ok(float_to_value(t))
    }

    pub fn math_sign(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        let num = to_float(self, &arg)?;
        if num.is_nan() || num == 0.0 {
            return Ok(arg);
        }
        if num > 0.0 {
            return Ok(int_to_value(1));
        }
        Ok(int_to_value(-1))
    }

    pub fn math_sin(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::sin(arg_float(self, &call, 0)?)))
    }

    pub fn math_sinh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::sinh(arg_float(self, &call, 0)?)))
    }

    pub fn math_sqrt(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::sqrt(arg_float(self, &call, 0)?)))
    }

    pub fn math_tan(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::tan(arg_float(self, &call, 0)?)))
    }

    pub fn math_tanh(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(float_to_value(gomath::tanh(arg_float(self, &call, 0)?)))
    }

    pub fn math_trunc(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        if let Value::Int(_) = arg {
            return Ok(arg);
        }
        Ok(float_to_value(gomath::trunc(to_float(self, &arg)?)))
    }
}

fn pow_float(xf: f64, yf: f64) -> Value {
    if xf.abs() == 1.0 && yf.is_infinite() {
        return Value::Float(f64::NAN);
    }
    if xf == 1.0 && yf.is_nan() {
        return Value::Float(f64::NAN);
    }
    float_to_value(gomath::pow(xf, yf))
}

pub fn pow(x: &Value, y: &Value) -> Value {
    if let (Value::Int(x), Value::Int(y)) = (x, y)
        && *y >= 0
    {
        if *y == 0 {
            return int_to_value(1);
        }
        if *x == 0 {
            return int_to_value(0);
        }
        let ip = ipow(*x, *y);
        if ip != 0 {
            return int_to_value(ip);
        }
    }
    pow_float(x.prim_to_float(), y.prim_to_float())
}

fn const_prop(f: f64) -> Value {
    value_prop(Value::Float(f), false, false, false)
}

fn create_math_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("E", |_| const_prop(std::f64::consts::E));
    t.put_str("LN10", |_| const_prop(std::f64::consts::LN_10));
    t.put_str("LN2", |_| const_prop(std::f64::consts::LN_2));
    t.put_str("LOG10E", |_| const_prop(std::f64::consts::LOG10_E));
    t.put_str("LOG2E", |_| const_prop(std::f64::consts::LOG2_E));
    t.put_str("PI", |_| const_prop(std::f64::consts::PI));
    t.put_str("SQRT1_2", |_| const_prop(sqrt1_2()));
    t.put_str("SQRT2", |_| const_prop(std::f64::consts::SQRT_2));

    t.put_sym(SYM_TO_STRING_TAG, |_| {
        value_prop(Value::String(ascii_string(CLASS_MATH)), false, false, true)
    });

    t.put_str("abs", |r| r.method_prop(|r, c| r.math_abs(c), "abs", 1));
    t.put_str("acos", |r| r.method_prop(|r, c| r.math_acos(c), "acos", 1));
    t.put_str("acosh", |r| {
        r.method_prop(|r, c| r.math_acosh(c), "acosh", 1)
    });
    t.put_str("asin", |r| r.method_prop(|r, c| r.math_asin(c), "asin", 1));
    t.put_str("asinh", |r| {
        r.method_prop(|r, c| r.math_asinh(c), "asinh", 1)
    });
    t.put_str("atan", |r| r.method_prop(|r, c| r.math_atan(c), "atan", 1));
    t.put_str("atanh", |r| {
        r.method_prop(|r, c| r.math_atanh(c), "atanh", 1)
    });
    t.put_str("atan2", |r| {
        r.method_prop(|r, c| r.math_atan2(c), "atan2", 2)
    });
    t.put_str("cbrt", |r| r.method_prop(|r, c| r.math_cbrt(c), "cbrt", 1));
    t.put_str("ceil", |r| r.method_prop(|r, c| r.math_ceil(c), "ceil", 1));
    t.put_str("clz32", |r| {
        r.method_prop(|r, c| r.math_clz32(c), "clz32", 1)
    });
    t.put_str("cos", |r| r.method_prop(|r, c| r.math_cos(c), "cos", 1));
    t.put_str("cosh", |r| r.method_prop(|r, c| r.math_cosh(c), "cosh", 1));
    t.put_str("exp", |r| r.method_prop(|r, c| r.math_exp(c), "exp", 1));
    t.put_str("expm1", |r| {
        r.method_prop(|r, c| r.math_expm1(c), "expm1", 1)
    });
    t.put_str("floor", |r| {
        r.method_prop(|r, c| r.math_floor(c), "floor", 1)
    });
    t.put_str("fround", |r| {
        r.method_prop(|r, c| r.math_fround(c), "fround", 1)
    });
    t.put_str("hypot", |r| {
        r.method_prop(|r, c| r.math_hypot(c), "hypot", 2)
    });
    t.put_str("imul", |r| r.method_prop(|r, c| r.math_imul(c), "imul", 2));
    t.put_str("log", |r| r.method_prop(|r, c| r.math_log(c), "log", 1));
    t.put_str("log1p", |r| {
        r.method_prop(|r, c| r.math_log1p(c), "log1p", 1)
    });
    t.put_str("log10", |r| {
        r.method_prop(|r, c| r.math_log10(c), "log10", 1)
    });
    t.put_str("log2", |r| r.method_prop(|r, c| r.math_log2(c), "log2", 1));
    t.put_str("max", |r| r.method_prop(|r, c| r.math_max(c), "max", 2));
    t.put_str("min", |r| r.method_prop(|r, c| r.math_min(c), "min", 2));
    t.put_str("pow", |r| r.method_prop(|r, c| r.math_pow(c), "pow", 2));
    t.put_str("random", |r| {
        r.method_prop(|r, c| r.math_random(c), "random", 0)
    });
    t.put_str("round", |r| {
        r.method_prop(|r, c| r.math_round(c), "round", 1)
    });
    t.put_str("sign", |r| r.method_prop(|r, c| r.math_sign(c), "sign", 1));
    t.put_str("sin", |r| r.method_prop(|r, c| r.math_sin(c), "sin", 1));
    t.put_str("sinh", |r| r.method_prop(|r, c| r.math_sinh(c), "sinh", 1));
    t.put_str("sqrt", |r| r.method_prop(|r, c| r.math_sqrt(c), "sqrt", 1));
    t.put_str("tan", |r| r.method_prop(|r, c| r.math_tan(c), "tan", 1));
    t.put_str("tanh", |r| r.method_prop(|r, c| r.math_tanh(c), "tanh", 1));
    t.put_str("trunc", |r| {
        r.method_prop(|r, c| r.math_trunc(c), "trunc", 1)
    });

    t
}

pub fn get_math_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_math_template)
}

impl Runtime {
    pub fn get_math(&mut self) -> ObjRef {
        if let Some(r) = self.global.math {
            return r;
        }
        let ret = self.reserve_object();
        self.global.math = Some(ret);
        self.new_templated_object(get_math_template(), Some(ret));
        ret
    }
}
