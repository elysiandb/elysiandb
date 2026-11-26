mod common;
mod jsrun;

use gocompat::json;
use jsrun::{new_runtime, on_big_stack, run_error_string};
use talos::Value;
use talos::host::{DocMap, DocValue};

fn without_go_error(records: &[json::Value]) -> Vec<json::Value> {
    records
        .iter()
        .filter(|r| {
            let path = common::s(r, "path");
            let key = r.get("key").and_then(json::Value::as_str);
            let top = path == "globalThis" && key == Some("GoError");
            let under = path == "globalThis.GoError" || path.starts_with("globalThis.GoError.");
            !top && !under
        })
        .cloned()
        .collect()
}

fn walk_surface() -> Vec<json::Value> {
    let script = String::from_utf8(common::read_golden("hooks/surface-walker.js")).expect("utf8");
    let mut rt = new_runtime();
    let entity = DocValue::new_map(DocMap::new());
    let mut ctx = DocMap::new();
    ctx.insert("entity".to_string(), entity.clone());
    rt.set_value("ctx", &DocValue::new_map(ctx))
        .expect("set ctx");
    let p = talos::compile("", script.as_bytes(), false).expect("compile");
    if let Err(e) = rt.run_program(&p) {
        panic!("{}", run_error_string(&e));
    }
    let f = rt.get_var("postRead").expect("postRead").expect("postRead");
    let c = rt.get_var("ctx").expect("ctx").unwrap_or(Value::Undefined);
    if let Err(e) = rt.call_function(&f, Value::Undefined, &[c]) {
        panic!("{}", run_error_string(&e));
    }
    let DocValue::Map(m) = &entity else {
        panic!("entity is not a map");
    };
    let surface = match m.borrow().get("surface") {
        Some(DocValue::String(s)) => s.to_string(),
        _ => panic!("no surface output"),
    };
    let v = json::unmarshal_value(surface.as_bytes()).expect("surface json");
    v.as_array().expect("records").clone()
}

#[test]
fn hook_surface_matches_golden_without_go_error() {
    on_big_stack(|| {
        let golden = json::unmarshal_value(&common::read_golden("hooks/surface.json"))
            .expect("surface.json");
        let golden = golden.as_array().expect("records").clone();
        assert_eq!(golden.len(), 1996);
        let want = without_go_error(&golden);
        assert_eq!(want.len(), 1986);
        let got = walk_surface();
        let mut diffs = Vec::new();
        for (i, w) in want.iter().enumerate() {
            match got.get(i) {
                Some(g) if g == w => {}
                g => diffs.push(format!("record {i}: want {w:?}\n got {g:?}")),
            }
            if diffs.len() >= 10 {
                break;
            }
        }
        assert!(diffs.is_empty(), "{}", diffs.join("\n"));
        assert_eq!(got.len(), want.len());
    });
}
