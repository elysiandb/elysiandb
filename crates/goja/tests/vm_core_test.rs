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

#[test]
fn test_big_int_is_not_an_integer_argument() {
    check(&[
        (
            "(5).toExponential(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toExponential (native)",
        ),
        (
            "(5).toFixed(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toFixed (native)",
        ),
        (
            "(5).toPrecision(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at toPrecision (native)",
        ),
        (
            "(5).toString(2n)",
            "!TypeError: Cannot convert a BigInt value to a number at toString (native)",
        ),
        (
            "(5).toExponential(Object(1n))",
            "!TypeError: Cannot convert a BigInt value to a number at toExponential (native)",
        ),
        (
            "\"abc\".charAt(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at charAt (native)",
        ),
        (
            "\"abc\".substring(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at substring (native)",
        ),
        (
            "[1,2,3].slice(1n)",
            "!TypeError: Cannot convert a BigInt value to a number at slice (native)",
        ),
        (
            "var a = [1,2]; a.length = 1n",
            "!TypeError: Cannot convert a BigInt value to a number at <eval>:1:18(10)",
        ),
    ]);
}
