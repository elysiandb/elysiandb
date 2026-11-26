use elysiandb::docpath::resolve_values;
use elysiandb::gocompat::json::{Value, unmarshal_value};

fn doc(text: &str) -> Value {
    unmarshal_value(text.as_bytes()).unwrap()
}

#[test]
fn values_are_resolved_depth_first_in_document_order() {
    let d = doc(r#"{"a":[{"b":1},[{"b":2},{"b":3}],{"c":4},{"b":[5,6]},{"b":7}]}"#);
    let got: Vec<String> = resolve_values(&d, b"a.b")
        .into_iter()
        .map(|v| format!("{v:?}"))
        .collect();
    let want: Vec<String> = [
        Value::Number(1.0),
        Value::Number(2.0),
        Value::Number(3.0),
        Value::Array(vec![Value::Number(5.0), Value::Number(6.0)]),
        Value::Number(7.0),
    ]
    .iter()
    .map(|v| format!("{v:?}"))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn missing_fields_and_scalars_resolve_to_nothing() {
    let d = doc(r#"{"a":{"b":1},"s":"x"}"#);
    assert!(resolve_values(&d, b"a.c").is_empty());
    assert!(resolve_values(&d, b"s.b").is_empty());
    assert_eq!(resolve_values(&d, b"a").len(), 1);
}

#[test]
fn deeply_nested_arrays_do_not_exhaust_the_stack() {
    let depth = 9_990;
    let text = format!(
        "{{\"a\":{}{{\"b\":1}}{}}}",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let d = doc(&text);
    let h = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || resolve_values(&d, b"a.b").len())
        .unwrap();
    assert_eq!(h.join().unwrap(), 1);
}
