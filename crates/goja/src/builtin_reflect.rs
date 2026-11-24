use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_reflect_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_reflect_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_reflect_template)
}

impl Runtime {
    pub fn get_reflect(&mut self) -> ObjRef {
        if let Some(r) = self.global.reflect {
            return r;
        }
        let ret = self.reserve_object();
        self.global.reflect = Some(ret);
        self.new_templated_object(get_reflect_template(), Some(ret));
        ret
    }
}
