mod jsrun;

use jsrun::*;
use talos::Value;
use talos::string::new_string_value;

#[test]
fn test_encode_uri() {
    const SCRIPT: &str = r#"
	encodeURI('тест')
	"#;
    test_script(SCRIPT, str_value("%D1%82%D0%B5%D1%81%D1%82"));
}

#[test]
fn test_decode_uri() {
    const SCRIPT: &str = r#"
	decodeURI("http://ru.wikipedia.org/wiki/%d0%ae%D0%bd%D0%B8%D0%BA%D0%BE%D0%B4")
	"#;
    test_script(
        SCRIPT,
        Value::String(new_string_value(
            "http://ru.wikipedia.org/wiki/Юникод".as_bytes(),
        )),
    );
}

#[test]
fn global_functions_match_the_baseline_outputs() {
    on_big_stack(|| {
        let cases: &[(&str, &str)] = &[
            (
                r#"parseInt("123456789012345678901234567890")"#,
                if cfg!(target_arch = "aarch64") {
                    "1.2345678901234568e+29"
                } else {
                    "1.2345678901234566e+29"
                },
            ),
            (
                r#"parseInt("0x1fffffffffffffffffff")"#,
                "1.5111572745182865e+23",
            ),
            (
                r#"parseInt("-ZZZZZZZZZZZZZZZZZZZZ", 36)"#,
                if cfg!(target_arch = "aarch64") {
                    "-1.3367494538843736e+31"
                } else {
                    "-1.3367494538843734e+31"
                },
            ),
            (r#"parseInt("  42abc")"#, "42"),
            (r#"parseInt("0x")"#, "NaN"),
            (r#"parseInt("12", 1)"#, "NaN"),
            (r#"parseFloat("  3.25e2xyz")"#, "325"),
            (r#"parseFloat("-Infinityx")"#, "-Infinity"),
            (r#"parseFloat("1e400")"#, "Infinity"),
            (r#"parseFloat(".e1")"#, "NaN"),
            (r#"isNaN("abc")"#, "true"),
            (r#"isFinite("12")"#, "true"),
            (r#"escape("aé€ /+")"#, "a%E9%u20AC%20/+"),
            (r#"unescape("%u20AC%E9%zz%4")"#, "€é%zz%4"),
            (r#"encodeURIComponent("a b&é")"#, "a%20b%26%C3%A9"),
            (
                r#"try { encodeURIComponent("\ud800") } catch(e) { String(e) }"#,
                "URIError: Malformed URI",
            ),
            (
                r#"try { decodeURI("%E0%A4%A") } catch(e) { String(e) }"#,
                "URIError: Malformed URI",
            ),
            (r#"decodeURIComponent("%23%3B%C3%A9")"#, "#;é"),
            (r#"decodeURI("%23%3B%C3%A9")"#, "%23%3Bé"),
        ];
        for (src, expected) in cases {
            let mut rt = new_runtime();
            let v = match rt.run_string(src) {
                Ok(v) => v,
                Err(e) => panic!("{src}: {e}"),
            };
            let s = match v.to_string(&mut rt) {
                Ok(s) => s.to_rust_string(),
                Err(e) => panic!("{src}: {e:?}"),
            };
            assert_eq!(&s, expected, "{src}");
        }
    });
}
