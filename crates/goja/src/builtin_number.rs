use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_number_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_number_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_number_template)
}

fn create_number_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_number_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_number_proto_template)
}

fn create_parse_float_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_parse_float_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_parse_float_template)
}

fn create_parse_int_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_parse_int_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_parse_int_template)
}

impl Runtime {
    pub fn get_number(&mut self) -> ObjRef {
        if let Some(r) = self.global.number {
            return r;
        }
        let ret = self.reserve_object();
        self.global.number = Some(ret);
        self.new_templated_func_object(get_number_template(), Some(ret), None, None);
        ret
    }

    pub fn get_number_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.number_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.number_prototype = Some(ret);
        self.new_templated_object(get_number_proto_template(), Some(ret));
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
