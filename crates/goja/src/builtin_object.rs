use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{
    CLASS_ARRAY, CLASS_OBJECT, Flag, FunctionCall, ObjExt, PropertyDescriptor,
    iterate_enumerable_properties, iterate_enumerable_string_properties, to_flag, to_method,
};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw, create_data_property_or_throw, is_array};
use crate::string::{STRING_OBJECT_NULL, STRING_OBJECT_UNDEFINED, ascii_string, new_string_value};
use crate::value::{Value, ValueProperty, int_to_value, nil_safe, value_prop};
use std::sync::OnceLock;

pub fn to_property_descriptor(r: &mut Runtime, v: &Value) -> JsResult<PropertyDescriptor> {
    let mut ret = PropertyDescriptor::default();
    if let Value::Object(o) = v {
        let descr = *o;
        ret.js_descriptor = Some(descr);
        ret.value = descr.get_str(r, &"value".into(), None)?;
        if let Some(p) = descr.get_str(r, &"writable".into(), None)? {
            ret.writable = to_flag(p.to_boolean());
        }
        if let Some(p) = descr.get_str(r, &"enumerable".into(), None)? {
            ret.enumerable = to_flag(p.to_boolean());
        }
        if let Some(p) = descr.get_str(r, &"configurable".into(), None)? {
            ret.configurable = to_flag(p.to_boolean());
        }
        ret.getter = descr.get_str(r, &"get".into(), None)?;
        ret.setter = descr.get_str(r, &"set".into(), None)?;
        if let Some(g) = &ret.getter
            && !g.is_undefined()
        {
            let go = r.to_object(g)?;
            if go.assert_callable(r).is_none() {
                r.type_error_result(true, "getter must be a function")?;
            }
        }
        if let Some(s) = &ret.setter
            && !s.is_undefined()
        {
            let so = r.to_object(s)?;
            if so.assert_callable(r).is_none() {
                r.type_error_result(true, "setter must be a function")?;
            }
        }
        if (ret.getter.is_some() || ret.setter.is_some())
            && (ret.value.is_some() || ret.writable != Flag::NotSet)
        {
            r.type_error_result(
                true,
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute",
            )?;
        }
    } else {
        let s = v.go_string(r)?;
        r.type_error_result(
            true,
            &format!("Property description must be an object: {s}"),
        )?;
    }
    Ok(ret)
}

fn with_own_value_property(
    r: &mut Runtime,
    o: ObjRef,
    name: &Value,
    f: &dyn Fn(&mut ValueProperty),
) -> bool {
    if crate::array::with_own_value_property(r, o, name, f) {
        return true;
    }
    let obj = r.obj_mut(o);
    let slot = match name {
        Value::Symbol(_) => match &mut obj.base.sym_values {
            Some(m) => m.get_mut(name),
            None => None,
        },
        _ => {
            let key = name.prim_string();
            obj.base.values.get_mut(&key)
        }
    };
    if let Some(Value::Property(p)) = slot {
        f(p);
        return true;
    }
    false
}

impl Runtime {
    pub fn builtin_object(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        if let Some(nt) = new_target {
            let obj_ctor = self.get_object();
            if nt != obj_ctor {
                let op = self.global.object_prototype;
                let proto = self.get_prototype_from_ctor(nt, None, op)?;
                return Ok(self.new_base_object(proto, CLASS_OBJECT));
            }
        }
        if let Some(arg) = args.first()
            && !arg.is_undefined()
            && !arg.is_null()
        {
            return arg.to_object(self);
        }
        Ok(self.new_object())
    }

    pub fn object_get_prototype_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.argument(0).to_object(self)?;
        Ok(match o.proto(self)? {
            None => Value::Null,
            Some(p) => Value::Object(p),
        })
    }

    pub fn value_prop_to_descriptor_object(&mut self, desc: Option<Value>) -> JsResult<Value> {
        let Some(desc) = desc.filter(|d| !d.is_empty()) else {
            return Ok(Value::Undefined);
        };
        let (writable, configurable, enumerable, accessor, value, get, set) = match &desc {
            Value::Property(v) => (
                v.writable,
                v.configurable,
                v.enumerable,
                v.accessor,
                v.value.clone(),
                v.getter_func,
                v.setter_func,
            ),
            _ => (true, true, true, false, desc.clone(), None, None),
        };
        let ret = self.new_object();
        if !accessor {
            ret.set_own_str(self, &"value".into(), value, false)?;
            ret.set_own_str(self, &"writable".into(), Value::Bool(writable), false)?;
        } else {
            let g = get.map(Value::Object).unwrap_or(Value::Undefined);
            ret.set_own_str(self, &"get".into(), g, false)?;
            let s = set.map(Value::Object).unwrap_or(Value::Undefined);
            ret.set_own_str(self, &"set".into(), s, false)?;
        }
        ret.set_own_str(self, &"enumerable".into(), Value::Bool(enumerable), false)?;
        ret.set_own_str(
            self,
            &"configurable".into(),
            Value::Bool(configurable),
            false,
        )?;
        Ok(Value::Object(ret))
    }

    pub fn object_get_own_property_descriptor(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let o = call.argument(0).to_object(self)?;
        let prop_name = call.argument(1).to_property_key(self)?;
        let p = o.get_own_prop(self, &prop_name)?;
        self.value_prop_to_descriptor_object(p)
    }

    pub fn object_get_own_property_descriptors(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let o = call.argument(0).to_object(self)?;
        let op = self.global.object_prototype;
        let result = self.new_base_object(op, CLASS_OBJECT);
        let mut it = o.iterate_keys(self)?;
        while let Some(item) = it.next(self)? {
            let prop = match item.value {
                None => match o.get_own_prop(self, &item.name)? {
                    None => continue,
                    Some(p) => p,
                },
                Some(v) => v,
            };
            let descriptor = self.value_prop_to_descriptor_object(Some(prop))?;
            if !descriptor.is_undefined() {
                create_data_property_or_throw(self, result, &item.name, descriptor)?;
            }
        }
        Ok(Value::Object(result))
    }

    pub fn object_get_own_property_names(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.argument(0).to_object(self)?;
        let keys = obj.string_keys(self, true, Vec::new())?;
        Ok(Value::Object(self.new_array_values(keys)))
    }

    pub fn object_get_own_property_symbols(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.argument(0).to_object(self)?;
        let syms = obj.symbols(self, true, Vec::new())?;
        Ok(Value::Object(self.new_array_values(syms)))
    }

    pub fn to_value_prop(&mut self, v: &Value) -> JsResult<Option<ValueProperty>> {
        if v.is_empty() || v.is_undefined() {
            return Ok(None);
        }
        let obj = self.to_object(v)?;
        let getter = obj.get_str(self, &"get".into(), None)?;
        let setter = obj.get_str(self, &"set".into(), None)?;
        let writable = obj.get_str(self, &"writable".into(), None)?;
        let value = obj.get_str(self, &"value".into(), None)?;
        if (getter.is_some() || setter.is_some()) && (value.is_some() || writable.is_some()) {
            self.type_error_result(
                true,
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute",
            )?;
        }
        let mut ret = ValueProperty::default();
        if let Some(w) = &writable
            && w.to_boolean()
        {
            ret.writable = true;
        }
        if let Some(e) = obj.get_str(self, &"enumerable".into(), None)?
            && e.to_boolean()
        {
            ret.enumerable = true;
        }
        if let Some(c) = obj.get_str(self, &"configurable".into(), None)?
            && c.to_boolean()
        {
            ret.configurable = true;
        }
        ret.value = value.unwrap_or(Value::Empty);
        if let Some(g) = &getter
            && !g.is_undefined()
        {
            let o = self.to_object(g)?;
            if o.assert_callable(self).is_none() {
                self.type_error_result(true, "getter must be a function")?;
            }
            ret.getter_func = Some(o);
        }
        if let Some(s) = &setter
            && !s.is_undefined()
        {
            let o = self.to_object(s)?;
            if o.assert_callable(self).is_none() {
                self.type_error_result(true, "setter must be a function")?;
            }
            ret.setter_func = Some(o);
        }
        if ret.getter_func.is_some() || ret.setter_func.is_some() {
            ret.accessor = true;
        }
        Ok(Some(ret))
    }

    pub fn _define_properties(&mut self, o: ObjRef, p: &Value) -> JsResult<()> {
        let props = p.to_object(self)?;
        let mut list = Vec::new();
        let mut it = iterate_enumerable_properties(self, props)?;
        while let Some(item) = it.next(self)? {
            let v = item.value.unwrap_or(Value::Undefined);
            let prop = to_property_descriptor(self, &v)?;
            list.push((item.name, prop));
        }
        for (name, prop) in list {
            o.define_own_property(self, &name, prop, true)?;
        }
        Ok(())
    }

    pub fn object_create(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut proto = None;
        let arg = call.argument(0);
        if !arg.is_null() {
            if let Value::Object(o) = arg {
                proto = Some(o);
            } else {
                let s = arg.go_string(self)?;
                self.type_error_result(
                    true,
                    &format!("Object prototype may only be an Object or null: {s}"),
                )?;
            }
        }
        let o = self.new_base_object(proto, CLASS_OBJECT);
        let props = call.argument(1);
        if !props.is_undefined() {
            self._define_properties(o, &props)?;
        }
        Ok(Value::Object(o))
    }

    pub fn object_define_property(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(obj) = call.argument(0) {
            let descr = to_property_descriptor(self, &call.argument(2))?;
            let key = call.argument(1).to_property_key(self)?;
            obj.define_own_property(self, &key, descr, true)?;
            return Ok(call.argument(0));
        }
        self.type_error_result(true, "Object.defineProperty called on non-object")?;
        Ok(Value::Empty)
    }

    pub fn object_define_properties(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = self.to_object(&call.argument(0))?;
        self._define_properties(obj, &call.argument(1))?;
        Ok(Value::Object(obj))
    }

    pub fn object_seal(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        if let Value::Object(obj) = arg {
            obj.prevent_extensions(self, true)?;
            let descr = PropertyDescriptor {
                configurable: Flag::False,
                ..PropertyDescriptor::default()
            };
            let mut it = obj.iterate_keys(self)?;
            while let Some(item) = it.next(self)? {
                if let Some(Value::Property(_)) = &item.value {
                    with_own_value_property(self, obj, &item.name, &|p| p.configurable = false);
                } else {
                    obj.define_own_property(self, &item.name, descr.clone(), true)?;
                }
            }
            return Ok(Value::Object(obj));
        }
        Ok(arg)
    }

    pub fn object_freeze(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        if let Value::Object(obj) = arg {
            obj.prevent_extensions(self, true)?;
            let mut it = obj.iterate_keys(self)?;
            while let Some(item) = it.next(self)? {
                if let Some(Value::Property(_)) = &item.value {
                    with_own_value_property(self, obj, &item.name, &|p| {
                        p.configurable = false;
                        if !p.accessor {
                            p.writable = false;
                        }
                    });
                } else {
                    let prop = obj.get_own_prop(self, &item.name)?;
                    let mut descr = PropertyDescriptor {
                        configurable: Flag::False,
                        ..PropertyDescriptor::default()
                    };
                    let is_accessor = matches!(&prop, Some(Value::Property(p)) if p.accessor);
                    if !is_accessor {
                        descr.writable = Flag::False;
                    }
                    obj.define_own_property(self, &item.name, descr, true)?;
                }
            }
            return Ok(Value::Object(obj));
        }
        Ok(arg)
    }

    pub fn object_prevent_extensions(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let arg = call.argument(0);
        if let Value::Object(obj) = arg {
            obj.prevent_extensions(self, true)?;
        }
        Ok(arg)
    }

    fn sealed_or_frozen(&mut self, call: FunctionCall<'_>, frozen: bool) -> JsResult<Value> {
        if let Value::Object(obj) = call.argument(0) {
            if obj.is_extensible(self)? {
                return Ok(Value::Bool(false));
            }
            let mut it = obj.iterate_keys(self)?;
            while let Some(item) = it.next(self)? {
                let prop = match item.value {
                    None => match obj.get_own_prop(self, &item.name)? {
                        None => continue,
                        Some(p) => p,
                    },
                    Some(v) => v,
                };
                if let Value::Property(p) = &prop {
                    if p.configurable || frozen && !p.value.is_empty() && p.writable {
                        return Ok(Value::Bool(false));
                    }
                } else {
                    return Ok(Value::Bool(false));
                }
            }
        }
        Ok(Value::Bool(true))
    }

    pub fn object_is_sealed(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.sealed_or_frozen(call, false)
    }

    pub fn object_is_frozen(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.sealed_or_frozen(call, true)
    }

    pub fn object_is_extensible(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(obj) = call.argument(0) {
            return Ok(Value::Bool(obj.is_extensible(self)?));
        }
        Ok(Value::Bool(false))
    }

    pub fn object_keys(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.argument(0).to_object(self)?;
        let keys = obj.string_keys(self, false, Vec::new())?;
        Ok(Value::Object(self.new_array_values(keys)))
    }

    pub fn object_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.argument(0).to_object(self)?;
        let mut values = Vec::new();
        let mut it = iterate_enumerable_string_properties(self, obj)?;
        while let Some(item) = it.next(self)? {
            let pair = self.new_array_values(vec![item.name, item.value.unwrap_or_default()]);
            values.push(Value::Object(pair));
        }
        Ok(Value::Object(self.new_array_values(values)))
    }

    pub fn object_values(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.argument(0).to_object(self)?;
        let mut values = Vec::new();
        let mut it = iterate_enumerable_string_properties(self, obj)?;
        while let Some(item) = it.next(self)? {
            let v = item.value.unwrap_or_default();
            self.root(&v);
            values.push(v);
        }
        Ok(Value::Object(self.new_array_values(values)))
    }

    pub fn objectproto_has_own_property(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let p = call.argument(0).to_property_key(self)?;
        let o = call.this.to_object(self)?;
        Ok(Value::Bool(o.has_own_property(self, &p)?))
    }

    pub fn objectproto_is_prototype_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(mut v) = call.argument(0) {
            let o = call.this.to_object(self)?;
            loop {
                match v.proto(self)? {
                    None => break,
                    Some(p) => {
                        if p == o {
                            return Ok(Value::Bool(true));
                        }
                        v = p;
                    }
                }
            }
        }
        Ok(Value::Bool(false))
    }

    pub fn objectproto_property_is_enumerable(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let p = call.argument(0).to_property_key(self)?;
        let o = call.this.to_object(self)?;
        let pv = o.get_own_prop(self, &p)?;
        match pv {
            None => Ok(Value::Bool(false)),
            Some(Value::Property(prop)) if !prop.enumerable => Ok(Value::Bool(false)),
            Some(_) => Ok(Value::Bool(true)),
        }
    }

    pub fn objectproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        match call.this {
            Value::Null => Ok(Value::String(STRING_OBJECT_NULL.clone())),
            Value::Undefined => Ok(Value::String(STRING_OBJECT_UNDEFINED.clone())),
            o => {
                let obj = o.to_object(self)?;
                let mut cls_name = if is_array(self, obj)? {
                    CLASS_ARRAY.to_string()
                } else {
                    obj.class_name(self).to_string()
                };
                if let Some(Value::String(s)) = obj.get_sym(self, SYM_TO_STRING_TAG, None)? {
                    cls_name = s.to_rust_string();
                }
                Ok(Value::String(new_string_value(
                    format!("[object {cls_name}]").as_bytes(),
                )))
            }
        }
    }

    pub fn objectproto_to_locale_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let m = self.get_v_str(call.this, &"toString".into())?;
        match to_method(self, m)? {
            Some(to_string) => to_string.call(self, call.this.clone(), &[]),
            None => Err(Throw::TypeError("undefined is not a method".to_string())),
        }
    }

    pub fn objectproto_get_proto(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        Ok(match o.proto(self)? {
            Some(p) => Value::Object(p),
            None => Value::Null,
        })
    }

    pub fn set_object_proto(&mut self, o: &Value, arg: &Value) -> JsResult<()> {
        self.check_object_coercible(o)?;
        let mut proto = None;
        if !arg.is_null() {
            match arg {
                Value::Object(obj) => proto = Some(*obj),
                _ => return Ok(()),
            }
        }
        if let Value::Object(o) = o {
            o.set_proto(self, proto, true)?;
        }
        Ok(())
    }

    pub fn objectproto_set_proto(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.set_object_proto(call.this, &call.argument(0))?;
        Ok(Value::Undefined)
    }

    pub fn objectproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(Value::Object(call.this.to_object(self)?))
    }

    pub fn object_assign(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let to = call.argument(0).to_object(self)?;
        if call.arguments.len() > 1 {
            for arg in &call.arguments[1..] {
                if !arg.is_undefined() && !arg.is_null() {
                    let source = arg.to_object(self)?;
                    let mut it = iterate_enumerable_properties(self, source)?;
                    while let Some(item) = it.next(self)? {
                        to.set_own(self, &item.name, item.value.unwrap_or_default(), true)?;
                    }
                }
            }
        }
        Ok(Value::Object(to))
    }

    pub fn object_is(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(Value::Bool(call.argument(0).same_as(&call.argument(1))))
    }

    pub fn to_proto(&mut self, proto: &Value) -> JsResult<Option<ObjRef>> {
        if !proto.is_null() {
            if let Value::Object(obj) = proto {
                return Ok(Some(*obj));
            }
            let s = proto.go_string(self)?;
            return Err(Throw::Value(self.new_type_error(&format!(
                "Object prototype may only be an Object or null: {s}"
            ))));
        }
        Ok(None)
    }

    pub fn object_set_prototype_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.argument(0);
        self.check_object_coercible(&o)?;
        let proto = self.to_proto(&call.argument(1))?;
        if let Value::Object(obj) = &o {
            obj.set_proto(self, proto, true)?;
        }
        Ok(o)
    }

    pub fn object_from_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.argument(0);
        self.check_object_coercible(&o)?;
        let op = self.global.object_prototype;
        let result = self.new_base_object(op, CLASS_OBJECT);
        let mut iter = self.get_iterator(&o, None)?;
        iter.iterate(self, &mut |r, next_value| {
            let item_obj = r.to_object(&next_value)?;
            let k = nil_safe(item_obj.get_idx(r, 0, None)?);
            let v = nil_safe(item_obj.get_idx(r, 1, None)?);
            let key = k.to_property_key(r)?;
            create_data_property_or_throw(r, result, &key, v)?;
            Ok(())
        })?;
        Ok(Value::Object(result))
    }

    pub fn object_has_own(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.argument(0);
        let obj = o.to_object(self)?;
        let p = call.argument(1).to_property_key(self)?;
        Ok(Value::Bool(obj.has_own_property(self, &p)?))
    }

    pub fn get_object(&mut self) -> ObjRef {
        if let Some(r) = self.global.object {
            return r;
        }
        let ret = self.reserve_object();
        self.global.object = Some(ret);
        self.new_templated_func_object(
            get_object_template(),
            Some(ret),
            Some(NativeFn::Fn(|r, call| {
                Ok(Value::Object(r.builtin_object(call.arguments, None)?))
            })),
            Some(crate::func::NativeCtor::Fn(|r, args, nt| {
                r.builtin_object(args, nt)
            })),
        );
        ret
    }
}

fn create_object_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t.put_str("length", |_| {
        value_prop(int_to_value(1), false, false, true)
    });
    t.put_str("name", |_| {
        value_prop(Value::String(ascii_string("Object")), false, false, true)
    });
    t.put_str("prototype", |r| {
        value_prop(
            r.global
                .object_prototype
                .map(Value::Object)
                .unwrap_or_default(),
            false,
            false,
            false,
        )
    });
    t.put_str("assign", |r| {
        r.method_prop(|r, c| r.object_assign(c), "assign", 2)
    });
    t.put_str("defineProperty", |r| {
        r.method_prop(|r, c| r.object_define_property(c), "defineProperty", 3)
    });
    t.put_str("defineProperties", |r| {
        r.method_prop(|r, c| r.object_define_properties(c), "defineProperties", 2)
    });
    t.put_str("entries", |r| {
        r.method_prop(|r, c| r.object_entries(c), "entries", 1)
    });
    t.put_str("getOwnPropertyDescriptor", |r| {
        r.method_prop(
            |r, c| r.object_get_own_property_descriptor(c),
            "getOwnPropertyDescriptor",
            2,
        )
    });
    t.put_str("getOwnPropertyDescriptors", |r| {
        r.method_prop(
            |r, c| r.object_get_own_property_descriptors(c),
            "getOwnPropertyDescriptors",
            1,
        )
    });
    t.put_str("getPrototypeOf", |r| {
        r.method_prop(|r, c| r.object_get_prototype_of(c), "getPrototypeOf", 1)
    });
    t.put_str("is", |r| r.method_prop(|r, c| r.object_is(c), "is", 2));
    t.put_str("getOwnPropertyNames", |r| {
        r.method_prop(
            |r, c| r.object_get_own_property_names(c),
            "getOwnPropertyNames",
            1,
        )
    });
    t.put_str("getOwnPropertySymbols", |r| {
        r.method_prop(
            |r, c| r.object_get_own_property_symbols(c),
            "getOwnPropertySymbols",
            1,
        )
    });
    t.put_str("create", |r| {
        r.method_prop(|r, c| r.object_create(c), "create", 2)
    });
    t.put_str("seal", |r| {
        r.method_prop(|r, c| r.object_seal(c), "seal", 1)
    });
    t.put_str("freeze", |r| {
        r.method_prop(|r, c| r.object_freeze(c), "freeze", 1)
    });
    t.put_str("preventExtensions", |r| {
        r.method_prop(
            |r, c| r.object_prevent_extensions(c),
            "preventExtensions",
            1,
        )
    });
    t.put_str("isSealed", |r| {
        r.method_prop(|r, c| r.object_is_sealed(c), "isSealed", 1)
    });
    t.put_str("isFrozen", |r| {
        r.method_prop(|r, c| r.object_is_frozen(c), "isFrozen", 1)
    });
    t.put_str("isExtensible", |r| {
        r.method_prop(|r, c| r.object_is_extensible(c), "isExtensible", 1)
    });
    t.put_str("keys", |r| {
        r.method_prop(|r, c| r.object_keys(c), "keys", 1)
    });
    t.put_str("setPrototypeOf", |r| {
        r.method_prop(|r, c| r.object_set_prototype_of(c), "setPrototypeOf", 2)
    });
    t.put_str("values", |r| {
        r.method_prop(|r, c| r.object_values(c), "values", 1)
    });
    t.put_str("fromEntries", |r| {
        r.method_prop(|r, c| r.object_from_entries(c), "fromEntries", 1)
    });
    t.put_str("hasOwn", |r| {
        r.method_prop(|r, c| r.object_has_own(c), "hasOwn", 2)
    });
    t
}

pub fn get_object_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_object_template)
}

pub fn get_object_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_object_proto_template)
}

fn create_object_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.put_str("constructor", |r| {
        let o = r.get_object();
        value_prop(Value::Object(o), true, false, true)
    });
    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.objectproto_to_string(c), "toString", 0)
    });
    t.put_str("toLocaleString", |r| {
        r.method_prop(
            |r, c| r.objectproto_to_locale_string(c),
            "toLocaleString",
            0,
        )
    });
    t.put_str("valueOf", |r| {
        r.method_prop(|r, c| r.objectproto_value_of(c), "valueOf", 0)
    });
    t.put_str("hasOwnProperty", |r| {
        r.method_prop(
            |r, c| r.objectproto_has_own_property(c),
            "hasOwnProperty",
            1,
        )
    });
    t.put_str("isPrototypeOf", |r| {
        r.method_prop(|r, c| r.objectproto_is_prototype_of(c), "isPrototypeOf", 1)
    });
    t.put_str("propertyIsEnumerable", |r| {
        r.method_prop(
            |r, c| r.objectproto_property_is_enumerable(c),
            "propertyIsEnumerable",
            1,
        )
    });
    t.put_str(crate::string::PROTO, |r| {
        let g = r.new_native_func(
            NativeFn::Fn(|r, c| r.objectproto_get_proto(c)),
            "get __proto__",
            0,
        );
        let s = r.new_native_func(
            NativeFn::Fn(|r, c| r.objectproto_set_proto(c)),
            "set __proto__",
            1,
        );
        Value::Property(Box::new(ValueProperty {
            accessor: true,
            getter_func: Some(g),
            setter_func: Some(s),
            configurable: true,
            ..ValueProperty::default()
        }))
    });
    t
}

pub fn object_ext_is_primitive(r: &Runtime, o: ObjRef) -> Option<&Value> {
    match &r.obj(o).ext {
        ObjExt::Primitive(v) => Some(v),
        _ => None,
    }
}
