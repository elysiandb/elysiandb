mod jsrun;
mod tzchild;

use gocompat::math::{floor, floor_arch, floor_go};
use jsrun::{new_runtime, on_big_stack};
use std::collections::HashMap;
use tzchild::{Zone, run_in_zone};

fn in_zone(base: &str, overrides: &str) -> String {
    let o: HashMap<&str, &str> = overrides
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split_once('\t').expect("a tab"))
        .collect();
    let mut out = String::new();
    for line in base.lines().filter(|l| !l.is_empty()) {
        let (script, want) = line.split_once('\t').expect("a tab");
        let want = o.get(script).copied().unwrap_or(want);
        out.push_str(script);
        out.push('\t');
        out.push_str(want);
        out.push('\n');
    }
    out
}

fn check_cases(table: String) {
    on_big_stack(move || {
        let mut failures = Vec::new();
        let mut checked = 0;
        for line in table.lines() {
            if line.is_empty() {
                continue;
            }
            let (script, want) = line.split_once('\t').expect("a tab");
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(v) => v.go_string(&mut rt).expect("go_string"),
                Err(e) => format!("!{}", e.error()),
            };
            checked += 1;
            if got != want {
                failures.push(format!("{script}\n  got:  {got}\n  want: {want}"));
            }
        }
        assert!(checked > 0);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn number_builtin_matches_goja() {
    check_cases(NUMBER_CASES.to_string());
}

#[test]
fn big_int_builtin_matches_goja() {
    check_cases(BIG_INT_CASES.to_string());
}

#[test]
#[ignore = "requires the VM to name the native frame of templated functions"]
fn constructor_errors_name_the_native_frame() {
    check_cases(NATIVE_FRAME_CASES.to_string());
}

#[test]
#[ignore = "requires ToInteger to reject BigInt values"]
fn big_int_precision_arguments_are_rejected() {
    check_cases(BIG_INT_TO_INTEGER_CASES.to_string());
}

#[test]
#[ignore = "requires the Array and Reflect builtins"]
fn number_cases_with_arrays_and_reflect_match_goja() {
    check_cases(ARRAY_AND_REFLECT_CASES.to_string());
}

#[test]
fn date_builtin_matches_goja_in_utc() {
    run_in_zone("date_builtin_matches_goja_in_utc", Zone::Env("UTC"), || {
        check_cases(DATE_CASES_UTC.to_string())
    });
}

#[test]
fn date_builtin_matches_goja_in_europe_paris() {
    run_in_zone(
        "date_builtin_matches_goja_in_europe_paris",
        Zone::Env("Europe/Paris"),
        || check_cases(in_zone(DATE_CASES_UTC, DATE_CASES_EUROPE_PARIS)),
    );
}

#[test]
fn date_builtin_matches_goja_in_america_new_york() {
    run_in_zone(
        "date_builtin_matches_goja_in_america_new_york",
        Zone::Env("America/New_York"),
        || check_cases(in_zone(DATE_CASES_UTC, DATE_CASES_AMERICA_NEW_YORK)),
    );
}

#[test]
fn date_builtin_matches_goja_in_australia_lord_howe() {
    run_in_zone(
        "date_builtin_matches_goja_in_australia_lord_howe",
        Zone::Env("Australia/Lord_Howe"),
        || check_cases(in_zone(DATE_CASES_UTC, DATE_CASES_AUSTRALIA_LORD_HOWE)),
    );
}

#[test]
fn go_floor_matches_the_architecture_floor() {
    let mut values = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.0,
        -1.0,
        1.5,
        -1.5,
        2.5,
        -2.5,
        4503599627370495.5,
        -4503599627370495.5,
        4503599627370496.0,
        9007199254740993.0,
        1e300,
        -1e300,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MAX,
        f64::MIN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..100_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        values.push(f64::from_bits(x));
    }
    for v in values {
        let a = floor_go(v);
        let b = floor_arch(v);
        let c = floor(v);
        if v.is_nan() {
            assert!(a.is_nan() && b.is_nan() && c.is_nan(), "{v:e}");
            continue;
        }
        assert_eq!(a.to_bits(), b.to_bits(), "{v:e}");
        assert_eq!(a.to_bits(), c.to_bits(), "{v:e}");
    }
}

const NUMBER_CASES: &str = r#"
(1.005).toFixed(2)	1.00
(-1.5).toFixed(0)	-2
(0.1).toFixed(20)	0.10000000000000000555
(1e21).toFixed(2)	1e+21
(123.456).toFixed()	123
(123.456).toFixed(-1)	!RangeError: toFixed() precision must be between 0 and 100 at toFixed (native)
(123.456).toFixed(101)	!RangeError: toFixed() precision must be between 0 and 100 at toFixed (native)
(123.456).toFixed(100)	123.4560000000000030695446184836328029632568359375000000000000000000000000000000000000000000000000000000
(NaN).toFixed(2)	NaN
(Infinity).toFixed(2)	Infinity
(-Infinity).toFixed(2)	-Infinity
(0).toFixed(2)	0.00
(-0).toFixed(2)	0.00
(1.45).toFixed(1)	1.4
(1e-10).toFixed(5)	0.00000
(123.456).toExponential()	1.23456e+2
(123.456).toExponential(undefined)	1.23456e+2
(123.456).toExponential(0)	1e+2
(123.456).toExponential(2)	1.23e+2
(123.456).toExponential(100)	1.2345600000000000306954461848363280296325683593750000000000000000000000000000000000000000000000000000e+2
(123.456).toExponential(101)	!RangeError: toExponential() precision must be between 0 and 100 at toExponential (native)
(123.456).toExponential(-1)	!RangeError: toExponential() precision must be between 0 and 100 at toExponential (native)
(NaN).toExponential(2)	NaN
(NaN).toExponential()	NaN
(Infinity).toExponential()	Infinity
(Infinity).toExponential(2)	Infinity
(-Infinity).toExponential(2)	-Infinity
(Infinity).toExponential(-5)	Infinity
(0).toExponential()	0e+0
(0).toExponential(2)	0.00e+0
(-0).toExponential()	0e+0
(100).toExponential()	1e+2
(1e-7).toExponential(3)	1.000e-7
(0).toPrecision(3)	0.00
(123.456).toPrecision()	123.456
(123.456).toPrecision(undefined)	123.456
(123.456).toPrecision(0)	!RangeError: toPrecision() precision must be between 1 and 100 at toPrecision (native)
(123.456).toPrecision(101)	!RangeError: toPrecision() precision must be between 1 and 100 at toPrecision (native)
(123.456).toPrecision(100)	123.4560000000000030695446184836328029632568359375000000000000000000000000000000000000000000000000000
(NaN).toPrecision(0)	NaN
(Infinity).toPrecision(0)	Infinity
(-Infinity).toPrecision(3)	-Infinity
(0.000001234).toPrecision(2)	0.0000012
(1e21).toPrecision(3)	1.00e+21
(123456789).toPrecision(3)	1.23e+8
(25).toString(36)	p
(255).toString(16)	ff
(255).toString(2)	11111111
(-255.5).toString(16)	-ff.8
(0.1).toString(3)	0.0022002200220022002200220022002201
(1e21).toString(36)	5v1j4f4ds79m9s
(123.456).toString(8)	173.3513615237574734
(0.5).toString(2)	0.1
(255).toString(1)	!RangeError: toString() radix argument must be between 2 and 36 at toString (native)
(255).toString(37)	!RangeError: toString() radix argument must be between 2 and 36 at toString (native)
(255).toString(undefined)	255
(255).toString(10.9)	255
(255).toString("16")	ff
(NaN).toString(16)	NaN
(Infinity).toString(16)	Infinity
(-Infinity).toString(2)	-Infinity
(255).toLocaleString()	255
(255).toLocaleString(16)	ff
(1234567.891).toLocaleString()	1234567.891
(1e300*10).toString()	1e+301
(-0).toString()	0
Number.prototype.toString()	0
Number.prototype.valueOf()	0
Number.prototype.toFixed(2)	0.00
Number.prototype.toString(16)	0
Number.prototype.toString.call(1n)	!TypeError: Value is not a number at call (native)
Number.prototype.toString.call("1")	!TypeError: Value is not a number at call (native)
Number.prototype.toString.call(true)	!TypeError: Value is not a number at call (native)
Number.prototype.toString.call(new Boolean(true))	!TypeError: Value is not a number: true at call (native)
Number.prototype.toString.call(new String("x"))	!TypeError: Value is not a number at call (native)
Number.prototype.toString.call(Object(Symbol("q")))	!TypeError: Value is not a number: q at call (native)
Number.prototype.valueOf.call({})	!TypeError: Value is not a number: [object Object] at call (native)
Number.prototype.valueOf.call(undefined)	!TypeError: Value is not a number: undefined at call (native)
Number.prototype.valueOf.call(null)	!TypeError: Value is not a number: null at call (native)
Number.prototype.valueOf.call("12")	!TypeError: Value is not a number: 12 at call (native)
Number.prototype.valueOf.call(new Boolean(false))	!TypeError: Value is not a number: false at call (native)
Number.prototype.valueOf.call(Object(5))	5
Number.prototype.valueOf.call(Object(1n))	!TypeError: Value is not a number: 1 at call (native)
Number.prototype.toFixed.call("1", 2)	!TypeError: Value is not a number: 1 at call (native)
Number.prototype.toFixed.call({valueOf(){return 1}}, 2)	!TypeError: Value is not a number: [object Object] at call (native)
Number.prototype.toPrecision.call({toString(){return "z"}}, 2)	!TypeError: Value is not a number: z at call (native)
Number.prototype.toExponential.call(Symbol("s"))	!TypeError: Value is not a number: s at call (native)
(5).toFixed({valueOf(){return 3}})	5.000
(5).toFixed(Symbol())	!TypeError: Cannot convert a Symbol value to a number at toFixed (native)
(5).toString(Symbol())	!TypeError: Cannot convert a Symbol value to a number at toString (native)
Number()	0
Number(undefined)	NaN
Number(null)	0
Number(true)	1
Number("")	0
Number(" 12 ")	12
Number("0x10")	16
Number("1_000")	NaN
Number("1e1000")	Infinity
Number(1n)	1
Number(12345678901234567890n)	-6101065172474983000
Number(-5n)	-5
Number(2n**64n)	0
Number(2n**64n+3n)	3
Number(Object(7n))	7
Number({valueOf(){return 3n}})	3
Number({[Symbol.toPrimitive](){return 4n}})	4
Number({})	NaN
new Number(3).valueOf()	3
new Number(3n).valueOf()	3
new Number().valueOf()	0
typeof new Number(3)	object
Object.prototype.toString.call(new Number(3))	[object Number]
Object.prototype.toString.call(Number.prototype)	[object Number]
new Number(100111122133144155).toString()	100111122133144160
(new Number(123.456)).toString(36)	3f.gez4w97ry
Number.isFinite(1)	true
Number.isFinite(1.5)	true
Number.isFinite(Infinity)	false
Number.isFinite(NaN)	false
Number.isFinite("1")	false
Number.isFinite()	false
Number.isInteger(1)	true
Number.isInteger(1.5)	false
Number.isInteger(-0)	true
Number.isInteger(1e300)	true
Number.isInteger(Infinity)	false
Number.isInteger(NaN)	false
Number.isInteger("1")	false
Number.isNaN(NaN)	true
Number.isNaN("x")	false
Number.isNaN(undefined)	false
Number.isNaN(1)	false
Number.isSafeInteger(9007199254740991)	true
Number.isSafeInteger(9007199254740992)	false
Number.isSafeInteger(-9007199254740991)	true
Number.isSafeInteger(-9007199254740992)	false
Number.isSafeInteger(-0)	true
Number.isSafeInteger(0)	true
Number.isSafeInteger(1.5)	false
Number.isSafeInteger("1")	false
Number.EPSILON	2.220446049250313e-16
Number.MAX_SAFE_INTEGER	9007199254740991
Number.MIN_SAFE_INTEGER	-9007199254740991
Number.MIN_VALUE	5e-324
Number.MAX_VALUE	1.7976931348623157e+308
Number.NaN	NaN
Number.NEGATIVE_INFINITY	-Infinity
Number.POSITIVE_INFINITY	Infinity
Number.parseFloat === parseFloat	true
Number.parseInt === parseInt	true
Number.length	1
Number.name	Number
Number.prototype.constructor === Number	true
Number.prototype.toFixed.length	1
Number.prototype.toString.length	1
Number.prototype.toLocaleString.length	0
Number.prototype.toLocaleString.name	toLocaleString
new Number(1) instanceof Number	true
1..toString()	1
(0x800000000000000000000).toString()	9.671406556917033e+24
(5).x	undefined
Number.prototype.x = 1; (5).x	1
Object.getPrototypeOf(new Number(1)) === Number.prototype	true
Number.call(null, "3")	3
(function(){ "use strict"; return typeof this }).call(5)	number
Number(-0)	0
1/Number(-0)	-Infinity
1/Number("-0")	-Infinity
(-1e-7).toString()	-1e-7
(1e-7).toFixed(10)	0.0000001000
(123.456e30).toFixed(3)	1.23456e+32
(0.000001).toPrecision(2)	0.0000010
(-7).toPrecision(1)	-7
(-42).toPrecision(1)	-4e+1
(25).toExponential(0)	3e+1
(-25).toExponential(0)	-3e+1
(12345).toExponential(3)	1.235e+4
(25.5).toFixed(0)	26
(-25.5).toFixed(0)	-26
(99.9).toFixed(0)	100
(99.99).toFixed(1)	100.0
new Number("0.000000000000000000001").toPrecision(1)	1e-21
"#;

const BIG_INT_CASES: &str = r#"
BigInt(1e21)	1000000000000000000000
BigInt(-0)	0
BigInt(2**53)	9007199254740992
BigInt("123")	123
BigInt(" 0x1f ")	31
BigInt("0b101")	5
BigInt("0o17")	15
BigInt("-12")	-12
BigInt("-0")	0
BigInt("")	0
BigInt("  ")	0
BigInt("12345678901234567890123")	12345678901234567890123
BigInt("-12345678901234567890123")	-12345678901234567890123
BigInt("+5")	5
BigInt(true)	1
BigInt(false)	0
BigInt()	0
BigInt({valueOf(){return 5}})	5
BigInt({valueOf(){return "7"}})	7
BigInt({valueOf(){return true}})	1
BigInt({valueOf(){return 3n}})	3
BigInt({toString(){return "9"}, valueOf: undefined})	9
BigInt(Object(4n))	4
BigInt(10n)	10
new BigInt(1)	!TypeError: BigInt is not a constructor at <eval>:1:1(2)
new BigInt()	!TypeError: BigInt is not a constructor at <eval>:1:1(1)
(1n).toString(undefined)	1
(1n).toString(-1)	!RangeError: radix must be an integer between 2 and 36 at toString (native)
(1n).toString(37)	!RangeError: radix must be an integer between 2 and 36 at toString (native)
(1n).toString(2)	1
(10n).toString(3)	101
(255n).toString(16)	ff
(-255n).toString(36)	-73
(255n).toString("16")	ff
(255n).toString(16.9)	ff
(255n).toLocaleString()	255
(255n).toLocaleString(2)	11111111
BigInt.prototype.toString.call(1)	!TypeError: requires that 'this' be a BigInt at call (native)
BigInt.prototype.toString.call(Object(3n))	3
BigInt.prototype.valueOf.call(Object(3n))	3
BigInt.prototype.valueOf.call("3")	!TypeError: requires that 'this' be a BigInt at call (native)
BigInt.prototype.valueOf.call({})	!TypeError: requires that 'this' be a BigInt at call (native)
BigInt.prototype.toString()	!TypeError: requires that 'this' be a BigInt at toString (native)
BigInt.asIntN(8, 255n)	-1
BigInt.asIntN(8, 127n)	127
BigInt.asIntN(8, 128n)	-128
BigInt.asIntN(8, -129n)	127
BigInt.asIntN(0, 5n)	0
BigInt.asIntN(64, 2n**63n)	-9223372036854775808
BigInt.asIntN(64, -(2n**63n))	-9223372036854775808
BigInt.asIntN(3, 25n)	1
BigInt.asIntN(8)	!TypeError: Cannot convert undefined to a BigInt at asIntN (native)
BigInt.asIntN()	!TypeError: Cannot convert undefined to a BigInt at asIntN (native)
BigInt.asIntN(-1, 1n)	!RangeError: Invalid index -1 at asIntN (native)
BigInt.asIntN(2**53, 1n)	!RangeError: Invalid index 9007199254740992 at asIntN (native)
BigInt.asIntN(1.9, 3n)	-1
BigInt.asIntN("8", "255")	-1
BigInt.asIntN(8, 255)	!TypeError: Cannot convert 255 to a BigInt at asIntN (native)
BigInt.asIntN(8, true)	1
BigInt.asIntN(8, "x")	!SyntaxError: Cannot convert x to a BigInt at asIntN (native)
BigInt.asIntN(1n, 1n)	!TypeError: Cannot convert a BigInt value to a number at asIntN (native)
BigInt.asIntN(NaN, 3n)	0
BigInt.asIntN(Symbol(), 3n)	!TypeError: Cannot convert a Symbol value to a number at asIntN (native)
BigInt.asUintN(8, 255n)	255
BigInt.asUintN(8, 256n)	0
BigInt.asUintN(8, -1n)	255
BigInt.asUintN(0, 5n)	0
BigInt.asUintN(64, -1n)	18446744073709551615
BigInt.asUintN(8)	!TypeError: Cannot convert undefined to a BigInt at asUintN (native)
BigInt.asUintN(-1, 1n)	!RangeError: Invalid index -1 at asUintN (native)
BigInt.asUintN(8, 1.5)	!TypeError: Cannot convert 1.5 to a BigInt at asUintN (native)
BigInt.length	1
BigInt.name	BigInt
BigInt.prototype.length	0
BigInt.prototype.name	BigInt
BigInt.prototype[Symbol.toStringTag]	BigInt
Object.prototype.toString.call(1n)	[object BigInt]
Object.prototype.toString.call(Object(1n))	[object BigInt]
Object.prototype.toString.call(BigInt.prototype)	[object BigInt]
typeof Object(1n)	object
Object(1n) instanceof BigInt	true
BigInt.prototype.constructor === BigInt	true
BigInt.asIntN.length	2
BigInt.prototype.toString.length	0
0xabcdef0123456789abcdef0123n	13611749155596469006689035551011
1n + 1n	2
2n ** 64n	18446744073709551616
(2n ** 64n).toString(16)	10000000000000000
-5n % 3n	-2
String(10n)	10
`${10n}`	10
10n == 10	true
10n == "10"	true
10n === 10	false
1n < 2	true
Object(1n) - 1n	0
"#;

const NATIVE_FRAME_CASES: &str = r#"
BigInt(1.5)	!RangeError: Cannot convert 1.5 to a BigInt at BigInt (native)
BigInt(NaN)	!RangeError: Cannot convert NaN to a BigInt at BigInt (native)
BigInt(Infinity)	!RangeError: Cannot convert Infinity to a BigInt at BigInt (native)
BigInt(-Infinity)	!RangeError: Cannot convert -Infinity to a BigInt at BigInt (native)
BigInt("1.5")	!SyntaxError: Cannot convert 1.5 to a BigInt at BigInt (native)
BigInt("abc")	!SyntaxError: Cannot convert abc to a BigInt at BigInt (native)
BigInt("1e3")	!SyntaxError: Cannot convert 1e3 to a BigInt at BigInt (native)
BigInt(undefined)	!TypeError: Cannot convert undefined to a BigInt at BigInt (native)
BigInt(null)	!TypeError: Cannot convert null to a BigInt at BigInt (native)
BigInt(Symbol("x"))	!TypeError: Cannot convert Symbol to a BigInt at BigInt (native)
BigInt({valueOf(){return 1.5}})	!RangeError: Cannot convert 1.5 to a BigInt at BigInt (native)
Number(Symbol())	!TypeError: Cannot convert a Symbol value to a number at Number (native)
"#;

const BIG_INT_TO_INTEGER_CASES: &str = r#"
(5).toExponential(1n)	!TypeError: Cannot convert a BigInt value to a number at toExponential (native)
"#;

const ARRAY_AND_REFLECT_CASES: &str = r#"
Number([])	0
String(Object.getOwnPropertyNames(Number))	length,name,prototype,EPSILON,isFinite,isInteger,isNaN,isSafeInteger,MAX_SAFE_INTEGER,MIN_SAFE_INTEGER,MIN_VALUE,MAX_VALUE,NaN,NEGATIVE_INFINITY,parseFloat,parseInt,POSITIVE_INFINITY
String(Object.getOwnPropertyNames(Number.prototype))	constructor,toExponential,toFixed,toLocaleString,toPrecision,toString,valueOf
Reflect.construct(Number, [1], function(){}).toString()	[object Number]
"#;

const DATE_CASES_UTC: &str = r#"
new Date(0).toString()	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)
new Date(0).toUTCString()	Thu, 01 Jan 1970 00:00:00 GMT
new Date(0).toISOString()	1970-01-01T00:00:00.000Z
new Date(0).toDateString()	Thu Jan 01 1970
new Date(0).toTimeString()	00:00:00 GMT+0000 (UTC)
new Date(0).toLocaleString()	01/01/1970, 00:00:00
new Date(0).toLocaleDateString()	01/01/1970
new Date(0).toLocaleTimeString()	00:00:00
new Date(1783166400000).toString()	Sat Jul 04 2026 12:00:00 GMT+0000 (UTC)
new Date(1783166400000).toTimeString()	12:00:00 GMT+0000 (UTC)
new Date(1783166400000).getTimezoneOffset()	0
new Date(0).getTimezoneOffset()	0
new Date(-1e15).toString()	Tue Apr 05 -29719 22:13:20 GMT+0000 (UTC)
new Date(-1e15).getTimezoneOffset()	0
new Date(8.64e15).toISOString()	+275760-09-13T00:00:00.000Z
new Date(-8.64e15).toISOString()	-271821-04-20T00:00:00.000Z
new Date(8.64e15).toString()	Sat Sep 13 275760 00:00:00 GMT+0000 (UTC)
new Date(-8.64e15).toString()	Tue Apr 20 -271821 00:00:00 GMT+0000 (UTC)
new Date(-8.64e15).toUTCString()	Tue, 20 Apr -271821 00:00:00 GMT
new Date(8.64e15 + 1).getTime()	NaN
new Date(-62198755200000).toISOString()	-0001-01-01T00:00:00.000Z
new Date(-1).toISOString()	1969-12-31T23:59:59.999Z
new Date(253402300800000).toISOString()	+10000-01-01T00:00:00.000Z
new Date(-62198755200001).toISOString()	-0002-12-31T23:59:59.999Z
new Date(-62198755200000).toString()	Fri Jan 01 -0001 00:00:00 GMT+0000 (UTC)
new Date(1.5).getTime()	1
new Date(-1.5).getTime()	-1
new Date(1.9).getMilliseconds()	1
new Date(-0.5).getTime()	0
new Date(NaN).getTime()	NaN
new Date(NaN).toString()	Invalid Date
new Date(NaN).toUTCString()	Invalid Date
new Date(NaN).toISOString()	!RangeError: Invalid time value at toISOString (native)
new Date(NaN).toDateString()	Invalid Date
new Date(NaN).toLocaleString()	Invalid Date
new Date(NaN).getFullYear()	NaN
new Date(NaN).getTimezoneOffset()	NaN
new Date(Infinity).getTime()	NaN
new Date(true).getTime()	1
new Date(null).getTime()	0
new Date(undefined).getTime()	NaN
new Date("x").getTime()	NaN
new Date(new Date(5)).getTime()	5
new Date(new Date(NaN)).getTime()	NaN
new Date({valueOf(){return 7}}).getTime()	7
new Date({toString(){return "2020-01-01"}, valueOf: undefined}).getTime()	1577836800000
new Date({[Symbol.toPrimitive](){return "2020-01-01T00:00:00Z"}}).getTime()	1577836800000
new Date(Symbol())	!TypeError: Cannot convert a Symbol value to a number at <eval>:1:1(3)
new Date(1n)	!TypeError: Cannot convert a BigInt value to a number at <eval>:1:1(2)
new Date(2016, 8, 1, 12, 23, 45).getTime()	1472732625000
new Date(2016, 8, 1, 12, 23, 45).toString()	Thu Sep 01 2016 12:23:45 GMT+0000 (UTC)
new Date(99, 0).getFullYear()	1999
new Date(0, 0).getFullYear()	1900
new Date(100, 0).getFullYear()	100
new Date(-1, 0).getFullYear()	-1
new Date(2026, 0, 31, 25).getDate()	1
new Date(2026, 13, 1).getMonth()	1
new Date(2026, -1, 1).getMonth()	11
new Date(2026, 0, 0).getDate()	31
new Date(2026, 0, 1, 0, 0, 0, 1500).getSeconds()	1
new Date(2026, 0, 1, 0, 0, 0, -1).getMilliseconds()	999
new Date(2026, 2, 29, 2, 30).getHours()	2
new Date(2026, 9, 25, 2, 30).getHours()	2
new Date(2026, 2, 8, 2, 30).getHours()	2
new Date(2026, 10, 1, 1, 30).getTime()	1793496600000
new Date(2026, NaN).getTime()	NaN
new Date(2026, 0, 1, 0, 0, 0, Infinity).getTime()	NaN
new Date(2e9, 0).getTime()	NaN
new Date(2147483647, 0).getTime()	NaN
new Date(2147483648, 0).getTime()	NaN
new Date(-2147483648, 0).getTime()	NaN
new Date(275760, 8, 13).getTime()	8640000000000000
new Date(1970, 0, 1, 0, 0, 0, 8.64e15).getTime()	8640000000000000
new Date(1970, 0, 1, 0, 0, 0, 9.3e18).getTime()	NaN
new Date(1970, 0, 1, 0, 0, 0, -9.3e18).getTime()	NaN
new Date(1970, 0, 1, 0, 0, 1e300).getTime()	NaN
new Date(1970, 0, 1e15).getTime()	NaN
new Date(1970, 1e15).getTime()	NaN
new Date(9007199254740991, 0).getTime()	NaN
Date.UTC(1970, 0)	0
Date.UTC(2016, 6, 5, 15, 34, 45)	1467732885000
Date.UTC(2026)	1767225600000
Date.UTC(2026, 13, 1)	1801440000000
Date.UTC(99, 0)	915148800000
Date.UTC(-1, 0)	-62198755200000
Date.UTC()	NaN
Date.UTC(NaN)	NaN
Date.UTC(2026, 0, 1, 0, 0, 0, 0.9)	1767225600000
Date.UTC(275760, 8, 13)	8640000000000000
Date.UTC(275760, 8, 13, 0, 0, 0, 1)	NaN
Date.UTC(-271821, 3, 20)	-8640000000000000
Date.UTC(-271821, 3, 19, 23, 59, 59, 999)	NaN
Date.UTC({valueOf(){return 2000}}, {valueOf(){return 1}})	949363200000
Date.parse("2016-09-01T12:34:56Z")	1472733296000
Date.parse("2026-01-02")	1767312000000
Date.parse("2026-01-02T10:00:00")	1767348000000
Date.parse("2026-01-02 10:00")	1767348000000
Date.parse("Jan 2, 2026")	1767312000000
Date.parse("2026/01/02")	1767312000000
Date.parse("02/01/2026")	1769904000000
Date.parse("not a date")	NaN
Date.parse("2026-01-02T10:00:00Z")	1767348000000
Date.parse("Fri, 02 Jan 2026 10:00:00 GMT")	1767348000000
Date.parse("2026-1-2")	1767312000000
Date.parse("2026-01-02T10:00:00.123+02:00")	1767340800123
Date.parse("2026-01-02T10:00")	1767348000000
Date.parse("20260102")	NaN
Date.parse("2026-01")	1767225600000
Date.parse("2026")	1767225600000
Date.parse("+002026-01-02")	1767312000000
Date.parse("-002026-01-02")	-126101491200000
Date.parse("-000000-01-02")	NaN
Date.parse("Friday, January 2, 2026")	1767312000000
Date.parse("2 January 2026")	1767312000000
Date.parse("Jan 2 2026 10:00 GMT+0200")	1767340800000
Date.parse("2026-01-02T10:00:00.1234Z")	1767348000123
Date.parse(" 2026-01-02")	1767312000000
Date.parse("2026-01-02T24:00:00Z")	1767398400000
Date.parse("2026-02-30")	1772409600000
Date.parse("2026-13-01")	NaN
Date.parse("2026-01-32")	NaN
Date.parse("2026-01-02T25:00")	NaN
Date.parse("2026-01-02T10:60")	NaN
Date.parse("2026-01-02T10:00:60")	NaN
Date.parse("2026-01-02T10:00:00,5Z")	1767348000500
Date.parse("2026-01-02T10:00:00.Z")	NaN
Date.parse("2026-01-02T10:00:00+0530")	1767328200000
Date.parse("2026-01-02T10:00:00+05")	1767330000000
Date.parse("2026-01-02T10:00:00+5:30")	NaN
Date.parse("2026-01-02T10:00:00+24:00")	NaN
Date.parse("2026-01-02T10:00:00z")	NaN
Date.parse("Mon, 02 Jan 2006 15:04:05 MST")	1136239445000
Date.parse("Mon Jan 02 2006 15:04:05 GMT-0700 (GMT Standard Time)")	1136239445000
Date.parse("Mon Jan  2 15:04:05 2006")	1136214245000
Date.parse("Mon Jan 02 3:4 PM -0700 2006")	1136239440000
Date.parse("December 04, 1986")	534038400000
Date.parse("2006-06-02T15:04:05.000")	1149260645000
Date.parse("2006-01-02 15:04:05.123")	1136214245123
Date.parse("2006T15:04-0700")	1136153040000
Date.parse("2006T15:04+07:00")	1136102640000
Date.parse("2019-01T12:00:00.52Z")	1546344000520
Date.parse("Sat, 01-Jan-2000 08:00:00 UT")	946713600000
Date.parse("Saturday, 01-Jan-00 08:00:00 UT")	946713600000
Date.parse("01 Jan 00 08:00 +0000")	946713600000
Date.parse("[Saturday] Jan 01 08:00:00 UT 2000")	946713600000
Date.parse("Janisamonthandtherestisignored01 2000 08:00:00 UT")	946713600000
Date.parse("Sat, 01/01 2000 08:00:00 UT")	946713600000
Date.parse("  Sat,  01  Jan  2000 (2001)08:00:00   UT  ")	946713600000
Date.parse("  01    Jan   00    08:00   +0000()((asfd)(Sat, 01-Jan-2000)) ")	946713600000
Date.parse("Sat, 01-Jan-2000 08:00:00 GMT+000")	946713600000
Date.parse("Sat, 01-Jan-2000 08:00:00 GMT+00:00")	946713600000
Date.parse("Sat, 01-Jan-2000 03:00:00 UTC-05:00")	946713600000
Date.parse("Sat, 01 Jan 0 08:00:00 UT")	946713600000
Date.parse("Sat, 01 Jan 49 08:00:00 UT")	2493100800000
Date.parse("Sat, 01 Jan 50 08:00:00 UT")	-631123200000
Date.parse("Sat, 01 Jan 100 08:00:00 UT")	-59011430400000
Date.parse("Jan 01 08:00 PM UT 2000")	946756800000
Date.parse("12:00 AM Jan 1 2000")	946684800000
Date.parse("12:30 am Jan 1 2000")	946686600000
Date.parse("May 25 2008 1:30 (PM)) UTC")	NaN
Date.parse("May 25 2008 1:30( )AM (PM)")	NaN
Date.parse("a1")	NaN
Date.parse("x_2")	NaN
Date.parse("May 25 2008 AAA (GMT)")	NaN
Date.parse("1 2 3 4")	NaN
Date.parse("1/2/3/4")	NaN
Date.parse("2000 -5")	-62324985600000
Date.parse("Jan 2000 -0")	NaN
Date.parse("2000 Jan 5 10:00 CEST")	947059200000
Date.parse("2000 Jan 5 10:00 WEST")	947062800000
Date.parse("2000 Jan 5 10:00 EEST")	947055600000
Date.parse("2000 Jan 5 10:00 cest")	947059200000
Date.parse("2000 Jan 5 10:00 UTCX")	NaN
Date.parse("2000 Jan 5 10:00 +123456789")	947061420000
Date.parse("2000 Jan 5 10:00 +1234567")	947061420000
Date.parse("2000 Jan 5 10:00 +12345")	947061420000
Date.parse("2000 Jan 5 10:00 +123")	947061420000
Date.parse("2000 Jan 5 10:00 +1")	947062800000
Date.parse("2000 Jan 5 10:00 +01:3")	NaN
Date.parse("2000 Jan 5 10:00:5.5")	947066405500
Date.parse("Jan 5 999999999")	NaN
Date.parse("Jan 5 -999999999")	NaN
Date.parse("Jan 5 +275760")	8639978227200000
Date.parse("Jan 5 -271821")	NaN
Date.parse("Dec 31 +275760")	NaN
Date.parse("1 Jan")	978307200000
Date.parse("Jan 1")	978307200000
Date.parse("13 Jan 2000")	947721600000
Date.parse("1 13 2000")	947721600000
Date.parse("Jan 32 2000")	946684800000
Date.parse("Ja 1 2000")	946684800000
Date.parse("2000-01-01T08:00:00.000000000099Z")	NaN
Date.parse("2000-01-01T08:00:00.9999999999Z")	NaN
Date.parse("Tue, 22 Jun 2021 13:54:40 GMT")	1624370080000
Date.parse("")	NaN
Date.parse()	NaN
Date.parse(0)	946684800000
Date.parse(new Date(0))	0
Date.parse({toString(){return "2020"}})	1577836800000
Date.parse(Symbol())	!TypeError: Cannot convert a Symbol value to a string at parse (native)
Date.parse("Mon, 02 Jan 2006 15:04:05 zzz")	NaN
Date.parse("2000-01-01TZ")	NaN
Date.parse("2000-01-01T24:00:00.001")	NaN
Date.parse("−2000")	NaN
Date.parse("2000é")	NaN
Date.parse("Jané 1 2000")	946684800000
var d = new Date(0); d.setMilliseconds(2345)	2345
var d = new Date(1000); d.setMilliseconds(23450000000000)	23450000001000
var d = new Date(0); d.setUTCMilliseconds(2345)	2345
var d = new Date(0); d.setSeconds(12)	12000
var d = new Date(0); d.setUTCSeconds(12)	12000
var d = new Date(0); d.setMinutes(12)	720000
var d = new Date(0); d.setHours(1)	3600000
var d = new Date(0); d.setUTCHours(1)	3600000
var d = new Date(0); d.setDate(2)	86400000
var d = new Date(0); d.setMonth(2)	5097600000
var d = new Date(0); d.setUTCMonth(2)	5097600000
var d = new Date(0); d.setFullYear(1971)	31536000000
var d = new Date(0); d.setFullYear(1971, 2, 3)	36806400000
var d = new Date(0); d.setUTCFullYear(1971, 2, 3)	36806400000
var d = new Date(0); d.setFullYear(1971, 2, 3, 4)	36806400000
var d = new Date(0); d.setMonth(2, 3, 4); d.getTime()	5270400000
var d = new Date(0); d.setDate(2, 3); d.getTime()	86400000
var d = new Date(0); d.setHours(1, 2, 3, 4)	3723004
var d = new Date(0); d.setHours(1, 2, 3, 4, 5)	3723004
var d = new Date(0); d.setMinutes(1, 2, 3)	62003
var d = new Date(0); d.setSeconds(1, 2)	1002
var d = new Date(0); d.setSeconds()	NaN
var d = new Date(0); d.setSeconds(NaN); d.getTime()	NaN
var d = new Date(0); d.setMilliseconds(); d.getTime()	NaN
var d = new Date(NaN); d.setMilliseconds(5)	NaN
var d = new Date(NaN); d.setSeconds(5)	NaN
var d = new Date(NaN); d.setFullYear(2000)	946684800000
var d = new Date(NaN); d.setUTCFullYear(2000)	946684800000
var d = new Date(NaN); d.setUTCFullYear(2000, 5)	959817600000
var d = new Date(NaN); d.setTime(5)	5
var d = new Date(0); d.setTime()	NaN
var d = new Date(0); d.setTime("12")	12
var d = new Date(0); d.setTime(8.64e15 + 1); d.getTime()	NaN
var d = new Date(0); d.setTime(1.9)	1
var d = new Date(0); d.setTime(-1.9)	-1
var d = new Date(0); d.setMilliseconds(8.64e15)	8640000000000000
var d = new Date(0); d.setUTCMilliseconds(-8.64e15)	-8640000000000000
var d = new Date(0); d.setUTCSeconds(8640000000000)	8640000000000000
var d = new Date(0); d.setUTCSeconds(-8640000000000)	-8640000000000000
var d = new Date(0); d.setMilliseconds(Infinity)	NaN
var d = new Date(0); d.setMilliseconds(-Infinity)	NaN
var d = new Date(0); d.setSeconds(Infinity)	NaN
var d = new Date(0); d.setFullYear(Infinity)	NaN
var d = new Date(0); d.setFullYear(2e9)	NaN
var d = new Date(0); d.setFullYear(-2e9)	NaN
var d = new Date(0); d.setFullYear(3e9)	NaN
var d = new Date(0); d.setMonth(1e18)	NaN
var d = new Date(0); d.setDate(1e15)	NaN
var d = new Date(0); d.setHours(1e17)	NaN
var d = new Date(0); d.setUTCDate(-1e10)	NaN
var d = new Date(2026, 0, 31); d.setMonth(1); d.getDate()	3
var d = new Date(0); d.setSeconds({valueOf(){ d.setTime(NaN); return 1 }})	NaN
var d = new Date(0); d.setMilliseconds({valueOf(){ d.setTime(NaN); return 1 }})	NaN
var d = new Date(0); d.setMilliseconds({valueOf(){ d.setTime(5000); return 1 }})	5001
var d = new Date(0); d.setSeconds({valueOf(){ d.setTime(3600000); return 1 }})	1000
var d = new Date(0); d.setFullYear({valueOf(){ d.setTime(NaN); return 2000 }})	946684800000
var log = ""; var d = new Date(0); d.setHours({valueOf(){log += "h"; return 1}}, {valueOf(){log += "m"; return NaN}}, {valueOf(){log += "s"; return 1}}); log	hm
var log = ""; new Date({valueOf(){log += "y"; return 2000}}, {valueOf(){log += "m"; return NaN}}, {valueOf(){log += "d"; return 1}}); log	ym
Date.prototype.toString.call({})	!TypeError: Method Date.prototype.toString is called on incompatible receiver at call (native)
Date.prototype.toString.call(1)	!TypeError: Value is not an object: 1 at call (native)
Date.prototype.toString.call(undefined)	!TypeError: Value is not an object: undefined at call (native)
Date.prototype.getTime.call(Date.prototype)	!TypeError: Method Date.prototype.getTime is called on incompatible receiver at call (native)
Date.prototype.setHours.call({}, 1)	!TypeError: Method Date.prototype.setHours is called on incompatible receiver at call (native)
Date.prototype.toISOString.call({})	!TypeError: Method Date.prototype.toISOString is called on incompatible receiver at call (native)
Date.prototype.toUTCString.call(null)	!TypeError: Value is not an object: null at call (native)
Date.prototype.valueOf.call("x")	!TypeError: Value is not an object: x at call (native)
Date.prototype.getTimezoneOffset.call({})	!TypeError: Method Date.prototype.getTimezoneOffset is called on incompatible receiver at call (native)
Date.prototype.toJSON.call({ toISOString: function () { return 1; } })	1
Date.prototype.toJSON.call({ toISOString: 5 })	!TypeError: toISOString is not a function at call (native)
Date.prototype.toJSON.call({ valueOf(){return NaN}, toISOString(){return 2} })	null
Date.prototype.toJSON.call({ valueOf(){return Infinity}, toISOString(){return 2} })	null
Date.prototype.toJSON.call(undefined)	!TypeError: Cannot convert undefined or null to object at call (native)
Date.prototype.toJSON.call(5)	!TypeError: toISOString is not a function at call (native)
new Date(NaN).toJSON()	null
new Date(0).toJSON()	1970-01-01T00:00:00.000Z
new Date(0)[Symbol.toPrimitive]("number")	0
new Date(0)[Symbol.toPrimitive]("string")	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)
new Date(0)[Symbol.toPrimitive]("default")	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)
new Date(0)[Symbol.toPrimitive]("x")	!TypeError: Invalid hint: x at [Symbol.toPrimitive] (native)
new Date(0)[Symbol.toPrimitive]()	!TypeError: Invalid hint: undefined at [Symbol.toPrimitive] (native)
new Date(0)[Symbol.toPrimitive](Symbol("q"))	!TypeError: Invalid hint: q at [Symbol.toPrimitive] (native)
new Date(0)[Symbol.toPrimitive](1)	!TypeError: Invalid hint: 1 at [Symbol.toPrimitive] (native)
Date.prototype[Symbol.toPrimitive].call(1, "number")	!TypeError: Value is not an object: 1 at call (native)
Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3}, toString(){return "s"}}, "number")	3
Date.prototype[Symbol.toPrimitive].call({valueOf(){return 3}, toString(){return "s"}}, "default")	s
new Date(0) + 1	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)1
new Date(0) - 1	-1
+new Date(5)	5
`${new Date(0)}`	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)
String(new Date(0))	Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)
new Date(0) == new Date(0).toString()	true
typeof Date()	string
typeof Date(0)	string
Date.length	7
Date.name	Date
Date.prototype.constructor === Date	true
Date.UTC.length	7
Date.parse.length	1
Date.now.length	0
typeof Date.now()	number
Date.now() > 1700000000000	true
Date.prototype.setHours.length	4
Date.prototype[Symbol.toPrimitive].name	[Symbol.toPrimitive]
Object.prototype.toString.call(new Date(0))	[object Date]
Object.prototype.toString.call(Date.prototype)	[object Object]
new Date(0) instanceof Date	true
Object.getPrototypeOf(new Date(0)) === Date.prototype	true
class D extends Date {}; new D(5).getTime()	5
class D extends Date {}; new D(5) instanceof D	true
new Date(2016, 8, 1, 12, 23, 45).getDay()	4
new Date(2016, 8, 1, 12, 23, 45).getUTCDay()	4
new Date(2016, 8, 1, 12, 23, 45).getUTCHours()	12
new Date(2016, 8, 1, 12, 23, 45).getUTCDate()	1
new Date(2016, 8, 1, 12, 23, 45).getUTCMonth()	8
new Date(2016, 8, 1, 12, 23, 45).getUTCFullYear()	2016
new Date(2016, 8, 1, 12, 23, 45).getUTCMinutes()	23
new Date(2016, 8, 1, 12, 23, 45).getUTCSeconds()	45
new Date(2016, 8, 1, 12, 23, 45, 678).getUTCMilliseconds()	678
new Date(-1e14).getDay()	4
new Date(-1e14).getFullYear()	-1199
new Date(1151877845000).getHours()	22
new Date(1e14).getTimezoneOffset()	0
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCHours(13); d.getHours()	13
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(52); d.getMinutes()	52
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCDate(31); d.getTime()	1475324625000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMonth(0, 15); d.getTime()	1452860625000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCSeconds(1, 2); d.getTime()	1472732581002
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(1, 2, 3); d.getTime()	1472731262003
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCHours(1, 2, 3, 4); d.getTime()	1472691723004
var d = new Date(1774746000000); d.setHours(2); d.getTime()	1774749600000
var d = new Date(1761354000000); d.setMinutes(30); d.getTime()	1761355800000
var d = new Date(1761350400000); d.setHours(2, 30); d.getTime()	1761359400000
Date.parse('Sat, 01-Jan-2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 UT')	946713600000
Date.parse('Sat, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('Jan 01 08:00:00 UT 2000')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 08:00:00 UT 2000')	946713600000
Date.parse('Saturday, 01-Jan-00 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00 UT')	946713600000
Date.parse('01 Jan 00 08:00 +0000')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 08:00 +0000')	946713600000
Date.parse('Mon, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Mon, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('Tue, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Tue, 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('[Saturday] Jan 01 08:00:00 UT 2000')	946713600000
Date.parse('parse-local-time:' + '[Saturday] Jan 01 08:00:00 UT 2000')	946713600000
Date.parse('Ignore all of this stuff because it is annoying 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Ignore all of this stuff because it is annoying 01 Jan 2000 08:00:00 UT')	946713600000
Date.parse('[Saturday] Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + '[Saturday] Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('All of this stuff is really annoying, so it will be ignored Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'All of this stuff is really annoying, so it will be ignored Jan 01 2000 08:00:00 UT')	946713600000
Date.parse('Sat, 01-Janisamonth-2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Janisamonth-2000 08:00:00 UT')	946713600000
Date.parse('Sat, 01 Janisamonth 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Janisamonth 2000 08:00:00 UT')	946713600000
Date.parse('Janisamonth 01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Janisamonth 01 2000 08:00:00 UT')	946713600000
Date.parse('Janisamonth 01 08:00:00 UT 2000')	946713600000
Date.parse('parse-local-time:' + 'Janisamonth 01 08:00:00 UT 2000')	946713600000
Date.parse('Saturday, 01-Janisamonth-00 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Janisamonth-00 08:00:00 UT')	946713600000
Date.parse('01 Janisamonth 00 08:00 +0000')	946713600000
Date.parse('parse-local-time:' + '01 Janisamonth 00 08:00 +0000')	946713600000
Date.parse('Janisamonthandtherestisignored01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Janisamonthandtherestisignored01 2000 08:00:00 UT')	946713600000
Date.parse('Jan01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Jan01 2000 08:00:00 UT')	946713600000
Date.parse('Sat, 2000/01/01 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 2000/01/01 08:00:00 UT')	946713600000
Date.parse('Sat, 01/01/2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01/01/2000 08:00:00 UT')	946713600000
Date.parse('Sat, 01/01 2000 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01/01 2000 08:00:00 UT')	946713600000
Date.parse('Sat, 01,Jan,2000,08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01,Jan,2000,08:00:00 UT')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00 UT')	946713600000
Date.parse('Sat, 01 Jan 2000 08:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00 UT')	946713600000
Date.parse('Jan 01 2000 08:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00 UT')	946713600000
Date.parse('Jan 01 08:00 UT 2000')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 08:00 UT 2000')	946713600000
Date.parse('Saturday, 01-Jan-00 08:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00 UT')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00 AM UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00 AM UT')	946713600000
Date.parse('Sat, 01 Jan 2000 08:00 AM UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00 AM UT')	946713600000
Date.parse('Jan 01 2000 08:00 AM UT')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00 AM UT')	946713600000
Date.parse('Jan 01 08:00 AM UT 2000')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 08:00 AM UT 2000')	946713600000
Date.parse('Saturday, 01-Jan-00 08:00 AM UT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00 AM UT')	946713600000
Date.parse('01 Jan 00 08:00 AM +0000')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 08:00 AM +0000')	946713600000
Date.parse('   Sat,   01-Jan-2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '   Sat,   01-Jan-2000   08:00:00   UT  ')	946713600000
Date.parse('  Sat,   01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,   01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('  Saturday,   01-Jan-00   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Saturday,   01-Jan-00   08:00:00   UT  ')	946713600000
Date.parse('  01    Jan   00    08:00   +0000   ')	946713600000
Date.parse('parse-local-time:' + '  01    Jan   00    08:00   +0000   ')	946713600000
Date.parse(' ()(Sat, 01-Jan-2000)  Sat,   01-Jan-2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + ' ()(Sat, 01-Jan-2000)  Sat,   01-Jan-2000   08:00:00   UT  ')	946713600000
Date.parse('  Sat()(Sat, 01-Jan-2000)01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat()(Sat, 01-Jan-2000)01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('  Sat,(02)01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,(02)01   Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('  Sat,  01(02)Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01(02)Jan   2000   08:00:00   UT  ')	946713600000
Date.parse('  Sat,  01  Jan  2000 (2001)08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01  Jan  2000 (2001)08:00:00   UT  ')	946713600000
Date.parse('  Sat,  01  Jan  2000 (01)08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01  Jan  2000 (01)08:00:00   UT  ')	946713600000
Date.parse('  Sat,  01  Jan  2000 (01:00:00)08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01  Jan  2000 (01:00:00)08:00:00   UT  ')	946713600000
Date.parse('  Sat,  01  Jan  2000  08:00:00 (CDT)UT  ')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01  Jan  2000  08:00:00 (CDT)UT  ')	946713600000
Date.parse('  Sat,  01  Jan  2000  08:00:00  UT((((CDT))))')	946713600000
Date.parse('parse-local-time:' + '  Sat,  01  Jan  2000  08:00:00  UT((((CDT))))')	946713600000
Date.parse('  Saturday,   01-Jan-00 ()(((asfd)))(Sat, 01-Jan-2000)08:00:00   UT  ')	946713600000
Date.parse('parse-local-time:' + '  Saturday,   01-Jan-00 ()(((asfd)))(Sat, 01-Jan-2000)08:00:00   UT  ')	946713600000
Date.parse('  01    Jan   00    08:00 ()(((asdf)))(Sat, 01-Jan-2000)+0000   ')	946713600000
Date.parse('parse-local-time:' + '  01    Jan   00    08:00 ()(((asdf)))(Sat, 01-Jan-2000)+0000   ')	946713600000
Date.parse('  01    Jan   00    08:00   +0000()((asfd)(Sat, 01-Jan-2000)) ')	946713600000
Date.parse('parse-local-time:' + '  01    Jan   00    08:00   +0000()((asfd)(Sat, 01-Jan-2000)) ')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT+0')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT+0')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT+00')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT+00')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT+000')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT+000')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT+0000')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT+0000')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00 GMT+00:00')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00 GMT+00:00')	946713600000
Date.parse('Sat, 01 Jan 2000 08:00:00 GMT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00 GMT')	946713600000
Date.parse('Saturday, 01-Jan-00 08:00:00 GMT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00 GMT')	946713600000
Date.parse('01 Jan 00 08:00 -0000')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 08:00 -0000')	946713600000
Date.parse('Sat, 01-Jan-2000 03:00:00 UTC-0500')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 03:00:00 UTC-0500')	946713600000
Date.parse('Sat, 01-Jan-2000 03:00:00 UTC-05:00')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 03:00:00 UTC-05:00')	946713600000
Date.parse('Sat, 01-Jan-2000 03:00:00 EST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 03:00:00 EST')	946713600000
Date.parse('Sat, 01 Jan 2000 03:00:00 EST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 03:00:00 EST')	946713600000
Date.parse('Saturday, 01-Jan-00 03:00:00 EST')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 03:00:00 EST')	946713600000
Date.parse('01 Jan 00 03:00 -0500')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 03:00 -0500')	946713600000
Date.parse('Sat, 01-Jan-2000 04:00:00 EDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 04:00:00 EDT')	946713600000
Date.parse('Sat, 01 Jan 2000 04:00:00 EDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 04:00:00 EDT')	946713600000
Date.parse('Saturday, 01-Jan-00 04:00:00 EDT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 04:00:00 EDT')	946713600000
Date.parse('01 Jan 00 04:00 -0400')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 04:00 -0400')	946713600000
Date.parse('Sat, 01-Jan-2000 02:00:00 CST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 02:00:00 CST')	946713600000
Date.parse('Sat, 01 Jan 2000 02:00:00 CST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 02:00:00 CST')	946713600000
Date.parse('Saturday, 01-Jan-00 02:00:00 CST')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 02:00:00 CST')	946713600000
Date.parse('01 Jan 00 02:00 -0600')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 02:00 -0600')	946713600000
Date.parse('Sat, 01-Jan-2000 03:00:00 CDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 03:00:00 CDT')	946713600000
Date.parse('Sat, 01 Jan 2000 03:00:00 CDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 03:00:00 CDT')	946713600000
Date.parse('Saturday, 01-Jan-00 03:00:00 CDT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 03:00:00 CDT')	946713600000
Date.parse('Sat, 01-Jan-2000 01:00:00 MST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 01:00:00 MST')	946713600000
Date.parse('Sat, 01 Jan 2000 01:00:00 MST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 01:00:00 MST')	946713600000
Date.parse('Saturday, 01-Jan-00 01:00:00 MST')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 01:00:00 MST')	946713600000
Date.parse('01 Jan 00 01:00 -0700')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 01:00 -0700')	946713600000
Date.parse('Sat, 01-Jan-2000 02:00:00 MDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 02:00:00 MDT')	946713600000
Date.parse('Sat, 01 Jan 2000 02:00:00 MDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 02:00:00 MDT')	946713600000
Date.parse('Saturday, 01-Jan-00 02:00:00 MDT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 02:00:00 MDT')	946713600000
Date.parse('Sat, 01-Jan-2000 00:00:00 PST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 00:00:00 PST')	946713600000
Date.parse('Sat, 01 Jan 2000 00:00:00 PST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 00:00:00 PST')	946713600000
Date.parse('Saturday, 01-Jan-00 00:00:00 PST')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 00:00:00 PST')	946713600000
Date.parse('01 Jan 00 00:00 -0800')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 00:00 -0800')	946713600000
Date.parse('Sat, 01-Jan-2000 PST')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 PST')	946713600000
Date.parse('Sat, 01-Jan-2000 01:00:00 PDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 01:00:00 PDT')	946713600000
Date.parse('Sat, 01 Jan 2000 01:00:00 PDT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 01:00:00 PDT')	946713600000
Date.parse('Saturday, 01-Jan-00 01:00:00 PDT')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 01:00:00 PDT')	946713600000
Date.parse('Sat, 01-Jan-2000 08:00:00')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00')	946713600000
Date.parse('Sat, 01 Jan 2000 08:00:00')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00')	946713600000
Date.parse('Jan 01 2000 08:00:00')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00:00')	946713600000
Date.parse('Jan 01 08:00:00 2000')	946713600000
Date.parse('parse-local-time:' + 'Jan 01 08:00:00 2000')	946713600000
Date.parse('Saturday, 01-Jan-00 08:00:00')	946713600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00')	946713600000
Date.parse('01 Jan 00 08:00')	946713600000
Date.parse('parse-local-time:' + '01 Jan 00 08:00')	946713600000
Date.parse('Sat, 01 Jan 0 08:00:00 UT')	946713600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 0 08:00:00 UT')	946713600000
Date.parse('Sat, 01 Jan 49 08:00:00 UT')	2493100800000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 49 08:00:00 UT')	2493100800000
Date.parse('Sat, 01 Jan 50 08:00:00 UT')	-631123200000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 50 08:00:00 UT')	-631123200000
Date.parse('Sat, 01 Jan 99 08:00:00 UT')	915177600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 99 08:00:00 UT')	915177600000
Date.parse('Sat, 01 Jan 100 08:00:00 UT')	-59011430400000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 100 08:00:00 UT')	-59011430400000
Date.parse('Sat, 01-Jan-2000 08:00 PM UT')	946756800000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00 PM UT')	946756800000
Date.parse('Sat, 01 Jan 2000 08:00 PM UT')	946756800000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00 PM UT')	946756800000
Date.parse('Jan 01 2000 08:00 PM UT')	946756800000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00 PM UT')	946756800000
Date.parse('Jan 01 08:00 PM UT 2000')	946756800000
Date.parse('parse-local-time:' + 'Jan 01 08:00 PM UT 2000')	946756800000
Date.parse('Saturday, 01-Jan-00 08:00 PM UT')	946756800000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00 PM UT')	946756800000
Date.parse('01 Jan 00 08:00 PM +0000')	946756800000
Date.parse('parse-local-time:' + '01 Jan 00 08:00 PM +0000')	946756800000
Date.parse('2000-01-01T08:00:00.000Z')	946713600000
Date.parse('parse-local-time:' + '2000-01-01T08:00:00.000Z')	NaN
Date.parse('2000-01-01T08:00:00Z')	946713600000
Date.parse('parse-local-time:' + '2000-01-01T08:00:00Z')	NaN
Date.parse('2000-01-01T08:00Z')	946713600000
Date.parse('parse-local-time:' + '2000-01-01T08:00Z')	NaN
Date.parse('2000-01T08:00:00.000Z')	946713600000
Date.parse('parse-local-time:' + '2000-01T08:00:00.000Z')	NaN
Date.parse('2000T08:00:00.000Z')	946713600000
Date.parse('parse-local-time:' + '2000T08:00:00.000Z')	NaN
Date.parse('2000T08:00Z')	946713600000
Date.parse('parse-local-time:' + '2000T08:00Z')	NaN
Date.parse('2000-01T00:00:00.000-08:00')	946713600000
Date.parse('parse-local-time:' + '2000-01T00:00:00.000-08:00')	NaN
Date.parse('2000-01T08:00:00.001Z')	946713600001
Date.parse('parse-local-time:' + '2000-01T08:00:00.001Z')	NaN
Date.parse('2000-01T08:00:00.099Z')	946713600099
Date.parse('parse-local-time:' + '2000-01T08:00:00.099Z')	NaN
Date.parse('2000-01T08:00:00.999Z')	946713600999
Date.parse('parse-local-time:' + '2000-01T08:00:00.999Z')	NaN
Date.parse('2000-01T00:00:00.001-08:00')	946713600001
Date.parse('parse-local-time:' + '2000-01T00:00:00.001-08:00')	NaN
Date.parse('2000-01-01T24:00Z')	946771200000
Date.parse('parse-local-time:' + '2000-01-01T24:00Z')	NaN
Date.parse('2000-01-01T24:00:00Z')	946771200000
Date.parse('parse-local-time:' + '2000-01-01T24:00:00Z')	NaN
Date.parse('2000-01-01T24:00:00.000Z')	946771200000
Date.parse('parse-local-time:' + '2000-01-01T24:00:00.000Z')	NaN
Date.parse('2000-01-01TZ')	NaN
Date.parse('parse-local-time:' + '2000-01-01TZ')	NaN
Date.parse('2000-01-01T60Z')	NaN
Date.parse('parse-local-time:' + '2000-01-01T60Z')	NaN
Date.parse('2000-01-01T60:60Z')	NaN
Date.parse('parse-local-time:' + '2000-01-01T60:60Z')	NaN
Date.parse('2000-01-0108:00Z')	NaN
Date.parse('parse-local-time:' + '2000-01-0108:00Z')	NaN
Date.parse('2000-01-01T08Z')	NaN
Date.parse('parse-local-time:' + '2000-01-01T08Z')	NaN
Date.parse('2000-01-01T24:01')	NaN
Date.parse('parse-local-time:' + '2000-01-01T24:01')	NaN
Date.parse('2000-01-01T24:00:01')	NaN
Date.parse('parse-local-time:' + '2000-01-01T24:00:01')	NaN
Date.parse('2000-01-01T24:00:00.001')	NaN
Date.parse('parse-local-time:' + '2000-01-01T24:00:00.001')	NaN
Date.parse('2000-01-01T24:00:00.999Z')	NaN
Date.parse('parse-local-time:' + '2000-01-01T24:00:00.999Z')	NaN
Date.parse('2000-01-02T00:00')	946771200000
Date.parse('parse-local-time:' + '2000-01-02T00:00')	NaN
Date.parse('2000-01-02T00:00:00')	946771200000
Date.parse('parse-local-time:' + '2000-01-02T00:00:00')	NaN
Date.parse('2000-01-02T00:00:00.000')	946771200000
Date.parse('parse-local-time:' + '2000-01-02T00:00:00.000')	NaN
Date.parse('2000-01-02')	946771200000
Date.parse('parse-local-time:' + '2000-01-02')	978393600000
Date.parse('May 25 2008 1:30 (PM)) UTC')	NaN
Date.parse('parse-local-time:' + 'May 25 2008 1:30 (PM)) UTC')	NaN
Date.parse('May 25 2008 1:30( )AM (PM)')	NaN
Date.parse('parse-local-time:' + 'May 25 2008 1:30( )AM (PM)')	NaN
Date.parse('a1')	NaN
Date.parse('parse-local-time:' + 'a1')	NaN
Date.parse('nasfdjklsfjoaifg1')	NaN
Date.parse('parse-local-time:' + 'nasfdjklsfjoaifg1')	NaN
Date.parse('x_2')	NaN
Date.parse('parse-local-time:' + 'x_2')	NaN
Date.parse('May 25 2008 AAA (GMT)')	NaN
Date.parse('parse-local-time:' + 'May 25 2008 AAA (GMT)')	NaN
var bad = ""; for (var i = 0; i < 24 * 365 * 100; i += 150) { var ms = i * (3600 * 1000); var s = (new Date(ms)).toString(); if (Date.parse(s) !== ms) bad += s + ";"; } bad + "|"	|
new Date('2000-01-01').getTimezoneOffset()*1000*60	0
"#;

const DATE_CASES_EUROPE_PARIS: &str = r#"
new Date(0).toString()	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)
new Date(0).toTimeString()	01:00:00 GMT+0100 (CET)
new Date(0).toLocaleString()	01/01/1970, 01:00:00
new Date(0).toLocaleTimeString()	01:00:00
new Date(1783166400000).toString()	Sat Jul 04 2026 14:00:00 GMT+0200 (CEST)
new Date(1783166400000).toTimeString()	14:00:00 GMT+0200 (CEST)
new Date(1783166400000).getTimezoneOffset()	-120
new Date(0).getTimezoneOffset()	-60
new Date(-1e15).toString()	Tue Apr 05 -29719 22:22:41 GMT+0009 (LMT)
new Date(-1e15).getTimezoneOffset()	-9.35
new Date(8.64e15).toString()	Sat Sep 13 275760 02:00:00 GMT+0200 (CEST)
new Date(-8.64e15).toString()	Tue Apr 20 -271821 00:09:21 GMT+0009 (LMT)
new Date(-62198755200000).toString()	Fri Jan 01 -0001 00:09:21 GMT+0009 (LMT)
new Date(2016, 8, 1, 12, 23, 45).getTime()	1472725425000
new Date(2016, 8, 1, 12, 23, 45).toString()	Thu Sep 01 2016 12:23:45 GMT+0200 (CEST)
new Date(2026, 2, 29, 2, 30).getHours()	3
new Date(2026, 10, 1, 1, 30).getTime()	1793493000000
new Date(275760, 8, 13).getTime()	8639999992800000
new Date(1970, 0, 1, 0, 0, 0, 8.64e15).getTime()	8639999992800000
Date.parse("2026-01-02T10:00:00")	1767344400000
Date.parse("2026-01-02 10:00")	1767344400000
Date.parse("Jan 2, 2026")	1767308400000
Date.parse("2026/01/02")	1767308400000
Date.parse("02/01/2026")	1769900400000
Date.parse("2026-1-2")	1767308400000
Date.parse("2026-01-02T10:00")	1767344400000
Date.parse("Friday, January 2, 2026")	1767308400000
Date.parse("2 January 2026")	1767308400000
Date.parse(" 2026-01-02")	1767308400000
Date.parse("Mon Jan  2 15:04:05 2006")	1136210645000
Date.parse("December 04, 1986")	534034800000
Date.parse("2006-06-02T15:04:05.000")	1149253445000
Date.parse("2006-01-02 15:04:05.123")	1136210645123
Date.parse("12:00 AM Jan 1 2000")	946681200000
Date.parse("12:30 am Jan 1 2000")	946683000000
Date.parse("2000 -5")	-62324986161000
Date.parse("2000 Jan 5 10:00:5.5")	947062805500
Date.parse("Jan 5 +275760")	8639978223600000
Date.parse("1 Jan")	978303600000
Date.parse("Jan 1")	978303600000
Date.parse("13 Jan 2000")	947718000000
Date.parse("1 13 2000")	947718000000
Date.parse("Jan 32 2000")	946681200000
Date.parse("Ja 1 2000")	946681200000
Date.parse(0)	946681200000
Date.parse("Jané 1 2000")	946681200000
var d = new Date(0); d.setHours(1)	0
var d = new Date(0); d.setHours(1, 2, 3, 4)	123004
var d = new Date(0); d.setHours(1, 2, 3, 4, 5)	123004
var d = new Date(NaN); d.setFullYear(2000)	946681200000
new Date(0)[Symbol.toPrimitive]("string")	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)
new Date(0)[Symbol.toPrimitive]("default")	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)
new Date(0) + 1	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)1
`${new Date(0)}`	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)
String(new Date(0))	Thu Jan 01 1970 01:00:00 GMT+0100 (CET)
new Date(2016, 8, 1, 12, 23, 45).getUTCHours()	10
new Date(1151877845000).getHours()	0
new Date(1e14).getTimezoneOffset()	-60
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCHours(13); d.getHours()	15
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCDate(31); d.getTime()	1475317425000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMonth(0, 15); d.getTime()	1452853425000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCSeconds(1, 2); d.getTime()	1472725381002
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(1, 2, 3); d.getTime()	1472724062003
var d = new Date(1774746000000); d.setHours(2); d.getTime()	1774746000000
var d = new Date(1761350400000); d.setHours(2, 30); d.getTime()	1761352200000
Date.parse('Sat, 01-Jan-2000 08:00:00')	946710000000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00')	946710000000
Date.parse('Sat, 01 Jan 2000 08:00:00')	946710000000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00')	946710000000
Date.parse('Jan 01 2000 08:00:00')	946710000000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00:00')	946710000000
Date.parse('Jan 01 08:00:00 2000')	946710000000
Date.parse('parse-local-time:' + 'Jan 01 08:00:00 2000')	946710000000
Date.parse('Saturday, 01-Jan-00 08:00:00')	946710000000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00')	946710000000
Date.parse('01 Jan 00 08:00')	946710000000
Date.parse('parse-local-time:' + '01 Jan 00 08:00')	946710000000
Date.parse('2000-01-02T00:00')	946767600000
Date.parse('2000-01-02T00:00:00')	946767600000
Date.parse('2000-01-02T00:00:00.000')	946767600000
Date.parse('parse-local-time:' + '2000-01-02')	978390000000
new Date('2000-01-01').getTimezoneOffset()*1000*60	-3600000
"#;

const DATE_CASES_AMERICA_NEW_YORK: &str = r#"
new Date(0).toString()	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)
new Date(0).toDateString()	Wed Dec 31 1969
new Date(0).toTimeString()	19:00:00 GMT-0500 (EST)
new Date(0).toLocaleString()	12/31/1969, 19:00:00
new Date(0).toLocaleDateString()	12/31/1969
new Date(0).toLocaleTimeString()	19:00:00
new Date(1783166400000).toString()	Sat Jul 04 2026 08:00:00 GMT-0400 (EDT)
new Date(1783166400000).toTimeString()	08:00:00 GMT-0400 (EDT)
new Date(1783166400000).getTimezoneOffset()	240
new Date(0).getTimezoneOffset()	300
new Date(-1e15).toString()	Tue Apr 05 -29719 17:17:18 GMT-0456 (LMT)
new Date(-1e15).getTimezoneOffset()	296.03333333333336
new Date(8.64e15).toString()	Fri Sep 12 275760 20:00:00 GMT-0400 (EDT)
new Date(-8.64e15).toString()	Mon Apr 19 -271821 19:03:58 GMT-0456 (LMT)
new Date(-62198755200000).toString()	Thu Dec 31 -0002 19:03:58 GMT-0456 (LMT)
new Date(2016, 8, 1, 12, 23, 45).getTime()	1472747025000
new Date(2016, 8, 1, 12, 23, 45).toString()	Thu Sep 01 2016 12:23:45 GMT-0400 (EDT)
new Date(2026, 2, 8, 2, 30).getHours()	1
new Date(2026, 10, 1, 1, 30).getTime()	1793511000000
new Date(275760, 8, 13).getTime()	NaN
new Date(1970, 0, 1, 0, 0, 0, 8.64e15).getTime()	NaN
Date.parse("2026-01-02T10:00:00")	1767366000000
Date.parse("2026-01-02 10:00")	1767366000000
Date.parse("Jan 2, 2026")	1767330000000
Date.parse("2026/01/02")	1767330000000
Date.parse("02/01/2026")	1769922000000
Date.parse("2026-1-2")	1767330000000
Date.parse("2026-01-02T10:00")	1767366000000
Date.parse("Friday, January 2, 2026")	1767330000000
Date.parse("2 January 2026")	1767330000000
Date.parse(" 2026-01-02")	1767330000000
Date.parse("Mon Jan  2 15:04:05 2006")	1136232245000
Date.parse("December 04, 1986")	534056400000
Date.parse("2006-06-02T15:04:05.000")	1149275045000
Date.parse("2006-01-02 15:04:05.123")	1136232245123
Date.parse("12:00 AM Jan 1 2000")	946702800000
Date.parse("12:30 am Jan 1 2000")	946704600000
Date.parse("2000 -5")	-62324967838000
Date.parse("2000 Jan 5 10:00:5.5")	947084405500
Date.parse("Jan 5 +275760")	8639978245200000
Date.parse("1 Jan")	978325200000
Date.parse("Jan 1")	978325200000
Date.parse("13 Jan 2000")	947739600000
Date.parse("1 13 2000")	947739600000
Date.parse("Jan 32 2000")	946702800000
Date.parse("Ja 1 2000")	946702800000
Date.parse(0)	946702800000
Date.parse("Jané 1 2000")	946702800000
var d = new Date(0); d.setHours(1)	-64800000
var d = new Date(0); d.setDate(2)	-2505600000
var d = new Date(0); d.setMonth(2)	-23760000000
var d = new Date(0); d.setFullYear(1971)	63072000000
var d = new Date(0); d.setFullYear(1971, 2, 3)	36892800000
var d = new Date(0); d.setFullYear(1971, 2, 3, 4)	36892800000
var d = new Date(0); d.setMonth(2, 3, 4); d.getTime()	-26179200000
var d = new Date(0); d.setDate(2, 3); d.getTime()	-2505600000
var d = new Date(0); d.setHours(1, 2, 3, 4)	-64676996
var d = new Date(0); d.setHours(1, 2, 3, 4, 5)	-64676996
var d = new Date(NaN); d.setFullYear(2000)	946702800000
var d = new Date(0); d.setFullYear({valueOf(){ d.setTime(NaN); return 2000 }})	978307200000
new Date(0)[Symbol.toPrimitive]("string")	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)
new Date(0)[Symbol.toPrimitive]("default")	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)
new Date(0) + 1	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)1
`${new Date(0)}`	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)
String(new Date(0))	Wed Dec 31 1969 19:00:00 GMT-0500 (EST)
new Date(2016, 8, 1, 12, 23, 45).getUTCHours()	16
new Date(1151877845000).getHours()	18
new Date(1e14).getTimezoneOffset()	300
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCHours(13); d.getHours()	9
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCDate(31); d.getTime()	1475339025000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMonth(0, 15); d.getTime()	1452875025000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCSeconds(1, 2); d.getTime()	1472746981002
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(1, 2, 3); d.getTime()	1472745662003
var d = new Date(1774746000000); d.setHours(2); d.getTime()	1774677600000
var d = new Date(1761350400000); d.setHours(2, 30); d.getTime()	1761287400000
Date.parse('Sat, 01-Jan-2000 08:00:00')	946731600000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00')	946731600000
Date.parse('Sat, 01 Jan 2000 08:00:00')	946731600000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00')	946731600000
Date.parse('Jan 01 2000 08:00:00')	946731600000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00:00')	946731600000
Date.parse('Jan 01 08:00:00 2000')	946731600000
Date.parse('parse-local-time:' + 'Jan 01 08:00:00 2000')	946731600000
Date.parse('Saturday, 01-Jan-00 08:00:00')	946731600000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00')	946731600000
Date.parse('01 Jan 00 08:00')	946731600000
Date.parse('parse-local-time:' + '01 Jan 00 08:00')	946731600000
Date.parse('2000-01-02T00:00')	946789200000
Date.parse('2000-01-02T00:00:00')	946789200000
Date.parse('2000-01-02T00:00:00.000')	946789200000
Date.parse('parse-local-time:' + '2000-01-02')	978411600000
new Date('2000-01-01').getTimezoneOffset()*1000*60	18000000
"#;

const DATE_CASES_AUSTRALIA_LORD_HOWE: &str = r#"
new Date(0).toString()	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)
new Date(0).toTimeString()	10:00:00 GMT+1000 (AEST)
new Date(0).toLocaleString()	01/01/1970, 10:00:00
new Date(0).toLocaleTimeString()	10:00:00
new Date(1783166400000).toString()	Sat Jul 04 2026 22:30:00 GMT+1030 (+1030)
new Date(1783166400000).toTimeString()	22:30:00 GMT+1030 (+1030)
new Date(1783166400000).getTimezoneOffset()	-630
new Date(0).getTimezoneOffset()	-600
new Date(-1e15).toString()	Wed Apr 06 -29719 08:49:40 GMT+1036 (LMT)
new Date(-1e15).getTimezoneOffset()	-636.3333333333334
new Date(8.64e15).toString()	Sat Sep 13 275760 10:30:00 GMT+1030 (+1030)
new Date(-8.64e15).toString()	Tue Apr 20 -271821 10:36:20 GMT+1036 (LMT)
new Date(-62198755200000).toString()	Fri Jan 01 -0001 10:36:20 GMT+1036 (LMT)
new Date(2016, 8, 1, 12, 23, 45).getTime()	1472694825000
new Date(2016, 8, 1, 12, 23, 45).toString()	Thu Sep 01 2016 12:23:45 GMT+1030 (+1030)
new Date(2026, 10, 1, 1, 30).getTime()	1793457000000
new Date(275760, 8, 13).getTime()	8639999962200000
new Date(1970, 0, 1, 0, 0, 0, 8.64e15).getTime()	8639999962200000
Date.parse("2026-01-02T10:00:00")	1767308400000
Date.parse("2026-01-02 10:00")	1767308400000
Date.parse("Jan 2, 2026")	1767272400000
Date.parse("2026/01/02")	1767272400000
Date.parse("02/01/2026")	1769864400000
Date.parse("2026-1-2")	1767272400000
Date.parse("2026-01-02T10:00")	1767308400000
Date.parse("Friday, January 2, 2026")	1767272400000
Date.parse("2 January 2026")	1767272400000
Date.parse(" 2026-01-02")	1767272400000
Date.parse("Mon Jan  2 15:04:05 2006")	1136174645000
Date.parse("December 04, 1986")	533998800000
Date.parse("2006-06-02T15:04:05.000")	1149222845000
Date.parse("2006-01-02 15:04:05.123")	1136174645123
Date.parse("12:00 AM Jan 1 2000")	946645200000
Date.parse("12:30 am Jan 1 2000")	946647000000
Date.parse("2000 -5")	-62325023780000
Date.parse("2000 Jan 5 10:00:5.5")	947026805500
Date.parse("Jan 5 +275760")	8639978187600000
Date.parse("1 Jan")	978267600000
Date.parse("Jan 1")	978267600000
Date.parse("13 Jan 2000")	947682000000
Date.parse("1 13 2000")	947682000000
Date.parse("Jan 32 2000")	946645200000
Date.parse("Ja 1 2000")	946645200000
Date.parse(0)	946645200000
Date.parse("Jané 1 2000")	946645200000
var d = new Date(0); d.setHours(1)	-32400000
var d = new Date(0); d.setHours(1, 2, 3, 4)	-32276996
var d = new Date(0); d.setHours(1, 2, 3, 4, 5)	-32276996
var d = new Date(NaN); d.setFullYear(2000)	946645200000
var d = new Date(0); d.setFullYear({valueOf(){ d.setTime(NaN); return 2000 }})	946681200000
new Date(0)[Symbol.toPrimitive]("string")	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)
new Date(0)[Symbol.toPrimitive]("default")	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)
new Date(0) + 1	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)1
`${new Date(0)}`	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)
String(new Date(0))	Thu Jan 01 1970 10:00:00 GMT+1000 (AEST)
new Date(2016, 8, 1, 12, 23, 45).getUTCHours()	1
new Date(2016, 8, 1, 12, 23, 45).getUTCMinutes()	53
new Date(-1e14).getDay()	5
new Date(1151877845000).getHours()	8
new Date(1e14).getTimezoneOffset()	-660
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCHours(13); d.getHours()	0
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(52); d.getMinutes()	22
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCDate(31); d.getTime()	1475286825000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMonth(0, 15); d.getTime()	1452822825000
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCSeconds(1, 2); d.getTime()	1472694781002
var d = new Date(2016, 8, 1, 12, 23, 45); d.setUTCMinutes(1, 2, 3); d.getTime()	1472691662003
var d = new Date(1774746000000); d.setHours(2); d.getTime()	1774710000000
var d = new Date(1761350400000); d.setHours(2, 30); d.getTime()	1761319800000
Date.parse('Sat, 01-Jan-2000 08:00:00')	946674000000
Date.parse('parse-local-time:' + 'Sat, 01-Jan-2000 08:00:00')	946674000000
Date.parse('Sat, 01 Jan 2000 08:00:00')	946674000000
Date.parse('parse-local-time:' + 'Sat, 01 Jan 2000 08:00:00')	946674000000
Date.parse('Jan 01 2000 08:00:00')	946674000000
Date.parse('parse-local-time:' + 'Jan 01 2000 08:00:00')	946674000000
Date.parse('Jan 01 08:00:00 2000')	946674000000
Date.parse('parse-local-time:' + 'Jan 01 08:00:00 2000')	946674000000
Date.parse('Saturday, 01-Jan-00 08:00:00')	946674000000
Date.parse('parse-local-time:' + 'Saturday, 01-Jan-00 08:00:00')	946674000000
Date.parse('01 Jan 00 08:00')	946674000000
Date.parse('parse-local-time:' + '01 Jan 00 08:00')	946674000000
Date.parse('2000-01-02T00:00')	946731600000
Date.parse('2000-01-02T00:00:00')	946731600000
Date.parse('2000-01-02T00:00:00.000')	946731600000
Date.parse('parse-local-time:' + '2000-01-02')	978354000000
new Date('2000-01-01').getTimezoneOffset()*1000*60	-39600000
"#;
