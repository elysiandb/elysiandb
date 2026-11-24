use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_string_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_string_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_string_template)
}

fn create_string_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_string_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_string_proto_template)
}

fn create_string_iterator_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_string_iterator_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_string_iterator_proto_template)
}

fn create_stringproto_trim_end_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_stringproto_trim_end_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_stringproto_trim_end_template)
}

fn create_stringproto_trim_start_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_stringproto_trim_start_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_stringproto_trim_start_template)
}

impl Runtime {
    pub fn get_string(&mut self) -> ObjRef {
        if let Some(r) = self.global.string {
            return r;
        }
        let ret = self.reserve_object();
        self.global.string = Some(ret);
        self.new_templated_func_object(get_string_template(), Some(ret), None, None);
        ret
    }

    pub fn get_string_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.string_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.string_prototype = Some(ret);
        self.new_templated_object(get_string_proto_template(), Some(ret));
        ret
    }

    pub fn get_string_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.string_iterator_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.string_iterator_prototype = Some(ret);
        self.new_templated_object(get_string_iterator_proto_template(), Some(ret));
        ret
    }

    pub fn get_stringproto_trim_end(&mut self) -> ObjRef {
        if let Some(r) = self.global.stringproto_trim_end {
            return r;
        }
        let ret = self.reserve_object();
        self.global.stringproto_trim_end = Some(ret);
        self.new_templated_func_object(get_stringproto_trim_end_template(), Some(ret), None, None);
        ret
    }

    pub fn get_stringproto_trim_start(&mut self) -> ObjRef {
        if let Some(r) = self.global.stringproto_trim_start {
            return r;
        }
        let ret = self.reserve_object();
        self.global.stringproto_trim_start = Some(ret);
        self.new_templated_func_object(
            get_stringproto_trim_start_template(),
            Some(ret),
            None,
            None,
        );
        ret
    }
}
