use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_set_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_set_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_set_template)
}

fn create_set_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_set_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_set_proto_template)
}

fn create_set_iterator_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_set_iterator_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_set_iterator_proto_template)
}

impl Runtime {
    pub fn get_set(&mut self) -> ObjRef {
        if let Some(r) = self.global.set {
            return r;
        }
        let ret = self.reserve_object();
        self.global.set = Some(ret);
        self.new_templated_func_object(get_set_template(), Some(ret), None, None);
        ret
    }

    pub fn get_set_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.set_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.set_prototype = Some(ret);
        self.new_templated_object(get_set_proto_template(), Some(ret));
        ret
    }

    pub fn get_set_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.set_iterator_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.set_iterator_prototype = Some(ret);
        self.new_templated_object(get_set_iterator_proto_template(), Some(ret));
        ret
    }
}
