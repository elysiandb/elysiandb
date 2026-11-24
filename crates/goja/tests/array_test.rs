mod jsrun;

use goja::Value;
use jsrun::*;

#[test]
fn test_array1() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let a = rt.new_array(None);
        a.set_own_idx(&mut rt, 0, str_value("test"), true)
            .expect("set");
        let l = a
            .get_str(&mut rt, &"length".into(), None)
            .expect("get")
            .unwrap_or(Value::Undefined);
        let l = l.to_integer(&mut rt).expect("to_integer");
        assert_eq!(l, 1, "Unexpected length: {l}");
    });
}

#[test]
fn test_array_canonical_index() {
    test_script(
        r#"
	var a = [];
	a["00"] = 1;
	a["01"] = 2;
	if (a[0] !== undefined) {
		throw new Error("a[0]");
	}
	"#,
        Value::Undefined,
    );
}
