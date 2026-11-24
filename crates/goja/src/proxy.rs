use crate::builtin_proxy::{ProxyTrapConfig, native_proxy_handler as native};
use crate::heap::{ObjRef, SymRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_FUNCTION, CLASS_OBJECT, CallFn, CtorFn, Flag, FunctionCall,
    IterNextFunc, ObjClass, ObjData, ObjExt, PropIter, PropIterItem, PropNameSet,
    PropertyDescriptor, UStr, to_method,
};
use crate::runtime::{JsResult, Runtime, Throw, to_length};
use crate::string::{JsString, STRING_FUNCTION, STRING_OBJECT_C, string_value_from_raw};
use crate::value::{Value, ValueProperty, int_to_ascii, nil_safe};
use std::rc::Rc;
use std::sync::LazyLock;

pub const CLASS_PROXY: &str = "Proxy";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proxy {
    pub proxy: ObjRef,
}

impl Proxy {
    pub fn revoke(self, rt: &mut Runtime) {
        ProxyObject::revoke(rt, self.proxy);
    }

    pub fn handler(self, rt: &Runtime) -> Option<ObjRef> {
        match &ProxyObject::data(rt, self.proxy).handler {
            Some(ProxyHandler::Js(h)) => Some(*h),
            _ => None,
        }
    }

    pub fn target(self, rt: &Runtime) -> Option<ObjRef> {
        ProxyObject::data(rt, self.proxy).target
    }

    pub fn to_value(self) -> Value {
        Value::Object(self.proxy)
    }
}

struct ProxyPropIter {
    names: Vec<Value>,
    idx: usize,
}

impl PropIter for ProxyPropIter {
    fn next(&mut self, _rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.idx < self.names.len() {
            let name = self.names[self.idx].clone();
            self.idx += 1;
            return Ok(Some(PropIterItem {
                name,
                ..PropIterItem::default()
            }));
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.values(&self.names);
    }
}

impl Runtime {
    pub fn new_proxy_object(
        &mut self,
        target: ObjRef,
        handler: ObjRef,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        self._new_proxy_object(target, ProxyHandler::Js(handler), proto)
    }

    pub fn _new_proxy_object(
        &mut self,
        target: ObjRef,
        handler: ProxyHandler,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let prototype = match proto {
            None => self.global.object_prototype,
            Some(p) => Some(p),
        };
        let mut base = BaseObject::new(CLASS_PROXY, prototype);
        base.extensible = false;
        let call = target.assert_callable(self);
        let ctor = target.assert_constructor(self);
        self.alloc_object(
            &PROXY_OBJECT_CLASS,
            base,
            ObjExt::Dyn(Box::new(ProxyObject {
                target: Some(target),
                handler: Some(handler),
                call,
                ctor,
            })),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyTrap {
    GetPrototypeOf,
    SetPrototypeOf,
    IsExtensible,
    PreventExtensions,
    GetOwnPropertyDescriptor,
    DefineProperty,
    Has,
    Get,
    Set,
    DeleteProperty,
    OwnKeys,
    Apply,
    Construct,
}

static TRAP_NAMES: LazyLock<[UStr; 13]> = LazyLock::new(|| {
    [
        "getPrototypeOf",
        "setPrototypeOf",
        "isExtensible",
        "preventExtensions",
        "getOwnPropertyDescriptor",
        "defineProperty",
        "has",
        "get",
        "set",
        "deleteProperty",
        "ownKeys",
        "apply",
        "construct",
    ]
    .map(UStr::new)
});

impl ProxyTrap {
    pub fn string(self) -> &'static UStr {
        &TRAP_NAMES[self as usize]
    }
}

#[derive(Clone, Copy)]
pub enum PropKey<'a> {
    Str(&'a UStr),
    Idx(i64),
    Sym(SymRef),
}

impl PropKey<'_> {
    pub fn to_value(self) -> Value {
        match self {
            PropKey::Str(s) => Value::String(string_value_from_raw(s)),
            PropKey::Idx(i) => Value::String(JsString::Ascii(int_to_ascii(i))),
            PropKey::Sym(s) => Value::Symbol(s),
        }
    }

    pub fn string(self, rt: &Runtime) -> String {
        match self {
            PropKey::Str(s) => s.string(),
            PropKey::Idx(i) => i.to_string(),
            PropKey::Sym(s) => match rt.symbol_desc(s) {
                Some(d) => d.to_rust_string(),
                None => String::new(),
            },
        }
    }

    fn get_own_prop(self, rt: &mut Runtime, o: ObjRef) -> JsResult<Option<Value>> {
        let v = match self {
            PropKey::Str(s) => o.get_own_prop_str(rt, s)?,
            PropKey::Idx(i) => o.get_own_prop_idx(rt, i)?,
            PropKey::Sym(s) => o.get_own_prop_sym(rt, s)?,
        };
        Ok(v.filter(|v| !v.is_empty()))
    }
}

#[derive(Clone)]
pub enum ProxyHandler {
    Js(ObjRef),
    Native(Rc<ProxyTrapConfig>),
}

impl ProxyHandler {
    fn trace(&self, t: &mut Tracer) {
        match self {
            ProxyHandler::Js(h) => t.obj(*h),
            ProxyHandler::Native(c) => t.values(&c.data),
        }
    }

    fn proxy_call(
        rt: &mut Runtime,
        handler: ObjRef,
        trap: ProxyTrap,
        args: &[Value],
    ) -> JsResult<Option<Value>> {
        let m = rt.get_v_str(&Value::Object(handler), trap.string())?;
        if let Some(m) = to_method(rt, m)? {
            return Ok(Some(m.call(rt, Value::Object(handler), args)?));
        }
        Ok(None)
    }

    fn bool_proxy_call(
        rt: &mut Runtime,
        handler: ObjRef,
        trap: ProxyTrap,
        args: &[Value],
    ) -> JsResult<Option<bool>> {
        Ok(ProxyHandler::proxy_call(rt, handler, trap, args)?.map(|v| v.to_boolean()))
    }

    pub fn get_prototype_of(&self, rt: &mut Runtime, target: ObjRef) -> JsResult<Option<Value>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::proxy_call(
                rt,
                *h,
                ProxyTrap::GetPrototypeOf,
                &[Value::Object(target)],
            ),
            ProxyHandler::Native(c) => native::get_prototype_of(rt, c, target),
        }
    }

    pub fn set_prototype_of(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        proto: Option<ObjRef>,
    ) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => {
                let proto_val = match proto {
                    Some(p) => Value::Object(p),
                    None => Value::Null,
                };
                ProxyHandler::bool_proxy_call(
                    rt,
                    *h,
                    ProxyTrap::SetPrototypeOf,
                    &[Value::Object(target), proto_val],
                )
            }
            ProxyHandler::Native(c) => native::set_prototype_of(rt, c, target, proto),
        }
    }

    pub fn is_extensible(&self, rt: &mut Runtime, target: ObjRef) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::bool_proxy_call(
                rt,
                *h,
                ProxyTrap::IsExtensible,
                &[Value::Object(target)],
            ),
            ProxyHandler::Native(c) => native::is_extensible(rt, c, target),
        }
    }

    pub fn prevent_extensions(&self, rt: &mut Runtime, target: ObjRef) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::bool_proxy_call(
                rt,
                *h,
                ProxyTrap::PreventExtensions,
                &[Value::Object(target)],
            ),
            ProxyHandler::Native(c) => native::prevent_extensions(rt, c, target),
        }
    }

    pub fn get_own_property_descriptor(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<Value>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::proxy_call(
                rt,
                *h,
                ProxyTrap::GetOwnPropertyDescriptor,
                &[Value::Object(target), prop.to_value()],
            ),
            ProxyHandler::Native(c) => native::get_own_property_descriptor(rt, c, target, prop),
        }
    }

    pub fn define_property(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
        desc: &PropertyDescriptor,
    ) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => {
                let d = desc.to_value(rt);
                with_root(rt, &d, |rt| {
                    ProxyHandler::bool_proxy_call(
                        rt,
                        *h,
                        ProxyTrap::DefineProperty,
                        &[Value::Object(target), prop.to_value(), d.clone()],
                    )
                })
            }
            ProxyHandler::Native(c) => native::define_property(rt, c, target, prop, desc),
        }
    }

    pub fn has(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::bool_proxy_call(
                rt,
                *h,
                ProxyTrap::Has,
                &[Value::Object(target), prop.to_value()],
            ),
            ProxyHandler::Native(c) => native::has(rt, c, target, prop),
        }
    }

    pub fn get(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
        receiver: &Value,
    ) -> JsResult<Option<Value>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::proxy_call(
                rt,
                *h,
                ProxyTrap::Get,
                &[Value::Object(target), prop.to_value(), receiver.clone()],
            ),
            ProxyHandler::Native(c) => native::get(rt, c, target, prop, receiver),
        }
    }

    pub fn set(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
        value: &Value,
        receiver: &Value,
    ) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::bool_proxy_call(
                rt,
                *h,
                ProxyTrap::Set,
                &[
                    Value::Object(target),
                    prop.to_value(),
                    value.clone(),
                    receiver.clone(),
                ],
            ),
            ProxyHandler::Native(c) => native::set(rt, c, target, prop, value, receiver),
        }
    }

    pub fn delete(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<bool>> {
        match self {
            ProxyHandler::Js(h) => ProxyHandler::bool_proxy_call(
                rt,
                *h,
                ProxyTrap::DeleteProperty,
                &[Value::Object(target), prop.to_value()],
            ),
            ProxyHandler::Native(c) => native::delete(rt, c, target, prop),
        }
    }

    pub fn own_keys(&self, rt: &mut Runtime, target: ObjRef) -> JsResult<Option<ObjRef>> {
        match self {
            ProxyHandler::Js(h) => {
                match ProxyHandler::proxy_call(
                    rt,
                    *h,
                    ProxyTrap::OwnKeys,
                    &[Value::Object(target)],
                )? {
                    Some(v) => Ok(Some(rt.to_object(&v)?)),
                    None => Ok(None),
                }
            }
            ProxyHandler::Native(c) => native::own_keys(rt, c, target),
        }
    }

    pub fn apply(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        this: &Value,
        args: &[Value],
    ) -> JsResult<Option<Value>> {
        match self {
            ProxyHandler::Js(h) => {
                let a = Value::Object(rt.new_array_values(args.to_vec()));
                with_root(rt, &a, |rt| {
                    ProxyHandler::proxy_call(
                        rt,
                        *h,
                        ProxyTrap::Apply,
                        &[Value::Object(target), this.clone(), a.clone()],
                    )
                })
            }
            ProxyHandler::Native(c) => native::apply(rt, c, target, this, args),
        }
    }

    pub fn construct(
        &self,
        rt: &mut Runtime,
        target: ObjRef,
        args: &[Value],
        new_target: ObjRef,
    ) -> JsResult<Option<Value>> {
        match self {
            ProxyHandler::Js(h) => {
                let a = Value::Object(rt.new_array_values(args.to_vec()));
                with_root(rt, &a, |rt| {
                    ProxyHandler::proxy_call(
                        rt,
                        *h,
                        ProxyTrap::Construct,
                        &[Value::Object(target), a.clone(), Value::Object(new_target)],
                    )
                })
            }
            ProxyHandler::Native(c) => native::construct(rt, c, target, args, new_target),
        }
    }
}

pub struct ProxyObject {
    pub target: Option<ObjRef>,
    pub handler: Option<ProxyHandler>,
    pub call: Option<CallFn>,
    pub ctor: Option<CtorFn>,
}

impl ObjData for ProxyObject {
    fn trace(&self, t: &mut Tracer) {
        t.opt_obj(self.target);
        if let Some(h) = &self.handler {
            h.trace(t);
        }
        if let Some(c) = &self.call {
            t.obj(c.obj);
        }
        if let Some(c) = &self.ctor {
            t.obj(c.obj);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn prop_to_value_prop(v: Option<&Value>) -> Option<ValueProperty> {
    match v {
        None => None,
        Some(Value::Property(p)) => Some((**p).clone()),
        Some(v) => Some(ValueProperty {
            value: v.clone(),
            writable: true,
            configurable: true,
            enumerable: true,
            ..ValueProperty::default()
        }),
    }
}

fn same_obj(rt: &Runtime, a: Option<ObjRef>, b: Option<ObjRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b || a.equal(rt, b),
        _ => false,
    }
}

fn same_value_obj(rt: &Runtime, v: &Value, o: Option<ObjRef>) -> bool {
    match (v, o) {
        (Value::Object(a), Some(b)) => *a == b || a.equal(rt, b),
        _ => false,
    }
}

fn with_root<T>(
    rt: &mut Runtime,
    v: &Value,
    f: impl FnOnce(&mut Runtime) -> JsResult<T>,
) -> JsResult<T> {
    if !matches!(v, Value::Object(_) | Value::Symbol(_) | Value::Property(_)) {
        return f(rt);
    }
    let mark = rt.extra_roots.len();
    rt.extra_roots.push(v.clone());
    let r = f(rt);
    rt.extra_roots.truncate(mark);
    r
}

fn type_error<T>(rt: &mut Runtime, msg: &str) -> JsResult<T> {
    Err(Throw::Value(rt.new_type_error(msg)))
}

impl ProxyObject {
    pub fn data(rt: &Runtime, o: ObjRef) -> &ProxyObject {
        match rt.ext_dyn::<ProxyObject>(o) {
            Some(p) => p,
            None => unreachable!("not a proxy object"),
        }
    }

    pub fn try_data(rt: &Runtime, o: ObjRef) -> Option<&ProxyObject> {
        rt.ext_dyn::<ProxyObject>(o)
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut ProxyObject {
        match rt.ext_dyn_mut::<ProxyObject>(o) {
            Some(p) => p,
            None => unreachable!("not a proxy object"),
        }
    }

    fn check_handler(rt: &mut Runtime, o: ObjRef) -> JsResult<(ObjRef, ProxyHandler)> {
        let d = ProxyObject::data(rt, o);
        if let (Some(h), Some(t)) = (&d.handler, d.target) {
            return Ok((t, h.clone()));
        }
        type_error(rt, "Proxy already revoked")
    }

    pub fn proto(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<ObjRef>> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.get_prototype_of(rt, target)? {
            let mut handler_proto = None;
            if !v.is_null() {
                handler_proto = Some(rt.to_object(&v)?);
            }
            return with_root(rt, &v, |rt| {
                if !target.is_extensible(rt)? {
                    let tp = target.proto(rt)?;
                    if !same_obj(rt, handler_proto, tp) {
                        return type_error(
                            rt,
                            "'getPrototypeOf' on proxy: proxy target is non-extensible but the trap did not return its actual prototype",
                        );
                    }
                }
                Ok(handler_proto)
            });
        }
        target.proto(rt)
    }

    pub fn set_proto(
        rt: &mut Runtime,
        o: ObjRef,
        proto: Option<ObjRef>,
        throw: bool,
    ) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.set_prototype_of(rt, target, proto)? {
            if v {
                if !target.is_extensible(rt)? {
                    let tp = target.proto(rt)?;
                    if !same_obj(rt, proto, tp) {
                        return type_error(
                            rt,
                            "'setPrototypeOf' on proxy: trap returned truish for setting a new prototype on the non-extensible proxy target",
                        );
                    }
                }
                return Ok(true);
            }
            rt.type_error_result(throw, "'setPrototypeOf' on proxy: trap returned falsish")?;
            return Ok(false);
        }
        target.set_proto(rt, proto, throw)
    }

    pub fn is_extensible(rt: &mut Runtime, o: ObjRef) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(boolean_trap_result) = h.is_extensible(rt, target)? {
            let te = target.is_extensible(rt)?;
            if boolean_trap_result != te {
                return type_error(
                    rt,
                    &format!(
                        "'isExtensible' on proxy: trap result does not reflect extensibility of proxy target (which is '{te}')"
                    ),
                );
            }
            return Ok(boolean_trap_result);
        }
        target.is_extensible(rt)
    }

    pub fn prevent_extensions(rt: &mut Runtime, o: ObjRef, throw: bool) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(boolean_trap_result) = h.prevent_extensions(rt, target)? {
            if !boolean_trap_result {
                rt.type_error_result(throw, "'preventExtensions' on proxy: trap returned falsish")?;
                return Ok(false);
            }
            if target.is_extensible(rt)? {
                return type_error(
                    rt,
                    "'preventExtensions' on proxy: trap returned truish but the proxy target is extensible",
                );
            }
        }
        target.prevent_extensions(rt, throw)
    }

    fn proxy_define_own_property_pre_check(
        rt: &mut Runtime,
        trap_result: bool,
        throw: bool,
    ) -> JsResult<bool> {
        if !trap_result {
            rt.type_error_result(throw, "'defineProperty' on proxy: trap returned falsish")?;
            return Ok(false);
        }
        Ok(true)
    }

    fn proxy_define_own_property_post_check(
        rt: &mut Runtime,
        prop: Option<Value>,
        target: ObjRef,
        descr: &PropertyDescriptor,
    ) -> JsResult<()> {
        let target_desc = prop_to_value_prop(prop.as_ref());
        let extensible_target = target.is_extensible(rt)?;
        let setting_config_false = descr.configurable == Flag::False;
        match &target_desc {
            None => {
                if !extensible_target {
                    return type_error(rt, "");
                }
                if setting_config_false {
                    return type_error(rt, "");
                }
            }
            Some(td) => {
                if !ProxyObject::__is_compatible_descriptor(rt, extensible_target, descr, Some(td))
                {
                    return type_error(rt, "");
                }
                if setting_config_false && td.configurable {
                    return type_error(rt, "");
                }
                if !td.value.is_empty()
                    && !td.configurable
                    && td.writable
                    && descr.writable == Flag::False
                {
                    return type_error(rt, "");
                }
            }
        }
        Ok(())
    }

    fn define_own_property(
        rt: &mut Runtime,
        o: ObjRef,
        name: PropKey<'_>,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(boolean_trap_result) = h.define_property(rt, target, name, &descr)? {
            if !ProxyObject::proxy_define_own_property_pre_check(rt, boolean_trap_result, throw)? {
                return Ok(false);
            }
            let prop = name.get_own_prop(rt, target)?;
            ProxyObject::proxy_define_own_property_post_check(rt, prop, target, &descr)?;
            return Ok(true);
        }
        match name {
            PropKey::Str(s) => target.define_own_property_str(rt, s, descr, throw),
            PropKey::Idx(i) => target.define_own_property_idx(rt, i, descr, throw),
            PropKey::Sym(s) => target.define_own_property_sym(rt, s, descr, throw),
        }
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::define_own_property(rt, o, PropKey::Str(name), descr, throw)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::define_own_property(rt, o, PropKey::Idx(idx), descr, throw)
    }

    pub fn define_own_property_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::define_own_property(rt, o, PropKey::Sym(s), descr, throw)
    }

    fn proxy_has_checks(
        rt: &mut Runtime,
        target_prop: Option<Value>,
        target: ObjRef,
        name: PropKey<'_>,
    ) -> JsResult<()> {
        if let Some(target_desc) = prop_to_value_prop(target_prop.as_ref()) {
            if !target_desc.configurable {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'has' on proxy: trap returned falsish for property '{n}' which exists in the proxy target as non-configurable"
                    ),
                );
            }
            if !target.is_extensible(rt)? {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'has' on proxy: trap returned falsish for property '{n}' but the proxy target is not extensible"
                    ),
                );
            }
        }
        Ok(())
    }

    fn has_property(rt: &mut Runtime, o: ObjRef, name: PropKey<'_>) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(b) = h.has(rt, target, name)? {
            if !b {
                let tp = name.get_own_prop(rt, target)?;
                ProxyObject::proxy_has_checks(rt, tp, target, name)?;
            }
            return Ok(b);
        }
        match name {
            PropKey::Str(s) => target.has_property_str(rt, s),
            PropKey::Idx(i) => target.has_property_idx(rt, i),
            PropKey::Sym(s) => target.has_property_sym(rt, s),
        }
    }

    pub fn has_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        ProxyObject::has_property(rt, o, PropKey::Str(name))
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        ProxyObject::has_property(rt, o, PropKey::Idx(idx))
    }

    pub fn has_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        ProxyObject::has_property(rt, o, PropKey::Sym(s))
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        Ok(ProxyObject::get_own_prop_str(rt, o, name)?.is_some())
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok(ProxyObject::get_own_prop_idx(rt, o, idx)?.is_some())
    }

    pub fn has_own_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        Ok(ProxyObject::get_own_prop_sym(rt, o, s)?.is_some())
    }

    fn proxy_get_own_property_descriptor(
        rt: &mut Runtime,
        target_prop: Option<Value>,
        target: ObjRef,
        trap_result: Value,
        name: PropKey<'_>,
    ) -> JsResult<Option<Value>> {
        let target_desc = prop_to_value_prop(target_prop.as_ref());
        let mut trap_result_obj = None;
        if !trap_result.is_undefined() && !trap_result.is_empty() {
            if let Value::Object(obj) = &trap_result {
                trap_result_obj = Some(*obj);
            } else {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'getOwnPropertyDescriptor' on proxy: trap returned neither object nor undefined for property '{n}'"
                    ),
                );
            }
        }
        let Some(trap_result_obj) = trap_result_obj else {
            let Some(td) = &target_desc else {
                return Ok(None);
            };
            if !td.configurable {
                return type_error(rt, "");
            }
            if !target.is_extensible(rt)? {
                return type_error(rt, "");
            }
            return Ok(None);
        };
        let extensible_target = target.is_extensible(rt)?;
        let mut result_desc = rt.to_property_descriptor(&Value::Object(trap_result_obj))?;
        result_desc.complete();
        if !ProxyObject::__is_compatible_descriptor(
            rt,
            extensible_target,
            &result_desc,
            target_desc.as_ref(),
        ) {
            let n = name.string(rt);
            return type_error(
                rt,
                &format!(
                    "'getOwnPropertyDescriptor' on proxy: trap returned descriptor for property '{n}' that is incompatible with the existing property in the proxy target"
                ),
            );
        }
        if result_desc.configurable == Flag::False {
            let Some(td) = &target_desc else {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property '{n}' which is non-existent in the proxy target"
                    ),
                );
            };
            if td.configurable {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property '{n}' which is configurable in the proxy target"
                    ),
                );
            }
            if result_desc.writable == Flag::False && td.writable {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'getOwnPropertyDescriptor' on proxy: trap reported non-configurable and writable for property '{n}' which is non-configurable, non-writable in the proxy target"
                    ),
                );
            }
        }
        if result_desc.writable == Flag::True
            && result_desc.configurable == Flag::True
            && result_desc.enumerable == Flag::True
        {
            return Ok(result_desc.value);
        }
        Ok(rt
            .to_value_prop(&Value::Object(trap_result_obj))?
            .map(|p| Value::Property(Box::new(p))))
    }

    fn get_own_prop(rt: &mut Runtime, o: ObjRef, name: PropKey<'_>) -> JsResult<Option<Value>> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.get_own_property_descriptor(rt, target, name)? {
            return with_root(rt, &v, |rt| {
                let tp = name.get_own_prop(rt, target)?;
                ProxyObject::proxy_get_own_property_descriptor(rt, tp, target, v.clone(), name)
            });
        }
        name.get_own_prop(rt, target)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        ProxyObject::get_own_prop(rt, o, PropKey::Str(name))
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        ProxyObject::get_own_prop(rt, o, PropKey::Idx(idx))
    }

    pub fn get_own_prop_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<Option<Value>> {
        ProxyObject::get_own_prop(rt, o, PropKey::Sym(s))
    }

    fn proxy_get_checks(
        rt: &mut Runtime,
        target_prop: Option<Value>,
        trap_result: &Value,
        name: PropKey<'_>,
    ) -> JsResult<()> {
        if let Some(Value::Property(target_desc)) = &target_prop {
            if !target_desc.accessor {
                if !target_desc.writable
                    && !target_desc.configurable
                    && !trap_result.same_as(&target_desc.value)
                {
                    let n = name.string(rt);
                    let expected =
                        nil_safe(Some(target_desc.value.clone()).filter(|v| !v.is_empty()))
                            .go_string(rt)?;
                    return type_error(
                        rt,
                        &format!(
                            "'get' on proxy: property '{n}' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected '{expected}' but got '{{}}')"
                        ),
                    );
                }
            } else if !target_desc.configurable
                && target_desc.getter_func.is_none()
                && !trap_result.is_undefined()
            {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'get' on proxy: property '{n}' is a non-configurable accessor property on the proxy target and does not have a getter function, but the trap did not return 'undefined' (got '{{}}')"
                    ),
                );
            }
        }
        Ok(())
    }

    fn get(
        rt: &mut Runtime,
        o: ObjRef,
        name: PropKey<'_>,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        let own = Value::Object(o);
        let receiver = receiver.unwrap_or(&own);
        if let Some(v) = h.get(rt, target, name, receiver)? {
            return with_root(rt, &v, |rt| {
                let tp = name.get_own_prop(rt, target)?;
                ProxyObject::proxy_get_checks(rt, tp, &v, name)?;
                Ok(Some(v.clone()))
            });
        }
        match name {
            PropKey::Str(s) => target.get_str(rt, s, Some(receiver)),
            PropKey::Idx(i) => target.get_idx(rt, i, Some(receiver)),
            PropKey::Sym(s) => target.get_sym(rt, s, Some(receiver)),
        }
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        ProxyObject::get(rt, o, PropKey::Str(name), receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        ProxyObject::get(rt, o, PropKey::Idx(idx), receiver)
    }

    pub fn get_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        ProxyObject::get(rt, o, PropKey::Sym(s), receiver)
    }

    fn proxy_set_pre_check(
        rt: &mut Runtime,
        trap_result: bool,
        throw: bool,
        name: PropKey<'_>,
    ) -> JsResult<bool> {
        if !trap_result && throw {
            let n = name.string(rt);
            return type_error(
                rt,
                &format!("'set' on proxy: trap returned falsish for property '{n}'"),
            );
        }
        Ok(trap_result)
    }

    fn proxy_set_post_check(
        rt: &mut Runtime,
        target_prop: Option<Value>,
        value: &Value,
        name: PropKey<'_>,
    ) -> JsResult<()> {
        if let Some(Value::Property(prop)) = &target_prop {
            if prop.accessor {
                if !prop.configurable && prop.setter_func.is_none() {
                    let n = name.string(rt);
                    return type_error(
                        rt,
                        &format!(
                            "'set' on proxy: trap returned truish for property '{n}' which exists in the proxy target as a non-configurable and non-writable accessor property without a setter"
                        ),
                    );
                }
            } else if !prop.configurable
                && !prop.writable
                && (prop.value.is_empty() || !prop.value.same_as(value))
            {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'set' on proxy: trap returned truish for property '{n}' which exists in the proxy target as a non-configurable and non-writable data property with a different value"
                    ),
                );
            }
        }
        Ok(())
    }

    fn proxy_set(
        rt: &mut Runtime,
        o: ObjRef,
        name: PropKey<'_>,
        value: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.set(rt, target, name, &value, receiver)? {
            if ProxyObject::proxy_set_pre_check(rt, v, throw, name)? {
                let tp = name.get_own_prop(rt, target)?;
                ProxyObject::proxy_set_post_check(rt, tp, &value, name)?;
                return Ok(true);
            }
            return Ok(false);
        }
        match name {
            PropKey::Str(s) => target.set_str(rt, s, value, receiver, throw),
            PropKey::Idx(i) => target.set_idx(rt, i, value, receiver, throw),
            PropKey::Sym(s) => target.set_sym(rt, s, value, receiver, throw),
        }
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::proxy_set(rt, o, PropKey::Str(name), v, &Value::Object(o), throw)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::proxy_set(rt, o, PropKey::Idx(idx), v, &Value::Object(o), throw)
    }

    pub fn set_own_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        ProxyObject::proxy_set(rt, o, PropKey::Sym(s), v, &Value::Object(o), throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        Ok((
            ProxyObject::proxy_set(rt, o, PropKey::Str(name), v, receiver, throw)?,
            true,
        ))
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        Ok((
            ProxyObject::proxy_set(rt, o, PropKey::Idx(idx), v, receiver, throw)?,
            true,
        ))
    }

    pub fn set_foreign_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        Ok((
            ProxyObject::proxy_set(rt, o, PropKey::Sym(s), v, receiver, throw)?,
            true,
        ))
    }

    fn proxy_delete_check(
        rt: &mut Runtime,
        trap_result: bool,
        target_prop: Option<Value>,
        name: PropKey<'_>,
        target: ObjRef,
        throw: bool,
    ) -> JsResult<()> {
        if trap_result {
            let Some(tp) = target_prop else {
                return Ok(());
            };
            if let Value::Property(target_desc) = &tp
                && !target_desc.configurable
            {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'deleteProperty' on proxy: property '{n}' is a non-configurable property but the trap returned truish"
                    ),
                );
            }
            if !target.is_extensible(rt)? {
                let n = name.string(rt);
                return type_error(
                    rt,
                    &format!(
                        "'deleteProperty' on proxy: trap returned truish for property '{n}' but the proxy target is non-extensible"
                    ),
                );
            }
        } else if throw {
            let n = name.string(rt);
            return type_error(
                rt,
                &format!("'deleteProperty' on proxy: trap returned falsish for property '{n}'"),
            );
        }
        Ok(())
    }

    fn delete(rt: &mut Runtime, o: ObjRef, name: PropKey<'_>, throw: bool) -> JsResult<bool> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.delete(rt, target, name)? {
            let tp = name.get_own_prop(rt, target)?;
            ProxyObject::proxy_delete_check(rt, v, tp, name, target, throw)?;
            return Ok(v);
        }
        match name {
            PropKey::Str(s) => target.delete_str(rt, s, throw),
            PropKey::Idx(i) => target.delete_idx(rt, i, throw),
            PropKey::Sym(s) => target.delete_sym(rt, s, throw),
        }
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        ProxyObject::delete(rt, o, PropKey::Str(name), throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        ProxyObject::delete(rt, o, PropKey::Idx(idx), throw)
    }

    pub fn delete_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, throw: bool) -> JsResult<bool> {
        ProxyObject::delete(rt, o, PropKey::Sym(s), throw)
    }

    pub fn keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        _accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        if let Some(mut v) = ProxyObject::proxy_own_keys(rt, o)? {
            if !all {
                let mark = rt.extra_roots.len();
                rt.extra_roots
                    .extend(v.iter().filter(|k| matches!(k, Value::Symbol(_))).cloned());
                let r = ProxyObject::retain_enumerable(rt, o, &mut v);
                rt.extra_roots.truncate(mark);
                r?;
            }
            return Ok(v);
        }
        let target = ProxyObject::data(rt, o).target;
        match target {
            Some(t) => t.keys(rt, all, Vec::new()),
            None => type_error(rt, "Proxy already revoked"),
        }
    }

    fn retain_enumerable(rt: &mut Runtime, o: ObjRef, v: &mut Vec<Value>) -> JsResult<()> {
        let mut k = 0;
        for i in 0..v.len() {
            let key = v[i].clone();
            let prop = o.get_own_prop(rt, &key)?;
            match &prop {
                None | Some(Value::Undefined) | Some(Value::Empty) => continue,
                Some(Value::Property(p)) if !p.enumerable => continue,
                _ => {}
            }
            if k != i {
                v[k] = v[i].clone();
            }
            k += 1;
        }
        v.truncate(k);
        Ok(())
    }

    fn proxy_own_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<Vec<Value>>> {
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        let Some(keys) = h.own_keys(rt, target)? else {
            return Ok(None);
        };
        let mut key_list: Vec<Value> = Vec::new();
        let mut key_set = PropNameSet::default();
        let mark = rt.extra_roots.len();
        rt.extra_roots.push(Value::Object(keys));
        let r = ProxyObject::collect_own_keys(rt, target, keys, &mut key_list, &mut key_set);
        rt.extra_roots.truncate(mark);
        r?;
        Ok(Some(key_list))
    }

    fn collect_own_keys(
        rt: &mut Runtime,
        target: ObjRef,
        keys: ObjRef,
        key_list: &mut Vec<Value>,
        key_set: &mut PropNameSet,
    ) -> JsResult<()> {
        let lv = nil_safe(keys.get_str(rt, &"length".into(), None)?);
        let l = to_length(rt, &lv)?;
        for k in 0..l {
            let item = keys.get_idx(rt, k, None)?;
            let item = match item {
                Some(v) if !v.is_empty() => v,
                _ => panic!("internal error"),
            };
            if !matches!(item, Value::String(_) | Value::Symbol(_)) {
                let s = item.go_string(rt)?;
                return type_error(rt, &format!("{s} is not a valid property name"));
            }
            if key_set.has(rt, &item)? {
                return type_error(rt, "'ownKeys' on proxy: trap returned duplicate entries");
            }
            if let Value::Symbol(_) = &item {
                rt.extra_roots.push(item.clone());
            }
            key_set.add(rt, &item)?;
            key_list.push(item);
        }
        let ext = target.is_extensible(rt)?;
        let mut it = target.iterate_keys(rt)?;
        while let Some(item) = it.next(rt)? {
            if key_set.has(rt, &item.name)? {
                key_set.delete(rt, &item.name)?;
            } else {
                if !ext {
                    let s = item.name.go_string(rt)?;
                    return type_error(
                        rt,
                        &format!("'ownKeys' on proxy: trap result did not include '{s}'"),
                    );
                }
                let prop = match item.value {
                    None => target.get_own_prop(rt, &item.name)?,
                    Some(v) => Some(v),
                };
                if let Some(Value::Property(p)) = &prop
                    && !p.configurable
                {
                    let s = item.name.go_string(rt)?;
                    return type_error(
                        rt,
                        &format!(
                            "'ownKeys' on proxy: trap result did not include non-configurable '{s}'"
                        ),
                    );
                }
            }
        }
        if !ext && !key_list.is_empty() && key_set.size() > 0 {
            return type_error(
                rt,
                "'ownKeys' on proxy: trap returned extra keys but proxy target is non-extensible",
            );
        }
        Ok(())
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let names = ProxyObject::string_keys(rt, o, true, Vec::new())?;
        Ok(Box::new(ProxyPropIter { names, idx: 0 }))
    }

    pub fn iterate_symbols(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let names = ProxyObject::symbols(rt, o, true, Vec::new())?;
        Ok(Box::new(ProxyPropIter { names, idx: 0 }))
    }

    pub fn iterate_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let names = ProxyObject::keys(rt, o, true, Vec::new())?;
        Ok(Box::new(ProxyPropIter { names, idx: 0 }))
    }

    pub fn assert_callable(rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        if ProxyObject::data(rt, o).call.is_some() {
            return Some(CallFn {
                obj: o,
                f: ProxyObject::apply_fn,
            });
        }
        None
    }

    fn apply_fn(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        ProxyObject::apply(rt, o, call)
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        let sp = rt.vm.sp as usize;
        rt.vm_push_ctx_for_call()?;
        rt.vm.prg = None;
        rt.vm.sb = (sp - n) as isize;
        let this = rt.vm.stack[sp - n - 2].clone();
        let mut buf = rt.vm.take_arg_buf();
        buf.extend_from_slice(&rt.vm.stack[sp - n..sp]);
        let saved = rt.in_native;
        let mark = rt.handles.len();
        rt.in_native = true;
        let ret = ProxyObject::apply(rt, o, FunctionCall::new(&this, &buf));
        rt.in_native = saved;
        rt.handles.truncate(mark);
        rt.vm.return_arg_buf(buf);
        let ret = ret?;
        rt.vm.stack[sp - n - 2] = if ret.is_empty() {
            Value::Undefined
        } else {
            ret
        };
        rt.vm.pop_ctx();
        rt.vm.sp -= n as isize + 1;
        rt.vm.pc += 1;
        Ok(())
    }

    pub fn assert_constructor(rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        if ProxyObject::data(rt, o).ctor.is_some() {
            return Some(CtorFn {
                obj: o,
                f: ProxyObject::construct,
            });
        }
        None
    }

    pub fn apply(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        let Some(f) = ProxyObject::data(rt, o).call else {
            return type_error(rt, "proxy target is not a function");
        };
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        let this = nil_safe(Some(call.this.clone()));
        if let Some(v) = h.apply(rt, target, &this, call.arguments)? {
            return Ok(v);
        }
        f.call_fc(rt, call)
    }

    pub fn construct(
        rt: &mut Runtime,
        o: ObjRef,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(ctor) = ProxyObject::data(rt, o).ctor else {
            return type_error(rt, "proxy target is not a constructor");
        };
        let new_target = new_target.unwrap_or(o);
        let (target, h) = ProxyObject::check_handler(rt, o)?;
        if let Some(v) = h.construct(rt, target, args, new_target)? {
            return rt.to_object(&v);
        }
        ctor.construct(rt, args, Some(new_target))
    }

    pub fn __is_compatible_descriptor(
        rt: &Runtime,
        extensible: bool,
        desc: &PropertyDescriptor,
        current: Option<&ValueProperty>,
    ) -> bool {
        let Some(current) = current else {
            return extensible;
        };
        if !current.configurable {
            if desc.configurable == Flag::True {
                return false;
            }
            if desc.enumerable != Flag::NotSet && desc.enumerable.bool() != current.enumerable {
                return false;
            }
            if desc.is_generic() {
                return true;
            }
            if desc.is_data() == current.accessor {
                return desc.configurable != Flag::False;
            }
            if desc.is_data() && !current.accessor {
                if !current.configurable {
                    if desc.writable == Flag::True && !current.writable {
                        return false;
                    }
                    if !current.writable
                        && let Some(v) = &desc.value
                        && !v.same_as(&current.value)
                    {
                        return false;
                    }
                }
                return true;
            }
            if desc.is_accessor() && current.accessor && !current.configurable {
                if let Some(s) = &desc.setter
                    && same_value_obj(rt, s, current.setter_func)
                {
                    return false;
                }
                if let Some(g) = &desc.getter
                    && same_value_obj(rt, g, current.getter_func)
                {
                    return false;
                }
            }
        }
        true
    }

    fn filter_keys(
        rt: &mut Runtime,
        o: ObjRef,
        mut vals: Vec<Value>,
        all: bool,
        symbols: bool,
    ) -> JsResult<Vec<Value>> {
        if !all {
            let mut k = 0;
            for i in 0..vals.len() {
                let prop = match &vals[i] {
                    Value::Symbol(s) if symbols => ProxyObject::get_own_prop_sym(rt, o, *s)?,
                    Value::Symbol(_) => continue,
                    _ if symbols => continue,
                    v => {
                        let name = v.string(rt)?;
                        ProxyObject::get_own_prop_str(rt, o, &name)?
                    }
                };
                match &prop {
                    None => continue,
                    Some(Value::Property(p)) if !p.enumerable => continue,
                    _ => {}
                }
                if k != i {
                    vals[k] = vals[i].clone();
                }
                k += 1;
            }
            vals.truncate(k);
        } else {
            vals.retain(|v| matches!(v, Value::Symbol(_)) == symbols);
        }
        Ok(vals)
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        _accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let keys = match ProxyObject::proxy_own_keys(rt, o)? {
            Some(vals) => vals,
            None => {
                let target = ProxyObject::data(rt, o).target;
                match target {
                    Some(t) => t.string_keys(rt, true, Vec::new())?,
                    None => return type_error(rt, "Proxy already revoked"),
                }
            }
        };
        let mark = rt.extra_roots.len();
        rt.extra_roots.extend(
            keys.iter()
                .filter(|k| matches!(k, Value::Symbol(_)))
                .cloned(),
        );
        let r = ProxyObject::filter_keys(rt, o, keys, all, false);
        rt.extra_roots.truncate(mark);
        r
    }

    pub fn symbols(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let symbols = match ProxyObject::proxy_own_keys(rt, o)? {
            Some(vals) => vals,
            None => {
                let target = ProxyObject::data(rt, o).target;
                match target {
                    Some(t) => t.symbols(rt, true, Vec::new())?,
                    None => return type_error(rt, "Proxy already revoked"),
                }
            }
        };
        let mark = rt.extra_roots.len();
        rt.extra_roots.extend(
            symbols
                .iter()
                .filter(|k| matches!(k, Value::Symbol(_)))
                .cloned(),
        );
        let r = ProxyObject::filter_keys(rt, o, symbols, all, true);
        rt.extra_roots.truncate(mark);
        let symbols = r?;
        if accum.is_empty() {
            return Ok(symbols);
        }
        accum.extend(symbols);
        Ok(accum)
    }

    pub fn class_name(rt: &Runtime, o: ObjRef) -> &'static str {
        let d = ProxyObject::data(rt, o);
        if d.target.is_none() {
            return CLASS_OBJECT;
        }
        if d.call.is_some() || d.ctor.is_some() {
            return CLASS_FUNCTION;
        }
        CLASS_OBJECT
    }

    pub fn type_of(rt: &Runtime, o: ObjRef) -> JsString {
        if ProxyObject::data(rt, o).call.is_none() {
            return STRING_OBJECT_C.clone();
        }
        STRING_FUNCTION.clone()
    }

    pub fn revoke(rt: &mut Runtime, o: ObjRef) {
        let d = ProxyObject::data_mut(rt, o);
        d.handler = None;
        d.target = None;
    }
}

pub fn proxy_revoked_error(rt: &mut Runtime) -> Throw {
    Throw::Value(rt.new_type_error("proxy has been revoked"))
}

pub fn proxy_class_name(rt: &mut Runtime, o: ObjRef) -> JsResult<&'static str> {
    if let Some(d) = ProxyObject::try_data(rt, o)
        && d.target.is_none()
    {
        return Err(proxy_revoked_error(rt));
    }
    Ok(o.class_name(rt))
}

pub static PROXY_OBJECT_CLASS: ObjClass = ObjClass {
    class_name: ProxyObject::class_name,
    type_of: ProxyObject::type_of,
    get_str: ProxyObject::get_str,
    get_idx: ProxyObject::get_idx,
    get_sym: ProxyObject::get_sym,
    get_own_prop_str: ProxyObject::get_own_prop_str,
    get_own_prop_idx: ProxyObject::get_own_prop_idx,
    get_own_prop_sym: ProxyObject::get_own_prop_sym,
    set_own_str: ProxyObject::set_own_str,
    set_own_idx: ProxyObject::set_own_idx,
    set_own_sym: ProxyObject::set_own_sym,
    set_foreign_str: ProxyObject::set_foreign_str,
    set_foreign_idx: ProxyObject::set_foreign_idx,
    set_foreign_sym: ProxyObject::set_foreign_sym,
    has_property_str: ProxyObject::has_property_str,
    has_property_idx: ProxyObject::has_property_idx,
    has_property_sym: ProxyObject::has_property_sym,
    has_own_property_str: ProxyObject::has_own_property_str,
    has_own_property_idx: ProxyObject::has_own_property_idx,
    has_own_property_sym: ProxyObject::has_own_property_sym,
    define_own_property_str: ProxyObject::define_own_property_str,
    define_own_property_idx: ProxyObject::define_own_property_idx,
    define_own_property_sym: ProxyObject::define_own_property_sym,
    delete_str: ProxyObject::delete_str,
    delete_idx: ProxyObject::delete_idx,
    delete_sym: ProxyObject::delete_sym,
    assert_callable: ProxyObject::assert_callable,
    vm_call: ProxyObject::vm_call,
    assert_constructor: ProxyObject::assert_constructor,
    proto: ProxyObject::proto,
    set_proto: ProxyObject::set_proto,
    is_extensible: ProxyObject::is_extensible,
    prevent_extensions: ProxyObject::prevent_extensions,
    iterate_string_keys: ProxyObject::iterate_string_keys,
    iterate_symbols: ProxyObject::iterate_symbols,
    iterate_keys: ProxyObject::iterate_keys,
    string_keys: ProxyObject::string_keys,
    symbols: ProxyObject::symbols,
    keys: ProxyObject::keys,
    ..BASE_CLASS
};
