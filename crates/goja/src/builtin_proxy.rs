use crate::func::NativeFn;
use crate::heap::{ObjRef, SymRef};
use crate::object::{FunctionCall, PropertyDescriptor};
use crate::proxy::{Proxy, ProxyHandler, ProxyObject};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::value::Value;
use std::rc::Rc;

pub type ProxyTargetTrap<R> = Box<dyn Fn(&mut Runtime, ObjRef) -> JsResult<R>>;
pub type ProxyKeyTrap<K, R> = Box<dyn Fn(&mut Runtime, ObjRef, K) -> JsResult<R>>;
pub type ProxyDefineTrap<K> =
    Box<dyn Fn(&mut Runtime, ObjRef, K, PropertyDescriptor) -> JsResult<bool>>;
pub type ProxyGetTrap<K> = Box<dyn Fn(&mut Runtime, ObjRef, K, &Value) -> JsResult<Value>>;
pub type ProxySetTrap<K> = Box<dyn Fn(&mut Runtime, ObjRef, K, &Value, &Value) -> JsResult<bool>>;
pub type ProxyStrTrap<R> = Box<dyn Fn(&mut Runtime, ObjRef, &str) -> JsResult<R>>;
pub type ProxyStrDefineTrap =
    Box<dyn Fn(&mut Runtime, ObjRef, &str, PropertyDescriptor) -> JsResult<bool>>;
pub type ProxyStrGetTrap = Box<dyn Fn(&mut Runtime, ObjRef, &str, &Value) -> JsResult<Value>>;
pub type ProxyStrSetTrap =
    Box<dyn Fn(&mut Runtime, ObjRef, &str, &Value, &Value) -> JsResult<bool>>;
pub type ProxyApplyTrap = Box<dyn Fn(&mut Runtime, ObjRef, &Value, &[Value]) -> JsResult<Value>>;
pub type ProxyConstructTrap =
    Box<dyn Fn(&mut Runtime, ObjRef, &[Value], ObjRef) -> JsResult<ObjRef>>;

#[derive(Default)]
pub struct ProxyTrapConfig {
    pub get_prototype_of: Option<ProxyTargetTrap<Option<ObjRef>>>,
    pub set_prototype_of: Option<ProxyKeyTrap<Option<ObjRef>, bool>>,
    pub is_extensible: Option<ProxyTargetTrap<bool>>,
    pub prevent_extensions: Option<ProxyTargetTrap<bool>>,
    pub get_own_property_descriptor: Option<ProxyStrTrap<PropertyDescriptor>>,
    pub get_own_property_descriptor_idx: Option<ProxyKeyTrap<i64, PropertyDescriptor>>,
    pub get_own_property_descriptor_sym: Option<ProxyKeyTrap<SymRef, PropertyDescriptor>>,
    pub define_property: Option<ProxyStrDefineTrap>,
    pub define_property_idx: Option<ProxyDefineTrap<i64>>,
    pub define_property_sym: Option<ProxyDefineTrap<SymRef>>,
    pub has: Option<ProxyStrTrap<bool>>,
    pub has_idx: Option<ProxyKeyTrap<i64, bool>>,
    pub has_sym: Option<ProxyKeyTrap<SymRef, bool>>,
    pub get: Option<ProxyStrGetTrap>,
    pub get_idx: Option<ProxyGetTrap<i64>>,
    pub get_sym: Option<ProxyGetTrap<SymRef>>,
    pub set: Option<ProxyStrSetTrap>,
    pub set_idx: Option<ProxySetTrap<i64>>,
    pub set_sym: Option<ProxySetTrap<SymRef>>,
    pub delete_property: Option<ProxyStrTrap<bool>>,
    pub delete_property_idx: Option<ProxyKeyTrap<i64, bool>>,
    pub delete_property_sym: Option<ProxyKeyTrap<SymRef, bool>>,
    pub own_keys: Option<ProxyTargetTrap<ObjRef>>,
    pub apply: Option<ProxyApplyTrap>,
    pub construct: Option<ProxyConstructTrap>,
}

pub mod native_proxy_handler {
    use super::ProxyTrapConfig;
    use crate::heap::ObjRef;
    use crate::object::PropertyDescriptor;
    use crate::proxy::PropKey;
    use crate::runtime::{JsResult, Runtime, str_to_int, to_int_strict};
    use crate::value::Value;

    pub fn get_prototype_of(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
    ) -> JsResult<Option<Value>> {
        if let Some(trap) = &h.get_prototype_of {
            let p = trap(rt, target)?;
            return Ok(Some(match p {
                Some(p) => Value::Object(p),
                None => Value::Null,
            }));
        }
        Ok(None)
    }

    pub fn set_prototype_of(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        proto: Option<ObjRef>,
    ) -> JsResult<Option<bool>> {
        if let Some(trap) = &h.set_prototype_of {
            return Ok(Some(trap(rt, target, proto)?));
        }
        Ok(None)
    }

    pub fn is_extensible(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
    ) -> JsResult<Option<bool>> {
        if let Some(trap) = &h.is_extensible {
            return Ok(Some(trap(rt, target)?));
        }
        Ok(None)
    }

    pub fn prevent_extensions(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
    ) -> JsResult<Option<bool>> {
        if let Some(trap) = &h.prevent_extensions {
            return Ok(Some(trap(rt, target)?));
        }
        Ok(None)
    }

    pub fn get_own_property_descriptor(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<Value>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.get_own_property_descriptor_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        let desc = trap(rt, target, idx)?;
                        return Ok(Some(desc.to_value(rt)));
                    }
                }
                if let Some(trap) = &h.get_own_property_descriptor {
                    let s = p.string();
                    let desc = trap(rt, target, &s)?;
                    return Ok(Some(desc.to_value(rt)));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.get_own_property_descriptor_idx {
                    let desc = trap(rt, target, to_int_strict(i))?;
                    return Ok(Some(desc.to_value(rt)));
                }
                if let Some(trap) = &h.get_own_property_descriptor {
                    let s = i.to_string();
                    let desc = trap(rt, target, &s)?;
                    return Ok(Some(desc.to_value(rt)));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.get_own_property_descriptor_sym {
                    let desc = trap(rt, target, s)?;
                    return Ok(Some(desc.to_value(rt)));
                }
                Ok(None)
            }
        }
    }

    pub fn define_property(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
        desc: &PropertyDescriptor,
    ) -> JsResult<Option<bool>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.define_property_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        return Ok(Some(trap(rt, target, idx, desc.clone())?));
                    }
                }
                if let Some(trap) = &h.define_property {
                    let s = p.string();
                    return Ok(Some(trap(rt, target, &s, desc.clone())?));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.define_property_idx {
                    return Ok(Some(trap(rt, target, to_int_strict(i), desc.clone())?));
                }
                if let Some(trap) = &h.define_property {
                    let s = i.to_string();
                    return Ok(Some(trap(rt, target, &s, desc.clone())?));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.define_property_sym {
                    return Ok(Some(trap(rt, target, s, desc.clone())?));
                }
                Ok(None)
            }
        }
    }

    pub fn has(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<bool>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.has_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        return Ok(Some(trap(rt, target, idx)?));
                    }
                }
                if let Some(trap) = &h.has {
                    let s = p.string();
                    return Ok(Some(trap(rt, target, &s)?));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.has_idx {
                    return Ok(Some(trap(rt, target, to_int_strict(i))?));
                }
                if let Some(trap) = &h.has {
                    let s = i.to_string();
                    return Ok(Some(trap(rt, target, &s)?));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.has_sym {
                    return Ok(Some(trap(rt, target, s)?));
                }
                Ok(None)
            }
        }
    }

    pub fn get(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
        receiver: &Value,
    ) -> JsResult<Option<Value>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.get_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        return Ok(Some(trap(rt, target, idx, receiver)?));
                    }
                }
                if let Some(trap) = &h.get {
                    let s = p.string();
                    return Ok(Some(trap(rt, target, &s, receiver)?));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.get_idx {
                    return Ok(Some(trap(rt, target, to_int_strict(i), receiver)?));
                }
                if let Some(trap) = &h.get {
                    let s = i.to_string();
                    return Ok(Some(trap(rt, target, &s, receiver)?));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.get_sym {
                    return Ok(Some(trap(rt, target, s, receiver)?));
                }
                Ok(None)
            }
        }
    }

    pub fn set(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
        value: &Value,
        receiver: &Value,
    ) -> JsResult<Option<bool>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.set_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        return Ok(Some(trap(rt, target, idx, value, receiver)?));
                    }
                }
                if let Some(trap) = &h.set {
                    let s = p.string();
                    return Ok(Some(trap(rt, target, &s, value, receiver)?));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.set_idx {
                    return Ok(Some(trap(rt, target, to_int_strict(i), value, receiver)?));
                }
                if let Some(trap) = &h.set {
                    let s = i.to_string();
                    return Ok(Some(trap(rt, target, &s, value, receiver)?));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.set_sym {
                    return Ok(Some(trap(rt, target, s, value, receiver)?));
                }
                Ok(None)
            }
        }
    }

    pub fn delete(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        prop: PropKey<'_>,
    ) -> JsResult<Option<bool>> {
        match prop {
            PropKey::Str(p) => {
                if let Some(trap) = &h.delete_property_idx {
                    let (idx, ok) = str_to_int(p);
                    if ok {
                        return Ok(Some(trap(rt, target, idx)?));
                    }
                }
                if let Some(trap) = &h.delete_property {
                    let s = p.string();
                    return Ok(Some(trap(rt, target, &s)?));
                }
                Ok(None)
            }
            PropKey::Idx(i) => {
                if let Some(trap) = &h.delete_property_idx {
                    return Ok(Some(trap(rt, target, to_int_strict(i))?));
                }
                if let Some(trap) = &h.delete_property {
                    let s = i.to_string();
                    return Ok(Some(trap(rt, target, &s)?));
                }
                Ok(None)
            }
            PropKey::Sym(s) => {
                if let Some(trap) = &h.delete_property_sym {
                    return Ok(Some(trap(rt, target, s)?));
                }
                Ok(None)
            }
        }
    }

    pub fn own_keys(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
    ) -> JsResult<Option<ObjRef>> {
        if let Some(trap) = &h.own_keys {
            return Ok(Some(trap(rt, target)?));
        }
        Ok(None)
    }

    pub fn apply(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        this: &Value,
        args: &[Value],
    ) -> JsResult<Option<Value>> {
        if let Some(trap) = &h.apply {
            return Ok(Some(trap(rt, target, this, args)?));
        }
        Ok(None)
    }

    pub fn construct(
        rt: &mut Runtime,
        h: &ProxyTrapConfig,
        target: ObjRef,
        args: &[Value],
        new_target: ObjRef,
    ) -> JsResult<Option<Value>> {
        if let Some(trap) = &h.construct {
            return Ok(Some(Value::Object(trap(rt, target, args, new_target)?)));
        }
        Ok(None)
    }
}

impl Runtime {
    pub fn new_native_proxy_handler(&mut self, native_handler: ProxyTrapConfig) -> ProxyHandler {
        ProxyHandler::Native(Rc::new(native_handler))
    }

    pub fn new_proxy_from_args(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        if args.len() >= 2
            && let Value::Object(target) = &args[0]
            && let Value::Object(proxy_handler) = &args[1]
        {
            return Ok(self.new_proxy_object(*target, *proxy_handler, proto));
        }
        Err(Throw::Value(self.new_type_error(
            "Cannot create proxy with a non-object as target or handler",
        )))
    }

    pub fn builtin_new_proxy(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("Proxy")));
        };
        let proxy = self.get_proxy();
        let op = self.global.object_prototype;
        let proto = self.get_prototype_from_ctor(new_target, Some(proxy), op)?;
        self.new_proxy_from_args(args, proto)
    }

    pub fn new_proxy(
        &mut self,
        target: ObjRef,
        native_handler: ProxyTrapConfig,
    ) -> JsResult<Proxy> {
        if let Some(p) = ProxyObject::try_data(self, target)
            && p.handler.is_none()
        {
            return Err(Throw::Value(self.new_type_error(
                "Cannot create proxy with a revoked proxy as target",
            )));
        }
        let handler = self.new_native_proxy_handler(native_handler);
        let proxy = self._new_proxy_object(target, handler, None);
        Ok(Proxy { proxy })
    }

    pub fn builtin_proxy_revocable(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if call.arguments.len() >= 2
            && let Value::Object(target) = call.arg(0)
            && let Value::Object(proxy_handler) = call.arg(1)
        {
            let proxy = self.new_proxy_object(*target, *proxy_handler, None);
            let revoke = self.new_native_func(
                NativeFn::closure_with(vec![Value::Object(proxy)], |r, d, _call| {
                    if let Value::Object(p) = &d[0] {
                        ProxyObject::revoke(r, *p);
                    }
                    Ok(Value::Undefined)
                }),
                "",
                0,
            );
            let ret = self.new_object();
            ret._put_prop(
                self,
                &"proxy".into(),
                Value::Object(proxy),
                true,
                true,
                true,
            );
            ret._put_prop(
                self,
                &"revoke".into(),
                Value::Object(revoke),
                true,
                true,
                true,
            );
            return Ok(Value::Object(ret));
        }
        Err(Throw::Value(self.new_type_error(
            "Cannot create proxy with a non-object as target or handler",
        )))
    }

    pub fn create_proxy(&mut self, val: ObjRef) {
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_proxy(args, nt),
            None,
            "Proxy",
            2,
        );
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.builtin_proxy_revocable(c)),
            "revocable",
            2,
        );
        o._put_prop(
            self,
            &"revocable".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
    }

    pub fn get_proxy(&mut self) -> ObjRef {
        if let Some(r) = self.global.proxy {
            return r;
        }
        let ret = self.reserve_object();
        self.global.proxy = Some(ret);
        self.create_proxy(ret);
        ret
    }
}

pub fn proxy_target(rt: &Runtime, o: ObjRef) -> Option<Option<ObjRef>> {
    ProxyObject::try_data(rt, o).map(|p| p.target)
}
