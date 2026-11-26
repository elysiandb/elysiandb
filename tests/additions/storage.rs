use elysiandb::gocompat::json::{Map, Value};
use elysiandb::storage;

const LEVELS: usize = 9_990;
const SMALL_STACK: usize = 64 << 10;

fn nested() -> Map {
    let mut v = Value::String("bottom".to_string());
    let mut depth = 1usize;
    let mut level = 0usize;
    while depth < LEVELS {
        if level % 4 == 1 && depth + 2 <= LEVELS {
            v = Value::DocList(vec![Map::from([("child".to_string(), v)])]);
            depth += 2;
        } else if level.is_multiple_of(4) {
            v = Value::Array(vec![Value::Int(level as i64), v]);
            depth += 1;
        } else {
            v = Value::Object(Map::from([
                ("child".to_string(), v),
                ("level".to_string(), Value::Number(level as f64)),
            ]));
            depth += 1;
        }
        level += 1;
    }

    Map::from([("child".to_string(), v)])
}

fn levels(doc: &Map) -> usize {
    let mut n = 1usize;
    let mut cur = doc.get("child");
    while let Some(v) = cur {
        cur = match v {
            Value::Array(a) => {
                n += 1;
                a.get(1)
            }
            Value::Object(m) => {
                n += 1;
                m.get("child")
            }
            Value::DocList(l) => {
                n += 2;
                l.first().and_then(|m| m.get("child"))
            }
            _ => None,
        };
    }

    n
}

fn leaf(doc: &mut Map) -> &mut Value {
    let Some(mut cur) = doc.get_mut("child") else {
        panic!("the document has no child");
    };
    loop {
        cur = match cur {
            Value::Array(a) => &mut a[1],
            Value::Object(m) => match m.get_mut("child") {
                Some(v) => v,
                None => panic!("an object level has no child"),
            },
            Value::DocList(l) => match l[0].get_mut("child") {
                Some(v) => v,
                None => panic!("a document list level has no child"),
            },
            other => return other,
        };
    }
}

#[test]
fn test_deep_copy_document_copies_9990_levels_on_a_small_stack() {
    let worker = std::thread::Builder::new()
        .name("deep-copy".to_string())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let original = nested();
            if levels(&original) != LEVELS {
                panic!(
                    "the document must be nested on {LEVELS} levels, got {}",
                    levels(&original)
                );
            }

            let mut copied = storage::deep_copy_document(&original);
            if copied != original {
                panic!("the copy differs from the original");
            }
            if levels(&copied) != levels(&original) {
                panic!("the copy lost levels");
            }

            *leaf(&mut copied) = Value::String("changed".to_string());
            if copied == original {
                panic!("changing the copy must not change the original");
            }

            let mut original = original;
            match leaf(&mut original) {
                Value::String(s) if s == "bottom" => {}
                _ => panic!("the original leaf changed"),
            }

            drop(copied);
            drop(original);
        })
        .unwrap_or_else(|e| panic!("cannot start the thread: {e}"));

    if worker.join().is_err() {
        panic!("the deep copy failed on a small stack");
    }
}
