use crate::builtin_symbol::SYM_ITERATOR;
use crate::heap::{FxHashMap, ObjRef, StashRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, IterNextFunc, ObjClass, ObjExt, PropIter, PropIterItem,
    PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime};
use crate::unistring;
use crate::value::{Value, ValueProperty, int_to_value, value_prop};

pub struct MappedProperty {
    pub stash: StashRef,
    pub idx: usize,
}

pub struct ArgumentsObject {
    pub length: usize,
    pub mapped: FxHashMap<unistring::String, MappedProperty>,
}

impl ArgumentsObject {
    pub fn trace(&self, t: &mut Tracer) {
        for m in self.mapped.values() {
            t.stash(m.stash);
        }
    }

    fn data(rt: &Runtime, o: ObjRef) -> &ArgumentsObject {
        match &rt.obj(o).ext {
            ObjExt::Arguments(a) => a,
            _ => unreachable!("not an arguments object"),
        }
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut ArgumentsObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::Arguments(a) => a,
            _ => unreachable!("not an arguments object"),
        }
    }

    fn mapped(rt: &Runtime, o: ObjRef, name: &UStr) -> Option<(StashRef, usize, ValueProperty)> {
        let m = ArgumentsObject::data(rt, o).mapped.get(name)?;
        let flags = match rt.obj(o).base.values.get(name) {
            Some(Value::Property(p)) => (**p).clone(),
            _ => return None,
        };
        Some((m.stash, m.idx, flags))
    }

    fn mapped_value(rt: &Runtime, stash: StashRef, idx: usize) -> Value {
        rt.heap.stash(stash).values[idx].clone()
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = ArgumentsObject::get_own_prop_str(rt, o, name)?;
        BaseObject::get_str_with_own_prop(rt, o, prop, name, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        if let Some((stash, idx, flags)) = ArgumentsObject::mapped(rt, o, name) {
            let v = ArgumentsObject::mapped_value(rt, stash, idx);
            if flags.writable && flags.enumerable && flags.configurable {
                return Ok(Some(v));
            }
            return Ok(Some(Value::Property(Box::new(ValueProperty {
                value: v,
                writable: flags.writable,
                configurable: flags.configurable,
                enumerable: flags.enumerable,
                ..ValueProperty::default()
            }))));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn init(rt: &mut Runtime, o: ObjRef) {
        let length = ArgumentsObject::data(rt, o).length;
        BaseObject::_put_prop(
            rt,
            o,
            &crate::vm::STR_LENGTH,
            int_to_value(length as i64),
            true,
            false,
            true,
        );
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if let Some((stash, idx, flags)) = ArgumentsObject::mapped(rt, o, name) {
            if !flags.writable {
                rt.type_error_result(throw, &format!("Property is not writable: {name}"))?;
                return Ok(false);
            }
            rt.heap.stash_mut(stash).values[idx] = val;
            return Ok(true);
        }
        BaseObject::set_own_str(rt, o, name, val, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = ArgumentsObject::get_own_prop_str(rt, o, name)?;
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        if let Some((_, _, flags)) = ArgumentsObject::mapped(rt, o, name) {
            if !BaseObject::check_delete_prop(rt, o, name, flags.configurable, throw)? {
                return Ok(false);
            }
            rt.obj_mut(o).base._delete(name);
            ArgumentsObject::data_mut(rt, o).mapped.remove(name);
            return Ok(true);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let wrapped = BaseObject::iterate_string_keys(rt, o)?;
        Ok(Box::new(ArgumentsPropIter {
            o,
            wrapped: Some(wrapped),
        }))
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if let Some((stash, idx, flags)) = ArgumentsObject::mapped(rt, o, name) {
            let existing = Value::Property(Box::new(ValueProperty {
                configurable: flags.configurable,
                writable: true,
                enumerable: flags.enumerable,
                value: ArgumentsObject::mapped_value(rt, stash, idx),
                ..ValueProperty::default()
            }));
            let Some(val) =
                BaseObject::_define_own_property(rt, o, name, Some(&existing), &descr, throw)?
            else {
                return Ok(false);
            };
            match val {
                Value::Property(prop) => {
                    if !prop.accessor {
                        rt.heap.stash_mut(stash).values[idx] = prop.value.clone();
                    }
                    if prop.accessor || !prop.writable {
                        ArgumentsObject::data_mut(rt, o).mapped.remove(name);
                        rt.obj_mut(o).base._put(name, Value::Property(prop));
                        return Ok(true);
                    }
                    ArgumentsObject::set_mapped_flags(
                        rt,
                        o,
                        name,
                        prop.configurable,
                        prop.enumerable,
                    );
                }
                v => {
                    rt.heap.stash_mut(stash).values[idx] = v;
                    ArgumentsObject::set_mapped_flags(rt, o, name, true, true);
                }
            }
            return Ok(true);
        }
        BaseObject::define_own_property_str(rt, o, name, descr, throw)
    }

    fn set_mapped_flags(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        configurable: bool,
        enumerable: bool,
    ) {
        if let Some(Value::Property(p)) = rt.obj_mut(o).base.values.get_mut(name) {
            p.configurable = configurable;
            p.enumerable = enumerable;
        }
    }
}

pub struct ArgumentsPropIter {
    pub o: ObjRef,
    pub wrapped: Option<IterNextFunc>,
}

impl PropIter for ArgumentsPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        let Some(w) = &mut self.wrapped else {
            return Ok(None);
        };
        let Some(mut item) = w.next(rt)? else {
            self.wrapped = None;
            return Ok(None);
        };
        if let Value::String(s) = &item.name {
            let name = s.string();
            if let Some((stash, idx, _)) = ArgumentsObject::mapped(rt, self.o, &name) {
                item.value = Some(ArgumentsObject::mapped_value(rt, stash, idx));
            }
        }
        Ok(Some(item))
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        if let Some(w) = &self.wrapped {
            w.trace(t);
        }
    }
}

pub static ARGUMENTS_OBJECT_CLASS: ObjClass = ObjClass {
    get_str: ArgumentsObject::get_str,
    get_own_prop_str: ArgumentsObject::get_own_prop_str,
    set_own_str: ArgumentsObject::set_own_str,
    set_foreign_str: ArgumentsObject::set_foreign_str,
    delete_str: ArgumentsObject::delete_str,
    iterate_string_keys: ArgumentsObject::iterate_string_keys,
    define_own_property_str: ArgumentsObject::define_own_property_str,
    ..BASE_CLASS
};

fn idx_name(i: usize) -> unistring::String {
    unistring::String::from_bytes(i.to_string().as_bytes())
}

impl Runtime {
    pub fn create_args_mapped(&mut self, formal_args: usize) -> ObjRef {
        let proto = self.global.object_prototype;
        let args = self.vm.args as usize;
        let v = self.alloc_object(
            &ARGUMENTS_OBJECT_CLASS,
            BaseObject::new("Arguments", proto),
            ObjExt::Arguments(Box::new(ArgumentsObject {
                length: args,
                mapped: FxHashMap::default(),
            })),
        );
        ArgumentsObject::init(self, v);
        let stash = self.vm.stash.expect("stash");
        let c = formal_args.min(args);
        let mut i = 0;
        while i < c {
            let name = idx_name(i);
            self.obj_mut(v).base._put(
                &name,
                Value::Property(Box::new(ValueProperty {
                    writable: true,
                    configurable: true,
                    enumerable: true,
                    ..ValueProperty::default()
                })),
            );
            ArgumentsObject::data_mut(self, v)
                .mapped
                .insert(name, MappedProperty { stash, idx: i });
            i += 1;
        }
        let extra = self.heap.stash(stash).extra_args.clone();
        for a in extra {
            self.obj_mut(v).base._put(&idx_name(i), a);
            i += 1;
        }
        let callee = self.vm.stack[self.vm.sb as usize - 1].clone();
        BaseObject::_put_prop(self, v, &crate::vm::STR_CALLEE, callee, true, false, true);
        let av = self.get_array_values();
        BaseObject::_put_sym(
            self,
            v,
            SYM_ITERATOR,
            value_prop(Value::Object(av), true, false, true),
        );
        v
    }

    pub fn create_args_unmapped(&mut self, formal_args: usize) -> ObjRef {
        let proto = self.global.object_prototype;
        let args = self.new_base_object(proto, "Arguments");
        let stash = self.vm.stash.expect("stash");
        let c = formal_args.min(self.vm.args as usize);
        let mut values = self.heap.stash(stash).values[..c].to_vec();
        values.extend_from_slice(&self.heap.stash(stash).extra_args);
        for (i, v) in values.into_iter().enumerate() {
            self.obj_mut(args).base._put(&idx_name(i), v);
        }
        let len = int_to_value(self.vm.args as i64);
        BaseObject::_put_prop(self, args, &crate::vm::STR_LENGTH, len, true, false, true);
        let thrower = self.new_thrower_property(false);
        self.obj_mut(args)
            .base
            ._put(&crate::vm::STR_CALLEE, thrower);
        let av = self.get_array_values();
        BaseObject::_put_sym(
            self,
            args,
            SYM_ITERATOR,
            value_prop(Value::Object(av), true, false, true),
        );
        args
    }
}
