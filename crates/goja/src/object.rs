use crate::unistring;
use std::sync::Arc;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PrivateEnvType {
    pub num_fields: u32,
    pub num_methods: u32,
}

#[derive(Clone, Debug)]
pub struct PrivateId {
    pub typ: Arc<PrivateEnvType>,
    pub name: unistring::String,
    pub idx: u32,
    pub is_method: bool,
}
