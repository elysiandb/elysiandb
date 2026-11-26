use crate::gocompat::json::{Map, Value};
use crate::uuid;

enum Slot {
    Field(String),
    Item,
    Sub,
}

enum Frame {
    Map {
        map: Map,
        keys: std::vec::IntoIter<String>,
        slot: Option<Slot>,
    },
    Array {
        items: std::vec::IntoIter<Value>,
        out: Vec<Value>,
        key: String,
    },
}

fn map_frame(map: Map, slot: Option<Slot>) -> Frame {
    let keys: Vec<String> = map.keys().cloned().collect();
    Frame::Map {
        map,
        keys: keys.into_iter(),
        slot,
    }
}

fn sub_entity_name(m: &Map) -> Option<String> {
    match m.get("@entity") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn link(name: &str, id: &str) -> Value {
    let mut m = Map::new();
    m.insert("@entity".to_string(), Value::String(name.to_string()));
    m.insert("id".to_string(), Value::String(id.to_string()));
    Value::Object(m)
}

pub fn extract_sub_entities(_entity: &[u8], data: &mut Map) -> Vec<Map> {
    let mut subs: Vec<Map> = Vec::new();
    let mut stack = vec![map_frame(std::mem::take(data), None)];

    loop {
        let Some(top) = stack.last_mut() else {
            return subs;
        };

        let mut push: Option<Frame> = None;
        let mut finished = false;
        match top {
            Frame::Map { map, keys, .. } => match keys.next() {
                Some(k) => {
                    let Some(v) = map.get_mut(&k) else {
                        continue;
                    };
                    match v {
                        Value::Object(val) => {
                            if let Some(name) = sub_entity_name(val) {
                                let (id, real_fields) = extract_id_and_check_fields(val);
                                let val = std::mem::take(val);
                                *v = link(&name, &id);
                                if real_fields {
                                    let sub = build_sub_entity(&name, &id, val);
                                    push = Some(map_frame(sub, Some(Slot::Sub)));
                                }
                            } else {
                                let val = std::mem::take(val);
                                push = Some(map_frame(val, Some(Slot::Field(k))));
                            }
                        }
                        Value::Array(arr) => {
                            let arr = std::mem::take(arr);
                            let len = arr.len();
                            push = Some(Frame::Array {
                                items: arr.into_iter(),
                                out: Vec::with_capacity(len),
                                key: k,
                            });
                        }
                        _ => {}
                    }
                }
                None => finished = true,
            },
            Frame::Array { items, out, .. } => match items.next() {
                Some(mut item) if matches!(item, Value::Object(_)) => {
                    let Value::Object(m) = &mut item else {
                        continue;
                    };
                    let m = std::mem::take(m);
                    if let Some(name) = sub_entity_name(&m) {
                        let (id, real_fields) = extract_id_and_check_fields(&m);
                        out.push(link(&name, &id));
                        if real_fields {
                            let sub = build_sub_entity(&name, &id, m);
                            push = Some(map_frame(sub, Some(Slot::Sub)));
                        }
                    } else {
                        push = Some(map_frame(m, Some(Slot::Item)));
                    }
                }
                Some(item) => out.push(item),
                None => finished = true,
            },
        }

        if let Some(frame) = push {
            stack.push(frame);
            continue;
        }

        if !finished {
            continue;
        }

        match stack.pop() {
            None => return subs,
            Some(Frame::Map { map, slot, .. }) => match slot {
                None => {
                    *data = map;
                    return subs;
                }
                Some(Slot::Sub) => subs.push(map),
                Some(Slot::Field(k)) => {
                    if let Some(Frame::Map { map: parent, .. }) = stack.last_mut() {
                        parent.insert(k, Value::Object(map));
                    }
                }
                Some(Slot::Item) => {
                    if let Some(Frame::Array { out, .. }) = stack.last_mut() {
                        out.push(Value::Object(map));
                    }
                }
            },
            Some(Frame::Array { out, key, .. }) => {
                if let Some(Frame::Map { map: parent, .. }) = stack.last_mut() {
                    parent.insert(key, Value::Array(out));
                }
            }
        }
    }
}

fn extract_id_and_check_fields(val: &Map) -> (String, bool) {
    let mut cur_id = String::new();
    if let Some(Value::String(s)) = val.get("id")
        && !s.is_empty()
    {
        cur_id = s.clone();
    } else if let Some(Value::String(s)) = val.get("@id")
        && !s.is_empty()
    {
        cur_id = s.clone();
    }

    let real_fields = val
        .keys()
        .any(|k| k != "@entity" && k != "id" && k != "@id");

    if cur_id.is_empty() {
        cur_id = uuid::new_v4();
    }

    (cur_id, real_fields)
}

fn build_sub_entity(name: &str, id: &str, val: Map) -> Map {
    let mut sub = Map::new();
    sub.insert("id".to_string(), Value::String(id.to_string()));
    sub.insert("@entity".to_string(), Value::String(name.to_string()));
    for (k, v) in val {
        if k != "@entity" && k != "id" && k != "@id" {
            sub.insert(k, v);
        }
    }

    sub
}

pub fn merge_with_existing(sub: Map, existing: Option<Map>) -> Map {
    let Some(mut merged) = existing else {
        return sub;
    };

    merged.extend(sub);
    merged
}
