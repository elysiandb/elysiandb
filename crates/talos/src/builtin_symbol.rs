use crate::func::{NativeCtor, NativeFn};
use crate::heap::{ObjRef, SymRef};
use crate::object::{BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, FunctionCall, ObjExt};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::{JsString, ascii_string, new_string_value, string_value_from_raw};
use crate::value::{Value, ValueProperty, value_prop};

pub const SYM_HAS_INSTANCE: SymRef = SymRef(0);
pub const SYM_IS_CONCAT_SPREADABLE: SymRef = SymRef(1);
pub const SYM_ITERATOR: SymRef = SymRef(2);
pub const SYM_MATCH: SymRef = SymRef(3);
pub const SYM_MATCH_ALL: SymRef = SymRef(4);
pub const SYM_REPLACE: SymRef = SymRef(5);
pub const SYM_SEARCH: SymRef = SymRef(6);
pub const SYM_SPECIES: SymRef = SymRef(7);
pub const SYM_SPLIT: SymRef = SymRef(8);
pub const SYM_TO_PRIMITIVE: SymRef = SymRef(9);
pub const SYM_TO_STRING_TAG: SymRef = SymRef(10);
pub const SYM_UNSCOPABLES: SymRef = SymRef(11);

pub const WELL_KNOWN_SYMBOLS_COUNT: u32 = 12;

const WELL_KNOWN_DESCS: [&str; WELL_KNOWN_SYMBOLS_COUNT as usize] = [
    "Symbol.hasInstance",
    "Symbol.isConcatSpreadable",
    "Symbol.iterator",
    "Symbol.match",
    "Symbol.matchAll",
    "Symbol.replace",
    "Symbol.search",
    "Symbol.species",
    "Symbol.split",
    "Symbol.toPrimitive",
    "Symbol.toStringTag",
    "Symbol.unscopables",
];

pub fn well_known_symbol_desc(s: SymRef) -> Option<JsString> {
    WELL_KNOWN_DESCS.get(s.0 as usize).map(|d| ascii_string(d))
}

impl Runtime {
    pub fn builtin_symbol(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        let desc = if !arg.is_undefined() {
            Some(arg.to_string(self)?)
        } else {
            None
        };
        Ok(Value::Symbol(self.new_symbol(desc)))
    }

    pub fn symbolproto_tostring(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut sym = match call.this {
            Value::Symbol(s) => Some(*s),
            _ => None,
        };
        if sym.is_none()
            && let Value::Object(obj) = call.this
            && let ObjExt::Primitive(Value::Symbol(s)) = &self.obj(*obj).ext
        {
            sym = Some(*s);
        }
        match sym {
            None => Err(Throw::Value(self.new_type_error(
                "Method Symbol.prototype.toString is called on incompatible receiver",
            ))),
            Some(s) => Ok(Value::String(self.symbol_descriptive_string(s))),
        }
    }

    pub fn symbolproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Symbol(_) = call.this {
            return Ok(call.this.clone());
        }
        if let Value::Object(obj) = call.this
            && let ObjExt::Primitive(Value::Symbol(s)) = &self.obj(*obj).ext
        {
            return Ok(Value::Symbol(*s));
        }
        Err(Throw::Value(self.new_type_error(
            "Symbol.prototype.valueOf requires that 'this' be a Symbol",
        )))
    }

    pub fn symbol_for(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let key = call.argument(0).to_string(self)?;
        let key_str = key.string();
        if let Some(v) = self.symbol_registry.get(&key_str) {
            return Ok(Value::Symbol(*v));
        }
        let v = self.new_symbol(Some(key));
        self.symbol_registry.insert(key_str, v);
        Ok(Value::Symbol(v))
    }

    pub fn symbol_keyfor(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        let Value::Symbol(sym) = arg else {
            let s = arg.go_string(self)?;
            return Err(Throw::Value(
                self.new_type_error(&format!("{s} is not a symbol")),
            ));
        };
        for (key, s) in &self.symbol_registry {
            if *s == sym {
                return Ok(Value::String(string_value_from_raw(key)));
            }
        }
        Ok(Value::Undefined)
    }

    pub fn this_symbol_value(&mut self, v: &Value) -> JsResult<SymRef> {
        if let Value::Symbol(s) = v {
            return Ok(*s);
        }
        if let Value::Object(obj) = v
            && let ObjExt::Primitive(Value::Symbol(s)) = &self.obj(*obj).ext
        {
            return Ok(*s);
        }
        Err(Throw::Value(self.new_type_error("Value is not a Symbol")))
    }

    fn create_symbol_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.set_object_impl(
            val,
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, op),
            ObjExt::None,
        );
        let s = self.get_symbol();
        val._put_prop(
            self,
            &"constructor".into(),
            Value::Object(s),
            true,
            false,
            true,
        );
        let g = self.new_native_func(
            NativeFn::Fn(|r, call| {
                let s = r.this_symbol_value(call.this)?;
                Ok(match r.symbol_desc(s) {
                    Some(d) => Value::String(d),
                    None => Value::Undefined,
                })
            }),
            "get description",
            0,
        );
        let _ = val.set_own_str(
            self,
            &"description".into(),
            Value::Property(Box::new(ValueProperty {
                configurable: true,
                getter_func: Some(g),
                accessor: true,
                ..ValueProperty::default()
            })),
            false,
        );
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.symbolproto_tostring(c)),
            "toString",
            0,
        );
        val._put_prop(
            self,
            &"toString".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.symbolproto_value_of(c)), "valueOf", 0);
        val._put_prop(self, &"valueOf".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.symbolproto_value_of(c)),
            "[Symbol.toPrimitive]",
            1,
        );
        val._put_sym(
            self,
            SYM_TO_PRIMITIVE,
            value_prop(Value::Object(f), false, false, true),
        );
        val._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(new_string_value(b"Symbol")),
                false,
                false,
                true,
            ),
        );
    }

    fn create_symbol(&mut self, val: ObjRef) {
        let proto = self.get_symbol_prototype();
        let o = self.new_native_func_and_construct(
            Some(val),
            Some(NativeFn::Fn(|r, c| r.builtin_symbol(c))),
            Some(NativeCtor::Fn(|r, _args, _nt| {
                Err(Throw::Value(
                    r.new_type_error("Symbol is not a constructor"),
                ))
            })),
            Some(proto),
            "Symbol",
            Value::Int(0),
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.symbol_for(c)), "for", 1);
        o._put_prop(self, &"for".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.symbol_keyfor(c)), "keyFor", 1);
        o._put_prop(self, &"keyFor".into(), Value::Object(f), true, false, true);
        for i in 0..WELL_KNOWN_SYMBOLS_COUNT {
            let n = &WELL_KNOWN_DESCS[i as usize]["Symbol.".len()..];
            o._put_prop(
                self,
                &n.into(),
                Value::Symbol(SymRef(i)),
                false,
                false,
                false,
            );
        }
    }

    pub fn get_symbol_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.symbol_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.symbol_prototype = Some(ret);
        self.create_symbol_proto(ret);
        ret
    }

    pub fn get_symbol(&mut self) -> ObjRef {
        if let Some(r) = self.global.symbol {
            return r;
        }
        let ret = self.reserve_object();
        self.global.symbol = Some(ret);
        self.create_symbol(ret);
        ret
    }
}
