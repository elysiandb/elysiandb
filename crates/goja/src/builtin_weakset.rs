use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_weak_set_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_weak_set_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_weak_set_template)
}

fn create_weak_set_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_weak_set_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_weak_set_proto_template)
}

impl Runtime {
    pub fn get_weak_set(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_set {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_set = Some(ret);
        self.new_templated_func_object(get_weak_set_template(), Some(ret), None, None);
        ret
    }

    pub fn get_weak_set_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_set_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_set_prototype = Some(ret);
        self.new_templated_object(get_weak_set_proto_template(), Some(ret));
        ret
    }
}
