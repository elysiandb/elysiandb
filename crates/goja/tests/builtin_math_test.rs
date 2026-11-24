mod jsrun;

use goja::Value;
use jsrun::*;

const MATH_CASES: &str = r##"
[Math.abs(-2.5), Math.abs(-0), Math.abs("x"), Math.abs(null)].join()	2.5,0,NaN,0	2.5,0,NaN,0	-
[Math.acos(0.5), Math.acos(-1), Math.acos(2), Math.acos(0.75)].join()	1.0471975511965976,3.141592653589793,NaN,0.7227342478134156	1.0471975511965976,3.141592653589793,NaN,0.7227342478134156	-
[Math.acosh(1), Math.acosh(1.5), Math.acosh(3), Math.acosh(1e9), Math.acosh(0.5)].join()	0,0.9624236501192069,1.7627471740390859,21.416413017506354,NaN	0,0.9624236501192069,1.7627471740390859,21.416413017506354,NaN	fma
[Math.asin(0.5), Math.asin(-0.8), Math.asin(1.5), Math.asin(-0)].join()	0.5235987755982989,-0.9272952180016123,NaN,0	0.5235987755982989,-0.9272952180016123,NaN,0	-
[Math.asinh(0.3), Math.asinh(-3), Math.asinh(1e-30), Math.asinh(1e10)].join()	0.29567304756342244,-1.8184464592320668,1e-30,23.7189981105004	0.29567304756342244,-1.8184464592320668,1e-30,23.7189981105004	fma
[Math.atan(0.3), Math.atan(-1.5), Math.atan(3), Math.atan(1e20)].join()	0.2914567944778671,-0.982793723247329,1.2490457723982544,1.5707963267948966	0.2914567944778671,-0.982793723247329,1.2490457723982544,1.5707963267948966	-
[Math.atan2(1, 2), Math.atan2(-1, -2), Math.atan2(0, -0), Math.atan2(-0, 0), Math.atan2(Infinity, -Infinity)].join()	0.4636476090008061,-2.677945044588987,3.141592653589793,0,2.356194490192345	0.4636476090008061,-2.677945044588987,3.141592653589793,0,2.356194490192345	-
[Math.atanh(0.3), Math.atanh(-0.7), Math.atanh(1), Math.atanh(2)].join()	0.30951960420311175,-0.8673005276940531,Infinity,NaN	0.30951960420311175,-0.8673005276940531,Infinity,NaN	fma
[Math.cbrt(27), Math.cbrt(-10), Math.cbrt(1e-310), Math.cbrt(0.001)].join()	3,-2.154434690031884,4.641588833612774e-104,0.1	3,-2.154434690031884,4.641588833612774e-104,0.1	-
[Math.ceil(1.2), Math.ceil(-1.2), Object.is(Math.ceil(-0.5), -0), Math.ceil(1e300)].join()	2,-1,true,1e+300	2,-1,true,1e+300	-
[Math.clz32(1), Math.clz32(0), Math.clz32(-1), Math.clz32(3.9), Math.clz32(2**32 + 5)].join()	31,32,0,30,29	31,32,0,30,29	-
[Math.cos(1), Math.cos(-3), Math.cos(1e10), Math.cos(1e300), Math.cos(Infinity)].join()	0.5403023058681398,-0.9899924966004454,0.873119622676856,-0.5753861119575491,NaN	0.5403023058681398,-0.9899924966004454,0.873119622676856,-0.5753861119575491,NaN	-
[Math.cosh(0.3), Math.cosh(-5), Math.cosh(30), Math.cosh(800)].join()	1.0453385141288605,74.20994852478785,5343237290762.231,Infinity	1.0453385141288605,74.20994852478785,5343237290762.23,Infinity	fma
[Math.exp(1), Math.exp(-3.5), Math.exp(700), Math.exp(-800)].join()	2.718281828459045,0.0301973834223185,1.0142320547350045e+304,0	2.718281828459045,0.0301973834223185,1.0142320547350045e+304,0	fma
[Math.expm1(1e-10), Math.expm1(0.2), Math.expm1(-0.9), Math.expm1(5), Math.expm1(40), Math.expm1(-50)].join()	1.00000000005e-10,0.22140275816016985,-0.5934303402594009,147.4131591025766,235385266837019970,-1	1.00000000005e-10,0.22140275816016985,-0.5934303402594009,147.4131591025766,235385266837019970,-1	fma
[Math.floor(1.7), Math.floor(-1.2), Object.is(Math.floor(-0), -0), Math.floor(2**60)].join()	1,-2,true,1152921504606847000	1,-2,true,1152921504606847000	-
[Math.fround(1.1), Math.fround(1e40), Math.fround(1e-50), Math.fround(-0.1)].join()	1.100000023841858,Infinity,0,-0.10000000149011612	1.100000023841858,Infinity,0,-0.10000000149011612	-
[Math.hypot(3, 4), Math.hypot(1, 2, 3), Math.hypot(1e200, 1e200), Math.hypot(0.1, 0.2, 0.3, 0.4), Math.hypot(-0), Math.hypot()].join()	5,3.741657386773941,1.414213562373095e+200,0.5477225575051662,0,0	5,3.741657386773941,1.414213562373095e+200,0.5477225575051662,0,0	-
[Math.imul(3, 4), Math.imul(-5, 12), Math.imul(0xffffffff, 5), Math.imul(2**31, 2)].join()	12,-60,-5,0	12,-60,-5,0	-
[Math.log(10), Math.log(0.5), Math.log(0), Math.log(-1)].join()	2.302585092994046,-0.6931471805599453,-Infinity,NaN	2.302585092994046,-0.6931471805599453,-Infinity,NaN	-
[Math.log1p(1e-10), Math.log1p(0.5), Math.log1p(-0.5), Math.log1p(10), Math.log1p(-1)].join()	9.999999999500001e-11,0.4054651081081644,-0.6931471805599453,2.3978952727983707,-Infinity	9.999999999500001e-11,0.4054651081081644,-0.6931471805599453,2.3978952727983707,-Infinity	-
[Math.log10(1000), Math.log10(2), Math.log10(1e-5), Math.log10(0.3)].join()	3,0.3010299956639812,-5,-0.5228787452803376	3,0.3010299956639812,-5,-0.5228787452803376	-
[Math.log2(8), Math.log2(10), Math.log2(0.1), Math.log2(3)].join()	3,3.321928094887362,-3.321928094887362,1.5849625007211563	3,3.321928094887362,-3.321928094887362,1.5849625007211563	-
[Math.max(1, 3, 2), Math.max(), Math.max(1, NaN), Object.is(Math.max(-0, 0), 0), Math.max("7", 2)].join()	3,-Infinity,NaN,true,7	3,-Infinity,NaN,true,7	-
[Math.min(1, 3, 2), Math.min(), Math.min(1, NaN), Object.is(Math.min(0, -0), -0)].join()	1,Infinity,NaN,true	1,Infinity,NaN,true	-
[Math.pow(2, 10), Math.pow(2, 0.5), Math.pow(-8, 1/3), Math.pow(1, Infinity), Math.pow(NaN, 0), Math.pow(10, -3), Math.pow(1.1, 1000)].join()	1024,1.4142135623730951,NaN,NaN,1,0.001,2.469932918005993e+41	1024,1.4142135623730951,NaN,NaN,1,0.001,2.469932918005993e+41	fma
[Math.round(2.5), Math.round(-2.5), Math.round(0.49999999999999994), Object.is(Math.round(-0.2), -0), Math.round(1e20)].join()	3,-2,0,true,100000000000000000000	3,-2,0,true,100000000000000000000	-
[Math.sign(-3), Math.sign(4), Object.is(Math.sign(-0), -0), Math.sign(NaN), Math.sign("-2")].join()	-1,1,true,NaN,-1	-1,1,true,NaN,-1	-
[Math.sin(1), Math.sin(-3), Math.sin(1e10), Math.sin(1e300), Object.is(Math.sin(-0), -0)].join()	0.8414709848078965,-0.1411200080598672,-0.48750602508751073,-0.8178819121159087,true	0.8414709848078965,-0.1411200080598672,-0.48750602508751073,-0.8178819121159087,true	-
[Math.sinh(0.3), Math.sinh(-2), Math.sinh(30), Math.sinh(1e-20)].join()	0.3045202934471426,-3.626860407847019,5343237290762.231,1e-20	0.3045202934471426,-3.626860407847019,5343237290762.23,1e-20	fma
[Math.sqrt(2), Math.sqrt(-1), Math.sqrt(1e-300), Object.is(Math.sqrt(-0), -0)].join()	1.4142135623730951,NaN,1e-150,true	1.4142135623730951,NaN,1e-150,true	-
[Math.tan(1), Math.tan(-3), Math.tan(1e10), Math.tan(Math.PI / 2), Math.tan(1e-8)].join()	1.557407724654902,0.1425465430742778,-0.5583496378112419,16331239353195392,1e-8	1.557407724654902,0.1425465430742778,-0.5583496378112419,16331239353195392,1e-8	-
[Math.tanh(0.3), Math.tanh(-0.7), Math.tanh(5), Math.tanh(50), Object.is(Math.tanh(-0), -0)].join()	0.2913126124515909,-0.6043677771171635,0.9999092042625951,1,true	0.2913126124515909,-0.6043677771171635,0.9999092042625951,1,true	fma
[Math.trunc(4.7), Math.trunc(-4.7), Object.is(Math.trunc(-0.3), -0), Math.trunc(2**60 + 0.5)].join()	4,-4,true,1152921504606847000	4,-4,true,1152921504606847000	-
[Math.E, Math.LN10, Math.LN2, Math.LOG10E, Math.LOG2E, Math.PI, Math.SQRT1_2, Math.SQRT2].join()	2.718281828459045,2.302585092994046,0.6931471805599453,0.4342944819032518,1.4426950408889634,3.141592653589793,0.7071067811865476,1.4142135623730951	2.718281828459045,2.302585092994046,0.6931471805599453,0.4342944819032518,1.4426950408889634,3.141592653589793,0.7071067811865476,1.4142135623730951	-
var s = 0; for (var i = 1; i < 200; i++) { s += Math.sin(i * 0.37) + Math.cos(i * 1.3) + Math.tan(i * 0.11) } s	-144.1078568187821	-144.1078568187821	-
var s = 0; for (var i = 1; i < 200; i++) { s += Math.asin(i / 201) + Math.acos(-i / 201) + Math.atan(i / 7) + Math.atan2(i, 100 - i) } s	1084.4003711824744	1084.4003711824744	-
var s = 0; for (var i = 1; i < 200; i++) { s += Math.sinh(i / 150) + Math.cosh(i / 30) + Math.tanh(i / 160) + Math.asinh(i / 3) + Math.acosh(1 + i / 13) + Math.atanh(i / 201) } s	13287.502556508964	13287.502556508964	fma
var s = 0; for (var i = 1; i < 200; i++) { s += Math.cbrt(i * 7.7) + Math.expm1(i / 20 - 5) + Math.log1p(i / 9) + Math.log2(i * 1.7) + Math.log10(i * 3.3) + Math.exp(i / 50) + Math.log(i * 0.9) } s	10232.186664490706	10232.186664490706	fma
var s = 0; for (var i = 1; i < 200; i++) { s += Math.hypot(i / 3, 7 - i / 9, 0.25) + Math.pow(i / 17, 1.7) } s	11678.672915207831	11678.672915207831	fma
try { Math.abs(1n) } catch (e) { String(e) }	TypeError: Cannot convert a BigInt value to a number	TypeError: Cannot convert a BigInt value to a number	-
try { Math.max(1, {valueOf: function() { throw new Error("vo") }}) } catch (e) { String(e) }	Error: vo	Error: vo	-
var c = 0; Math.min(NaN, {valueOf: function() { c++; return 1 }}, {valueOf: function() { c++; return 2 }}); c	2	2	-
var c = 0; try { Math.hypot({valueOf: function() { c++; return Infinity }}, {valueOf: function() { c++; throw new Error("h") }}) } catch (e) { c + String(e) }	2Error: h	2Error: h	-
String(Math) + Object.prototype.toString.call(Math) + Object.getOwnPropertyNames(Math).length	[object Math][object Math]43	[object Math][object Math]43	-
"##;

#[test]
fn test_ipow_overflow() {
    test_script_with_test_lib(
        r#"
	assert.sameValue(Math.pow(65536, 6), 7.922816251426434e+28);
	assert.sameValue(Math.pow(10, 19), 1e19);
	assert.sameValue(Math.pow(2097151, 3), 9223358842721534000);
	assert.sameValue(Math.pow(2097152, 3), 9223372036854776000);
	assert.sameValue(Math.pow(-2097151, 3), -9223358842721534000);
	assert.sameValue(Math.pow(-2097152, 3), -9223372036854776000);
	assert.sameValue(Math.pow(9007199254740992, 0), 1);
	assert.sameValue(Math.pow(-9007199254740992, 0), 1);
	assert.sameValue(Math.pow(0, 0), 1);
	"#,
        Value::Undefined,
    );
}

#[test]
fn math_builtin_matches_goja() {
    on_big_stack(|| {
        let mut failures = Vec::new();
        let mut checked = 0;
        for line in MATH_CASES.lines().filter(|l| !l.is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            let (script, arm64, amd64, fma) = (f[0], f[1], f[2], f[3]);
            let want = if cfg!(target_arch = "aarch64") {
                arm64
            } else if fma == "fma" && gocompat::math::use_fma() {
                continue;
            } else {
                amd64
            };
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
        assert!(checked > 30);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
