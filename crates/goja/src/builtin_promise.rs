use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_promise_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_promise_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_promise_template)
}

fn create_promise_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_promise_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_promise_proto_template)
}

impl Runtime {
    pub fn get_promise(&mut self) -> ObjRef {
        if let Some(r) = self.global.promise {
            return r;
        }
        let ret = self.reserve_object();
        self.global.promise = Some(ret);
        self.new_templated_func_object(get_promise_template(), Some(ret), None, None);
        ret
    }

    pub fn get_promise_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.promise_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.promise_prototype = Some(ret);
        self.new_templated_object(get_promise_proto_template(), Some(ret));
        ret
    }
}
