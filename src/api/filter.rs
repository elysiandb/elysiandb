use std::collections::BTreeSet;

use crate::datastore::element_string;
use crate::docpath::{self, Resolved};
use crate::gocompat::json::{Map, Value};
use crate::gocompat::strconv;
use crate::gocompat::strings;
use crate::gocompat::time::{Duration, Time};
use crate::query::{Filters, Ops};

pub fn search_pattern(search: &[u8]) -> Vec<u8> {
    if search.contains(&b'*') {
        return search.to_vec();
    }

    let mut out = Vec::with_capacity(search.len() + 2);
    out.push(b'*');
    out.extend_from_slice(search);
    out.push(b'*');
    out
}

enum SearchItem<'a> {
    Value(&'a Value),
    Map(&'a Map),
}

pub fn search_matches_entity(entity_data: &Map, search: &[u8]) -> bool {
    let pattern = strings::to_lower(&search_pattern(search));

    let mut stack: Vec<SearchItem<'_>> = vec![SearchItem::Map(entity_data)];
    while let Some(cur) = stack.pop() {
        match cur {
            SearchItem::Map(m) | SearchItem::Value(Value::Object(m)) => {
                for (key, val) in m {
                    if !is_searchable_key(key.as_bytes()) {
                        continue;
                    }

                    stack.push(SearchItem::Value(val));
                }
            }
            SearchItem::Value(Value::Array(items)) => {
                stack.extend(items.iter().map(SearchItem::Value));
            }
            SearchItem::Value(Value::DocList(maps)) => {
                stack.extend(maps.iter().map(SearchItem::Map));
            }
            SearchItem::Value(Value::String(s)) => {
                if docpath::match_glob_lower(&pattern, &strings::to_lower(s.as_bytes())) {
                    return true;
                }
            }
            SearchItem::Value(_) => {}
        }
    }

    false
}

fn is_searchable_key(key: &[u8]) -> bool {
    docpath::is_searchable_key(key)
}

pub fn filters_match_entity(entity_data: &Map, filters: &Filters) -> bool {
    match_leaf_strict(entity_data, filters)
}

pub fn stored_filters_match_entity(entity_data: &Map, filters: &Filters) -> bool {
    for (field, ops) in filters {
        let values = resolve_values(entity_data, field);
        if !values.is_empty() && !match_any_value(&values, ops) {
            return false;
        }
    }

    true
}

pub(crate) fn match_boolean(value: bool, ops: &Ops) -> bool {
    for (op, cmp) in ops {
        let b = cmp == b"true" || cmp == b"1";
        match op.as_slice() {
            b"eq" => {
                if value != b {
                    return false;
                }
            }
            b"neq" => {
                if value == b {
                    return false;
                }
            }
            _ => return false,
        }
    }

    true
}

fn match_array(arr: &[Value], ops: &Ops) -> bool {
    let arr_str: Vec<Vec<u8>> = arr.iter().map(element_string).collect();
    let in_array = |val: &[u8]| arr_str.iter().any(|a| a == val);

    for (op, cmp) in ops {
        let values: Vec<&[u8]> = cmp.split(|&c| c == b',').collect();

        match op.as_slice() {
            b"contains" => {
                if !in_array(cmp) {
                    return false;
                }
            }
            b"not_contains" => {
                if in_array(cmp) {
                    return false;
                }
            }
            b"all" => {
                for v in &values {
                    if !in_array(v) {
                        return false;
                    }
                }
            }
            b"any" => {
                if !values.iter().any(|v| in_array(v)) {
                    return false;
                }
            }
            b"eq" => {
                if arr_str.len() != values.len() {
                    return false;
                }

                let matched: BTreeSet<&[u8]> = arr_str.iter().map(Vec::as_slice).collect();
                for v in &values {
                    if !matched.contains(v) {
                        return false;
                    }
                }
            }
            b"none" => {
                for v in &values {
                    if in_array(v) {
                        return false;
                    }
                }
            }
            _ => return false,
        }
    }

    true
}

pub(crate) fn is_comparison_operator(op: &[u8]) -> bool {
    matches!(op, b"eq" | b"neq" | b"lt" | b"lte" | b"gt" | b"gte")
}

pub(crate) fn day() -> Duration {
    Duration(24 * Duration::HOUR.0)
}

fn match_date(value: &[u8], ops: &Ops) -> (bool, bool) {
    let Some((t_val, date_only_val)) = parse_date(value) else {
        return (false, false);
    };

    let mut comparisons: Vec<(&[u8], Time, bool)> = Vec::with_capacity(ops.len());
    for (op, cmp) in ops {
        if !is_comparison_operator(op) {
            return (false, false);
        }

        let Some((t_cmp, date_only_cmp)) = parse_date(cmp) else {
            return (false, false);
        };

        comparisons.push((op, t_cmp, date_only_cmp));
    }

    for (op, value, date_only) in comparisons {
        let (mut tv, mut tc) = (t_val.clone(), value);
        if date_only_val || date_only {
            tv = tv.truncate(day());
            tc = tc.truncate(day());
        }

        let holds = match op {
            b"eq" => tv.equal(&tc),
            b"neq" => !tv.equal(&tc),
            b"lt" => tv.before(&tc),
            b"lte" => !tv.after(&tc),
            b"gt" => tv.after(&tc),
            b"gte" => !tv.before(&tc),
            _ => true,
        };
        if !holds {
            return (true, false);
        }
    }

    (true, true)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

fn match_string(value: &[u8], ops: &Ops) -> bool {
    for (op, cmp) in ops {
        let cmp = cmp.as_slice();
        let holds = match op.as_slice() {
            b"eq" => docpath::match_glob(cmp, value),
            b"neq" => !docpath::match_glob(cmp, value),
            b"lt" => value < cmp,
            b"lte" => value <= cmp,
            b"gt" => value > cmp,
            b"gte" => value >= cmp,
            b"contains" => contains(&strings::to_lower(value), &strings::to_lower(cmp)),
            b"not_contains" => !contains(&strings::to_lower(value), &strings::to_lower(cmp)),
            _ => false,
        };
        if !holds {
            return false;
        }
    }

    true
}

pub(crate) fn match_string_or_date(value: &[u8], ops: &Ops) -> bool {
    let (handled, ok) = match_date(value, ops);
    if handled {
        return ok;
    }

    match_string(value, ops)
}

pub(crate) fn match_number(value: f64, ops: &Ops) -> bool {
    for (op, cmp) in ops {
        let Ok(num) = strconv::parse_float(cmp, 64) else {
            return false;
        };

        let holds = match op.as_slice() {
            b"eq" => value == num,
            b"neq" => value != num,
            b"lt" => value < num,
            b"lte" => value <= num,
            b"gt" => value > num,
            b"gte" => value >= num,
            _ => false,
        };
        if !holds {
            return false;
        }
    }

    true
}

pub(crate) fn parse_date(s: &[u8]) -> Option<(Time, bool)> {
    docpath::parse_date(s)
}

pub(crate) fn match_leaf_strict(entity: &Map, filters: &Filters) -> bool {
    for (field, ops) in filters {
        if !match_any_value(&resolve_values(entity, field), ops) {
            return false;
        }
    }

    true
}

fn match_any_value(values: &[Resolved<'_>], ops: &Ops) -> bool {
    values.iter().any(|v| match_value(v, ops))
}

fn match_value(val: &Resolved<'_>, ops: &Ops) -> bool {
    match val {
        Resolved::Value(Value::String(s)) => match_string_or_date(s.as_bytes(), ops),
        Resolved::Value(Value::Number(f)) => match_number(*f, ops),
        Resolved::Value(Value::Int(n)) => match_number(*n as f64, ops),
        Resolved::Value(Value::Array(a)) => match_array(a, ops),
        Resolved::Value(Value::Bool(b)) => match_boolean(*b, ops),
        _ => false,
    }
}

pub(crate) fn resolve_values<'a>(data: &'a Map, path: &[u8]) -> Vec<Resolved<'a>> {
    docpath::resolve_map_values(data, path)
}
