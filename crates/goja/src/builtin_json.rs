use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_json_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_json_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_json_template)
}

impl Runtime {
    pub fn get_json(&mut self) -> ObjRef {
        if let Some(r) = self.global.json {
            return r;
        }
        let ret = self.reserve_object();
        self.global.json = Some(ret);
        self.new_templated_object(get_json_template(), Some(ret));
        ret
    }
}
