use elysiandb::api;
use elysiandb::gocompat::json::Value;
use elysiandb::query::Sort;

use super::common::{read_jsonl, str_field};
use super::{array, boolean, go_maps, text};

#[test]
fn sort_corpus() {
    let rows = read_jsonl("api/sort_corpus.jsonl");
    let mut checked = 0;
    for row in &rows {
        let mut data = go_maps(str_field(row, "input").as_bytes());
        let spec: &[Value] = match row.get("sorts") {
            Some(Value::Null) => &[],
            _ => array(row, "sorts"),
        };
        let sorts: Vec<Sort> = spec
            .iter()
            .map(|s| {
                let Some([field, asc]) = s.as_array().map(Vec::as_slice) else {
                    panic!("bad sort {s:?}");
                };
                Sort::new(text(field), boolean(asc))
            })
            .collect();

        api::sort_entities(&mut data, &sorts);

        let got: Vec<&str> = data
            .iter()
            .map(|d| d.get("id").and_then(Value::as_str).unwrap_or_default())
            .collect();
        let want: Vec<&str> = array(row, "ids").iter().map(text).collect();
        if got != want {
            panic!("{row:?}: got {got:?}");
        }

        checked += 1;
    }

    assert_eq!(checked, 65);
    assert_eq!(checked, rows.len());
}
