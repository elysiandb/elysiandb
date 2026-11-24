use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_map_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_map_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_map_template)
}

fn create_map_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_map_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_map_proto_template)
}

fn create_map_iterator_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_map_iterator_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_map_iterator_proto_template)
}

impl Runtime {
    pub fn get_map(&mut self) -> ObjRef {
        if let Some(r) = self.global.map {
            return r;
        }
        let ret = self.reserve_object();
        self.global.map = Some(ret);
        self.new_templated_func_object(get_map_template(), Some(ret), None, None);
        ret
    }

    pub fn get_map_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.map_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.map_prototype = Some(ret);
        self.new_templated_object(get_map_proto_template(), Some(ret));
        ret
    }

    pub fn get_map_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.map_iterator_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.map_iterator_prototype = Some(ret);
        self.new_templated_object(get_map_iterator_proto_template(), Some(ret));
        ret
    }
}
