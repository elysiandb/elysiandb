mod jsrun;

use jsrun::{new_runtime, on_big_stack};

fn check(cases: &'static [(&'static str, &'static str)]) {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for (script, want) in cases {
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(v) => v.go_string(&mut rt).expect("go_string"),
                Err(e) => format!("!{}", e.error()),
            };
            if got != *want {
                failures.push(format!("{script}\n  got:  {got:?}\n  want: {want:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn test_native_constructor_frames_are_named() {
    check(&[
        (
            "var s; try { Number(Symbol()); } catch (e) { s = e.stack; } s",
            "TypeError: Cannot convert a Symbol value to a number\n\tat Number (native)\n\tat <eval>:1:20(5)\n",
        ),
        (
            "var s; try { new Date(Symbol()); } catch (e) { s = e.stack; } s",
            "TypeError: Cannot convert a Symbol value to a number\n\tat <eval>:1:14(5)\n",
        ),
        (
            "var s; try { BigInt(1.5); } catch (e) { s = e.stack; } s",
            "RangeError: Cannot convert 1.5 to a BigInt\n\tat BigInt (native)\n\tat <eval>:1:20(4)\n",
        ),
        (
            "var s; try { [1].map(function() { BigInt(0.5); }); } catch (e) { s = e.stack; } s",
            "RangeError: Cannot convert 0.5 to a BigInt\n\tat BigInt (native)\n\tat <eval>:1:41(3)\n\tat map (native)\n\tat <eval>:1:21(7)\n",
        ),
    ]);
}
