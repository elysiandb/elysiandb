use crate::gocompat::json::Map;

pub fn deep_copy_document(document: &Map) -> Map {
    document.clone()
}
