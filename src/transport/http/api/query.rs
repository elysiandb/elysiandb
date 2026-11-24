use std::collections::BTreeMap;
use std::fmt;

use crate::crypto::sha256;
use crate::gocompat::json::{self, Decode, DecodeState, Error, Expected, Map, RawMessage, Value};
use crate::gocompat::strconv;
use crate::json_struct;
use crate::query::{FilterNode, Filters, Ops, Query};

json_struct! {
    #[derive(Default)]
    struct RawPayload {
        entity: String => "entity",
        offset: i64 => "offset",
        limit: i64 => "limit",
        filters: Option<Map> => "filters",
        sorts: Option<BTreeMap<String, String>> => "sorts",
        count_only: bool => "countOnly",
        fields: String => "fields",
    }
}

json_struct! {
    #[derive(Default)]
    struct RawSorts {
        sorts: RawMessage => "sorts",
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryPayload {
    pub entity: String,
    pub offset: i64,
    pub limit: i64,
    pub filters: Option<Map>,
    pub sorts: Option<BTreeMap<String, String>>,
    pub count_only: bool,
    pub fields: String,
    pub sort_order: Vec<String>,
}

impl Decode for QueryPayload {
    const EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        let data = d.data();
        let start = d.start();
        let mut payload = RawPayload::default();
        payload.decode(d)?;
        let end = d.start();
        let raw_value = &data[start..end];

        let mut raw = RawSorts::default();
        let _ = json::unmarshal(raw_value, &mut raw);

        *self = QueryPayload {
            entity: payload.entity,
            offset: payload.offset,
            limit: payload.limit,
            filters: payload.filters,
            sorts: payload.sorts,
            count_only: payload.count_only,
            fields: payload.fields,
            sort_order: object_keys_in_order(raw.sorts.as_bytes()),
        };

        Ok(())
    }
}

fn object_keys_in_order(raw: &[u8]) -> Vec<String> {
    let mut d = DecodeState::new(raw);
    if raw.is_empty() || d.peek() != json::Kind::Object || !json::valid(raw) {
        return Vec::new();
    }

    let mut keys = Vec::new();
    let _ = d.object_members(|d, key| {
        keys.push(String::from_utf8_lossy(key).into_owned());
        d.skip();
        Ok(())
    });

    keys
}

json_struct! {
    #[derive(Default)]
    struct NormalizedSort {
        field: Vec<u8> => "Field",
        ascending: bool => "Ascending",
    }
}

json_struct! {
    #[derive(Default)]
    struct NormalizedPayload {
        count_only: bool => "countOnly",
        entity: String => "entity",
        fields: String => "fields",
        filters: Option<Map> => "filters",
        limit: i64 => "limit",
        offset: i64 => "offset",
        sort_order: Vec<NormalizedSort> => "sortOrder",
        sorts: Option<BTreeMap<String, String>> => "sorts",
    }
}

impl QueryPayload {
    pub fn hash(&self) -> [u8; sha256::SIZE] {
        let normalized = NormalizedPayload {
            count_only: self.count_only,
            entity: self.entity.clone(),
            fields: self.fields.clone(),
            filters: Some(self.filters.clone().unwrap_or_default()),
            limit: self.limit,
            offset: self.offset,
            sort_order: self
                .query_for_sorts()
                .ordered_sorts()
                .into_iter()
                .map(|s| NormalizedSort {
                    field: s.field,
                    ascending: s.ascending,
                })
                .collect(),
            sorts: self.sorts.clone(),
        };

        let b = json::marshal(&normalized).unwrap_or_default();

        sha256::sum256(&b)
    }

    pub fn query_for_sorts(&self) -> Query {
        Query {
            sorts: sort_map(self.sorts.as_ref()),
            sort_order: self
                .sort_order
                .iter()
                .map(|k| k.as_bytes().to_vec())
                .collect(),
            ..Query::default()
        }
    }
}

pub fn sort_map(sorts: Option<&BTreeMap<String, String>>) -> BTreeMap<Vec<u8>, Vec<u8>> {
    sorts
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn decode_query_payload(body: &[u8]) -> Result<QueryPayload, Error> {
    let mut payload = QueryPayload::default();
    json::unmarshal(body, &mut payload)?;
    Ok(payload)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError(String);

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FilterError {}

pub fn filter_value_to_string(val: &Value) -> Option<Vec<u8>> {
    match val {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Number(f) => Some(strconv::format_float(*f, b'f', -1, 64).into_bytes()),
        Value::Bool(b) => Some(strconv::format_bool(*b).as_bytes().to_vec()),
        _ => None,
    }
}

fn parse_leaf(raw: &Map) -> Result<Filters, FilterError> {
    let mut leaf = Filters::new();
    for (field, v) in raw {
        let Value::Object(ops_raw) = v else {
            return Err(FilterError(format!("invalid filter for field {field}")));
        };

        let mut ops = Ops::new();
        for (op, val) in ops_raw {
            let Some(s) = filter_value_to_string(val) else {
                return Err(FilterError(format!(
                    "invalid value for {field}.{op}: expected a string, a number or a boolean"
                )));
            };

            ops.insert(op.as_bytes().to_vec(), s);
        }

        leaf.insert(field.as_bytes().to_vec(), ops);
    }

    Ok(leaf)
}

struct ParseFrame<'a> {
    items: &'a [Value],
    is_or: bool,
    next: usize,
    nodes: Vec<FilterNode>,
}

fn item_error(is_or: bool) -> FilterError {
    FilterError(if is_or {
        "or item must be an object".to_string()
    } else {
        "and item must be an object".to_string()
    })
}

pub fn parse_filter_node(raw: Option<&Map>) -> Result<FilterNode, FilterError> {
    let Some(root) = raw else {
        return Ok(FilterNode::default());
    };

    let mut stack: Vec<ParseFrame<'_>> = Vec::new();
    let mut cur: &Map = root;
    loop {
        let branch = match (cur.get("or"), cur.get("and")) {
            (Some(v), _) => Some((v, true)),
            (None, Some(v)) => Some((v, false)),
            (None, None) => None,
        };

        let mut result = match branch {
            Some((v, is_or)) => {
                let Value::Array(items) = v else {
                    return Err(FilterError(if is_or {
                        "or must be an array".to_string()
                    } else {
                        "and must be an array".to_string()
                    }));
                };

                match items.first() {
                    None => {
                        if is_or {
                            FilterNode::with_or(Vec::new())
                        } else {
                            FilterNode::with_and(Vec::new())
                        }
                    }
                    Some(Value::Object(m)) => {
                        stack.push(ParseFrame {
                            items,
                            is_or,
                            next: 0,
                            nodes: Vec::with_capacity(items.len()),
                        });
                        cur = m;
                        continue;
                    }
                    Some(_) => return Err(item_error(is_or)),
                }
            }
            None => FilterNode::with_leaf(parse_leaf(cur)?),
        };

        loop {
            let Some(top) = stack.last_mut() else {
                return Ok(result);
            };

            top.nodes.push(result);
            top.next += 1;
            let items = top.items;
            if top.next < items.len() {
                let Value::Object(m) = &items[top.next] else {
                    return Err(item_error(top.is_or));
                };
                cur = m;
                break;
            }

            let Some(done) = stack.pop() else {
                return Ok(FilterNode::default());
            };
            result = if done.is_or {
                FilterNode::with_or(done.nodes)
            } else {
                FilterNode::with_and(done.nodes)
            };
        }
    }
}
