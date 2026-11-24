use crate::api::{self, ImportData, ImportRejection};
use crate::gocompat::errors::BoxError;
use crate::storage;

pub use crate::api::{
    ByteFields, add_entity_type, apply_filters_to_list, apply_includes, count_all_entities,
    count_entities_per_type, create_entity_type, delete_all, delete_all_entities,
    delete_entity_by_id, delete_entity_type, entity_exists, entity_type_exists, filter_fields,
    get_entity_schema, get_list_of_ids, import_all, is_core_entity_type, iterate_entities,
    list_entities, list_entity_types, list_public_entity_types, read_entity_by_id,
    restore_entities, update_entity_by_id, update_entity_schema, update_list_of_entities,
    write_entity, write_entity_bytes, write_list_of_entities,
};

pub fn validate_import(data: &ImportData) -> Vec<ImportRejection> {
    api::validate_import(data, &get_entity_schema)
}

pub fn wipe_all() -> Result<(), BoxError> {
    storage::clear_all();

    Ok(())
}
