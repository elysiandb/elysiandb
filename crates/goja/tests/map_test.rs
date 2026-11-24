use goja::Value;
use goja::builtin_symbol::{SYM_ITERATOR, SYM_TO_STRING_TAG};
use goja::deps::big;
use goja::map::{OrderedMap, value_hash};
use goja::string::{ascii_string, new_string_value};
use goja::value::{float_to_value, int_to_value, new_big_int};
use std::sync::Arc;

fn test_map_hash_val(v1: &Value, v2: &Value, expected: bool) {
    let actual = value_hash(v1) == value_hash(v2);
    assert_eq!(actual, expected, "testMapHashVal failed for {v1:?}, {v2:?}");
}

fn same(a: &Value, b: Option<&Value>) -> bool {
    match b {
        Some(b) => a.same_as(b),
        None => false,
    }
}

#[test]
fn test_map_hash() {
    test_map_hash_val(&Value::Float(f64::NAN), &Value::Float(f64::NAN), true);
    test_map_hash_val(&Value::Bool(true), &Value::Bool(false), false);
    test_map_hash_val(&Value::Bool(true), &Value::Bool(true), true);
    test_map_hash_val(&int_to_value(0), &Value::Float(-0.0), true);
    test_map_hash_val(
        &Value::String(ascii_string("Test")),
        &Value::String(ascii_string("Test")),
        true,
    );
    test_map_hash_val(
        &Value::String(new_string_value("Тест".as_bytes())),
        &Value::String(new_string_value("Тест".as_bytes())),
        true,
    );
    test_map_hash_val(&float_to_value(1.2345), &float_to_value(1.2345), true);
    test_map_hash_val(
        &Value::Symbol(SYM_ITERATOR),
        &Value::Symbol(SYM_TO_STRING_TAG),
        false,
    );
    test_map_hash_val(
        &Value::Symbol(SYM_ITERATOR),
        &Value::Symbol(SYM_ITERATOR),
        true,
    );
    test_map_hash_val(
        &new_big_int(big::Int::new_int(1)),
        &new_big_int(big::Int::new_int(-1)),
        false,
    );
    test_map_hash_val(
        &Value::BigInt(Arc::new(big::Int::new_int(1))),
        &Value::BigInt(Arc::new(big::Int::new_int(1))),
        true,
    );
}

#[test]
fn test_ordered_map() {
    let mut m = OrderedMap::new();
    for i in 0..50i64 {
        m.set(int_to_value(i), Value::String(ascii_string(&i.to_string())));
    }
    assert_eq!(m.size(), 50, "Unexpected size");
    for i in 0..50i64 {
        let expected = Value::String(ascii_string(&i.to_string()));
        assert!(
            same(&expected, m.get(&int_to_value(i))),
            "Wrong value for {i}"
        );
    }
    let mut i = 0i64;
    while i < 50 {
        assert!(m.remove(&int_to_value(i)), "remove({i}) return false");
        i += 2;
    }
    assert_eq!(m.size(), 25, "Unexpected size");
    let mut iter = m.new_iter();
    let mut count = 0;
    while let Some((k, _)) = m.iter_next(&mut iter) {
        m.remove(&k);
        count += 1;
    }
    assert_eq!(count, 25, "Unexpected iter count");
    assert_eq!(m.size(), 0, "Unexpected size");
}

#[test]
fn test_ordered_map_collision() {
    let mut m = OrderedMap::new();
    let n1: u64 = 123456789;
    let n2 = f64::from_bits(n1);
    let n1_key = int_to_value(n1 as i64);
    let n2_key = float_to_value(n2);
    m.set(n1_key.clone(), Value::String(ascii_string("n1")));
    m.set(n2_key.clone(), Value::String(ascii_string("n2")));
    assert!(
        m.size() != m.hash_table_len(),
        "Expected a collision but there wasn't one"
    );
    assert!(
        same(&Value::String(ascii_string("n2")), m.get(&n2_key)),
        "unexpected n2Val"
    );
    assert!(
        same(&Value::String(ascii_string("n1")), m.get(&n1_key)),
        "unexpected nVal"
    );
    assert!(m.remove(&n1_key), "removing n1Key returned false");
    assert!(
        same(&Value::String(ascii_string("n2")), m.get(&n2_key)),
        "2: unexpected n2Val"
    );
}

#[test]
fn test_ordered_map_iter() {
    let mut m = OrderedMap::new();
    let mut iter = m.new_iter();
    assert!(m.iter_next(&mut iter).is_none(), "entry should be nil");
    let mut iter1 = m.new_iter();
    m.set(int_to_value(1), Value::Bool(true));
    assert!(m.iter_next(&mut iter).is_none(), "2: entry should be nil");
    let ent = m.iter_next(&mut iter1).expect("entry is nil");
    assert!(int_to_value(1).same_as(&ent.0), "unexpected key");
    assert!(Value::Bool(true).same_as(&ent.1), "unexpected value");
}

#[test]
fn test_ordered_map_iter_visit_after_re_add() {
    let mut m = OrderedMap::new();
    let one = int_to_value(1);
    let two = int_to_value(2);
    m.set(one.clone(), Value::Bool(true));
    m.set(two.clone(), Value::Bool(true));
    let mut iter = m.new_iter();
    let entry = m.iter_next(&mut iter).expect("entry");
    assert!(one.same_as(&entry.0), "1: unexpected key");
    assert!(m.remove(&one), "remove returned false");
    let entry = m.iter_next(&mut iter).expect("entry");
    assert!(two.same_as(&entry.0), "2: unexpected key");
    m.set(one.clone(), Value::Bool(true));
    let entry = m.iter_next(&mut iter).expect("entry is nil");
    assert!(one.same_as(&entry.0), "3: unexpected key");
}

#[test]
fn test_ordered_map_iter_add_after_clear() {
    let mut m = OrderedMap::new();
    let one = int_to_value(1);
    m.set(one.clone(), Value::Bool(true));
    let mut iter = m.new_iter();
    m.iter_next(&mut iter);
    m.clear();
    m.set(one.clone(), Value::Bool(true));
    let entry = m.iter_next(&mut iter).expect("entry is nil");
    assert!(entry.0.same_as(&one), "unexpected key");
    assert!(m.iter_next(&mut iter).is_none(), "entry is not nil");
}

#[test]
fn test_ordered_map_iter_delete_current() {
    let mut m = OrderedMap::new();
    let one = int_to_value(1);
    let two = int_to_value(2);
    let mut iter = m.new_iter();
    m.set(one.clone(), Value::Bool(true));
    m.set(two.clone(), Value::Bool(true));
    let entry = m.iter_next(&mut iter).expect("entry");
    assert!(entry.0.same_as(&one), "unexpected key");
    m.remove(&one);
    let entry = m.iter_next(&mut iter).expect("entry");
    assert!(entry.0.same_as(&two), "2: unexpected key");
}

#[test]
fn removed_entries_are_released() {
    let mut m = OrderedMap::new();
    for i in 0..100_000i64 {
        m.set(int_to_value(i), Value::Bool(true));
        if i >= 16 {
            let mut it = m.new_iter();
            let (k, _) = m.iter_next(&mut it).expect("first");
            m.iter_close(&mut it);
            m.remove(&k);
        }
    }
    assert_eq!(m.size(), 16);
    assert!(m.allocated_entries() <= 17, "{}", m.allocated_entries());
    let mut it = m.new_iter();
    m.iter_next(&mut it);
    m.clear();
    assert!(m.allocated_entries() <= 1, "{}", m.allocated_entries());
    assert!(m.iter_next(&mut it).is_none());
    assert_eq!(m.allocated_entries(), 0);
}
