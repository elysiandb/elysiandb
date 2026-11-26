mod jsrun;

use gocompat::time::Time;
use jsrun::*;
use talos::Value;
use talos::host::DocValue;

#[test]
fn test_json_parse_reviver() {
    test_script(
        r#"
	JSON.parse('{"p": 5}', function(key, value) {
	  return typeof value === 'number'
        ? value * 2 // return value * 2 for numbers
	    : value     // return everything else unchanged
	 })["p"]
	"#,
        int(10),
    );
}

#[test]
fn test_quote_malformed_surrogate_pair() {
    test_script(r#"JSON.stringify("\uD800")"#, str_value(r#""\ud800""#));
}

#[test]
fn test_eof_wrapping() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let err = match vm.run_string("JSON.parse('{')") {
            Ok(_) => panic!("Expected error"),
            Err(e) => e.error(),
        };
        assert!(
            err.contains("invalid JSON: unexpected end of input"),
            "Error doesn't contain human-friendly wrapper: {err}"
        );
    });
}

#[test]
fn test_json_escape() {
    test_script(
        r#"
	var a = "\\+1";
	JSON.stringify(a);
	"#,
        str_value(r#""\\+1""#),
    );
}

#[test]
fn test_json_object_in_array() {
    test_script(
        r#"
	var a = "[{\"a\":1},{\"a\":2}]";
	JSON.stringify(JSON.parse(a)) == a;
	"#,
        Value::Bool(true),
    );
}

#[test]
fn test_json_quirky_numbers() {
    test_script(
        r#"
	var s;
	s = JSON.stringify(NaN);
	if (s != "null") {
		throw new Error("NaN: " + s);
	}

	s = JSON.stringify(Infinity);
	if (s != "null") {
		throw new Error("Infinity: " + s);
	}

	s = JSON.stringify(-Infinity);
	if (s != "null") {
		throw new Error("-Infinity: " + s);
	}

	"#,
        Value::Undefined,
    );
}

#[test]
fn test_json_nil() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        vm.set("i", Value::Null).expect("set");
        let ret = match vm.run_string("\n\tJSON.stringify(i);\n\t") {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let s = ret.go_string(&mut vm).expect("string");
        assert_eq!(s, "null", "Expected 'null', got: {s}");
    });
}
const JSON_CASES: &[(&str, &str)] = &[
    (r##"JSON.stringify([1], null, "é").length"##, r##"7"##),
    (
        r##"escape(JSON.stringify([1], null, "é"))"##,
        r##"%5B%0A%C3%A91%0A%5D"##,
    ),
    (
        r##"escape(JSON.stringify({a: {x: [], y: {z: 1}}}, null, 2))"##,
        r##"%7B%0A%20%20%22a%22%3A%20%7B%0A%20%20%20%20%22x%22%3A%20%5B%5D%2C%0A%20%20%20%20%22y%22%3A%20%7B%0A%20%20%20%20%20%20%20%20%22z%22%3A%201%0A%20%20%20%20%20%20%7D%0A%20%20%7D%0A%7D"##,
    ),
    (r##"JSON.stringify({a:1}, null, NaN)"##, r##"{"a":1}"##),
    (
        r##"try { JSON.parse("1", null) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot convert undefined or null to object"##,
    ),
    (
        r##"escape(JSON.stringify("\ud800\udc00\udbff"))"##,
        r##"%22%uD800%uDC00%5Cudbff%22"##,
    ),
    (
        r##"escape(JSON.stringify({a:{}, b:{c:1}}, null, 1))"##,
        r##"%7B%0A%20%22a%22%3A%20%7B%7D%2C%0A%20%22b%22%3A%20%7B%0A%20%20%20%22c%22%3A%201%0A%20%20%7D%0A%7D"##,
    ),
    (r##"JSON.stringify(Object(Symbol()))"##, r##"undefined"##),
    (
        r##"try{JSON.stringify(Object(1n))}catch(e){String(e)}"##,
        r##"TypeError: Do not know how to serialize a BigInt"##,
    ),
    (
        r##"escape(JSON.stringify([1,2], function(k,v){return v}, "ab".repeat(8)))"##,
        r##"%5B%0Aababababab1%2C%0Aababababab2%0A%5D"##,
    ),
    (
        r##"escape(JSON.stringify([1], null, "\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9"))"##,
        r##"%5B%0A%C3%A9%C3%A9%C3%A9%C3%A9%C3%A91%0A%5D"##,
    ),
    (
        r##"JSON.stringify({a: [1, {b: 2}]}, null, 2)"##,
        r##"{
  "a": [
    1,
    {
      "b": 2
    }
  ]
}"##,
    ),
    (r##"JSON.stringify({a: 1, b: 2}, ["b"])"##, r##"{"b":2}"##),
    (
        r##"escape(JSON.stringify(" \ud800"))"##,
        r##"%22%20%5Cud800%22"##,
    ),
    (
        r##"JSON.stringify({k: 1e21, m: -0, n: NaN, i: Infinity})"##,
        r##"{"k":1e+21,"m":0,"n":null,"i":null}"##,
    ),
    (
        r##"JSON.stringify({b: 1, a: [1, "x", null, undefined, function () {}], u: undefined, d: new Date(0)})"##,
        r##"{"b":1,"a":[1,"x",null,null,null],"d":"1970-01-01T00:00:00.000Z"}"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('{"a":1.0,"b":1e2,"c":[1,{"d":null}],"e":12345678901234567890}'))"##,
        r##"{"a":1,"b":100,"c":[1,{"d":null}],"e":12345678901234567000}"##,
    ),
    (r##"typeof JSON.parse("1.0")"##, r##"number"##),
    (
        r##"var a = {}; a.a = a; try { JSON.stringify(a) } catch (e) { String(e) }"##,
        r##"TypeError: Converting circular structure to JSON"##,
    ),
    (
        r##"var r=[]; try { JSON.parse("1 2") } catch (e) { r.push(String(e)) } try { JSON.parse("") } catch (e) { r.push(String(e)) } try { JSON.parse("[1,") } catch (e) { r.push(String(e)) } try { JSON.parse("{bad") } catch (e) { r.push(String(e)) } try { JSON.parse("\"\\x\"") } catch (e) { r.push(String(e)) } try { JSON.parse("01") } catch (e) { r.push(String(e)) } r.join("|")"##,
        r##"SyntaxError: invalid JSON at byte 3: unexpected character '2'|SyntaxError: invalid JSON: unexpected end of input|SyntaxError: invalid JSON: unexpected end of input|SyntaxError: invalid JSON at byte 2: unexpected character 'b'|SyntaxError: invalid JSON at byte 3: invalid escape sequence|SyntaxError: invalid JSON at byte 2: unexpected character '1'"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('[1,2,{"a":[3,{"b":4}]}]', function(k, v) { return typeof v === "number" ? v * 2 : v }))"##,
        r##"[2,4,{"a":[6,{"b":8}]}]"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('{"a":1,"b":2,"c":{"d":3}}', function(k, v) { if (k === "b") return undefined; return v }))"##,
        r##"{"a":1,"c":{"d":3}}"##,
    ),
    (
        r##"var log=[]; JSON.parse('{"a":[1,2],"b":{"c":3}}', function(k, v) { log.push(k); return v }); log.join(",")"##,
        r##"0,1,a,c,b,"##,
    ),
    (
        r##"JSON.stringify({a:"\u2028\u2029<>&\u0001\u001f\"\\"})"##,
        r##"{"a":"  <>&\u0001\u001f\"\\"}"##,
    ),
    (
        r##"JSON.stringify([undefined, function(){}, Symbol()])"##,
        r##"[null,null,null]"##,
    ),
    (
        r##"JSON.stringify({toJSON: function(k) { return "key:" + k }})"##,
        r##""key:""##,
    ),
    (
        r##"JSON.stringify({a: {toJSON: function(k) { return "key:" + k }}})"##,
        r##"{"a":"key:a"}"##,
    ),
    (
        r##"JSON.stringify(new Number(3)) + JSON.stringify(new String("x")) + JSON.stringify(new Boolean(false))"##,
        r##"3"x"false"##,
    ),
    (
        r##"JSON.stringify({a:1,b:[1,2]}, null, "--")"##,
        r##"{
--"a": 1,
--"b": [
----1,
----2
--]
}"##,
    ),
    (
        r##"JSON.stringify({a:1,b:[1,2]}, null, new Number(3))"##,
        r##"{
   "a": 1,
   "b": [
      1,
      2
   ]
}"##,
    ),
    (
        r##"JSON.stringify({a:1,b:[1,2]}, null, new String("xy"))"##,
        r##"{
xy"a": 1,
xy"b": [
xyxy1,
xyxy2
xy]
}"##,
    ),
    (
        r##"JSON.stringify({a:1,b:2,c:3}, ["c", 1, new Number(2), "a", "c", {}, null, new String("b")])"##,
        r##"{"c":3,"a":1,"b":2}"##,
    ),
    (r##"JSON.stringify("\u00e9\u4e2d")"##, r##""é中""##),
    (r##"JSON.stringify("\u00e9\u4e2d").length"##, r##"4"##),
    (r##"JSON.parse('"\\ud800"').charCodeAt(0)"##, r##"65533"##),
    (r##"JSON.parse('"\\ud83d\\ude00"').length"##, r##"2"##),
    (r##"JSON.parse('{"__proto__": 1}').__proto__"##, r##"1"##),
    (
        r##"Object.keys(JSON.parse('{"b":1,"a":2,"1":3,"0":4}')).join()"##,
        r##"0,1,b,a"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('{"a":1,"a":2}'))"##,
        r##"{"a":2}"##,
    ),
    (
        r##"try{JSON.parse("[1,]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 4: unexpected character ']'"##,
    ),
    (
        r##"try{JSON.parse("{\"a\":1,}")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 8: unexpected character '}'"##,
    ),
    (
        r##"try{JSON.parse("tru")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("[1 2]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 4: unexpected character '2'"##,
    ),
    (
        r##"try{JSON.parse("{\"a\" 1}")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 6: unexpected character '1'"##,
    ),
    (
        r##"try{JSON.parse("\"abc")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("1 \"abc")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 3: unexpected character '"'"##,
    ),
    (
        r##"try{JSON.parse("-")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("1.")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("[\u0001]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 2: unexpected character '\^A'"##,
    ),
    (
        r##"try{JSON.parse("\u00e9")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character 'é'"##,
    ),
    (
        r##"try{JSON.parse("1 \u00e9")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 3: unexpected character 'é'"##,
    ),
    (
        r##"try{JSON.parse(" ")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("{")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("{\"a\":")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("[")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: unexpected end of input"##,
    ),
    (
        r##"try{JSON.parse("}")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character '}'"##,
    ),
    (
        r##"try{JSON.parse("]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character ']'"##,
    ),
    (
        r##"try{JSON.parse("[}")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 2: unexpected character '}'"##,
    ),
    (
        r##"try{JSON.parse("{]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 2: unexpected character ']'"##,
    ),
    (
        r##"try{JSON.parse("{1:2}")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 2: unexpected character '1'"##,
    ),
    (
        r##"try{JSON.parse('"\\u12"')}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON at byte 6: invalid escape sequence"##,
    ),
    (
        r##"try{JSON.parse("1e400")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: 1e400 is out of range"##,
    ),
    (
        r##"try{JSON.parse("[1e400]")}catch(e){String(e)}"##,
        r##"SyntaxError: invalid JSON: 1e400 is out of range"##,
    ),
    (
        r##"JSON.parse("-0") === 0 && 1/JSON.parse("-0")"##,
        r##"-Infinity"##,
    ),
    (r##"JSON.parse(" [1, 2 ] ").length"##, r##"2"##),
    (r##"JSON.parse("1e-400")"##, r##"0"##),
    (
        r##"JSON.stringify(JSON.parse("[[[[[]]]]]"))"##,
        r##"[[[[[]]]]]"##,
    ),
    (
        r##"var n=100000; var s="[".repeat(n)+"]".repeat(n); var a=JSON.parse(s); var d=0; while(Array.isArray(a)&&a.length){a=a[0];d++} d"##,
        r##"99999"##,
    ),
    (
        r##"var n=3000; var o=[]; var c=o; for(var i=0;i<n;i++){var x=[]; c.push(x); c=x} JSON.stringify(o).length"##,
        r##"6002"##,
    ),
    (
        r##"var n=3000; var o={}; var c=o; for(var i=0;i<n;i++){var x={}; c.a=x; c=x} JSON.stringify(o, null, 1).length"##,
        r##"9027002"##,
    ),
    (
        r##"var n=20000; var s="[".repeat(n)+"1"+"]".repeat(n); JSON.stringify(JSON.parse(s, function(k,v){return v})).length"##,
        r##"40001"##,
    ),
    (r##"JSON.stringify(1n === 1n ? {} : 0)"##, r##"{}"##),
    (
        r##"try { JSON.stringify(1n) } catch(e) { String(e) }"##,
        r##"TypeError: Do not know how to serialize a BigInt"##,
    ),
    (
        r##"BigInt.prototype.toJSON = function() { return this.toString() }; JSON.stringify([1n])"##,
        r##"["1"]"##,
    ),
    (
        r##"JSON.stringify({a:[]}, null, 2)"##,
        r##"{
  "a": []
}"##,
    ),
    (
        r##"JSON.stringify([[], {}], null, 2)"##,
        r##"[
  [],
  {}
]"##,
    ),
    (
        r##"JSON.stringify([{}, []], null, 2)"##,
        r##"[
  {},
  []
]"##,
    ),
    (
        r##"JSON.stringify(Object.defineProperty({}, "x", {get: function(){ return 5 }, enumerable: true}))"##,
        r##"{"x":5}"##,
    ),
    (
        r##"JSON.stringify(new Proxy({a:1, b:2}, {}))"##,
        r##"{"a":1,"b":2}"##,
    ),
    (r##"JSON.stringify(new Proxy([1,2], {}))"##, r##"[1,2]"##),
    (
        r##"var p = Proxy.revocable({}, {}); p.revoke(); try { JSON.stringify({}, [p.proxy]) } catch(e) { String(e) }"##,
        r##"TypeError: proxy has been revoked"##,
    ),
    (
        r##"var p = Proxy.revocable({}, {}); p.revoke(); try { JSON.stringify(p.proxy) } catch(e) { String(e) }"##,
        r##"TypeError: Proxy already revoked"##,
    ),
    (r##"JSON.stringify("abc", null, 2)"##, r##""abc""##),
    (
        r##"JSON.stringify({}, function(k, v) { return k === "" ? [1, 2] : v * 10 })"##,
        r##"[10,20]"##,
    ),
    (r##"String(JSON)"##, r##"[object JSON]"##),
    (
        r##"Object.prototype.toString.call(JSON)"##,
        r##"[object JSON]"##,
    ),
    (
        r##"JSON.parse.length + "," + JSON.stringify.length + "," + JSON.parse.name"##,
        r##"2,3,parse"##,
    ),
    (
        r##"JSON.parse({toString: function() { return "[7]" }})[0]"##,
        r##"7"##,
    ),
    (
        r##"try { JSON.parse(Symbol()) } catch(e) { String(e) }"##,
        r##"TypeError: Cannot convert a Symbol value to a string"##,
    ),
    (r##"JSON.parse("1", 5)"##, r##"1"##),
    (r##"JSON.stringify({a: 1}, null, -5)"##, r##"{"a":1}"##),
    (
        r##"JSON.stringify({a: 1}, null, 2.9)"##,
        r##"{
  "a": 1
}"##,
    ),
    (r##"JSON.stringify({a: 1}, null, "")"##, r##"{"a":1}"##),
    (
        r##"JSON.stringify({a: 1}, null, 20)"##,
        r##"{
          "a": 1
}"##,
    ),
    (r##"JSON.stringify({"\ud800": 1})"##, r##"{"\ud800":1}"##),
    (
        r##"var o = JSON.parse('{"é":1,"a中":2,"😀":3}'); Object.keys(o).map(function(k){ return k.length + ":" + escape(k) }).join() + "|" + o["é"] + o["a中"] + o["😀"]"##,
        r##"1:%E9,2:a%u4E2D,2:%uD83D%uDE00|123"##,
    ),
    (
        r##"var o = JSON.parse('{"\\u00e9":1}'); Object.keys(o)[0] === "é""##,
        r##"true"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('{"é":{"xé":[1]}}'))"##,
        r##"{"é":{"xé":[1]}}"##,
    ),
    (
        r##"var o = {}; o["é"] = 1; JSON.stringify(o) === JSON.stringify(JSON.parse('{"é":1}'))"##,
        r##"true"##,
    ),
    (
        r##"JSON.stringify({"\ud800": 1, "a\udc00b": 2})"##,
        r##"{"\ud800":1,"a\udc00b":2}"##,
    ),
    (
        r##"var o = JSON.parse('{"0":1,"b":2,"é":3,"1":4}'); Object.keys(o).join()"##,
        r##"0,1,b,é"##,
    ),
    (
        r##"JSON.stringify({a: [], b: {}, c: [[]], d: [{}]}, null, 3)"##,
        r##"{
   "a": [],
   "b": {},
   "c": [
            []
         ],
   "d": [
            {}
         ]
}"##,
    ),
    (
        r##"JSON.stringify([1, [2, [3, [4]]], {x: {y: {z: []}}}], null, "\t")"##,
        r##"[
	1,
	[
		2,
		[
			3,
			[
				4
			]
		]
	],
	{
		"x": {
			"y": {
				"z": []
			}
		}
	}
]"##,
    ),
    (
        r##"JSON.stringify([new Number(1), new String("s"), new Boolean(true), Object(2n) === 0], function(k, v) { return v })"##,
        r##"[1,"s",true,false]"##,
    ),
    (
        r##"JSON.stringify({a: 1, b: 2}, function(k, v) { return k === "a" ? undefined : v })"##,
        r##"{"b":2}"##,
    ),
    (
        r##"JSON.stringify({a: {b: {c: 1}}}, function(k, v) { return k === "b" ? [k, typeof v] : v })"##,
        r##"{"a":{"b":["b","object"]}}"##,
    ),
    (
        r##"var calls = []; JSON.stringify({a: 1, b: [1, {c: 2}]}, function(k, v) { calls.push(k + "@" + (Array.isArray(this) ? "arr" : typeof this)); return v }); calls.join()"##,
        r##"@object,a@object,b@object,0@arr,1@arr,c@object"##,
    ),
    (
        r##"JSON.stringify({x: {toJSON: function(k) { return {y: k} }}})"##,
        r##"{"x":{"y":"x"}}"##,
    ),
    (r##"JSON.stringify([1, 2], ["0"])"##, r##"[1,2]"##),
    (
        r##"JSON.stringify({1: "a", 2: "b", x: "c"}, [2, "x", 1, 2])"##,
        r##"{"2":"b","x":"c","1":"a"}"##,
    ),
    (
        r##"JSON.stringify(Object.create({inherited: 1}, {own: {value: 2, enumerable: true}, hidden: {value: 3}}))"##,
        r##"{"own":2}"##,
    ),
    (
        r##"JSON.stringify({u: undefined, f: function(){}, s: Symbol(), n: null, d: new Date(NaN)})"##,
        r##"{"n":null,"d":null}"##,
    ),
    (
        r##"JSON.stringify("\u0000\u0007\u001f\u007f\u0080 ")"##,
        r##""\u0000\u0007\u001f ""##,
    ),
    (
        r##"JSON.stringify([-0, 0.1, 1e-7, 1e21, 123456789012345680000, -1e-7, 5e-324])"##,
        r##"[0,0.1,1e-7,1e+21,123456789012345680000,-1e-7,5e-324]"##,
    ),
    (
        r##"JSON.stringify([2**53, 2**53 + 2, -(2**31), 4294967295])"##,
        r##"[9007199254740992,9007199254740994,-2147483648,4294967295]"##,
    ),
    (
        r##"var a = [1, 2, 3]; a.length = 5; JSON.stringify(a)"##,
        r##"[1,2,3,null,null]"##,
    ),
    (
        r##"var a = []; a[3] = 1; JSON.stringify(a)"##,
        r##"[null,null,null,1]"##,
    ),
    (
        r##"var o = {}; Object.defineProperty(o, "x", {get: function() { delete this.y; return 1 }, enumerable: true}); o.y = 2; JSON.stringify(o)"##,
        r##"{"x":1}"##,
    ),
    (
        r##"var o = {a: 1}; JSON.stringify(o, function(k, v) { if (k === "a") { o.b = 2 } return v })"##,
        r##"{"a":1}"##,
    ),
    (
        r##"JSON.stringify(new Proxy({a: 1}, {ownKeys: function(){ return ["a", "z"] }, getOwnPropertyDescriptor: function(t, k) { return {value: 1, enumerable: true, configurable: true} }}))"##,
        r##"{"a":1}"##,
    ),
    (
        r##"JSON.parse('[1, 2, {"a": [3, {"b": null}]}]', function(k, v) { return Array.isArray(v) ? v.length : v })"##,
        r##"3"##,
    ),
    (
        r##"JSON.parse('{"a": 1, "b": 2}', function(k, v) { if (k === "a") { this.c = 3 } return v }).c"##,
        r##"3"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('{"a": [1, 2]}', function(k, v) { if (k === "0") { return undefined } return v }))"##,
        r##"{"a":[null,2]}"##,
    ),
    (
        r##"JSON.parse("[]", function(k, v) { return typeof this + ":" + k })"##,
        r##"object:"##,
    ),
    (
        r##"try { JSON.parse('{"a":1}', function(k, v) { throw new Error("rev") }) } catch(e) { String(e) }"##,
        r##"Error: rev"##,
    ),
    (r##"JSON.parse(" \t\n\r1 \t\n\r")"##, r##"1"##),
    (
        r##"JSON.parse('"\\/\\b\\f\\n\\r\\t\\"\\\\"').length"##,
        r##"8"##,
    ),
    (
        r##"JSON.parse(1) + JSON.parse(true) + JSON.parse(null)"##,
        r##"2"##,
    ),
    (
        r##"try { JSON.parse(undefined) } catch(e) { String(e) }"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character 'u'"##,
    ),
    (
        r##"try { JSON.parse("NaN") } catch(e) { String(e) }"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character 'N'"##,
    ),
    (
        r##"try { JSON.parse("'a'") } catch(e) { String(e) }"##,
        r##"SyntaxError: invalid JSON at byte 1: unexpected character '''"##,
    ),
    (
        r##"try { JSON.parse("[1]x") } catch(e) { String(e) }"##,
        r##"SyntaxError: invalid JSON at byte 4: unexpected character 'x'"##,
    ),
    (
        r##"try { JSON.parse("{\"a\":1}}") } catch(e) { String(e) }"##,
        r##"SyntaxError: invalid JSON at byte 8: unexpected character '}'"##,
    ),
    (
        r##"JSON.stringify(JSON.parse('1E2')) + JSON.stringify(JSON.parse('-0.0')) + JSON.stringify(JSON.parse('1e-2'))"##,
        r##"10000.01"##,
    ),
    (
        r##"escape(JSON.stringify([1], null, "abcdefghijk"))"##,
        r##"%5B%0Aabcdefghij1%0A%5D"##,
    ),
    (
        r##"escape(JSON.stringify([1], null, "abcdefghij"))"##,
        r##"%5B%0Aabcdefghij1%0A%5D"##,
    ),
    (
        r##"escape(JSON.stringify("\b\f\n\r\t\v\u000e\u001a"))"##,
        r##"%22%5Cb%5Cf%5Cn%5Cr%5Ct%5Cu000b%5Cu000e%5Cu001a%22"##,
    ),
    (
        r##"escape(JSON.stringify({"\b": "\\\""}))"##,
        r##"%7B%22%5Cb%22%3A%22%5C%5C%5C%22%22%7D"##,
    ),
    (
        r##"JSON.stringify([1, [2]], null, 10.9)"##,
        r##"[
          1,
          [
                    2
          ]
]"##,
    ),
    (
        r##"escape(JSON.stringify([1], null, 11))"##,
        r##"%5B%0A%20%20%20%20%20%20%20%20%20%201%0A%5D"##,
    ),
];

fn check_cases(cases: Vec<(&'static str, &'static str)>) {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for (script, want) in cases {
            let mut rt = new_runtime();
            let got = match rt.run_string(script) {
                Ok(v) => v.go_string(&mut rt).expect("go_string"),
                Err(e) => format!("!{}", e.error()),
            };
            if got != want {
                failures.push(format!("{script}\n  got:  {got}\n  want: {want}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn json_builtin_matches_the_baseline() {
    assert!(JSON_CASES.len() > 100);
    check_cases(JSON_CASES.to_vec());
}

#[test]
fn json_stringify_space_follows_the_go_float_conversion() {
    let want = if cfg!(target_arch = "x86_64") {
        "%7B%22a%22%3A1%7D"
    } else {
        "%7B%0A%20%20%20%20%20%20%20%20%20%20%22a%22%3A%201%0A%7D"
    };
    check_cases(vec![(
        "escape(JSON.stringify({a:1}, null, Infinity))",
        want,
    )]);
}

#[test]
fn test_json_marshal_object() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let o = vm.new_object();
        o.set_own_str(&mut vm, &"test".into(), int(42), true)
            .expect("set");
        let e = vm.get("Error").expect("get").expect("Error");
        o.set_own_str(&mut vm, &"testfunc".into(), e, true)
            .expect("set");
        let b = vm.object_marshal_json(o).expect("marshal");
        let s = String::from_utf8(b).expect("utf8");
        assert_eq!(s, r#"{"test":42}"#, "Unexpected value: {s}");
    });
}

#[test]
fn test_json_marshal_go_date() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let o = vm.new_object();
        let t = vm
            .to_value(&DocValue::Time(Time::from_unix(86400, 0).utc()))
            .expect("to_value");
        o.set_own_str(&mut vm, &"test".into(), t, true)
            .expect("set");
        let b = vm.object_marshal_json(o).expect("marshal");
        let s = String::from_utf8(b).expect("utf8");
        assert_eq!(
            s, r#"{"test":"1970-01-02T00:00:00Z"}"#,
            "Unexpected value: {s}"
        );
    });
}

#[test]
fn test_json_marshal_object_circular() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let o = vm.new_object();
        o.set_own_str(&mut vm, &"o".into(), Value::Object(o), true)
            .expect("set");
        let err = match vm.object_marshal_json(o) {
            Ok(_) => panic!("Expected error"),
            Err(e) => e.error(),
        };
        assert!(
            err.ends_with("Converting circular structure to JSON"),
            "Unexpected error: {err}"
        );
    });
}
