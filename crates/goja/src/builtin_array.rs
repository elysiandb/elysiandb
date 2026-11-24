use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_template)
}

fn create_array_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_array_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_proto_template)
}

fn create_array_values_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_array_values_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_values_template)
}

fn create_array_to_string_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_array_to_string_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_to_string_template)
}

fn create_array_iterator_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_array_iterator_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_iterator_proto_template)
}

impl Runtime {
    pub fn get_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array = Some(ret);
        self.new_templated_func_object(get_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_array_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_prototype = Some(ret);
        self.new_templated_array_object(get_array_proto_template(), Some(ret));
        ret
    }

    pub fn get_array_values(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_values {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_values = Some(ret);
        self.new_templated_func_object(get_array_values_template(), Some(ret), None, None);
        ret
    }

    pub fn get_array_to_string(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_to_string {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_to_string = Some(ret);
        self.new_templated_func_object(get_array_to_string_template(), Some(ret), None, None);
        ret
    }

    pub fn get_array_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_iterator_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_iterator_prototype = Some(ret);
        self.new_templated_object(get_array_iterator_proto_template(), Some(ret));
        ret
    }
}
