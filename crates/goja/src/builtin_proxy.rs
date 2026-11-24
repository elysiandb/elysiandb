use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_proxy_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_proxy_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_proxy_template)
}

impl Runtime {
    pub fn get_proxy(&mut self) -> ObjRef {
        if let Some(r) = self.global.proxy {
            return r;
        }
        let ret = self.reserve_object();
        self.global.proxy = Some(ret);
        self.new_templated_func_object(get_proxy_template(), Some(ret), None, None);
        ret
    }
}

pub fn proxy_target(rt: &Runtime, o: ObjRef) -> Option<Option<ObjRef>> {
    if rt.is_proxy(o) {
        return Some(None);
    }
    None
}
