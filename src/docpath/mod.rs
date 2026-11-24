#[allow(clippy::module_inception)]
mod docpath;

pub use docpath::{
    CORE_FIELDS_PREFIX, Node, Resolved, SortValue, is_searchable_key, match_glob, match_glob_lower,
    parse_date, parse_sort_date, resolve_map_values, resolve_values, sort_map_value, sort_value,
    split,
};
