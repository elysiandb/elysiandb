use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};

use super::{doc, setup, value};

#[test]
fn field_get_nested_value() {
    let _s = setup();
    let data = doc(
        r#"{"name":"John","address":{"city":"Paris","geo":{"lat":48.85,"lng":2.35},"empty":{}}}"#,
    );

    let tests: [(&str, Option<Value>); 7] = [
        ("name", Some(value(r#""John""#))),
        ("address.city", Some(value(r#""Paris""#))),
        ("address.geo.lat", Some(value("48.85"))),
        ("address.geo.lng", Some(value("2.35"))),
        ("address.geo.missing", None),
        ("missing", None),
        ("address.empty", Some(value("{}"))),
    ];

    for (path, expected) in tests {
        let got = api::get_nested_value(&data, path.as_bytes());
        if got.is_some() != expected.is_some() {
            panic!(
                "GetNestedValue({path:?}) found={}, want {}",
                got.is_some(),
                expected.is_some()
            );
        }
        if let (Some(got), Some(expected)) = (got, &expected)
            && got != expected
        {
            panic!("GetNestedValue({path:?})={got:?}, want {expected:?}");
        }
    }
}

#[test]
fn set_nested_field() {
    let _s = setup();
    let mut dest = Map::new();
    let _ = api::filter_fields(&Map::new(), &[]);
    if let Err(err) = api::set_nested_field(&mut dest, "a.b.c", Value::Int(42)) {
        panic!("{err}");
    }
    let expected = doc(r#"{"a":{"b":{"c":42}}}"#);
    if dest != expected {
        panic!("SetNestedField result={dest:?}, want {expected:?}");
    }
}

#[test]
fn filter_fields() {
    let _s = setup();
    let data = doc(
        r#"{"id":"u1","name":"Alice","address":{"city":"Paris","geo":{"lat":48.85,"lng":2.35}}}"#,
    );
    let fields = vec![b"id".to_vec(), b"address.geo.lat".to_vec()];
    let got = api::filter_fields(&data, &fields);
    let expected = doc(r#"{"id":"u1","address":{"geo":{"lat":48.85}}}"#);
    if got != expected {
        panic!("FilterFields={got:?}, want {expected:?}");
    }
}

#[test]
fn parse_fields_param() {
    let _s = setup();
    let tests: [(&str, &[&str]); 4] = [
        ("name,email", &["name", "email"]),
        (" name , address.city ", &["name", "address.city"]),
        ("", &[]),
        (",,,", &[]),
    ];
    for (input, expected) in tests {
        let got = api::parse_fields_param(input.as_bytes());
        let expected: Vec<Vec<u8>> = expected.iter().map(|s| s.as_bytes().to_vec()).collect();
        if got != expected {
            panic!("ParseFieldsParam({input:?})={got:?}, want {expected:?}");
        }
    }
}
