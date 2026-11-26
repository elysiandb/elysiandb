mod array_cases;
mod jsrun;

use jsrun::*;
use talos::object::ObjExt;
use talos::runtime::RunError;
use talos::{ObjRef, Runtime, Value};

fn outcome(rt: &mut Runtime, src: &str) -> String {
    match rt.run_string(src) {
        Ok(v) => {
            if v.is_empty() {
                return "nil".to_string();
            }
            v.to_string(rt).expect("to_string").to_rust_string()
        }
        Err(RunError::Exception(ex)) => {
            let v = ex.value().clone();
            format!(
                "EXC {}",
                v.to_string(rt).expect("to_string").to_rust_string()
            )
        }
        Err(e) => format!("ERR {}", e.error()),
    }
}

fn representation(rt: &mut Runtime) -> String {
    let Some(Value::Object(o)) = rt.get("a").expect("get") else {
        return "notobj".to_string();
    };
    match &rt.obj(o).ext {
        ObjExt::Array(a) => format!(
            "dense(len={},cap={},length={},obj={},pvc={})",
            a.values.len(),
            a.values.capacity(),
            a.length,
            a.obj_count as i64,
            a.prop_value_count as i64
        ),
        ObjExt::SparseArray(a) => format!(
            "sparse(items={},length={},pvc={})",
            a.items.len(),
            a.length,
            a.prop_value_count as i64
        ),
        _ => "other".to_string(),
    }
}

#[test]
fn array_behaviour_matches_the_baseline() {
    on_big_stack(|| {
        for (i, (script, want)) in array_cases::BEHAVIOUR.iter().enumerate() {
            let mut rt = new_runtime();
            let src = format!("{}{}", array_cases::PRELUDE, script);
            let got = outcome(&mut rt, &src);
            assert_eq!(&got, want, "case {i}:\n{script}");
        }
    });
}

#[test]
fn array_representation_matches_the_baseline() {
    on_big_stack(|| {
        for (i, (script, want)) in array_cases::REPRESENTATION.iter().enumerate() {
            let mut rt = new_runtime();
            if let Err(e) = rt.run_string(script) {
                panic!("case {i}: {}", run_error_string(&e));
            }
            let got = representation(&mut rt);
            assert_eq!(&got, want, "case {i}:\n{script}");
        }
    });
}

fn run(rt: &mut Runtime, src: &str) {
    if let Err(e) = rt.run_string(src) {
        panic!("{}", run_error_string(&e));
    }
}

fn global_array(rt: &mut Runtime, name: &str) -> ObjRef {
    match rt.get(name).expect("get") {
        Some(Value::Object(o)) => o,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn sparse_array_switch_with_literal_cutoffs() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        run(&mut vm, "var a = [];\na[20470] = 5;");
        let a = global_array(&mut vm, "a");
        assert!(matches!(vm.obj(a).ext, ObjExt::SparseArray(_)));
        run(
            &mut vm,
            r#"
	var cutoffIdx = 17911;
	for (var i = a.length - 1; i >= cutoffIdx; i--) {
		a[i] = i;
	}
	if (a.length != 20471) {
		throw new Error("Invalid length: " + a.length);
	}
	for (var i = 0; i < cutoffIdx; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}
	for (var i = cutoffIdx; i < a.length; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}"#,
        );
        assert!(matches!(vm.obj(a).ext, ObjExt::Array(_)));
        run(
            &mut vm,
            r#"
	a[20471] = 20471;
	if (a.length != 20472) {
		throw new Error("Invalid length: " + a.length);
	}
	for (var i = cutoffIdx; i < a.length; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}"#,
        );
        assert!(matches!(vm.obj(a).ext, ObjExt::Array(_)));
        run(
            &mut vm,
            r#"
	var cutoffIdx1 = 18425;
	for (var i = cutoffIdx; i < cutoffIdx1; i++) {
		delete a[i];
	}
	a[25590] = 25590;
	if (a.length != 25591) {
		throw new Error("Invalid length: " + a.length);
	}
	for (var i = 0; i < cutoffIdx1; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}
	for (var i = cutoffIdx1; i < 20472; i++) {
		if (a[i] !== i) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}
	for (var i = 20472; i < 25590; i++) {
		if (a[i] !== undefined) {
			throw new Error("Invalid value at " + i + ": " + a[i]);
		}
	}
	if (a[25590] !== 25590) {
		throw new Error("Invalid value at 25590: " + a[25590]);
	}"#,
        );
        assert!(matches!(vm.obj(a).ext, ObjExt::SparseArray(_)));
    });
}

fn pseudo_random(n: usize, seed: u64, modulo: u64) -> Vec<i64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = (s * 1103515245 + 12345) % 2147483648;
            (s % modulo) as i64
        })
        .collect()
}

fn js_comparator_log(values: &[i64], std_array: bool) -> (Vec<i64>, Vec<(i64, i64)>) {
    let list: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    let setup = if std_array {
        format!("var a = [{}];", list.join(","))
    } else {
        format!(
            "var a = [{}];\na.pop();\na.push({});",
            list.join(","),
            values.last().map_or(0, |v| *v)
        )
    };
    let src = format!(
        "{setup}\nvar log = [];\na.sort(function (x, y) {{ log.push(x, y); return x - y; }});\nvar out = a.concat(log);\nout"
    );
    let mut rt = new_runtime();
    let v = match rt.run_string(&src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    };
    let o = v.as_object().expect("array");
    let len = {
        let l = o
            .get_str(&mut rt, &"length".into(), None)
            .expect("length")
            .expect("length");
        l.to_integer(&mut rt).expect("int")
    };
    let mut all = Vec::with_capacity(len as usize);
    for i in 0..len {
        let e = o.get_idx(&mut rt, i, None).expect("get").expect("elem");
        all.push(e.to_integer(&mut rt).expect("int"));
    }
    let sorted = all[..values.len()].to_vec();
    let pairs = all[values.len()..]
        .chunks(2)
        .map(|c| (c[0], c[1]))
        .collect();
    (sorted, pairs)
}

fn go_stable_log(values: &[i64]) -> (Vec<i64>, Vec<(i64, i64)>) {
    let mut v = values.to_vec();
    let mut log = Vec::new();
    gocompat::sort::slice_stable(&mut v, |x, y| {
        log.push((*x, *y));
        x < y
    });
    (v, log)
}

#[test]
fn sort_calls_the_comparator_like_go_sort_stable() {
    on_big_stack(|| {
        for n in (0..70).chain([99, 128, 161, 257]) {
            for (seed, modulo) in [(3u64, 7u64), (11, 1000), (5, 2)] {
                let values = pseudo_random(n, seed, modulo);
                let want = go_stable_log(&values);
                assert_eq!(
                    js_comparator_log(&values, true),
                    want,
                    "std n={n} seed={seed}"
                );
                if n > 0 {
                    assert_eq!(
                        js_comparator_log(&values, false),
                        want,
                        "generic n={n} seed={seed}"
                    );
                }
            }
        }
    });
}

#[test]
fn own_key_order_matches_the_baseline() {
    test_script(
        r#"
	[Array, Array.prototype, [].values().__proto__].map(function (o) {
		return Object.getOwnPropertyNames(o).join(",") + ";" + Object.getOwnPropertySymbols(o).length;
	}).join("|");
	"#,
        str_value(
            "length,name,prototype,from,isArray,of;1|length,constructor,at,concat,copyWithin,entries,every,fill,filter,find,findIndex,findLast,findLastIndex,flat,flatMap,forEach,includes,indexOf,join,keys,lastIndexOf,map,pop,push,reduce,reduceRight,reverse,shift,slice,some,sort,splice,toLocaleString,toString,unshift,with,toReversed,toSorted,toSpliced,values;2|next;1",
        ),
    );
}

#[test]
fn length_changes_follow_the_baseline() {
    test_script(
        r#"
	var r = [];
	var a = [1, 2, 3, 4];
	Object.defineProperty(a, "length", { writable: false });
	try { a.pop(); } catch (e) { r.push(e.message); }
	r.push(a.length, a[3], 3 in a);
	var b = [];
	b[100000] = 1;
	Object.defineProperty(b, "100000", { value: 2, configurable: false });
	b.length = 100000;
	r.push(b.length, b[100000]);
	var c = [1, 2, 3];
	Object.defineProperty(c, "length", { value: { valueOf: function () { c.length = 1; return 3; } } });
	r.push(c.length);
	r.join("|");
	"#,
        str_value("length is not writable|4||false|100000||1"),
    );
}
