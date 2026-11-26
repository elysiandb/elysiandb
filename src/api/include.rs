use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use crate::gocompat::json::{Map, Value};
use crate::gocompat::strings;
use crate::query::Filters;
use crate::sync::RwLock;

use super::plan::include_reader;
use super::storage::is_core_entity_type;

const MAX_INCLUDE_DEPTH: usize = 32;

pub type ReadEntityById = Arc<dyn Fn(&[u8], &[u8]) -> Option<Map> + Send + Sync>;

static READ_ENTITY_BY_ID_FUNC: RwLock<Option<ReadEntityById>> = RwLock::new(None);

pub fn set_read_entity_by_id_func(f: Option<ReadEntityById>) {
    *READ_ENTITY_BY_ID_FUNC.write() = f;
}

pub fn read_entity_by_id_func() -> Option<ReadEntityById> {
    READ_ENTITY_BY_ID_FUNC.read().clone()
}

pub type EntityReader<'a> = Box<dyn FnMut(&[u8], &[u8]) -> Option<Map> + 'a>;

type IncludeTree = BTreeMap<Vec<u8>, Vec<Vec<u8>>>;

pub fn apply_includes(data: &mut [Map], includes_param: &[u8]) {
    let mut read = include_reader();
    apply_includes_with(data, includes_param, &mut read);
}

pub(crate) fn apply_includes_with(
    data: &mut [Map],
    includes_param: &[u8],
    read: &mut EntityReader<'_>,
) {
    let all_mode = strings::trim_space(includes_param) == b"all";
    let include_tree = build_include_tree(includes_param, all_mode);
    for entity_data in data.iter_mut() {
        let doc = std::mem::take(entity_data);
        *entity_data = apply_includes_iterative(doc, include_tree.clone(), all_mode, read);
    }
}

fn split_once_dot(s: &[u8]) -> (&[u8], Option<&[u8]>) {
    match s.iter().position(|&c| c == b'.') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    }
}

fn add_path(tree: &mut IncludeTree, path: &[u8]) {
    let (head, rest) = split_once_dot(path);
    tree.entry(head.to_vec())
        .or_default()
        .push(rest.unwrap_or_default().to_vec());
}

fn build_include_tree(includes_param: &[u8], all_mode: bool) -> IncludeTree {
    let mut include_tree = IncludeTree::new();
    if all_mode {
        return include_tree;
    }

    for inc in includes_param.split(|&c| c == b',') {
        add_path(&mut include_tree, strings::trim_space(inc));
    }

    include_tree
}

fn build_next_tree(subs: &[Vec<u8>], force_all: bool) -> IncludeTree {
    let mut next = IncludeTree::new();
    if force_all {
        return next;
    }

    for sub in subs {
        if !sub.is_empty() {
            add_path(&mut next, sub);
        }
    }

    next
}

fn include_reference(m: &Map) -> Vec<u8> {
    let ent = match m.get("@entity") {
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"".as_slice(),
    };
    let id = match m.get("id") {
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"".as_slice(),
    };
    if ent.is_empty() || id.is_empty() {
        return Vec::new();
    }

    let mut key = Vec::with_capacity(ent.len() + 1 + id.len());
    key.extend_from_slice(ent);
    key.push(0);
    key.extend_from_slice(id);
    key
}

fn has_entity_key(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.contains_key("@entity"))
}

fn collect_include_fields(entity_data: &Map, tree: &IncludeTree, force_all: bool) -> Vec<Vec<u8>> {
    if !force_all {
        return tree.keys().cloned().collect();
    }

    let mut fields = Vec::new();
    for (k, v) in entity_data {
        let include = match v {
            Value::Object(m) => m.contains_key("@entity"),
            Value::Array(items) => items.iter().any(has_entity_key),
            _ => false,
        };
        if include {
            fields.push(k.as_bytes().to_vec());
        }
    }

    fields
}

fn include_single_entity(m: Map, reader: &mut EntityReader<'_>) -> Map {
    if let Some(Value::String(ent)) = m.get("@entity")
        && !is_core_entity_type(ent.as_bytes())
        && let Some(Value::String(id)) = m.get("id")
        && !id.is_empty()
        && let Some(mut read) = reader(ent.as_bytes(), id.as_bytes())
    {
        read.insert("@entity".to_string(), Value::String(ent.clone()));
        return read;
    }

    m
}

enum Dest {
    Root,
    Field(String),
    Item,
}

struct ArrayWork {
    field: String,
    items: std::vec::IntoIter<Value>,
    out: Vec<Map>,
}

struct Frame {
    map: Map,
    tree: IncludeTree,
    fields: std::vec::IntoIter<Vec<u8>>,
    array: Option<ArrayWork>,
    depth: usize,
    ancestor: Vec<u8>,
    dest: Dest,
}

fn new_frame(
    map: Map,
    tree: IncludeTree,
    force_all: bool,
    depth: usize,
    ancestor: Vec<u8>,
    dest: Dest,
) -> Frame {
    let fields = if depth >= MAX_INCLUDE_DEPTH {
        Vec::new()
    } else {
        collect_include_fields(&map, &tree, force_all)
    };

    Frame {
        map,
        tree,
        fields: fields.into_iter(),
        array: None,
        depth,
        ancestor,
        dest,
    }
}

fn deliver(stack: &mut [Frame], dest: Dest, map: Map) -> Option<Map> {
    match dest {
        Dest::Root => Some(map),
        Dest::Field(field) => {
            if let Some(parent) = stack.last_mut() {
                parent.map.insert(field, Value::Object(map));
            }
            None
        }
        Dest::Item => {
            if let Some(parent) = stack.last_mut()
                && let Some(work) = parent.array.as_mut()
            {
                work.out.push(map);
            }
            None
        }
    }
}

fn apply_includes_iterative(
    doc: Map,
    tree: IncludeTree,
    force_all: bool,
    read: &mut EntityReader<'_>,
) -> Map {
    let mut ancestors: HashSet<Vec<u8>> = HashSet::new();
    let mut stack = vec![new_frame(doc, tree, force_all, 0, Vec::new(), Dest::Root)];

    loop {
        let Some(top) = stack.last_mut() else {
            return Map::new();
        };

        let mut descend: Option<(Map, Vec<Vec<u8>>, Dest)> = None;
        if let Some(work) = top.array.as_mut() {
            match work.items.next() {
                Some(mut item) => {
                    let Value::Object(m) = &mut item else {
                        continue;
                    };
                    let m = std::mem::take(m);
                    let subs = top
                        .tree
                        .get(work.field.as_bytes())
                        .cloned()
                        .unwrap_or_default();
                    descend = Some((m, subs, Dest::Item));
                }
                None => {
                    if let Some(work) = top.array.take() {
                        top.map.insert(work.field, Value::DocList(work.out));
                    }
                    continue;
                }
            }
        } else if let Some(field) = top.fields.next() {
            let Ok(key) = String::from_utf8(field) else {
                continue;
            };
            match top.map.get_mut(&key) {
                Some(Value::Object(m)) => {
                    let m = std::mem::take(m);
                    let subs = top.tree.get(key.as_bytes()).cloned().unwrap_or_default();
                    descend = Some((m, subs, Dest::Field(key)));
                }
                Some(Value::Array(items)) => {
                    let items = std::mem::take(items);
                    top.array = Some(ArrayWork {
                        field: key,
                        items: items.into_iter(),
                        out: Vec::new(),
                    });
                    continue;
                }
                _ => continue,
            }
        }

        let Some((m, subs, dest)) = descend else {
            let Some(done) = stack.pop() else {
                return Map::new();
            };
            if !done.ancestor.is_empty() {
                ancestors.remove(&done.ancestor);
            }
            if let Some(root) = deliver(&mut stack, done.dest, done.map) {
                return root;
            }
            continue;
        };

        let depth = top.depth + 1;
        let reference = include_reference(&m);
        if !reference.is_empty() && ancestors.contains(&reference) {
            deliver(&mut stack, dest, m);
            continue;
        }

        let included = include_single_entity(m, read);
        if depth >= MAX_INCLUDE_DEPTH {
            deliver(&mut stack, dest, included);
            continue;
        }

        if !reference.is_empty() {
            ancestors.insert(reference.clone());
        }
        let next_tree = build_next_tree(&subs, force_all);
        stack.push(new_frame(
            included, next_tree, force_all, depth, reference, dest,
        ));
    }
}

pub fn extract_auto_includes(filters: &Filters) -> Vec<u8> {
    let mut set: BTreeSet<&[u8]> = BTreeSet::new();
    for field in filters.keys() {
        if let Some(i) = field.iter().rposition(|&c| c == b'.') {
            set.insert(&field[..i]);
        }
    }

    set.into_iter()
        .collect::<Vec<&[u8]>>()
        .join(b",".as_slice())
}

pub fn merge_includes(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }

    let mut seen: HashSet<&[u8]> = HashSet::new();
    let mut out: Vec<u8> = Vec::new();
    for s in [a, b] {
        for p in s.split(|&c| c == b',') {
            let p = strings::trim_space(p);
            if p.is_empty() {
                continue;
            }

            if seen.insert(p) {
                if !out.is_empty() {
                    out.push(b',');
                }
                out.extend_from_slice(p);
            }
        }
    }

    out
}
