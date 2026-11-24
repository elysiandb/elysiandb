use crate::heap::ObjRef;
use crate::object_template::ObjectTemplate;
use crate::runtime::Runtime;
use std::sync::OnceLock;

fn create_typed_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_typed_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_typed_array_template)
}

fn create_typed_array_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_typed_array_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_typed_array_proto_template)
}

fn create_typed_array_values_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_typed_array_values_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_typed_array_values_template)
}

fn create_uint8_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_uint8_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_uint8_array_template)
}

fn create_uint8_clamped_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_uint8_clamped_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_uint8_clamped_array_template)
}

fn create_int8_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_int8_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_int8_array_template)
}

fn create_uint16_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_uint16_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_uint16_array_template)
}

fn create_int16_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_int16_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_int16_array_template)
}

fn create_uint32_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_uint32_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_uint32_array_template)
}

fn create_int32_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_int32_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_int32_array_template)
}

fn create_float32_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_float32_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_float32_array_template)
}

fn create_float64_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_float64_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_float64_array_template)
}

fn create_big_int64_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_big_int64_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_int64_array_template)
}

fn create_big_uint64_array_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_big_uint64_array_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_big_uint64_array_template)
}

fn create_data_view_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_data_view_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_data_view_template)
}

fn create_data_view_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_data_view_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_data_view_proto_template)
}

fn create_array_buffer_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));
    t
}

pub fn get_array_buffer_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_buffer_template)
}

fn create_array_buffer_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t
}

pub fn get_array_buffer_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_buffer_proto_template)
}

impl Runtime {
    pub fn get_typed_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.typed_array = Some(ret);
        self.new_templated_func_object(get_typed_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_typed_array_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.typed_array_prototype = Some(ret);
        self.new_templated_object(get_typed_array_proto_template(), Some(ret));
        ret
    }

    pub fn get_typed_array_values(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array_values {
            return r;
        }
        let ret = self.reserve_object();
        self.global.typed_array_values = Some(ret);
        self.new_templated_func_object(get_typed_array_values_template(), Some(ret), None, None);
        ret
    }

    pub fn get_uint8_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint8_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint8_array = Some(ret);
        self.new_templated_func_object(get_uint8_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_uint8_clamped_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint8_clamped_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint8_clamped_array = Some(ret);
        self.new_templated_func_object(get_uint8_clamped_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_int8_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int8_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int8_array = Some(ret);
        self.new_templated_func_object(get_int8_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_uint16_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint16_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint16_array = Some(ret);
        self.new_templated_func_object(get_uint16_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_int16_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int16_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int16_array = Some(ret);
        self.new_templated_func_object(get_int16_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_uint32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint32_array = Some(ret);
        self.new_templated_func_object(get_uint32_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_int32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int32_array = Some(ret);
        self.new_templated_func_object(get_int32_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_float32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.float32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.float32_array = Some(ret);
        self.new_templated_func_object(get_float32_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_float64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.float64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.float64_array = Some(ret);
        self.new_templated_func_object(get_float64_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_big_int64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int64_array = Some(ret);
        self.new_templated_func_object(get_big_int64_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_big_uint64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_uint64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_uint64_array = Some(ret);
        self.new_templated_func_object(get_big_uint64_array_template(), Some(ret), None, None);
        ret
    }

    pub fn get_data_view(&mut self) -> ObjRef {
        if let Some(r) = self.global.data_view {
            return r;
        }
        let ret = self.reserve_object();
        self.global.data_view = Some(ret);
        self.new_templated_func_object(get_data_view_template(), Some(ret), None, None);
        ret
    }

    pub fn get_data_view_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.data_view_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.data_view_prototype = Some(ret);
        self.new_templated_object(get_data_view_proto_template(), Some(ret));
        ret
    }

    pub fn get_array_buffer(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_buffer {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_buffer = Some(ret);
        self.new_templated_func_object(get_array_buffer_template(), Some(ret), None, None);
        ret
    }

    pub fn get_array_buffer_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_buffer_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_buffer_prototype = Some(ret);
        self.new_templated_object(get_array_buffer_proto_template(), Some(ret));
        ret
    }
}

pub fn add_typed_arrays(t: &mut ObjectTemplate) {
    use crate::value::{Value, value_prop};
    t.put_str("ArrayBuffer", |r| {
        value_prop(Value::Object(r.get_array_buffer()), true, false, true)
    });
    t.put_str("DataView", |r| {
        value_prop(Value::Object(r.get_data_view()), true, false, true)
    });
    t.put_str("Uint8Array", |r| {
        value_prop(Value::Object(r.get_uint8_array()), true, false, true)
    });
    t.put_str("Uint8ClampedArray", |r| {
        value_prop(
            Value::Object(r.get_uint8_clamped_array()),
            true,
            false,
            true,
        )
    });
    t.put_str("Int8Array", |r| {
        value_prop(Value::Object(r.get_int8_array()), true, false, true)
    });
    t.put_str("Uint16Array", |r| {
        value_prop(Value::Object(r.get_uint16_array()), true, false, true)
    });
    t.put_str("Int16Array", |r| {
        value_prop(Value::Object(r.get_int16_array()), true, false, true)
    });
    t.put_str("Uint32Array", |r| {
        value_prop(Value::Object(r.get_uint32_array()), true, false, true)
    });
    t.put_str("Int32Array", |r| {
        value_prop(Value::Object(r.get_int32_array()), true, false, true)
    });
    t.put_str("Float32Array", |r| {
        value_prop(Value::Object(r.get_float32_array()), true, false, true)
    });
    t.put_str("Float64Array", |r| {
        value_prop(Value::Object(r.get_float64_array()), true, false, true)
    });
    t.put_str("BigInt64Array", |r| {
        value_prop(Value::Object(r.get_big_int64_array()), true, false, true)
    });
    t.put_str("BigUint64Array", |r| {
        value_prop(Value::Object(r.get_big_uint64_array()), true, false, true)
    });
}
