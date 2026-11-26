use crate::func::{BaseFuncObject, NativeCtor, NativeFn, native_vm_call};
use crate::heap::{FxHashMap, ObjRef, SymRef, Tracer};
use crate::map::OrderedMap;
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, CLASS_FUNCTION, CLASS_OBJECT, CallFn, CtorFn,
    FunctionCall, IterNextFunc, ObjClass, ObjExt, PropertyDescriptor, UStr, to_idx,
};
use crate::runtime::{JsResult, Runtime, Throw, str_to_array_idx};
use crate::string::{JsString, STRING_FUNCTION, new_string_value};
use crate::unistring;
use crate::value::{Value, int_to_value, nil_safe};

pub type TemplatePropFactory = fn(&mut Runtime) -> Value;
pub type ProtoFactory = fn(&mut Runtime) -> Option<ObjRef>;

#[derive(Default)]
pub struct ObjectTemplate {
    pub prop_names: Vec<unistring::String>,
    pub props: FxHashMap<unistring::String, TemplatePropFactory>,
    pub sym_props: FxHashMap<SymRef, TemplatePropFactory>,
    pub sym_prop_names: Vec<SymRef>,
    pub proto_factory: Option<ProtoFactory>,
}

impl ObjectTemplate {
    pub fn new() -> ObjectTemplate {
        ObjectTemplate::default()
    }

    pub fn put_str(&mut self, name: &str, f: TemplatePropFactory) {
        let name = unistring::String::new(name);
        self.props.insert(name.clone(), f);
        self.prop_names.push(name);
    }

    pub fn put_sym(&mut self, s: SymRef, f: TemplatePropFactory) {
        self.sym_props.insert(s, f);
        self.sym_prop_names.push(s);
    }
}

pub struct TemplatedObject {
    pub tmpl: &'static ObjectTemplate,
    pub proto_materialised: bool,
    pub prop_names_materialised: bool,
    pub f: Option<NativeFn>,
    pub construct: Option<NativeCtor>,
}

impl TemplatedObject {
    pub fn trace(&self, t: &mut Tracer) {
        if let Some(f) = &self.f {
            f.trace(t);
        }
        if let Some(c) = &self.construct {
            c.trace(t);
        }
    }

    pub fn is_func(&self) -> bool {
        self.f.is_some() || self.construct.is_some()
    }

    fn data(rt: &Runtime, o: ObjRef) -> &TemplatedObject {
        match &rt.obj(o).ext {
            ObjExt::Template(t) => t,
            _ => unreachable!("not a templated object"),
        }
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut TemplatedObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::Template(t) => t,
            _ => unreachable!("not a templated object"),
        }
    }

    pub fn materialise_proto(rt: &mut Runtime, o: ObjRef) {
        let d = TemplatedObject::data(rt, o);
        if !d.proto_materialised {
            let pf = d.tmpl.proto_factory;
            if let Some(pf) = pf {
                let p = pf(rt);
                rt.obj_mut(o).base.prototype = p;
            }
            TemplatedObject::data_mut(rt, o).proto_materialised = true;
        }
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        match rt.obj(o).base.values.get(name) {
            Some(Value::Property(prop)) if prop.getter_func.is_none() => {
                if prop.value.is_empty() {
                    return Ok(Some(Value::Undefined));
                }
                return Ok(Some(prop.value.clone()));
            }
            Some(Value::Property(_)) => return BaseObject::get_str(rt, o, name, receiver),
            Some(v) if !v.is_empty() => return Ok(Some(v.clone())),
            _ => {}
        }
        let own_prop = TemplatedObject::get_own_prop_str(rt, o, name)?;
        if own_prop.is_none() {
            TemplatedObject::materialise_proto(rt, o);
        }
        BaseObject::get_str_with_own_prop(rt, o, own_prop, name, receiver)
    }

    pub fn get_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let own_prop = TemplatedObject::get_own_prop_sym(rt, o, s)?;
        if own_prop.is_none() {
            TemplatedObject::materialise_proto(rt, o);
        }
        BaseObject::get_with_own_prop(rt, o, own_prop, &Value::Symbol(s), receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<Option<Value>> {
        if let Some(v) = rt.obj(o).base.values.get(p) {
            if v.is_empty() {
                return Ok(None);
            }
            return Ok(Some(v.clone()));
        }
        let f = TemplatedObject::data(rt, o).tmpl.props.get(p).copied();
        if let Some(f) = f {
            let v = f(rt);
            rt.obj_mut(o).base.values.insert(p.clone(), v.clone());
            return Ok(Some(v));
        }
        Ok(None)
    }

    pub fn materialise_symbols(rt: &mut Runtime, o: ObjRef) {
        if rt.obj(o).base.sym_values.is_none() {
            rt.obj_mut(o).base.sym_values = Some(OrderedMap::new());
            let tmpl = TemplatedObject::data(rt, o).tmpl;
            for p in &tmpl.sym_prop_names {
                let v = (tmpl.sym_props[p])(rt);
                if let Some(m) = &mut rt.obj_mut(o).base.sym_values {
                    m.set(Value::Symbol(*p), v);
                }
            }
        }
    }

    pub fn get_own_prop_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<Option<Value>> {
        if rt.obj(o).base.sym_values.is_none()
            && !TemplatedObject::data(rt, o).tmpl.sym_props.contains_key(&s)
        {
            return Ok(None);
        }
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::get_own_prop_sym(rt, o, s)
    }

    pub fn materialise_prop_names(rt: &mut Runtime, o: ObjRef) {
        let obj = rt.heap.get_mut(o);
        if let ObjExt::Template(t) = &mut obj.ext
            && !t.prop_names_materialised
        {
            if obj.base.prop_names.is_empty() {
                obj.base.prop_names = t.tmpl.prop_names.clone();
            }
            t.prop_names_materialised = true;
        }
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        match rt.obj_mut(o).base.values.get_mut(p) {
            Some(Value::Property(prop)) if prop.writable && prop.setter_func.is_none() => {
                prop.value = v;
                return Ok(true);
            }
            Some(Value::Property(_)) => return BaseObject::set_own_str(rt, o, p, v, throw),
            Some(Value::Empty) | None => {}
            Some(slot) => {
                *slot = v;
                return Ok(true);
            }
        }
        let existing = TemplatedObject::get_own_prop_str(rt, o, p)?;
        if existing.is_none() {
            TemplatedObject::materialise_proto(rt, o);
            TemplatedObject::materialise_prop_names(rt, o);
        }
        BaseObject::set_own_str(rt, o, p, v, throw)
    }

    pub fn set_own_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        TemplatedObject::materialise_symbols(rt, o);
        TemplatedObject::materialise_proto(rt, o);
        BaseObject::set_own_sym(rt, o, name, val, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let own_prop = TemplatedObject::get_own_prop_str(rt, o, name)?;
        if own_prop.is_none() {
            TemplatedObject::materialise_proto(rt, o);
        }
        BaseObject::_set_foreign_str(rt, o, name, own_prop.as_ref(), val, receiver, throw)
    }

    pub fn proto(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<ObjRef>> {
        TemplatedObject::materialise_proto(rt, o);
        Ok(rt.obj(o).base.prototype)
    }

    pub fn set_proto(
        rt: &mut Runtime,
        o: ObjRef,
        proto: Option<ObjRef>,
        throw: bool,
    ) -> JsResult<bool> {
        TemplatedObject::data_mut(rt, o).proto_materialised = true;
        let ret = BaseObject::set_proto(rt, o, proto, throw)?;
        if ret {
            TemplatedObject::data_mut(rt, o).proto_materialised = true;
        }
        Ok(ret)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        o.set_foreign_str(rt, &crate::object::idx_string(name), val, receiver, throw)
    }

    pub fn set_foreign_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        TemplatedObject::materialise_proto(rt, o);
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::set_foreign_sym(rt, o, name, val, receiver, throw)
    }

    pub fn has_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        if o.has_own_property_str(rt, name)? {
            return Ok(true);
        }
        TemplatedObject::materialise_proto(rt, o);
        if let Some(p) = rt.obj(o).base.prototype {
            return p.has_property_str(rt, name);
        }
        Ok(false)
    }

    pub fn has_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        if TemplatedObject::has_own_property_sym(rt, o, s)? {
            return Ok(true);
        }
        TemplatedObject::materialise_proto(rt, o);
        if let Some(p) = rt.obj(o).base.prototype {
            return p.has_property_sym(rt, s);
        }
        Ok(false)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        if let Some(v) = rt.obj(o).base.values.get(name) {
            return Ok(!v.is_empty());
        }
        Ok(TemplatedObject::data(rt, o).tmpl.props.contains_key(name))
    }

    pub fn has_own_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        if let Some(m) = &rt.obj(o).base.sym_values {
            return Ok(m.has(&Value::Symbol(s)));
        }
        Ok(TemplatedObject::data(rt, o).tmpl.sym_props.contains_key(&s))
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let existing_val = TemplatedObject::get_own_prop_str(rt, o, name)?;
        if let Some(v) =
            BaseObject::_define_own_property(rt, o, name, existing_val.as_ref(), &descr, throw)?
        {
            rt.obj_mut(o).base.values.insert(name.clone(), v);
            if existing_val.is_none() {
                TemplatedObject::materialise_prop_names(rt, o);
                rt.obj_mut(o).base.prop_names.push(name.clone());
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn define_own_property_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::define_own_property_sym(rt, o, s, descr, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        if let Some(val) = TemplatedObject::get_own_prop_str(rt, o, name)? {
            if !BaseObject::check_delete(rt, o, name, &val, throw)? {
                return Ok(false);
            }
            TemplatedObject::materialise_prop_names(rt, o);
            rt.obj_mut(o).base._delete(name);
            if TemplatedObject::data(rt, o).tmpl.props.contains_key(name) {
                rt.obj_mut(o).base.values.insert(name.clone(), Value::Empty);
            }
        }
        Ok(true)
    }

    pub fn delete_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, throw: bool) -> JsResult<bool> {
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::delete_sym(rt, o, s, throw)
    }

    pub fn materialise_props(rt: &mut Runtime, o: ObjRef) {
        let tmpl = TemplatedObject::data(rt, o).tmpl;
        for name in &tmpl.prop_names {
            if !rt.obj(o).base.values.contains_key(name) {
                let v = (tmpl.props[name])(rt);
                rt.obj_mut(o).base.values.insert(name.clone(), v);
            }
        }
        TemplatedObject::materialise_prop_names(rt, o);
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        TemplatedObject::materialise_props(rt, o);
        BaseObject::iterate_string_keys(rt, o)
    }

    pub fn iterate_symbols(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::iterate_symbols(rt, o)
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        keys: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        if all {
            TemplatedObject::materialise_prop_names(rt, o);
        } else {
            TemplatedObject::materialise_props(rt, o);
        }
        BaseObject::string_keys(rt, o, all, keys)
    }

    pub fn symbols(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        TemplatedObject::materialise_symbols(rt, o);
        BaseObject::symbols(rt, o, all, accum)
    }

    pub fn keys(rt: &mut Runtime, o: ObjRef, all: bool, accum: Vec<Value>) -> JsResult<Vec<Value>> {
        let accum = TemplatedObject::string_keys(rt, o, all, accum)?;
        TemplatedObject::symbols(rt, o, all, accum)
    }
}

pub struct TemplatedFuncObject;

impl TemplatedFuncObject {
    pub fn source(rt: &mut Runtime, o: ObjRef) -> JsResult<JsString> {
        let n = nil_safe(o.get_str(rt, &"name".into(), None)?);
        let n = n.to_string(rt)?;
        Ok(new_string_value(
            format!("function {}() {{ [native code] }}", n.to_rust_string()).as_bytes(),
        ))
    }

    fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        let f = TemplatedObject::data(rt, o).f.clone();
        match f {
            Some(f) => {
                let saved = rt.in_native;
                rt.in_native = true;
                let r = f.call(rt, call);
                rt.in_native = saved;
                r
            }
            None => Ok(Value::Undefined),
        }
    }

    fn construct(
        rt: &mut Runtime,
        o: ObjRef,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let c = TemplatedObject::data(rt, o).construct.clone();
        match c {
            Some(c) => {
                let saved = rt.in_native;
                rt.in_native = true;
                let r = c.construct(rt, args, new_target);
                rt.in_native = saved;
                r
            }
            None => Err(Throw::TypeError("Not a constructor".to_string())),
        }
    }

    pub fn assert_callable(rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        if TemplatedObject::data(rt, o).f.is_some() {
            return Some(CallFn {
                obj: o,
                f: TemplatedFuncObject::call,
            });
        }
        None
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        let f = TemplatedObject::data(rt, o).f.clone();
        native_vm_call(rt, f, n)
    }

    pub fn assert_constructor(rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        if TemplatedObject::data(rt, o).construct.is_some() {
            return Some(CtorFn {
                obj: o,
                f: TemplatedFuncObject::construct,
            });
        }
        None
    }

    pub fn type_of(_rt: &Runtime, _o: ObjRef) -> JsString {
        STRING_FUNCTION.clone()
    }

    pub fn has_instance(rt: &mut Runtime, o: ObjRef, v: &Value) -> JsResult<bool> {
        BaseFuncObject::has_instance(rt, o, v)
    }
}

pub struct TemplatedArrayObject;

impl TemplatedArrayObject {
    fn get_len_prop(rt: &mut Runtime, o: ObjRef) -> JsResult<(i64, bool)> {
        let p = TemplatedObject::get_own_prop_str(rt, o, &"length".into())?;
        match p {
            Some(Value::Property(lp)) => Ok((lp.value.prim_to_integer(), lp.writable)),
            _ => Err(Throw::Value(rt.new_type_error("missing length property"))),
        }
    }

    fn set_len_value(rt: &mut Runtime, o: ObjRef, l: i64) {
        if let Some(Value::Property(lp)) = rt.obj_mut(o).base.values.get_mut(&UStr::from("length"))
        {
            lp.value = int_to_value(l);
        }
    }

    pub fn _set_own_idx(rt: &mut Runtime, o: ObjRef, idx: u32) -> JsResult<()> {
        let (l, _) = TemplatedArrayObject::get_len_prop(rt, o)?;
        let l = l as u32;
        if idx >= l {
            TemplatedArrayObject::set_len_value(rt, o, i64::from(idx) + 1);
        }
        Ok(())
    }

    pub fn set_length(rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        let (old_len, writable) = TemplatedArrayObject::get_len_prop(rt, o)?;
        let old_len = old_len as u32;
        let mut l = l;
        if l == old_len {
            return Ok(true);
        }
        if !writable {
            rt.type_error_result(throw, "length is not writable")?;
            return Ok(false);
        }
        let mut ret = true;
        if l < old_len {
            TemplatedObject::materialise_prop_names(rt, o);
            rt.obj_mut(o).base.fix_prop_order();
            let (names, count) = {
                let b = &rt.obj(o).base;
                (b.prop_names.clone(), b.idx_prop_count)
            };
            let i = names[..count].partition_point(|n| str_to_array_idx(n) < l);
            let mut j = count as isize - 1;
            while j >= i as isize {
                let name = names[j as usize].clone();
                if !o.delete_str(rt, &name, false)? {
                    l = str_to_array_idx(&name) + 1;
                    ret = false;
                    break;
                }
                j -= 1;
            }
        }
        TemplatedArrayObject::set_len_value(rt, o, i64::from(l));
        Ok(ret)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        value: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if name == "length" {
            let l = rt.to_length_uint32(&value)?;
            return TemplatedArrayObject::set_length(rt, o, l, throw);
        }
        if !TemplatedObject::set_own_str(rt, o, name, value, throw)? {
            return Ok(false);
        }
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            TemplatedArrayObject::_set_own_idx(rt, o, idx)?;
        }
        Ok(true)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if !TemplatedObject::set_own_str(rt, o, &crate::object::idx_string(p), v, throw)? {
            return Ok(false);
        }
        let idx = to_idx(p);
        if idx != u32::MAX {
            TemplatedArrayObject::_set_own_idx(rt, o, idx)?;
        }
        Ok(true)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if name == "length" {
            return crate::array::define_array_length_templated(rt, o, descr, throw);
        }
        if !TemplatedObject::define_own_property_str(rt, o, name, descr, throw)? {
            return Ok(false);
        }
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            TemplatedArrayObject::_set_own_idx(rt, o, idx)?;
        }
        Ok(true)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if !TemplatedObject::define_own_property_str(
            rt,
            o,
            &crate::object::idx_string(p),
            desc,
            throw,
        )? {
            return Ok(false);
        }
        let idx = to_idx(p);
        if idx != u32::MAX {
            TemplatedArrayObject::_set_own_idx(rt, o, idx)?;
        }
        Ok(true)
    }
}

pub const TEMPLATED_CLASS: ObjClass = ObjClass {
    get_str: TemplatedObject::get_str,
    get_sym: TemplatedObject::get_sym,
    get_own_prop_str: TemplatedObject::get_own_prop_str,
    get_own_prop_sym: TemplatedObject::get_own_prop_sym,
    set_own_str: TemplatedObject::set_own_str,
    set_own_sym: TemplatedObject::set_own_sym,
    set_foreign_str: TemplatedObject::set_foreign_str,
    set_foreign_idx: TemplatedObject::set_foreign_idx,
    set_foreign_sym: TemplatedObject::set_foreign_sym,
    proto: TemplatedObject::proto,
    set_proto: TemplatedObject::set_proto,
    has_property_str: TemplatedObject::has_property_str,
    has_property_sym: TemplatedObject::has_property_sym,
    has_own_property_str: TemplatedObject::has_own_property_str,
    has_own_property_sym: TemplatedObject::has_own_property_sym,
    define_own_property_str: TemplatedObject::define_own_property_str,
    define_own_property_sym: TemplatedObject::define_own_property_sym,
    delete_str: TemplatedObject::delete_str,
    delete_sym: TemplatedObject::delete_sym,
    iterate_string_keys: TemplatedObject::iterate_string_keys,
    iterate_symbols: TemplatedObject::iterate_symbols,
    string_keys: TemplatedObject::string_keys,
    symbols: TemplatedObject::symbols,
    keys: TemplatedObject::keys,
    ..BASE_CLASS
};

pub static TEMPLATED_OBJECT_CLASS: ObjClass = TEMPLATED_CLASS;

pub static TEMPLATED_FUNC_OBJECT_CLASS: ObjClass = ObjClass {
    assert_callable: TemplatedFuncObject::assert_callable,
    vm_call: TemplatedFuncObject::vm_call,
    assert_constructor: TemplatedFuncObject::assert_constructor,
    type_of: TemplatedFuncObject::type_of,
    has_instance: TemplatedFuncObject::has_instance,
    ..TEMPLATED_CLASS
};

pub static TEMPLATED_ARRAY_OBJECT_CLASS: ObjClass = ObjClass {
    set_own_str: TemplatedArrayObject::set_own_str,
    set_own_idx: TemplatedArrayObject::set_own_idx,
    define_own_property_str: TemplatedArrayObject::define_own_property_str,
    define_own_property_idx: TemplatedArrayObject::define_own_property_idx,
    ..TEMPLATED_CLASS
};

impl Runtime {
    pub fn new_templated_object(
        &mut self,
        tmpl: &'static ObjectTemplate,
        obj: Option<ObjRef>,
    ) -> ObjRef {
        let obj = obj.unwrap_or_else(|| self.reserve_object());
        self.set_object_impl(
            obj,
            &TEMPLATED_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, None),
            ObjExt::Template(Box::new(TemplatedObject {
                tmpl,
                proto_materialised: false,
                prop_names_materialised: false,
                f: None,
                construct: None,
            })),
        );
        obj
    }

    pub fn new_templated_func_object(
        &mut self,
        tmpl: &'static ObjectTemplate,
        obj: Option<ObjRef>,
        f: Option<NativeFn>,
        ctor: Option<NativeCtor>,
    ) -> ObjRef {
        let obj = obj.unwrap_or_else(|| self.reserve_object());
        self.set_object_impl(
            obj,
            &TEMPLATED_FUNC_OBJECT_CLASS,
            BaseObject::new(CLASS_FUNCTION, None),
            ObjExt::Template(Box::new(TemplatedObject {
                tmpl,
                proto_materialised: false,
                prop_names_materialised: false,
                f,
                construct: ctor,
            })),
        );
        obj
    }

    pub fn new_templated_array_object(
        &mut self,
        tmpl: &'static ObjectTemplate,
        obj: Option<ObjRef>,
    ) -> ObjRef {
        let obj = obj.unwrap_or_else(|| self.reserve_object());
        self.set_object_impl(
            obj,
            &TEMPLATED_ARRAY_OBJECT_CLASS,
            BaseObject::new(CLASS_ARRAY, None),
            ObjExt::Template(Box::new(TemplatedObject {
                tmpl,
                proto_materialised: false,
                prop_names_materialised: false,
                f: None,
                construct: None,
            })),
        );
        obj
    }
}
