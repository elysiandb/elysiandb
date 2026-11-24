use crate::heap::ObjRef;
use crate::ipow::ipow;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use crate::value::{Value, float_to_value, int_to_value};
use gocompat::math::{frexp, ldexp};
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
    let xf = x.prim_to_float();
    let yf = y.prim_to_float();
    if xf.abs() == 1.0 && yf.is_infinite() {
        return Value::Float(f64::NAN);
    }
    if xf == 1.0 && yf.is_nan() {
        return Value::Float(f64::NAN);
    }
    float_to_value(gocompat::math::pow(xf, yf))
}

fn create_math_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
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
