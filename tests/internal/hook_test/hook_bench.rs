use std::time::{Duration, Instant};

use elysiandb::api;
use elysiandb::gocompat::json::Value;
use elysiandb::hook;
use talos::host::MapRef;

use crate::fold_test::{BUDGET_SCRIPT, random_purchase};
use crate::hook_corpus::{field, read_jsonl};
use crate::{entity, obj, rng, s, setup, write};

fn go_p50(name: &str) -> i64 {
    let text =
        std::fs::read_to_string(crate::hook_corpus::golden_path("hooks/bench-baseline.json"))
            .expect("bench baseline");
    let v = elysiandb::gocompat::json::unmarshal_value(text.as_bytes()).expect("baseline");
    if let Some(Value::Array(cases)) = v.get("cases") {
        for c in cases {
            if field(c, "name") == name {
                return match c.get("ns_p50") {
                    Some(Value::Number(n)) => *n as i64,
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                };
            }
        }
    }
    0
}

fn measure(
    name: &str,
    script: &str,
    make: &dyn Fn() -> MapRef,
    min_runs: usize,
    min_time: Duration,
) -> (i64, i64) {
    for _ in 0..3 {
        let _ = hook::apply_post_read_script("", script, &make(), true);
    }
    let mut times: Vec<i64> = Vec::new();
    let start = Instant::now();
    while times.len() < min_runs || start.elapsed() < min_time {
        let e = make();
        let t0 = Instant::now();
        let _ = hook::apply_post_read_script("", script, &e, true);
        times.push(t0.elapsed().as_nanos() as i64);
        if times.len() >= 100_000 {
            break;
        }
    }
    times.sort_unstable();
    let p50 = times[times.len() / 2];
    let go = go_p50(name);
    println!(
        "{name}: rust p50 {p50} ns, go p50 {go} ns, ratio {:.2}",
        p50 as f64 / go.max(1) as f64
    );
    (p50, go)
}

#[test]
#[ignore = "local measurement of the hook bench, run by hand"]
fn hook_bench_parts() {
    let input = vec![b'x'; 300];
    let n = 100_000;
    let t0 = Instant::now();
    let mut acc = 0u8;
    for _ in 0..n {
        acc ^= elysiandb::crypto::sha256::sum256(&input)[0];
    }
    println!(
        "sha256 300 bytes: {} ns ({acc})",
        t0.elapsed().as_nanos() / n
    );

    let e = entity(
        r#"{"id":"x","price":12,"ratio":0.5,"tags":["a","b"],"meta":{"k":"v","n":1},"nul":null,"s":"str","b":true,"big":12345678901234567890}"#,
    );
    let d = talos::host::DocValue::Map(e);
    let t0 = Instant::now();
    for _ in 0..n {
        acc ^= talos::host::json::marshal(&d).expect("marshal")[0];
    }
    println!("marshal entity: {} ns ({acc})", t0.elapsed().as_nanos() / n);

    let t0 = Instant::now();
    for _ in 0..n {
        let rt = talos::Runtime::new();
        drop(rt);
    }
    println!("runtime new: {} ns", t0.elapsed().as_nanos() / n);

    let p = talos::compile(
        "",
        b"function postRead(ctx) {\nctx.partial = 1; throw new Error(\"boom\")\n}",
        false,
    )
    .expect("compile");
    let n = 20_000;
    let t0 = Instant::now();
    for _ in 0..n {
        let mut rt = talos::Runtime::new();
        let _ = rt.run_program(&p);
        let f = rt.get_var("postRead").ok().flatten().expect("fn");
        let ctx = talos::Value::Object(rt.new_object());
        let e = rt.call_function(&f, talos::Value::Undefined, &[ctx]);
        acc ^= e.err().map_or(0, |e| e.error().len() as u8);
    }
    println!(
        "bare baseline throw: {} ns ({acc})",
        t0.elapsed().as_nanos() / n
    );

    let plain = || entity(r#"{"id":"x"}"#);
    let script = "function postRead(ctx) {\nctx.entity.partial = 1; throw new Error(\"boom\")\n}";
    let t0 = Instant::now();
    for _ in 0..n {
        let _ = hook::apply_post_read_script("", script, &plain(), true);
    }
    println!("hook throw: {} ns", t0.elapsed().as_nanos() / n);

    let unsafe_doc = || {
        let e = entity(r#"{"id":"x"}"#);
        e.borrow_mut().insert(
            "b".to_string(),
            talos::host::DocValue::Bytes(talos::host::TypedSlice::new(vec![1u8])),
        );
        e
    };
    let t0 = Instant::now();
    for _ in 0..n {
        let _ = hook::apply_post_read_script("", script, &unsafe_doc(), true);
    }
    println!(
        "hook throw without memo: {} ns",
        t0.elapsed().as_nanos() / n
    );

    let ok = "function postRead(ctx) {\nctx.entity.partial = 1\n}";
    let t0 = Instant::now();
    for _ in 0..n {
        let _ = hook::apply_post_read_script("", ok, &unsafe_doc(), true);
    }
    println!("hook ok without memo: {} ns", t0.elapsed().as_nanos() / n);
}

fn eligible(script: &str, make: &dyn Fn() -> MapRef, want_error: &str) -> bool {
    let got = match hook::apply_post_read_script("", script, &make(), true) {
        Ok(()) => String::new(),
        Err(e) => e.message().to_string(),
    };
    got == want_error
}

#[test]
#[ignore = "local measurement of the hook bench, run by hand"]
fn hook_bench_local() {
    let plain = || entity(r#"{"id":"x"}"#);
    let mut kept: Vec<(String, f64)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut take = |name: &str, ok: bool, (r, g): (i64, i64)| {
        if ok && g > 0 {
            kept.push((name.to_string(), r as f64 / g as f64));
        } else {
            skipped.push(name.to_string());
        }
    };

    {
        let _s = setup(true);
        hook::init_hooks();
        let script = "function postRead(ctx) { ctx.entity.x = 1 }";
        let ok = eligible(script, &plain, "");
        take(
            "trivial",
            ok,
            measure("trivial", script, &plain, 2000, Duration::from_secs(2)),
        );
    }

    {
        let _s = setup(true);
        hook::init_hooks();
        let _ = api::create_entity_type(b"purchase");
        let mut r = rng(7);
        for i in 0..1200 {
            let d = random_purchase(&mut r, &format!("p{i:05}"));
            write("purchase", d);
        }
        let budget = || entity(r#"{"id":"2021-06","cap":500.0,"spent_manual_adjust":1.5}"#);
        let ok = eligible(BUDGET_SCRIPT, &budget, "");
        hook::set_folds_enabled(true);
        take(
            "budget-folds-on",
            ok,
            measure(
                "budget-folds-on",
                BUDGET_SCRIPT,
                &budget,
                500,
                Duration::from_secs(2),
            ),
        );
        hook::set_folds_enabled(false);
        take(
            "budget-folds-off",
            ok,
            measure(
                "budget-folds-off",
                BUDGET_SCRIPT,
                &budget,
                200,
                Duration::from_secs(2),
            ),
        );
        hook::set_folds_enabled(true);
    }

    {
        let _s = setup(true);
        hook::init_hooks();
        for i in 0..3 {
            write(
                "order",
                obj(vec![
                    ("id", s(&format!("o{i}"))),
                    ("group", s("g")),
                    ("amount", Value::Number(i as f64 + 0.5)),
                    ("tags", Value::Array(vec![s("t")])),
                    ("owner", Value::Object(obj(vec![("n", s("x"))]))),
                ]),
            );
        }
        let input = r#"{"id":"x","price":12,"ratio":0.5,"tags":["a","b"],"meta":{"k":"v","n":1},"nul":null,"s":"str","b":true,"big":12345678901234567890}"#;
        let corpus = || entity(input);
        for row in read_jsonl("hooks/corpus-utc.jsonl").iter() {
            let label = format!("corpus/{}", field(row, "label"));
            if label.starts_with("corpus/timeout") {
                continue;
            }
            let want = match row.get("expected_rust") {
                Some(r) => field(r, "error").to_string(),
                None => field(row, "error").to_string(),
            };
            let ok = eligible(field(row, "script"), &corpus, &want);
            take(
                &label,
                ok,
                measure(
                    &label,
                    field(row, "script"),
                    &corpus,
                    200,
                    Duration::from_millis(200),
                ),
            );
        }
    }

    let mut ratios: Vec<f64> = kept.iter().map(|(_, r)| *r).collect();
    ratios.sort_by(|a, b| a.partial_cmp(b).expect("ratio"));
    for (name, r) in &kept {
        if *r > 1.2 {
            println!("over 1.20: {name} {r:.2}");
        }
    }
    println!("skipped {}: {}", skipped.len(), skipped.join(","));
    println!(
        "median ratio {:.2} over {} cases",
        ratios[ratios.len() / 2],
        ratios.len()
    );
}
