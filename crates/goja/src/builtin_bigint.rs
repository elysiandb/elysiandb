use crate::deps::big;
use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use crate::string_ascii::{ParseErr, string_to_int};
use std::sync::OnceLock;

pub(crate) fn string_to_big_int(s: &[u8]) -> Option<big::Int> {
    match string_to_int(s) {
        Ok(n) => Some(big::Int::new_int(n)),
        Err(ParseErr::Range) => Some(big::Int::set_string(s, 0).unwrap_or_default()),
        Err(ParseErr::Syntax) if s == b"-0" => Some(big::Int::new()),
        Err(ParseErr::Syntax) => None,
    }
}

fn create_big_int_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_big_int_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_int_template)
}

fn create_big_int_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_big_int_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_int_proto_template)
}

impl Runtime {
    pub fn get_big_int(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int = Some(ret);
        self.new_templated_func_object(get_big_int_template(), Some(ret), None, None);
        ret
    }

    pub fn get_big_int_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int_prototype = Some(ret);
        self.new_templated_object(get_big_int_proto_template(), Some(ret));
        ret
    }
}
