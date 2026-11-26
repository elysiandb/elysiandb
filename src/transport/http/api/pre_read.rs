use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use talos::host::{DocMap, DocValue, MapRef};

use crate::acl::{self, ReadMode};
use crate::api::{self, SeqFilter};
use crate::datastore::Doc;
use crate::globals;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::{self, Map, Value};
use crate::hook::{self, Request};
use crate::sync::Mutex;

use super::response::{Spool, encode_list};

const HOOK_HELPER_STACK: usize = 8 << 20;

pub struct HookPool {
    cap: usize,
    used: AtomicUsize,
}

impl HookPool {
    pub fn cap(&self) -> usize {
        self.cap
    }

    fn try_acquire(&self) -> bool {
        let mut cur = self.used.load(Ordering::SeqCst);
        loop {
            if cur >= self.cap {
                return false;
            }
            match self
                .used
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }

    fn release(&self) {
        self.used.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn hook_helpers() -> &'static HookPool {
    static POOL: OnceLock<HookPool> = OnceLock::new();
    POOL.get_or_init(|| HookPool {
        cap: globals::procs().max(1),
        used: AtomicUsize::new(0),
    })
}

struct Token<'a>(&'a HookPool);

impl Drop for Token<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub fn apply_post_read_hooks<T, F>(
    hooks: &Request,
    entity: &str,
    data: &[Map],
    finish: F,
) -> Vec<Option<T>>
where
    T: Send,
    F: Fn(&MapRef) -> T + Sync,
{
    let slots: Vec<Mutex<Option<T>>> = (0..data.len()).map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);

    let run = || {
        loop {
            let i = next.fetch_add(1, Ordering::SeqCst);
            if i >= data.len() || hooks.stopped() {
                return;
            }
            let doc = hook::document_from_json(&data[i]);
            let doc = hooks.post_read(entity, &doc);
            let out = finish(&doc);
            DocValue::Map(doc).release_graph();
            *slots[i].lock() = Some(out);
        }
    };

    if data.len() <= 1 {
        run();
    } else {
        let pool = hook_helpers();
        let want = data.len().min(pool.cap()) - 1;
        std::thread::scope(|s| {
            let mut helpers = Vec::new();
            for _ in 0..want {
                if !pool.try_acquire() {
                    break;
                }
                let token = Token(pool);
                let run = &run;
                let h = std::thread::Builder::new()
                    .name("hook-helper".to_string())
                    .stack_size(HOOK_HELPER_STACK)
                    .spawn_scoped(s, move || {
                        let _token = token;
                        run();
                    });
                match h {
                    Ok(h) => helpers.push(h),
                    Err(e) => panic!("cannot start thread hook-helper: {e}"),
                }
            }

            run();

            for h in helpers {
                if let Err(p) = h.join() {
                    std::panic::resume_unwind(p);
                }
            }
        });
    }

    slots.into_iter().map(|m| m.into_inner()).collect()
}

pub enum Documents {
    Plain(Vec<Map>),
    Hooked(Vec<MapRef>),
    Encoded(Vec<Result<Vec<u8>, BoxError>>),
}

fn encode_plain(item: &Map, fields: &[Vec<u8>]) -> Result<Vec<u8>, BoxError> {
    if fields.is_empty() {
        return marshal_map(item);
    }

    marshal_map(&api::filter_fields(item, fields))
}

fn encode_hooked(item: &MapRef, fields: &[Vec<u8>]) -> Result<Vec<u8>, BoxError> {
    if fields.is_empty() {
        return marshal_document(item);
    }

    marshal_document(&filter_document_fields(item, fields))
}

impl Documents {
    pub fn len(&self) -> usize {
        match self {
            Documents::Plain(d) => d.len(),
            Documents::Hooked(d) => d.len(),
            Documents::Encoded(d) => d.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn release(self) {
        if let Documents::Hooked(docs) = self {
            for doc in &docs {
                release_document(doc);
            }
        }
    }

    pub fn pre_read(self, hooks: &Request, entity: &str) -> Documents {
        match self {
            Documents::Plain(data) => Documents::Hooked(
                data.iter()
                    .map(|item| hooks.pre_read(entity, &hook::document_from_json(item)))
                    .collect(),
            ),
            Documents::Hooked(data) => Documents::Hooked(
                data.iter()
                    .map(|item| hooks.pre_read(entity, item))
                    .collect(),
            ),
            encoded => encoded,
        }
    }

    pub fn apply_filters(self, filters: &crate::query::Filters) -> Documents {
        if filters.is_empty() {
            return self;
        }

        match self {
            Documents::Plain(data) => {
                Documents::Plain(api::apply_filters_to_list(data, Some(filters)))
            }
            Documents::Hooked(data) => {
                let mut kept = Vec::with_capacity(data.len());
                for doc in data {
                    if api::filters_match_entity(&json_view(&doc), filters) {
                        kept.push(doc);
                    } else {
                        release_document(&doc);
                    }
                }
                Documents::Hooked(kept)
            }
            encoded => encoded,
        }
    }

    pub fn post_read(self, hooks: &Request, entity: &str, fields: &[Vec<u8>]) -> Documents {
        match self {
            Documents::Plain(data) => {
                let out =
                    apply_post_read_hooks(hooks, entity, &data, |doc| encode_hooked(doc, fields));
                Documents::Encoded(
                    out.into_iter()
                        .zip(data.iter())
                        .map(|(r, item)| r.unwrap_or_else(|| encode_plain(item, fields)))
                        .collect(),
                )
            }
            Documents::Hooked(mut data) => {
                apply_post_read_hooks_in_place(hooks, entity, &mut data);
                let out = data.iter().map(|doc| encode_hooked(doc, fields)).collect();
                for doc in &data {
                    release_document(doc);
                }
                Documents::Encoded(out)
            }
            encoded => encoded,
        }
    }

    pub fn encode(self, fields: &[Vec<u8>]) -> Result<Spool, BoxError> {
        match self {
            Documents::Plain(data) => {
                encode_list(data.iter().map(|item| encode_plain(item, fields)))
            }
            Documents::Hooked(data) => {
                let res = encode_list(data.iter().map(|doc| encode_hooked(doc, fields)));
                for doc in &data {
                    release_document(doc);
                }
                res
            }
            Documents::Encoded(data) => encode_list(data),
        }
    }
}

fn map_id(m: &MapRef) -> usize {
    Rc::as_ptr(m) as *const () as usize
}

pub fn json_view(doc: &MapRef) -> Map {
    enum Dest {
        Root,
        Key(usize, String),
        Item(usize, usize),
    }
    enum Node {
        Object(Map),
        List(Vec<Value>),
    }
    enum Work {
        Visit(DocValue, Dest),
        Close(usize, Dest, usize),
    }

    fn place(nodes: &mut [Node], root: &mut Value, d: Dest, x: Value) {
        match d {
            Dest::Root => *root = x,
            Dest::Key(i, k) => {
                if let Node::Object(m) = &mut nodes[i] {
                    m.insert(k, x);
                }
            }
            Dest::Item(i, j) => {
                if let Node::List(l) = &mut nodes[i] {
                    l[j] = x;
                }
            }
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut root = Value::Null;
    let mut path: HashSet<usize> = HashSet::new();
    let mut work = vec![Work::Visit(DocValue::Map(doc.clone()), Dest::Root)];

    while let Some(w) = work.pop() {
        match w {
            Work::Close(i, d, id) => {
                path.remove(&id);
                let x = match std::mem::replace(&mut nodes[i], Node::List(Vec::new())) {
                    Node::Object(m) => Value::Object(m),
                    Node::List(l) => Value::Array(l),
                };
                place(&mut nodes, &mut root, d, x);
            }
            Work::Visit(v, d) => match &v {
                DocValue::Nil => place(&mut nodes, &mut root, d, Value::Null),
                DocValue::Bool(b) => place(&mut nodes, &mut root, d, Value::Bool(*b)),
                DocValue::Int64(n) => place(&mut nodes, &mut root, d, Value::Int(*n)),
                DocValue::Float64(f) => place(&mut nodes, &mut root, d, Value::Number(*f)),
                DocValue::String(s) => {
                    place(&mut nodes, &mut root, d, Value::String(s.to_string()))
                }
                DocValue::Map(m) => {
                    let id = map_id(m);
                    if !path.insert(id) {
                        place(&mut nodes, &mut root, d, Value::Null);
                        continue;
                    }
                    let entries: Vec<(String, DocValue)> = m
                        .borrow()
                        .iter()
                        .map(|(k, x)| (k.clone(), x.clone()))
                        .collect();
                    let i = nodes.len();
                    nodes.push(Node::Object(Map::new()));
                    work.push(Work::Close(i, d, id));
                    for (k, x) in entries {
                        work.push(Work::Visit(x, Dest::Key(i, k)));
                    }
                }
                DocValue::Slice(h) => {
                    let id = (Rc::as_ptr(&h.backing) as *const () as usize) | 1;
                    if !path.insert(id) {
                        place(&mut nodes, &mut root, d, Value::Null);
                        continue;
                    }
                    let items = h.to_vec();
                    let i = nodes.len();
                    nodes.push(Node::List(vec![Value::Null; items.len()]));
                    work.push(Work::Close(i, d, id));
                    for (j, x) in items.into_iter().enumerate() {
                        work.push(Work::Visit(x, Dest::Item(i, j)));
                    }
                }
                _ => place(&mut nodes, &mut root, d, Value::Null),
            },
        }
    }

    match &mut root {
        Value::Object(m) => std::mem::take(m),
        _ => Map::new(),
    }
}

fn nested_document_value(doc: &MapRef, path: &[u8]) -> Option<DocValue> {
    let mut current = DocValue::Map(doc.clone());
    for part in path.split(|&c| c == b'.') {
        let DocValue::Map(m) = &current else {
            return None;
        };
        let key = std::str::from_utf8(part).ok()?;
        let next = m.borrow().get(key).cloned()?;
        current = next;
    }

    Some(current)
}

fn set_nested_document_value(dest: &MapRef, path: &str, value: DocValue) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = dest.clone();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current.borrow_mut().insert((*part).to_string(), value);
            return;
        }

        let next = {
            let mut m = current.borrow_mut();
            match m.get(*part) {
                Some(DocValue::Map(inner)) => inner.clone(),
                Some(_) => return,
                None => {
                    let inner: MapRef = Rc::new(RefCell::new(DocMap::new()));
                    m.insert((*part).to_string(), DocValue::Map(inner.clone()));
                    inner
                }
            }
        };
        current = next;
    }
}

pub fn filter_document_fields(doc: &MapRef, fields: &[Vec<u8>]) -> MapRef {
    let filtered: MapRef = Rc::new(RefCell::new(DocMap::new()));
    for field in fields {
        if let Some(value) = nested_document_value(doc, field)
            && let Ok(path) = std::str::from_utf8(field)
        {
            set_nested_document_value(&filtered, path, value);
        }
    }

    filtered
}

pub fn marshal_document(doc: &MapRef) -> Result<Vec<u8>, BoxError> {
    talos::host::json::marshal(&DocValue::Map(doc.clone())).map_err(errors::new)
}

pub fn marshal_map(doc: &Map) -> Result<Vec<u8>, BoxError> {
    json::marshal(doc).map_err(|e| Box::new(e) as BoxError)
}

pub fn release_document(doc: &MapRef) {
    DocValue::Map(doc.clone()).release_graph();
}

pub fn paginate_documents(selected: Vec<MapRef>, offset: i64, limit: i64) -> Vec<MapRef> {
    let len = selected.len() as i64;
    let start = offset.clamp(0, len);
    let mut end = len;
    if limit > 0 && limit < end - start {
        end = start + limit;
    }

    let mut page = Vec::with_capacity((end - start) as usize);
    for (i, doc) in selected.into_iter().enumerate() {
        let i = i as i64;
        if i >= start && i < end {
            page.push(doc);
        } else {
            release_document(&doc);
        }
    }

    page
}

pub fn apply_pre_read_hooks_and_filter(
    hooks: &Request,
    entity: &str,
    data: &[Map],
    matches: &dyn Fn(&Map) -> bool,
    offset: i64,
    limit: i64,
) -> Vec<MapRef> {
    let mut selected: Vec<MapRef> = Vec::new();
    for item in data {
        if hooks.stopped() {
            break;
        }

        let item = hooks.pre_read(entity, &hook::document_from_json(item));
        if !matches(&json_view(&item)) {
            release_document(&item);
            continue;
        }

        selected.push(item);
        if limit > 0 && selected.len() as i64 >= offset + limit {
            break;
        }
    }

    paginate_documents(selected, offset, limit)
}

pub fn apply_post_read_hooks_in_place(hooks: &Request, entity: &str, data: &mut [MapRef]) {
    for item in data.iter_mut() {
        if hooks.stopped() {
            return;
        }

        *item = hooks.post_read(entity, item);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn lazy_pre_read_page(
    hooks: &Request,
    entity: &str,
    each: impl FnOnce(&mut dyn FnMut(Doc) -> bool),
    includes: &[u8],
    stored: &dyn Fn(&Map) -> bool,
    candidates: Option<&SeqFilter>,
    mode: ReadMode,
    username: &[u8],
    matches: &dyn Fn(&Map) -> bool,
    offset: i64,
    limit: i64,
) -> Vec<MapRef> {
    let mut selected: Vec<MapRef> = Vec::new();
    let mut visit = |d: Doc| -> bool {
        if let Some(candidates) = candidates
            && !candidates(d.seq())
        {
            return true;
        }

        if hooks.stopped() {
            return false;
        }

        let Ok(mut item) = d.decode() else {
            return true;
        };

        if !includes.is_empty() {
            let mut list = vec![item];
            api::apply_includes(&mut list, includes);
            item = list.pop().unwrap_or_default();
        }

        if !stored(&item) {
            return true;
        }

        if mode == ReadMode::ReadNone
            || (mode == ReadMode::ReadOwned && !acl::owned_by(&item, username))
        {
            return true;
        }

        let item = hooks.pre_read(entity, &hook::document_from_json(&item));
        if !matches(&json_view(&item)) {
            release_document(&item);
            return true;
        }

        selected.push(item);

        limit <= 0 || (selected.len() as i64) < offset + limit
    };
    each(&mut visit);

    paginate_documents(selected, offset, limit)
}
