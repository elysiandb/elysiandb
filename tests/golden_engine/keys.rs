use std::sync::Arc;

use crate::common;
use elysiandb::datastore::{
    self, INDEX_COMPOSITE, INDEX_FILTER, INDEX_REFS, INDEX_SORT, IndexSpec,
};
use elysiandb::docpath::{self, SortValue};
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Map, Value};

const FIELDS: [&str; 17] = [
    "",
    "a",
    "a.b",
    "a..b",
    "a.b.c",
    "tags",
    "date",
    "num",
    "title",
    "status",
    "items.name",
    "items.n",
    "author",
    "missing",
    "missing.deep",
    "co",
    "matrix",
];

const COMPOSITES: [&str; 5] = [
    "status,num",
    "title,date",
    "a,a.b",
    "tags,num",
    "status,missing",
];

pub fn corpus() -> Vec<Vec<u8>> {
    let rows = common::read_jsonl("engine/corpus.jsonl");
    assert_eq!(rows.len(), 2023);
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            assert_eq!(row.get("n").and_then(Value::as_i64), Some(i as i64));
            STD_ENCODING
                .decode_string(common::str_field(row, "raw").as_bytes())
                .unwrap_or_else(|e| panic!("corpus {i}: {e:?}"))
        })
        .collect()
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

pub fn hex_list(keys: &[Vec<u8>]) -> Value {
    Value::Array(keys.iter().map(|k| Value::String(common::hex(k))).collect())
}

pub fn hex_value(b: &[u8]) -> Value {
    Value::String(common::hex(b))
}

pub fn same_json(label: &str, got: &Value, want: &Value) {
    let a = json::marshal(got).unwrap_or_else(|e| panic!("{label}: {e}"));
    let b = json::marshal(want).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert!(
        a == b,
        "{label}:\n got  {}\n want {}",
        String::from_utf8_lossy(&a),
        String::from_utf8_lossy(&b)
    );
}

fn sort_value_kind(doc: Option<&Value>, field: &[u8]) -> Value {
    let sv = match doc {
        Some(d) => docpath::sort_value(d, field),
        None => SortValue::Nil,
    };
    match sv {
        SortValue::Nil => obj(vec![("kind", Value::String("nil".into()))]),
        SortValue::Time(t) => obj(vec![
            ("kind", Value::String("time".into())),
            ("unix", Value::Int(t.unix())),
            ("nanos", Value::Int(t.nanosecond())),
        ]),
        SortValue::Value(v) => match v {
            Value::Null => obj(vec![("kind", Value::String("nil".into()))]),
            Value::String(_) => obj(vec![("kind", Value::String("string".into()))]),
            Value::Number(f) => obj(vec![
                ("kind", Value::String("number".into())),
                ("bits", Value::String(format!("{:016x}", f.to_bits()))),
            ]),
            Value::Bool(b) => obj(vec![
                ("kind", Value::String("bool".into())),
                ("value", Value::Bool(*b)),
            ]),
            Value::Object(_) => obj(vec![("kind", Value::String("object".into()))]),
            Value::Array(_) => obj(vec![("kind", Value::String("array".into()))]),
            _ => obj(vec![("kind", Value::String("other".into()))]),
        },
    }
}

fn sorted_strings(v: &Value) -> Value {
    match v {
        Value::Array(a) => {
            let mut items: Vec<String> = a
                .iter()
                .map(|x| x.as_str().unwrap_or_default().to_string())
                .collect();
            items.sort();
            Value::Array(items.into_iter().map(Value::String).collect())
        }
        other => other.clone(),
    }
}

fn row_for(i: usize, doc: Option<&Value>) -> Value {
    let mut fields = Map::new();
    for f in FIELDS {
        let field = f.as_bytes();
        let mut entry = Map::new();
        let filter = datastore::filter_keys(doc, field);
        if !filter.is_empty() {
            entry.insert("filter".into(), hex_list(&filter));
        }
        let mut unique = Map::new();
        for kind in [INDEX_FILTER, INDEX_SORT, INDEX_REFS] {
            let u =
                datastore::unique_keys(datastore::extract_keys(&IndexSpec::new(kind, field), doc));
            if !u.is_empty() {
                unique.insert(
                    String::from_utf8(vec![kind.0]).unwrap_or_default(),
                    hex_list(&u),
                );
            }
        }
        if !unique.is_empty() {
            entry.insert("unique".into(), Value::Object(unique));
        }
        entry.insert("sort".into(), hex_value(&datastore::sort_key(doc, field)));
        entry.insert("sort_value".into(), sort_value_kind(doc, field));
        fields.insert(f.to_string(), Value::Object(entry));
    }

    let refs = datastore::ref_keys(doc);
    let mut pairs: Vec<(String, String)> = datastore::document_refs(doc)
        .into_iter()
        .map(|r| (common::hex(&r.path), common::hex(&r.target)))
        .collect();
    pairs.sort();
    let doc_refs = if pairs.is_empty() {
        Value::Null
    } else {
        Value::Array(
            pairs
                .into_iter()
                .map(|(p, t)| Value::Array(vec![Value::String(p), Value::String(t)]))
                .collect(),
        )
    };

    let composite: Vec<Value> = COMPOSITES
        .iter()
        .map(|c| {
            let (keys, irregular) =
                datastore::composite_keys(&IndexSpec::new(INDEX_COMPOSITE, c.as_bytes()), doc);
            obj(vec![
                ("spec", Value::String((*c).into())),
                ("keys", hex_list(&keys)),
                ("irregular", Value::Bool(irregular)),
            ])
        })
        .collect();

    let grams = datastore::search_grams(doc);
    let grams = if grams.is_empty() {
        Value::Null
    } else {
        Value::Array(
            grams
                .into_iter()
                .map(|g| Value::Int(i64::from(g)))
                .collect(),
        )
    };

    let trigrams: Vec<Vec<u8>> = datastore::document_trigrams(doc).into_iter().collect();

    let mut elements = Map::new();
    if let Some(Value::Object(m)) = doc {
        for (k, v) in m {
            if let Value::Array(items) = v {
                let out = if items.is_empty() {
                    Value::Null
                } else {
                    Value::Array(
                        items
                            .iter()
                            .map(|item| hex_value(&datastore::element_string(item)))
                            .collect(),
                    )
                };
                elements.insert(common::hex(k.as_bytes()), out);
            }
        }
    }

    obj(vec![
        ("doc", Value::Int(i as i64)),
        ("decoded", Value::Bool(doc.is_some())),
        ("fields", Value::Object(fields)),
        ("refs", sorted_strings(&hex_list(&refs))),
        ("doc_refs", doc_refs),
        ("composite", Value::Array(composite)),
        ("grams", grams),
        ("trigrams", hex_list(&trigrams)),
        ("elements", Value::Object(elements)),
    ])
}

#[test]
fn golden_index_keys() {
    let corpus = corpus();
    let rows = common::read_jsonl("engine/index_keys.jsonl");
    assert_eq!(rows.len(), 2023);
    assert_eq!(rows.len(), corpus.len());

    let mut checked = 0;
    for (i, want) in rows.iter().enumerate() {
        let doc: Option<Arc<Value>> = datastore::decode_doc(&corpus[i]);
        let got = row_for(i, doc.as_deref());

        let mut want = want.clone();
        if let Value::Object(m) = &mut want
            && let Some(refs) = m.get("refs").cloned()
        {
            m.insert("refs".into(), sorted_strings(&refs));
        }
        same_json(&format!("index_keys row {i}"), &got, &want);
        checked += 1;
    }

    assert_eq!(checked, 2023);
}
