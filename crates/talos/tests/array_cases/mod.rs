pub const PRELUDE: &str = r#"function J(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var t = typeof v;
  if (t === "number") return (v === 0 && 1 / v < 0) ? "0" : String(v);
  if (t === "boolean") return String(v);
  if (t === "string") return '"' + v + '"';
  if (t === "function") return "fn";
  if (Array.isArray(v)) {
    var out = "[";
    for (var i = 0; i < v.length; i++) {
      if (i > 0) out += ",";
      if (!(i in v)) { out += "_"; continue; }
      var x = v[i];
      out += (x === undefined || typeof x === "function") ? "null" : J(x);
    }
    return out + "]";
  }
  var keys = Object.keys(v);
  var parts = [];
  for (var k = 0; k < keys.length; k++) {
    var val = v[keys[k]];
    if (val === undefined || typeof val === "function") continue;
    parts.push('"' + keys[k] + '":' + J(val));
  }
  return "{" + parts.join(",") + "}";
}
"#;

pub const BEHAVIOUR: &[(&str, &str)] = &[
    (
        r#"var log = [];
var a = [];
var seed = 7;
for (var i = 0; i < 57; i++) { seed = (seed * 1103515245 + 12345) % 2147483648; a.push(seed % 13); }
a.sort(function (x, y) { log.push(x + ":" + y); return x - y; });
a.join(",") + "|" + log.length + "|" + log.join(" ")"#,
        r#"0,0,0,0,0,1,1,1,1,1,1,2,2,2,3,3,3,3,3,3,4,4,4,4,5,5,5,5,5,5,5,6,6,6,6,7,7,7,8,8,8,8,8,8,8,8,8,9,9,9,10,10,11,12,12,12,12|415|8:8 8:8 1:8 1:8 1:8 5:8 5:8 5:8 5:1 12:8 12:12 3:12 3:12 3:8 3:8 3:8 3:5 3:1 3:12 3:12 3:8 3:8 3:8 3:5 3:3 1:12 1:12 1:8 1:8 1:8 1:5 1:3 1:3 1:1 0:12 0:12 0:8 0:8 0:8 0:5 0:3 0:3 0:1 0:1 5:12 5:12 5:8 5:8 5:8 5:5 10:12 10:12 10:8 6:12 6:12 6:10 6:8 6:8 6:8 6:5 4:12 4:12 4:10 4:8 4:8 4:8 4:6 4:5 4:5 4:3 4:12 4:12 4:10 4:8 4:8 4:8 4:6 4:5 4:5 4:4 3:12 3:12 3:10 3:8 3:8 3:8 3:6 3:5 3:5 3:4 3:4 3:3 3:12 3:12 3:10 3:8 3:8 3:8 3:6 3:5 3:5 3:4 3:4 3:3 5:12 5:12 5:10 5:8 5:8 5:8 5:6 5:5 0:12 0:12 0:10 0:8 0:8 0:8 0:6 0:5 0:5 0:5 0:4 0:4 0:3 0:3 0:3 0:3 0:1 0:1 0:0 11:0 8:11 8:0 4:11 4:8 4:0 1:11 1:8 1:4 1:0 5:11 5:8 5:4 9:11 9:8 9:11 9:9 7:11 7:9 7:9 7:8 7:5 8:11 8:9 8:9 8:8 2:11 2:9 2:9 2:8 2:8 2:7 2:5 2:4 2:1 1:11 1:9 1:9 1:8 1:8 1:7 1:5 1:4 1:2 1:1 8:11 8:9 8:9 8:8 5:11 5:9 5:9 5:8 5:8 5:8 5:7 5:5 2:11 2:9 2:9 2:8 2:8 2:8 2:7 2:5 2:5 2:4 2:2 1:11 1:9 1:9 1:8 1:8 1:8 1:7 1:5 1:5 1:4 1:2 1:2 1:1 3:11 3:9 3:9 3:8 3:8 3:8 3:7 3:5 3:5 3:4 3:2 0:11 0:9 0:9 0:8 0:8 0:8 0:7 0:5 0:5 0:4 0:3 0:2 0:2 0:1 0:1 0:1 0:0 6:11 6:9 6:9 6:8 6:8 6:8 6:7 6:5 8:11 8:9 8:9 8:8 3:6 1:6 1:3 10:6 5:10 5:6 5:3 7:10 7:6 9:10 9:7 7:10 7:9 7:7 4:10 4:9 4:7 4:7 4:6 4:5 4:3 0:10 0:9 0:7 0:7 0:6 0:5 0:4 0:3 0:1 12:10 8:12 8:10 8:9 8:7 5:12 5:10 5:9 5:8 5:7 5:7 5:6 5:5 6:12 6:10 6:9 6:8 6:7 6:7 6:6 8:12 8:10 8:9 8:8 2:12 2:10 2:9 2:8 2:8 2:7 2:7 2:6 2:6 2:5 2:5 2:4 2:3 2:1 12:12 5:5 1:8 2:6 3:5 4:5 1:3 2:1 2:3 1:1 0:1 0:0 0:1 1:1 1:1 1:1 3:3 2:4 2:3 2:3 2:3 3:4 4:4 3:4 4:5 4:4 8:8 6:12 7:10 8:8 5:8 6:6 5:5 5:6 6:8 7:8 8:12 9:10 9:12 11:10 9:10 11:12 11:12 6:5 3:6 5:5 4:5 2:3 3:2 1:1 0:1 0:0 0:1 0:1 1:1 1:2 2:2 1:2 2:2 3:4 4:4 3:3 3:4 3:3 4:5 4:4 4:5 7:8 9:8 8:8 6:8 7:6 6:7 5:6 6:5 5:5 5:5 6:6 7:8 7:8 7:7 7:7 9:9 8:11 8:10 8:8 8:9 8:9 10:12 12:11 9:11 10:10 9:10 10:11 12:12"#,
    ),
    (
        r#"var log = [];
var a = [5, 1, 4, 2, 3, 9, 0, 8, 7, 6];
a.sort(function (x, y) { log.push(x + "" + y); return x > y; });
a.join(",") + "|" + log.join(" ")"#,
        r#"5,1,4,2,3,9,0,8,7,6|15 41 24 32 93 09 80 78 67"#,
    ),
    (
        r#"var a = [3, undefined, 1, , 2, undefined, , 0];
a.sort();
J(a) + "|" + a.length + "|" + (5 in a) + "|" + (6 in a) + "|" + (7 in a)"#,
        r#"[0,1,2,3,null,null,_,_]|8|true|false|false"#,
    ),
    (
        r#"var a = [10, 9, 1, 100, 25, "b", "a", true, null, {}, 3.5, -1];
a.sort();
a.map(String).join(",")"#,
        r#"-1,1,10,100,25,3.5,9,[object Object],a,b,null,true"#,
    ),
    (
        r#"var a = [3, 1, 2];
a.sort(function () { return -0; });
a.join(",")"#,
        r#"2,1,3"#,
    ),
    (
        r#"var a = [3, 1, 2];
a.sort(function () { return NaN; });
a.join(",")"#,
        r#"3,1,2"#,
    ),
    (
        r#"var a = [];
a[20470] = 5;
var k = Object.keys(a);
k.length + "|" + a.length + "|" + a[20470]"#,
        r#"1|20471|5"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.length = 1;
a.length = 5;
J(a) + "|" + a.length + "|" + Object.keys(a).join(",")"#,
        r#"[1,_,_,_,_]|5|0"#,
    ),
    (
        r#"var a = [1, 2, 3, 4];
Object.defineProperty(a, "length", { writable: false });
var r = [];
try { a.push(5); } catch (e) { r.push(e.message); }
try { a.pop(); } catch (e) { r.push(e.message); }
try { "use strict"; a.length = 0; } catch (e) { r.push(e.message); }
r.push(a.length, J(a));
r.join("|")"#,
        r#"length is not writable|length is not writable|4|[1,2,3,_]"#,
    ),
    (
        r#""use strict";
var a = [1, 2, 3];
Object.defineProperty(a, "1", { value: 9, configurable: false });
var r = [];
try { a.length = 0; } catch (e) { r.push(e.name + ":" + e.message); }
r.push(a.length);
try { delete a[1]; } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"2"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.freeze(a);
var r = [Object.isFrozen(a), Object.isSealed(a), Object.isExtensible(a)];
var d = Object.getOwnPropertyDescriptor(a, "length");
r.push(d.writable, d.configurable, d.enumerable, d.value);
try { (function () { "use strict"; a.push(1); })(); } catch (e) { r.push(e.message); }
r.join("|")"#,
        r#"true|true|false|false|false|false|3|Cannot add property 3, object is not extensible"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.seal(a);
var d = Object.getOwnPropertyDescriptor(a, "0");
[Object.isSealed(a), Object.isFrozen(a), d.writable, d.configurable].join("|")"#,
        r#"true|false|true|false"#,
    ),
    (
        r#"var a = [1, [2, [3, [4]]], , 5];
[J(a.flat()), J(a.flat(Infinity)), J(a.flat(undefined)), J(a.flat(0))].join("|")"#,
        r#"[1,2,[3,[4]],5]|[1,2,3,4,5]|[1,[2,[3,[4]]],5]|[1,[2,[3,[4]]],5]"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5];
var r = [];
r.push(J(a.splice(1, 2)), J(a));
r.push(J(a.splice(1, 0, "x", "y", "z")), J(a));
r.push(J(a.splice(-2)), J(a));
r.push(J(a.splice()), J(a));
r.join("|")"#,
        r#"[2,3]|[1,4,5]|[]|[1,"x","y","z",4,5]|[4,5]|[1,"x","y","z"]|[]|[1,"x","y","z"]"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = [a.unshift(0, -1), J(a), a.shift(), J(a), a.pop(), J(a)];
r.join("|")"#,
        r#"5|[0,-1,1,2,3]|0|[-1,1,2,3]|3|[-1,1,2]"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5];
[J(a.copyWithin(0, 3)), J([1, 2, 3, 4, 5].copyWithin(1, 0, 3)), J([1, 2, 3, 4, 5].fill(0, 1, -1))].join("|")"#,
        r#"[4,5,3,4,5]|[1,1,2,3,5]|[1,0,0,0,5]"#,
    ),
    (
        r#"var a = [1, 2, NaN, 2, -0, undefined];
[a.indexOf(2), a.lastIndexOf(2), a.indexOf(NaN), a.includes(NaN), a.includes(0), a.indexOf(0), a.includes(undefined), a.indexOf(undefined), a.indexOf(2, -2), a.lastIndexOf(2, -4)].join(",")"#,
        r#"1,3,-1,true,false,4,true,5,-1,1"#,
    ),
    (
        r#"var a = [1, , 3];
[a.indexOf(undefined), a.includes(undefined), a.find(function (x) { return x === undefined; }), a.findIndex(function (x) { return x === undefined; }), a.findLast(function (x) { return x === undefined; }), a.findLastIndex(function (x) { return x === undefined; })].join(",")"#,
        r#"-1,true,,1,,1"#,
    ),
    (
        r#"var calls = [];
[1, , 3].forEach(function (v, i, o) { calls.push(v + "@" + i + "@" + (o.length)); });
var m = [1, , 3].map(function (v) { return v * 2; });
calls.join(",") + "|" + J(m) + "|" + (1 in m) + "|" + m.length"#,
        r#"1@0@3,3@2@3|[2,_,6]|false|3"#,
    ),
    (
        r#"var r = [];
r.push([1, 2, 3].reduce(function (a, b) { return a + b; }));
r.push([1, 2, 3].reduceRight(function (a, b) { return a + "" + b; }));
try { [].reduce(function () {}); } catch (e) { r.push(e.name + ":" + e.message); }
try { [, ,].reduceRight(function () {}); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].reduce(5); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].reduce(); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].filter({}); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].map(5); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].forEach(); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"6|321|TypeError:No initial value|TypeError:No initial value|TypeError:5 is not a function|TypeError:Cannot convert undefined or null to object|TypeError:[object Object] is not a function|TypeError:Value is not an object: 5|TypeError:Value is not an object: undefined"#,
    ),
    (
        r#"var r = [];
try { [1, 2].with(5, 1); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1, 2].with(-3.5, 1); } catch (e) { r.push(e.name + ":" + e.message); }
r.push(J([1, 2, 3].with(-1, 9)), J([1, , 3].with(0, 0)));
r.push(J([3, 1, 2].toSorted()), J([1, 2, 3].toReversed()), J([1, 2, 3].toSpliced(1, 1, "a", "b")));
try { [1].toSorted(1); } catch (e) { r.push(e.name + ":" + e.message); }
try { [1].sort(null); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"RangeError:Invalid index 5|RangeError:Invalid index -3.5|[1,2,9]|[0,null,3]|[1,2,3]|[3,2,1]|[1,"a","b",3]|TypeError:The comparison function must be either a function or undefined|TypeError:The comparison function must be either a function or undefined"#,
    ),
    (
        r#"var r = [];
try { new Array(-1); } catch (e) { r.push(e.name + ":" + e.message); }
try { new Array(1.5); } catch (e) { r.push(e.name + ":" + e.message); }
try { new Array(4294967296); } catch (e) { r.push(e.name + ":" + e.message); }
try { [].length = -1; } catch (e) { r.push(e.name + ":" + e.message); }
try { [].length = "x"; } catch (e) { r.push(e.name + ":" + e.message); }
r.push(new Array(3).length, new Array("3").length, new Array(1, 2).length, Array(2).length, new Array(-0).length);
r.join("|")"#,
        r#"RangeError:Invalid array length|RangeError:Invalid array length|RangeError:Invalid array length|RangeError:Invalid array length|RangeError:Invalid array length|3|1|2|2|0"#,
    ),
    (
        r#"class A extends Array {}
var a = new A(3);
var b = new A(-0);
var c = a.map(function (x) { return x; });
[a instanceof A, a.length, b instanceof A, c instanceof A, A.from([1, 2]) instanceof A, A.of(1) instanceof A, a.slice() instanceof A, a.concat() instanceof A].join(",")"#,
        r#"true,3,false,true,true,true,true,true"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.constructor = {};
a.constructor[Symbol.species] = function (n) { this.n = n; };
var r = a.map(function (x) { return x * 2; });
var s = a.filter(function () { return true; });
J(r) + "|" + J(s) + "|" + r.n + "|" + s.n"#,
        r#"{"0":2,"1":4,"2":6,"n":3}|{"0":1,"1":2,"2":3,"n":0}|3|0"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.constructor = 5;
try { a.map(function () {}); } catch (e) { e.name + ":" + e.message; }"#,
        r#"TypeError:Species is not a constructor"#,
    ),
    (
        r#"var r = [];
var it = [1, 2][Symbol.iterator]();
r.push(Object.prototype.toString.call(it), it.next().value, it.next().value, it.next().done, it.next().done);
var e = ["a", "b"].entries();
r.push(J(e.next()), J(e.next()), J(e.next()));
var k = ["a", "b"].keys();
r.push(k.next().value, k.next().value);
try { it.next.call({}); } catch (x) { r.push(x.name + ":" + x.message); }
r.push(Array.prototype.values === Array.prototype[Symbol.iterator]);
r.join("|")"#,
        r#"[object Array Iterator]|1|2|true|true|{"value":[0,"a"],"done":false}|{"value":[1,"b"],"done":false}|{"done":true}|0|1|TypeError:Method Array Iterator.prototype.next called on incompatible receiver [object Object]|true"#,
    ),
    (
        r#"var r = [];
r.push(J(Array.from("abc")), J(Array.from({ length: 3 }, function (x, i) { return [x, i]; })));
r.push(J(Array.from((function* () { yield 1; yield 2; })())), J(Array.of(7, 8)), Array.isArray([]), Array.isArray({ length: 0 }));
try { Array.from([1], 5); } catch (e) { r.push(e.name + ":" + e.message); }
try { Array.from(null); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"["a","b","c"]|[[null,0],[null,1],[null,2]]|[1,2]|[7,8]|true|false|TypeError:5 is not a function|TypeError:Cannot convert undefined or null to object"#,
    ),
    (
        r#"var r = [];
r.push([1, [2, [3]], null, undefined, "x"].join(), [1, 2].join(undefined), [1, 2].join(null), [].join(), [, ,].join("-"));
r.push(String([1, [2, 3]]), [1.5, -0, 1e21, 1e-7].join(";"), [{ toString: function () { return "o"; } }].join());
r.push(["s", "a", { toLocaleString: function () { return "L"; } }].toLocaleString());
try { [{ toLocaleString: 5 }].toLocaleString(); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"1,2,3,,,x|1,2|1null2||-|1,2,3|1.5;0;1e+21;1e-7|o|s,a,L|TypeError:Property 'toLocaleString' of object [object Object] is not a function"#,
    ),
    (
        r#"var r = [];
r.push(Array.prototype.toString.call({ join: function () { return "J"; } }), Array.prototype.toString.call({}), Array.prototype.toString.call(5));
r.push(Array.prototype.join.call("abc", "-"), Array.prototype.map.call("abc", function (c) { return c + c; }).join());
r.join("|")"#,
        r#"J|[object Object]|[object Number]|a-b-c|aa,bb,cc"#,
    ),
    (
        r#"var a = [1, 2, 3];
var s = [];
a.concat = Array.prototype.concat;
var b = a.concat([4, 5], 6, [[7]]);
var o = { length: 2, 0: "a", 1: "b" };
o[Symbol.isConcatSpreadable] = true;
var c = [].concat(o, "s");
J(b) + "|" + J(c)"#,
        r#"[1,2,3,4,5,6,[7]]|["a","b","s"]"#,
    ),
    (
        r#"var a = [];
a[5] = 1;
a[100000] = 2;
var r = [a.length, Object.keys(a).join(",")];
a.length = 6;
r.push(a.length, Object.keys(a).join(","), a[5]);
a.length = 0;
r.push(a.length, Object.keys(a).length);
r.join("|")"#,
        r#"100001|5,100000|6|5|1|0|0"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "50000", { value: 2, configurable: false, enumerable: true });
var r = [];
try { a.length = 50000; } catch (e) { r.push(e.message); }
r.push(a.length);
a.length = 50001;
r.push(a.length, a[50000]);
r.join("|")"#,
        r#"50000|50001|"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "50000", { value: 2, configurable: false, enumerable: true });
var r = [];
try { a.length = 50001; } catch (e) { r.push(e.message); }
r.push(a.length, a[50000]);
try { Object.defineProperty(a, "length", { value: 0 }); } catch (e) { r.push(e.message); }
r.push(a.length, a[50000]);
r.join("|")"#,
        r#"50001|2|Cannot redefine property: length|50001|2"#,
    ),
    (
        r#"var a = [1, 2, 3];
var keys = [];
for (var k in a) { keys.push(k); }
a.x = 1;
a[10] = 5;
for (var k in a) { keys.push(k); }
keys.join(",") + "|" + Object.getOwnPropertyNames(a).join(",")"#,
        r#"0,1,2,0,1,2,10,x|0,1,2,10,length,x"#,
    ),
    (
        r#"var a = [];
a[200000] = 1;
a.x = 2;
a[3] = 4;
Object.getOwnPropertyNames(a).join(",") + "|" + (Object.getOwnPropertyNames(a).length + Object.getOwnPropertySymbols(a).length)"#,
        r#"3,200000,length,x|4"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 1, { get: function () { return "g"; }, enumerable: true, configurable: true });
var r = [a[1], J(a), a.join(), a.indexOf("g"), a.slice().join(), a.length];
a.sort();
r.push(a.join());
r.join("|")"#,
        r#"g|[1,"g",3]|1,g,3|1|1,g,3|3|1,g,g"#,
    ),
    (
        r#"var a = [5, 4, 3, 2, 1];
var seen = [];
a.sort(function (x, y) { seen.push(a.join("")); return x - y; });
seen.join(" ")"#,
        r#"54321 45321 43521 34521 34251 32451 23451 23415 23145 21345"#,
    ),
    (
        r#"var a = [5, 4, 3, 2, 1];
a.pop();
a.push(1);
var seen = [];
a.sort(function (x, y) { seen.push(a.join("")); return x - y; });
seen.join(" ") + "|" + a.join("")"#,
        r#"54321 54321 54321 54321 54321 54321 54321 54321 54321 54321|12345"#,
    ),
    (
        r#"var a = [3, 1, 2];
var o = { 0: 3, 1: 1, 2: 2, length: 3 };
Array.prototype.sort.call(o);
var u = { 0: "b", 2: "a", 5: undefined, length: 7 };
Array.prototype.sort.call(u);
J(o) + "|" + J(u)"#,
        r#"{"0":1,"1":2,"2":3,"length":3}|{"0":"a","1":"b","length":7}"#,
    ),
    (
        r#"Array.prototype[3] = "p";
var a = [0, 1, 2, , 4];
var r = [a[3], a.join(), a.indexOf("p"), a.includes("p"), 3 in a, a.hasOwnProperty(3)];
delete Array.prototype[3];
r.join("|")"#,
        r#"p|0,1,2,p,4|3|true|true|false"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25];
var log = [];
a.sort(function (x, y) { log.push(x - y); return y - x; });
a.join(",") + "|" + log.length"#,
        r#"25,24,23,22,21,20,19,18,17,16,15,14,13,12,11,10,9,8,7,6,5,4,3,2,1|208"#,
    ),
    (
        r#"var log = [];
var a = [];
for (var i = 0; i < 100; i++) a.push((i * 37) % 101);
a.sort(function (x, y) { log.push(x + "-" + y); return (x % 7) - (y % 7); });
a.join(",") + "|" + log.length + "|" + log.slice(0, 40).join(" ") + "|" + log.slice(-40).join(" ")"#,
        r#"0,84,77,70,63,56,49,42,35,28,21,14,98,7,91,57,50,43,36,29,22,15,99,8,92,1,85,78,71,37,30,23,16,100,9,93,2,86,79,72,65,58,51,44,10,94,3,87,80,73,66,59,52,45,38,31,24,17,74,67,60,53,46,39,32,25,18,11,95,4,88,81,47,40,33,26,19,12,96,5,89,82,75,68,61,54,20,13,97,6,90,83,76,69,62,55,48,41,34,27|712|37-0 74-37 10-74 10-37 47-74 84-47 84-74 84-10 84-37 84-0 20-47 57-20 57-47 57-74 57-10 57-37 57-84 94-20 94-47 94-74 94-10 30-20 30-47 30-74 30-94 30-10 30-37 67-20 67-47 67-74 3-20 3-47 3-67 3-74 3-94 40-20 40-47 77-20 77-40 77-47|51-58 44-87 44-94 44-10 81-26 54-47 68-33 17-46 88-60 4-53 24-38 31-74 31-52 31-38 24-60 17-67 17-74 88-18 4-11 88-47 81-95 88-95 4-95 81-40 81-47 54-13 34-75 41-20 61-5 68-89 61-75 68-20 68-75 54-20 34-83 41-76 34-62 41-55 34-48 41-48"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a[i] = i;
a.length = 2;
a[4500] = 1;
var b = [];
b.length = 10000;
b[5000] = 1;
var c = [];
c[4097] = 1;
var d = [];
d[4096] = 1;
[a.length, Object.keys(a).length, b.length, c.length, d.length].join(",")"#,
        r#"4501,3,10000,4098,4097"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3];
r.push(a.at(-1), a.at(5), a.at("1"), a.at(-4), [, 1].at(0));
r.push([1, 2, 3].slice(-2).join(), [1, 2, 3].slice(1, -1).join(), [1, 2, 3].slice("1").join(), [1, , 3].slice(0, 2).length, 1 in [1, , 3].slice());
r.join("|")"#,
        r#"3||2|||2,3|2|2,3|2|false"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = [];
r.push(a.every(function (x) { return x > 0; }), a.some(function (x) { return x > 2; }), a.every(function (x, i, o) { o.length = 1; return true; }), a.length);
r.join("|")"#,
        r#"true|true|true|1"#,
    ),
    (
        r#"var calls = [];
var a = [1, 2, 3, 4];
a.forEach(function (v, i) { calls.push(v); if (i === 0) { a.push(9); a.shift(); } });
calls.join(",") + "|" + a.join(",")"#,
        r#"1,3,4,9|2,3,4,9"#,
    ),
    (
        r#"var a = [1, 2, 3];
var it = a.values();
var r = [it.next().value];
a.length = 1;
r.push(it.next().done);
a.push(7, 8);
r.push(J(it.next()));
r.join("|")"#,
        r#"1|true|{"done":true}"#,
    ),
    (
        r#"var a = Array.prototype;
a.push(1, 2, 3, 4, 5);
var r = [a.length, a[0]];
a.length = 3;
r.push(a.length, a.join());
a.shift();
r.push(a.length, a.join());
try { Object.defineProperty(a, "length", { value: 0, writable: false }); r.push(a.length); a.push(1); } catch (e) { r.push(e.message); }
r.join("|")"#,
        r#"5|1|3|1,2,3|2|2,3|0"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, "length", { value: 1 });
var r = [a.length, J(a)];
try { Object.defineProperty(a, "length", { configurable: true }); } catch (e) { r.push(e.message); }
try { Object.defineProperty(a, "length", { get: function () {} }); } catch (e) { r.push(e.message); }
r.push(Object.defineProperty(a, "length", { value: "5" }).length);
r.join("|")"#,
        r#"1|[1]|Cannot redefine property: length|Cannot redefine property: length|5"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = [];
Object.preventExtensions(a);
try { (function () { "use strict"; a[3] = 4; })(); } catch (e) { r.push(e.message); }
try { (function () { "use strict"; a.push(4); })(); } catch (e) { r.push(e.message); }
try { Object.defineProperty(a, "5", { value: 1 }); } catch (e) { r.push(e.message); }
r.push(a.length);
r.join("|")"#,
        r#"Cannot add property 3, object is not extensible|Cannot add property 3, object is not extensible|Cannot define property 5, object is not extensible|3"#,
    ),
    (
        r#"var a = [3, 2, 1];
var r = [];
Object.defineProperty(a, "0", { writable: false });
try { (function () { "use strict"; a.sort(); })(); } catch (e) { r.push(e.name + ":" + e.message); }
r.push(a.join());
try { (function () { "use strict"; a[0] = 9; })(); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"TypeError:|3,2,1|TypeError:"#,
    ),
    (
        r#"var r = [];
var a = [1, 2];
r.push(Object.getOwnPropertyNames(a).join(), J(Object.getOwnPropertyDescriptor(a, "length")), J(Object.getOwnPropertyDescriptor(a, 0)));
var b = [];
b[10] = 1;
r.push(J(Object.getOwnPropertyDescriptors(b)));
r.join("|")"#,
        r#"0,1,length|{"value":2,"writable":true,"enumerable":false,"configurable":false}|{"value":1,"writable":true,"enumerable":true,"configurable":true}|{"10":{"value":1,"writable":true,"enumerable":true,"configurable":true},"length":{"writable":true,"enumerable":false,"configurable":false}}"#,
    ),
    (
        r#"var a = [];
a[0] = 1;
a[2] = 3;
a.splice(1, 0, 2);
var b = [1, 2, 3, 4, 5, 6];
var c = b.splice(2);
J(a) + "|" + J(b) + "|" + J(c)"#,
        r#"[1,2,_,3]|[1,2]|[3,4,5,6]"#,
    ),
    (
        r#"var big = [];
for (var i = 0; i < 300; i++) big.push(i);
big.splice(10, 280);
var x = [];
for (var i = 0; i < 40; i++) x.push(i);
x.splice(5, 2, "a", "b", "c", "d");
big.join(",") + "|" + x.join(",")"#,
        r#"0,1,2,3,4,5,6,7,8,9,290,291,292,293,294,295,296,297,298,299|0,1,2,3,4,a,b,c,d,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3];
r.push(a.toString === Array.prototype.toString, typeof a.toString, a.toString(), Object.prototype.toString.call(a));
r.push(Array.prototype.length, Array.length, Array.name, Array.prototype.concat.length, Array.prototype.slice.length, Array.prototype.splice.name);
r.join("|")"#,
        r#"true|function|1,2,3|[object Array]|0|1|Array|1|2|splice"#,
    ),
    (
        r#"var r = [];
var desc = Object.getOwnPropertyDescriptor(Array, Symbol.species);
r.push(typeof desc.get, desc.get.name, Array[Symbol.species] === Array);
var u = Array.prototype[Symbol.unscopables];
r.push(Object.getPrototypeOf(u), Object.keys(u).join(","));
r.join("|")"#,
        r#"function|get [Symbol.species]|true||copyWithin,entries,fill,find,findIndex,findLast,findLastIndex,flat,flatMap,includes,keys,values,groupBy,groupByToMap,toReversed,toSorted,toSpliced"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5];
var r = [];
r.push(J(a.flatMap(function (x) { return [x, [x * 2]]; })));
r.push(J([[1], [2]].flatMap(function (x, i, o) { return o.length; })));
r.join("|")"#,
        r#"[1,[2],2,[4],3,[6],4,[8],5,[10]]|[2,2]"#,
    ),
    (
        r#"var r = [];
var o = { length: 3, 0: "a", 2: "c" };
r.push(Array.prototype.join.call(o), J(Array.prototype.slice.call(o)), Array.prototype.indexOf.call(o, "c"), Array.prototype.lastIndexOf.call(o, "a"));
Array.prototype.push.call(o, "d");
r.push(o.length, o[3]);
Array.prototype.reverse.call(o);
r.push(J(o));
Array.prototype.splice.call(o, 1, 1);
r.push(J(o));
Array.prototype.unshift.call(o, "z");
r.push(J(o));
r.push(Array.prototype.pop.call(o), J(o));
r.join("|")"#,
        r#"a,,c|["a",_,"c"]|2|0|4|d|{"0":"d","1":"c","3":"a","length":4}|{"0":"d","2":"a","length":3}|{"0":"z","1":"d","3":"a","length":4}|a|{"0":"z","1":"d","length":3}"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.length = 4294967295;
var r = [a.length];
try { a.push(1); } catch (e) { r.push(e.name + ":" + e.message); }
r.push(a.length, a[4294967295], a[4294967294]);
a[4294967295] = 9;
r.push(a.length, a[4294967295], Object.keys(a).length);
r.join("|")"#,
        r#"4294967295|RangeError:Invalid array length|4294967295|1||4294967295|9|4"#,
    ),
    (
        r#"var a = [];
a["4294967294"] = 1;
var r = [a.length];
a["4294967295"] = 2;
r.push(a.length, Object.keys(a).join(","));
r.join("|")"#,
        r#"4294967295|4294967295|4294967294,4294967295"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = [];
r.push(delete a[1], a.length, 1 in a, J(a), a.indexOf(undefined));
r.push(delete a[10], delete a.length, a.length);
r.join("|")"#,
        r#"true|3|false|[1,_,3]|-1|true|false|3"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5, 6, 7, 8, 9];
var r = [];
r.push(J(a.slice(2, 5).reverse()), J([1, , 3].reverse()), J([1, 2, , 4].reverse()));
r.join("|")"#,
        r#"[5,4,3]|[3,_,1]|[4,_,2,1]"#,
    ),
    (
        r#"var n = 0;
var a = { length: 3, 0: 1, 1: 2, 2: 3 }; n = 5;
var m = Array.prototype.map.call(a, function (x) { return x; });
J(m) + "|" + n + "|" + Array.isArray(a)"#,
        r#"[1,2,3]|5|false"#,
    ),
    (
        r#"var a = [10, 1, 3];
var b = a.toSorted(function (x, y) { return x - y; });
var c = [10, 1, 3].sort();
J(a) + "|" + J(b) + "|" + J(c)"#,
        r#"[10,1,3]|[1,3,10]|[1,10,3]"#,
    ),
    (
        r#"var a = [3, 2, 1];
try {
  a.sort(function () { throw new Error("cmp"); });
} catch (e) { e.message + "|" + a.join(); }"#,
        r#"cmp|3,2,1"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3];
var idx = a.findIndex(function (x, i, o) { o[i + 1] = 0; return x === 0; });
r.push(idx, a.join());
r.push([].find(function () { return true; }), [,].find(function () { return true; }));
r.join("|")"#,
        r#"1|1,0,0||"#,
    ),
    (
        r#"var r = [];
r.push(J(Array.from({ length: 2, 0: "x" })));
r.push(J(Array.from([1, , 3])));
var o = { 0: 1, length: 2 };
r.push(J(Array.from(o, function (v, i) { return v === undefined ? "u" + i : v; })));
r.join("|")"#,
        r#"["x",null]|[1,null,3]|[1,"u1"]"#,
    ),
    (
        r#"var arr = [1, 2, 3];
var s = 0;
for (var v of arr) { s += v; if (v === 1) arr.push(4); }
var e = [];
for (var [i, v] of ["a", "b"].entries()) e.push(i + v);
s + "|" + e.join()"#,
        r#"10|0a,1b"#,
    ),
    (
        r#"var a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
a.length = 17;
a.push(1);
a.length = 3;
a.push(2);
J(a)"#,
        r#"[0,1,2,2]"#,
    ),
    (
        r#"var x = [];
for (var i = 0; i < 1500; i++) x[i * 3] = i;
var r = [x.length, Object.keys(x).length];
for (var i = 0; i < 4500; i++) if (x[i] === undefined) x[i] = -1;
r.push(x.length, x[4499], x[3]);
r.join("|")
"#,
        r#"4498|1500|4500|-1|1"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, "length", { value: { valueOf: function () { a.length = 1; return 3; } } });
a.length + "|" + J(a)"#,
        r#"1|[1]"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, "length", { value: { valueOf: function () { a.length = 1; a.length; return 3; } } });
a.length + "|" + J(a)"#,
        r#"3|[1,_,_]"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.length = { valueOf: function () { a.push(9); return 2; } };
a.length + "|" + J(a)"#,
        r#"2|[1,2]"#,
    ),
    (
        r#"var m = [1, , 3].map(function (x) { return x; });
var r = [m.indexOf(undefined), m.includes(undefined), m.lastIndexOf(undefined), J(m.slice()), J(m.reverse())];
r.join("|")"#,
        r#"-1|false|-1|[1,_,3]|[3,_,1]"#,
    ),
    (
        r#"var m = [3, , 1].map(function (x) { return x; });
m.sort();
J(m) + "|" + (1 in m) + "|" + (2 in m)"#,
        r#"[1,3,_]|true|false"#,
    ),
    (
        r#"var m = [3, , 1].map(function (x) { return x; });
var s = m.toSorted();
J(s) + "|" + J(m.toReversed()) + "|" + J(m.with(0, 7)) + "|" + J(m.toSpliced(0, 1))"#,
        r#"[1,3,_]|[1,_,3]|[7,_,1]|[_,1]"#,
    ),
    (
        r#"var m = [3, , 1].map(function (x) { return x; });
var c = m.splice(0, 2);
J(c) + "|" + J(m) + "|" + (1 in c)"#,
        r#"[3,null]|[1]|true"#,
    ),
    (
        r#"var a = [];
a[3] = 3;
a[1] = 1;
a.unshift("x");
var b = [1, , 3];
b.shift();
J(a) + "|" + J(b) + "|" + (0 in b)"#,
        r#"["x",_,1,_,3]|[_,3]|false"#,
    ),
    (
        r#"var a = [1, 2, 3];
var it = 0;
var r = a.toLocaleString.call([{ toLocaleString: function () { a.push(it++); return "x"; } }, "2", "3"]);
r + "|" + a.length"#,
        r#"x,2,3|4"#,
    ),
    (
        r#"var a = [{ toLocaleString: function () { a.length = 1; return "x"; } }, "b", "c"];
a.toLocaleString() + "|" + a.length"#,
        r#"x,,|1"#,
    ),
    (
        r#"var a = [5, 3, 1, 4, 2];
var calls = 0;
a.sort(function (x, y) { calls++; if (calls === 3) a[0] = 100; return x - y; });
J(a) + "|" + calls"#,
        r#"[1,2,4,100,5]|10"#,
    ),
    (
        r#"var a = [5, 3, 1, 4, 2];
a.sort(function (x, y) { if (a.length < 8) a.push(0); return x - y; });
J(a)"#,
        r#"[1,2,3,4,5,0,0,0]"#,
    ),
    (
        r#"function tag(s) { return s; }
var s = tag`a${1}b`;
var r = [Object.isFrozen(s), Object.isFrozen(s.raw), s.length, s.raw.length, Object.isExtensible(s), Object.isExtensible(s.raw)];
var d = Object.getOwnPropertyDescriptor(s, "length");
r.push(d.writable, d.configurable, Object.getOwnPropertyDescriptor(s, 0).writable);
try { "use strict"; s.raw.push(1); } catch (e) { r.push(e.message); }
r.join("|")"#,
        r#"true|true|2|2|false|false|false|false|false|Cannot add property 2, object is not extensible"#,
    ),
    (
        r#"var a = [1, 2, 3];
var k = [];
for (var x in a) { k.push(x); if (x === "0") { a[200000] = 1; } }
k.join(",") + "|" + Object.keys(a).join(",")"#,
        r#"0,1,2|0,1,2,200000"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
var k = [];
for (var x in a) { k.push(x); a[5] = 5; a[200000] = 2; }
k.join(",") + "|" + Object.keys(a).join(",")"#,
        r#"100000,200000|5,100000,200000"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
a[3] = 3;
var r = [];
r.push(a.indexOf(3), a.lastIndexOf(1), a.includes(undefined), a.join(""));
a.reverse();
r.push(a[0], a[99997], Object.keys(a).join(","));
r.join("|")"#,
        r#"3|100000|true|31|1|3|0,99997"#,
    ),
    (
        r#"var a = [];
a[100000] = "z";
a[2] = "b";
a[7] = "a";
a.sort();
J(Object.keys(a)) + "|" + a[0] + a[1] + a[2] + "|" + a.length"#,
        r#"["0","1","2"]|abz|100001"#,
    ),
    (
        r#"var a = [];
a[5000] = 1;
var b = a.slice(4990);
var c = a.splice(4999, 2, "q");
J(b) + "|" + J(c) + "|" + a.length + "|" + a[4999]"#,
        r#"[_,_,_,_,_,_,_,_,_,_,1]|[_,1]|5000|q"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
var r = [];
Object.freeze(a);
r.push(Object.isFrozen(a), Object.getOwnPropertyDescriptor(a, "length").writable, Object.getOwnPropertyDescriptor(a, "100000").writable);
try { "use strict"; a[1] = 1; } catch (e) { r.push(e.message); }
r.join("|")"#,
        r#"true|false|false"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "length", { writable: false });
var r = [Object.getOwnPropertyDescriptor(a, "length").writable];
try { a.push(1); } catch (e) { r.push(e.message); }
try { Object.defineProperty(a, "length", { writable: true }); } catch (e) { r.push(e.message); }
try { a[100001] = 1; r.push(a[100001]); } catch (e) { r.push(e.message); }
r.join("|")"#,
        r#"false|length is not writable|Cannot redefine property: length|"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "100000", { get: function () { return "g"; }, configurable: true });
var r = [a[100000], a.length];
delete a[100000];
r.push(a[100000], a.length, 100000 in a);
r.join("|")"#,
        r#"g|100001||100001|false"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
var proto = [];
proto[5] = "p";
Object.setPrototypeOf(a, proto);
var r = [a[5], a.indexOf("p"), 5 in a, a.hasOwnProperty(5)];
r.join("|")"#,
        r#"p|5|true|false"#,
    ),
    (
        r#"var proto = [1, 2, 3];
var a = [9];
Object.setPrototypeOf(a, proto);
var r = [a[1], a.length, a.indexOf(2), a.join()];
a[2] = "x";
r.push(proto[2], a[2], a.length);
r.join("|")"#,
        r#"2|1|-1|9|3|x|3"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 1, { set: function (v) { this.seen = v; }, configurable: true });
a[1] = 42;
var r = [a.seen, a[1], a.length];
a.fill(7);
r.push(a.seen, J(a));
r.join("|")"#,
        r#"42||3|7|[7,null,7]"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 1, { get: function () { return 5; } });
var r = [];
try { (function () { "use strict"; a[1] = 3; })(); } catch (e) { r.push(e.name + "/" + e.message); }
try { (function () { "use strict"; a.length = 0; })(); } catch (e) { r.push(e.name + "/" + e.message); }
r.push(a.length, a[0], a[1]);
r.join("|")"#,
        r#"0||"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 2000; i++) a[i * 3] = i;
var r = [a.length, Object.keys(a).length];
a.length = 3000;
r.push(Object.keys(a).length, a[2997]);
a.length = 2998;
r.push(Object.keys(a).length);
r.join("|")"#,
        r#"5998|2000|1000|999|1000"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "99999", { value: 2, configurable: false, enumerable: true, writable: true });
var r = [];
try { a.length = 99999; } catch (e) { r.push(e.message); }
r.push(a.length, a[99999], a[100000], Object.keys(a).join(","));
r.join("|")"#,
        r#"99999|||"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
Object.defineProperty(a, "100000", { value: 2, configurable: false, enumerable: true, writable: true });
var r = [];
try { a.length = 100000; } catch (e) { r.push(e.message); }
r.push(a.length, a[100000], Object.keys(a).join(","));
try { "use strict"; a.length = 0; } catch (e) { r.push(e.message); }
r.push(a.length, Object.keys(a).join(","));
r.join("|")"#,
        r#"100000|||0|"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5];
var r = [];
r.push(J(Array.prototype.slice.call(a, -2)), J(Array.prototype.splice.call(a, 1, 1, 9, 8)), J(a));
r.push(J(Array.prototype.concat.call(1, 2)), J(Array.prototype.concat.call("ab", ["c"])));
r.join("|")"#,
        r#"[4,5]|[2]|[1,9,8,3,4,5]|[{},2]|[{"0":"a","1":"b"},"c"]"#,
    ),
    (
        r#"var r = [];
r.push(J([].concat.call(null === null ? [] : 0, [1], [[2]])));
var o = {};
o[Symbol.isConcatSpreadable] = true;
o.length = 2;
o[1] = "y";
r.push(J([0].concat(o)));
var arr = [1, 2];
arr[Symbol.isConcatSpreadable] = undefined;
r.push(J([].concat(arr)));
arr[Symbol.isConcatSpreadable] = 0;
r.push(J([].concat(arr)));
r.join("|")"#,
        r#"[1,[2]]|[0,_,"y"]|[1,2]|[[1,2]]"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3, 4];
r.push(J(a.copyWithin(-1, 0)), J([1, 2, 3, 4, 5].copyWithin(2, 0, 2)), J([1, , 3, , 5].copyWithin(0, 1)));
var o = { length: 4, 0: "a", 2: "c" };
Array.prototype.copyWithin.call(o, 1, 0, 3);
r.push(J(o));
Array.prototype.fill.call(o, "f", 3);
r.push(J(o));
r.join("|")"#,
        r#"[1,2,3,1]|[1,2,1,2,5]|[_,3,_,5,5]|{"0":"a","1":"a","3":"c","length":4}|{"0":"a","1":"a","3":"f","length":4}"#,
    ),
    (
        r#"var r = [];
r.push(J([3, 2, 1].sort(undefined)), J([1, 10, 2, 21].sort()), J(["b", "a", "C", "á"].sort()));
r.push(J([true, false, null, undefined, NaN, 0, -1, "1"].sort()));
r.join("|")"#,
        r#"[1,2,3]|[1,10,2,21]|["C","a","b","á"]|[-1,0,"1",NaN,false,null,true,null]"#,
    ),
    (
        r#"var a = [];
var log = [];
for (var i = 0; i < 43; i++) a.push({ k: (i * 17) % 11, i: i });
a.sort(function (x, y) { log.push(x.i + "/" + y.i); return x.k - y.k; });
a.map(function (o) { return o.i; }).join(",") + "|" + log.length + "|" + log.join(" ")"#,
        r#"0,11,22,33,2,13,24,35,4,15,26,37,6,17,28,39,8,19,30,41,10,21,32,1,12,23,34,3,14,25,36,5,16,27,38,7,18,29,40,9,20,31,42|268|1/0 2/1 2/0 3/1 4/3 4/1 4/2 5/3 6/5 6/3 6/1 6/4 7/5 8/7 8/5 8/3 8/1 8/6 9/7 10/9 10/7 10/5 10/3 10/1 10/8 11/9 11/7 11/5 11/3 11/1 11/10 11/8 11/6 11/4 11/2 11/0 12/9 12/7 12/5 12/3 12/1 13/9 13/7 13/5 13/3 13/12 13/1 13/10 13/8 13/6 13/4 13/2 14/9 14/7 14/5 14/3 15/9 15/7 15/5 15/14 15/3 15/12 15/1 15/10 15/8 15/6 15/4 16/9 16/7 16/5 17/9 17/7 17/16 17/5 17/14 17/3 17/12 17/1 17/10 17/8 17/6 18/9 18/7 19/9 19/18 19/7 19/16 19/5 19/14 19/3 19/12 19/1 19/10 19/8 21/20 22/20 22/21 23/20 23/21 24/20 24/23 24/21 24/22 25/20 25/23 26/20 26/25 26/23 26/21 26/24 27/20 27/25 28/20 28/27 28/25 28/23 28/21 28/26 29/20 29/27 30/20 30/29 30/27 30/25 30/23 30/21 30/28 31/20 32/31 32/20 32/29 32/27 32/25 32/23 32/21 33/31 33/20 33/29 33/27 33/25 33/23 33/32 33/21 33/30 33/28 33/26 33/24 33/22 34/31 34/20 34/29 34/27 34/25 34/23 35/31 35/20 35/29 35/27 35/25 35/34 35/23 35/32 35/21 35/30 35/28 35/26 35/24 36/31 36/20 36/29 36/27 36/25 37/31 37/20 37/29 37/27 37/36 37/25 37/34 37/23 37/32 37/21 37/30 37/28 37/26 38/31 38/20 38/29 38/27 39/31 39/20 39/29 39/38 39/27 39/36 39/25 39/34 39/23 39/32 39/21 39/30 39/28 41/40 42/40 21/10 26/5 28/3 39/12 30/1 26/15 33/8 24/17 35/6 33/13 24/2 22/11 33/2 24/4 35/13 24/13 35/15 35/4 28/8 39/17 26/17 37/6 28/17 39/19 30/8 39/8 30/10 30/19 36/5 38/3 27/14 23/3 34/12 21/12 32/1 23/12 34/14 25/3 34/3 36/14 25/14 29/7 27/9 38/18 27/16 38/7 29/9 20/18 29/18 31/9 20/9 40/10 41/21 41/26 41/39 41/30 41/10 40/5 40/18 40/20 42/9 40/9 40/29 42/31"#,
    ),
    (
        r#"var a = [];
var log = [];
for (var i = 0; i < 64; i++) a.push(63 - i);
a.sort(function (x, y) { log.push(x + "/" + y); return x - y; });
a.join(",") + "|" + log.length + "|" + log.join(" ")"#,
        r#"0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63|604|62/63 61/63 61/62 60/63 60/62 60/61 59/63 59/62 59/61 59/60 58/63 58/62 58/61 58/60 58/59 57/63 57/62 57/61 57/60 57/59 57/58 56/63 56/62 56/61 56/60 56/59 56/58 56/57 55/63 55/62 55/61 55/60 55/59 55/58 55/57 55/56 54/63 54/62 54/61 54/60 54/59 54/58 54/57 54/56 54/55 53/63 53/62 53/61 53/60 53/59 53/58 53/57 53/56 53/55 53/54 52/63 52/62 52/61 52/60 52/59 52/58 52/57 52/56 52/55 52/54 52/53 51/63 51/62 51/61 51/60 51/59 51/58 51/57 51/56 51/55 51/54 51/53 51/52 50/63 50/62 50/61 50/60 50/59 50/58 50/57 50/56 50/55 50/54 50/53 50/52 50/51 49/63 49/62 49/61 49/60 49/59 49/58 49/57 49/56 49/55 49/54 49/53 49/52 49/51 49/50 48/63 48/62 48/61 48/60 48/59 48/58 48/57 48/56 48/55 48/54 48/53 48/52 48/51 48/50 48/49 47/63 47/62 47/61 47/60 47/59 47/58 47/57 47/56 47/55 47/54 47/53 47/52 47/51 47/50 47/49 47/48 46/63 46/62 46/61 46/60 46/59 46/58 46/57 46/56 46/55 46/54 46/53 46/52 46/51 46/50 46/49 46/48 46/47 45/63 45/62 45/61 45/60 45/59 45/58 45/57 45/56 45/55 45/54 45/53 45/52 45/51 45/50 45/49 45/48 45/47 45/46 44/63 44/62 44/61 44/60 44/59 44/58 44/57 44/56 44/55 44/54 44/53 44/52 44/51 44/50 44/49 44/48 44/47 44/46 44/45 42/43 41/43 41/42 40/43 40/42 40/41 39/43 39/42 39/41 39/40 38/43 38/42 38/41 38/40 38/39 37/43 37/42 37/41 37/40 37/39 37/38 36/43 36/42 36/41 36/40 36/39 36/38 36/37 35/43 35/42 35/41 35/40 35/39 35/38 35/37 35/36 34/43 34/42 34/41 34/40 34/39 34/38 34/37 34/36 34/35 33/43 33/42 33/41 33/40 33/39 33/38 33/37 33/36 33/35 33/34 32/43 32/42 32/41 32/40 32/39 32/38 32/37 32/36 32/35 32/34 32/33 31/43 31/42 31/41 31/40 31/39 31/38 31/37 31/36 31/35 31/34 31/33 31/32 30/43 30/42 30/41 30/40 30/39 30/38 30/37 30/36 30/35 30/34 30/33 30/32 30/31 29/43 29/42 29/41 29/40 29/39 29/38 29/37 29/36 29/35 29/34 29/33 29/32 29/31 29/30 28/43 28/42 28/41 28/40 28/39 28/38 28/37 28/36 28/35 28/34 28/33 28/32 28/31 28/30 28/29 27/43 27/42 27/41 27/40 27/39 27/38 27/37 27/36 27/35 27/34 27/33 27/32 27/31 27/30 27/29 27/28 26/43 26/42 26/41 26/40 26/39 26/38 26/37 26/36 26/35 26/34 26/33 26/32 26/31 26/30 26/29 26/28 26/27 25/43 25/42 25/41 25/40 25/39 25/38 25/37 25/36 25/35 25/34 25/33 25/32 25/31 25/30 25/29 25/28 25/27 25/26 24/43 24/42 24/41 24/40 24/39 24/38 24/37 24/36 24/35 24/34 24/33 24/32 24/31 24/30 24/29 24/28 24/27 24/26 24/25 22/23 21/23 21/22 20/23 20/22 20/21 19/23 19/22 19/21 19/20 18/23 18/22 18/21 18/20 18/19 17/23 17/22 17/21 17/20 17/19 17/18 16/23 16/22 16/21 16/20 16/19 16/18 16/17 15/23 15/22 15/21 15/20 15/19 15/18 15/17 15/16 14/23 14/22 14/21 14/20 14/19 14/18 14/17 14/16 14/15 13/23 13/22 13/21 13/20 13/19 13/18 13/17 13/16 13/15 13/14 12/23 12/22 12/21 12/20 12/19 12/18 12/17 12/16 12/15 12/14 12/13 11/23 11/22 11/21 11/20 11/19 11/18 11/17 11/16 11/15 11/14 11/13 11/12 10/23 10/22 10/21 10/20 10/19 10/18 10/17 10/16 10/15 10/14 10/13 10/12 10/11 9/23 9/22 9/21 9/20 9/19 9/18 9/17 9/16 9/15 9/14 9/13 9/12 9/11 9/10 8/23 8/22 8/21 8/20 8/19 8/18 8/17 8/16 8/15 8/14 8/13 8/12 8/11 8/10 8/9 7/23 7/22 7/21 7/20 7/19 7/18 7/17 7/16 7/15 7/14 7/13 7/12 7/11 7/10 7/9 7/8 6/23 6/22 6/21 6/20 6/19 6/18 6/17 6/16 6/15 6/14 6/13 6/12 6/11 6/10 6/9 6/8 6/7 5/23 5/22 5/21 5/20 5/19 5/18 5/17 5/16 5/15 5/14 5/13 5/12 5/11 5/10 5/9 5/8 5/7 5/6 4/23 4/22 4/21 4/20 4/19 4/18 4/17 4/16 4/15 4/14 4/13 4/12 4/11 4/10 4/9 4/8 4/7 4/6 4/5 2/3 1/3 1/2 0/3 0/2 0/1 33/54 38/49 41/46 42/45 43/44 1/14 2/13 3/12 1/8 2/7 3/6 1/5 2/4 3/5 3/4 11/44 17/38 20/35 22/33 23/32 11/28 13/26 14/25 15/24 19/28 21/26 22/25 23/24"#,
    ),
    (
        r#"var a = [];
var log = [];
for (var i = 0; i < 41; i++) a.push(i % 2 ? i : -i);
var o = {};
for (var i = 0; i < 41; i++) o[i] = a[i];
o.length = 41;
Array.prototype.sort.call(o, function (x, y) { log.push(x + "/" + y); return y - x; });
var out = [];
for (var i = 0; i < 41; i++) out.push(o[i]);
out.join(",") + "|" + log.length + "|" + log.join(" ")"#,
        r#"39,37,35,33,31,29,27,25,23,21,19,17,15,13,11,9,7,5,3,1,0,-2,-4,-6,-8,-10,-12,-14,-16,-18,-20,-22,-24,-26,-28,-30,-32,-34,-36,-38,-40|234|1/0 -2/0 3/-2 3/0 3/1 -4/-2 5/-4 5/-2 5/0 5/1 5/3 -6/-4 7/-6 7/-4 7/-2 7/0 7/1 7/3 7/5 -8/-6 9/-8 9/-6 9/-4 9/-2 9/0 9/1 9/3 9/5 9/7 -10/-8 11/-10 11/-8 11/-6 11/-4 11/-2 11/0 11/1 11/3 11/5 11/7 11/9 -12/-10 13/-12 13/-10 13/-8 13/-6 13/-4 13/-2 13/0 13/1 13/3 13/5 13/7 13/9 13/11 -14/-12 15/-14 15/-12 15/-10 15/-8 15/-6 15/-4 15/-2 15/0 15/1 15/3 15/5 15/7 15/9 15/11 15/13 -16/-14 17/-16 17/-14 17/-12 17/-10 17/-8 17/-6 17/-4 17/-2 17/0 17/1 17/3 17/5 17/7 17/9 17/11 17/13 17/15 -18/-16 19/-18 19/-16 19/-14 19/-12 19/-10 19/-8 19/-6 19/-4 19/-2 19/0 19/1 19/3 19/5 19/7 19/9 19/11 19/13 19/15 19/17 21/-20 -22/-20 23/-22 23/-20 23/21 -24/-22 25/-24 25/-22 25/-20 25/21 25/23 -26/-24 27/-26 27/-24 27/-22 27/-20 27/21 27/23 27/25 -28/-26 29/-28 29/-26 29/-24 29/-22 29/-20 29/21 29/23 29/25 29/27 -30/-28 31/-30 31/-28 31/-26 31/-24 31/-22 31/-20 31/21 31/23 31/25 31/27 31/29 -32/-30 33/-32 33/-30 33/-28 33/-26 33/-24 33/-22 33/-20 33/21 33/23 33/25 33/27 33/29 33/31 -34/-32 35/-34 35/-32 35/-30 35/-28 35/-26 35/-24 35/-22 35/-20 35/21 35/23 35/25 35/27 35/29 35/31 35/33 -36/-34 37/-36 37/-34 37/-32 37/-30 37/-28 37/-26 37/-24 37/-22 37/-20 37/21 37/23 37/25 37/27 37/29 37/31 37/33 37/35 -38/-36 39/-38 39/-36 39/-34 39/-32 39/-30 39/-28 39/-26 39/-24 39/-22 39/-20 39/21 39/23 39/25 39/27 39/29 39/31 39/33 39/35 39/37 21/0 -28/9 -22/3 -20/1 31/9 25/15 23/17 21/19 -28/-10 -22/-16 -20/-18 -40/0 -40/-20 -40/-30 -40/-36 -40/-38"#,
    ),
    (
        r#"var a = [3, 1, 2];
var r = [];
r.push(J(a.entries().next()), Object.getPrototypeOf(a.keys()) === Object.getPrototypeOf([].values()), typeof Object.getPrototypeOf(Object.getPrototypeOf(a.values()))[Symbol.iterator]);
var it = a.values();
r.push(it[Symbol.iterator]() === it, it.toString(), Object.prototype.hasOwnProperty.call(it, "next"));
var it2 = Array.prototype.values.call({ length: 2, 0: "x", 1: "y" });
r.push(it2.next().value, it2.next().value, it2.next().done);
r.join("|")"#,
        r#"{"value":[0,3],"done":false}|true|function|true|[object Array Iterator]|false|x|y|true"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3];
a[Symbol.iterator] = function* () { yield 9; };
r.push(J(Array.from(a)), J([...a]));
var b = [1, 2];
Object.defineProperty(Array.prototype, Symbol.iterator, { value: function* () { yield "p"; }, configurable: true, writable: true });
r.push(J(Array.from(b)));
r.join("|")"#,
        r#"[9]|[9]|["p"]"#,
    ),
    (
        r#"var r = [];
function C(n) { this.args = arguments.length; this.n = n; }
var x = Array.from.call(C, { length: 2, 0: "a" });
r.push(x instanceof C, x.length, x.args, x.n, x[0], x[1], x.hasOwnProperty(1));
var y = Array.from.call(C, [1, 2]);
r.push(y.args, y.n, y.length, y[1]);
var z = Array.of.call(C, 4, 5);
r.push(z.args, z.n, z.length, z[1]);
r.join("|")"#,
        r#"true|2|1|2|a||true|0||2|2|1|2|2|5"#,
    ),
    (
        r#"var r = [];
var a = [1, 2, 3];
r.push(J(a.flat.call({ length: 2, 0: [1, [2]], 1: 3 })), J([[[1]]].flat(2)), J([1, [2, , 4]].flat()));
r.push(J([1, 2].flatMap(function (x) { return x === 1 ? [] : [[x]]; })));
try { [1].flatMap(); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"[1,[2],3]|[1]|[1,2,4]|[[2]]|TypeError:Value is not an object: undefined"#,
    ),
    (
        r#"var r = [];
var big = [];
big.length = 4294967295;
try { big.push(1); } catch (e) { r.push(e.name + ":" + e.message); }
r.push(big.length);
r.join("|")"#,
        r#"RangeError:Invalid array length|4294967295"#,
    ),
    (
        r#"var r = [];
var o = { length: 9007199254740991 };
try { Array.prototype.push.call(o, 1); } catch (e) { r.push(e.name + ":" + e.message); }
try { Array.prototype.splice.call(o, 0, 0, 1); } catch (e) { r.push(e.name + ":" + e.message); }
try { Array.prototype.toSorted.call({ length: 4294967295 }); } catch (e) { r.push(e.name + ":" + e.message); }
try { Array.prototype.toReversed.call({ length: 4294967296 }); } catch (e) { r.push(e.name + ":" + e.message); }
try { Array.prototype.with.call({ length: 4294967296 }, 0, 1); } catch (e) { r.push(e.name + ":" + e.message); }
r.join("|")"#,
        r#"TypeError:Invalid array length|TypeError:Invalid array length|RangeError:Invalid array length|RangeError:Invalid array length|RangeError:Invalid array length"#,
    ),
    (
        r#"var r = [];
var o = { length: 3, 0: 1, 1: 2, 2: 3 };
r.push(Array.prototype.pop.call(o), o.length, Array.prototype.shift.call(o), J(o));
var e = { length: 0 };
r.push(Array.prototype.pop.call(e), e.length, Array.prototype.shift.call(e), e.length);
var s = new String("ab");
try { Array.prototype.pop.call(s); } catch (x) { r.push(x.name + ":" + x.message); }
r.join("|")"#,
        r#"3|2|1|{"0":2,"length":1}||0||0|TypeError:Cannot delete property '1' of a String"#,
    ),
    (
        r#"var a = [1, 2, 3];
var b = Object.freeze([1, 2, 3]);
var r = [];
try { b.pop(); } catch (e) { r.push(e.message); }
try { b.shift(); } catch (e) { r.push(e.message); }
try { b.reverse(); } catch (e) { r.push(e.message); }
try { b.sort(); } catch (e) { r.push(e.message); }
try { b.fill(0); } catch (e) { r.push(e.message); }
try { b.copyWithin(0, 1); } catch (e) { r.push(e.message); }
try { b.splice(0, 1); } catch (e) { r.push(e.message); }
try { b.unshift(0); } catch (e) { r.push(e.message); }
r.push(J(b));
r.join("|")"#,
        r#"Cannot delete property '2' of 1,2,3|||||||Cannot add property 3, object is not extensible|[1,2,3]"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.seal(a);
var r = [];
try { a.pop(); } catch (e) { r.push(e.message); }
try { a.push(1); } catch (e) { r.push(e.message); }
a[0] = 9;
r.push(J(a), a.length);
r.join("|")"#,
        r#"Cannot delete property '2' of 1,2,3|Cannot add property 3, object is not extensible|[9,2,3]|3"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.preventExtensions(a);
a.length = 1;
var r = [a.length];
try { a.push(1); } catch (e) { r.push(e.message); }
a.length = 3;
r.push(a.length, 1 in a);
r.join("|")"#,
        r#"1|Cannot add property 1, object is not extensible|3|false"#,
    ),
    (
        r#"var a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
var log = [];
var r = a.map(function (v, i, o) { if (i === 2) { o.length = 5; } log.push(v); return v * 10; });
J(r) + "|" + log.join(",") + "|" + r.length"#,
        r#"[0,10,20,30,40,_,_,_,_,_]|0,1,2,3,4|10"#,
    ),
    (
        r#"var a = [0, 1, 2, 3, 4];
var r = a.filter(function (v, i, o) { if (i === 0) { o.push(9); o[3] = 33; } return v % 2 === 1 || v === 33; });
J(r) + "|" + a.length"#,
        r#"[1,33]|6"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = a.reduce(function (acc, v, i, o) { if (i === 1) delete o[2]; return acc + v; });
var s = [1, 2, 3].reduceRight(function (acc, v, i, o) { if (i === 1) o.length = 0; return acc + v; });
r + "|" + s"#,
        r#"3|5"#,
    ),
    (
        r#"var a = [];
a.length = 3;
var r = [a.map(function () { return 1; }).length, J(a.map(function () { return 1; })), a.filter(function () { return true; }).length];
r.push(a.every(function () { return false; }), a.some(function () { return true; }));
r.join("|")"#,
        r#"3|[_,_,_]|0|true|false"#,
    ),
    (
        r#"var a = new Array(3);
a[1] = "x";
var r = [];
r.push(a.findIndex(function (x) { return x === undefined; }), a.findLastIndex(function (x) { return x === undefined; }));
r.push(J(a.entries().next()), J(Array.from(a.keys())), J([...a.values()]));
r.join("|")"#,
        r#"0|2|{"value":[0,null],"done":false}|[0,1,2]|[null,"x",null]"#,
    ),
    (
        r#"var a = [3, 1, 2];
a.foo = "bar";
a[-1] = "neg";
a["1.5"] = "frac";
var r = [Object.keys(a).join(","), a.length, a[-1], J(a)];
a.sort();
r.push(Object.keys(a).join(","));
r.join("|")"#,
        r#"0,1,2,foo,-1,1.5|3|neg|[3,1,2]|0,1,2,foo,-1,1.5"#,
    ),
    (
        r#"var a = [1, 2, 3];
var desc = Object.getOwnPropertyDescriptor(a, 1);
Object.defineProperty(a, 1, { enumerable: false });
var r = [J(desc), Object.keys(a).join(","), J(Object.getOwnPropertyDescriptor(a, 1)), a.join()];
Object.defineProperty(a, 1, { enumerable: true, value: 7 });
r.push(Object.keys(a).join(","), a.join());
r.join("|")"#,
        r#"{"value":2,"writable":true,"enumerable":true,"configurable":true}|0,2|{"value":2,"writable":true,"enumerable":false,"configurable":true}|1,2,3|0,1,2|1,7,3"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 5, { value: 9 });
var r = [a.length, Object.keys(a).join(","), J(Object.getOwnPropertyDescriptor(a, 5)), Object.getOwnPropertyNames(a).join(",")];
r.join("|")"#,
        r#"6|0,1,2|{"value":9,"writable":false,"enumerable":false,"configurable":false}|0,1,2,5,length"#,
    ),
    (
        r#"var a = [10, 20, 30];
var b = a.slice();
var c = Array.prototype.slice.call(a, 1);
b[0] = 99;
var d = Array.from(a);
d.push(1);
var e = a.concat();
e.pop();
[J(a), J(b), J(c), J(d), J(e)].join("|")"#,
        r#"[10,20,30]|[99,20,30]|[20,30]|[10,20,30,1]|[10,20]"#,
    ),
    (
        r#"var r = [];
r.push([].sort() instanceof Array, [1].reverse()[0], [].pop(), [].shift(), [].join("x"), [,].join("x"), J([].splice(0)), J([].concat([], [])));
r.push([undefined].join(), [null].join(), [[[]]].join(), [[], []].join(), J([[]].flat()));
r.join("|")"#,
        r#"true|1|||||[]|[]||||,|[]"#,
    ),
    (
        r#"var a = [1, 2, 3];
var r = [];
var s = a.splice(1, { valueOf: function () { a.push(4); return 1; } });
r.push(J(s), J(a));
var t = [1, 2, 3, 4].slice({ valueOf: function () { return 1; } }, { valueOf: function () { return -1; } });
r.push(J(t));
r.join("|")"#,
        r#"[2]|[1,3]|[2,3]"#,
    ),
    (
        r#"var a = [5, 6, 7];
var r = [];
r.push(a.at({ valueOf: function () { a.length = 1; return 0; } }), a.length);
r.push(J([1, 2, 3].fill({ valueOf: function () { return 9; } }, { valueOf: function () { return 1; } })));
r.join("|")"#,
        r#"5|1|[1,{},{}]"#,
    ),
    (
        r#"var a = [1, 2];
a.length = 0;
for (var i = 0; i < 20; i++) a.push(i);
a.length = 5;
var b = a.concat(a);
J(b) + "|" + b.length"#,
        r#"[0,1,2,3,4,0,1,2,3,4]|10"#,
    ),
    (
        r#"var x = [];
x[3] = 1;
x.length = 2;
var y = [1, 2, 3];
y.length = 2;
y[5] = 5;
J(x) + "|" + x.length + "|" + J(y) + "|" + y.length
"#,
        r#"[_,_]|2|[1,2,_,_,_,5]|6"#,
    ),
];

pub const REPRESENTATION: &[(&str, &str)] = &[
    (
        r#"var a = [];"#,
        r#"dense(len=0,cap=0,length=0,obj=0,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [1, , 3];"#,
        r#"dense(len=3,cap=3,length=3,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[4096] = 1;"#,
        r#"dense(len=4097,cap=4097,length=4097,obj=1,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[4097] = 1;"#,
        r#"sparse(items=1,length=4098,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[20470] = 5;"#,
        r#"sparse(items=1,length=20471,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a.push(i);"#,
        r#"dense(len=5000,cap=5000,length=5000,obj=5000,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a.push(i);
a.length = 0;
a[6000] = 1;"#,
        r#"dense(len=6001,cap=10000,length=6001,obj=5001,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a.push(i);
a.length = 0;
a[6200] = 1;"#,
        r#"dense(len=6201,cap=10000,length=6201,obj=5001,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a[i] = i;
a.length = 2;
a[4500] = 1;"#,
        r#"dense(len=4501,cap=5000,length=4501,obj=5001,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a[i] = i;
a.length = 2;"#,
        r#"dense(len=2,cap=5000,length=2,obj=5000,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a[i] = i;
a.length = 1500;"#,
        r#"dense(len=1500,cap=5000,length=1500,obj=5000,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 5000; i++) a[i] = i;
a.length = 17;"#,
        r#"dense(len=17,cap=17,length=17,obj=5000,pvc=0)"#,
    ),
    (
        r#"var a = [];
a.length = 10000;
a[5000] = 1;"#,
        r#"sparse(items=1,length=10000,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a[50000] = 1;"#,
        r#"sparse(items=4,length=50001,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 500; i++) a[i] = i;
a[5001] = 1;"#,
        r#"dense(len=5002,cap=5002,length=5002,obj=501,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 500; i++) a[i] = i;
a[5000] = 1;"#,
        r#"dense(len=5001,cap=5001,length=5001,obj=501,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
for (var i = 0; i < 1100; i++) a[i] = i;"#,
        r#"sparse(items=1101,length=100001,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[10000] = 1;
for (var i = 0; i < 1300; i++) a[i] = i;"#,
        r#"dense(len=10001,cap=10001,length=10001,obj=1301,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[10000] = 1;
for (var i = 0; i < 1250; i++) a[i] = i;"#,
        r#"sparse(items=1251,length=10001,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[10000] = 1;
for (var i = 0; i < 1260; i++) Object.defineProperty(a, i, { value: i, writable: true, enumerable: true, configurable: true });"#,
        r#"dense(len=10001,cap=10001,length=10001,obj=1260,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[10000] = 1;
for (var i = 0; i < 1260; i++) Object.defineProperty(a, i, { value: i });"#,
        r#"dense(len=10001,cap=10001,length=10001,obj=1260,pvc=1259)"#,
    ),
    (
        r#"var a = [1, 2, 3].map(function (x) { return x; });"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [1, , 3].map(function (x) { return x; });"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3, 4];
a.pop();"#,
        r#"dense(len=3,cap=4,length=3,obj=4,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3, 4];
a.pop();
a.push(9);"#,
        r#"dense(len=4,cap=8,length=4,obj=5,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 0, { value: 9 });"#,
        r#"dense(len=3,cap=3,length=3,obj=4,pvc=1)"#,
    ),
    (
        r#"var a = [1, 2, 3];
Object.defineProperty(a, 0, { value: 9, writable: true, enumerable: true, configurable: true });"#,
        r#"dense(len=3,cap=3,length=3,obj=4,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
delete a[1];"#,
        r#"dense(len=3,cap=3,length=3,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.splice(1, 1);"#,
        r#"dense(len=2,cap=3,length=2,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.splice(1, 0, 7, 8);"#,
        r#"dense(len=5,cap=5,length=5,obj=5,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.unshift(0);"#,
        r#"dense(len=4,cap=6,length=4,obj=4,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.shift();"#,
        r#"dense(len=2,cap=3,length=2,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3, 4, 5, 6].filter(function (x) { return x % 2; });"#,
        r#"dense(len=3,cap=4,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = Array.from([1, 2, 3]);"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = Array.from({ length: 3 });"#,
        r#"dense(len=3,cap=4,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = Array.of(1, 2);"#,
        r#"dense(len=2,cap=2,length=2,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2].concat([3, 4]);"#,
        r#"dense(len=4,cap=8,length=4,obj=4,pvc=0)"#,
    ),
    (
        r#"var a = [3, 1, 2].slice(1);"#,
        r#"dense(len=2,cap=2,length=2,obj=2,pvc=0)"#,
    ),
    (
        r#"var a = [3, 1, 2].toSorted();"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [1, [2, 3]].flat();"#,
        r#"dense(len=3,cap=4,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = new Array(5);"#,
        r#"dense(len=0,cap=0,length=5,obj=0,pvc=0)"#,
    ),
    (
        r#"var a = new Array(1, 2, 3);"#,
        r#"dense(len=3,cap=3,length=3,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [];
a[100000] = 1;
a.length = 0;
a.push(1);"#,
        r#"sparse(items=1,length=1,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 2000; i++) a[i * 7] = i;"#,
        r#"dense(len=13994,cap=15257,length=13994,obj=2000,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 2000; i++) a[i * 9] = i;"#,
        r#"dense(len=17992,cap=18618,length=17992,obj=2000,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 1999; i >= 0; i--) a[i * 7] = i;"#,
        r#"dense(len=13994,cap=13994,length=13994,obj=2000,pvc=0)"#,
    ),
    (
        r#"var a = [0];
a[4097] = 1;"#,
        r#"sparse(items=2,length=4098,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 1000; i++) a.push(i);
a.splice(10, 0, 1, 2, 3);"#,
        r#"dense(len=1003,cap=1024,length=1003,obj=1003,pvc=0)"#,
    ),
    (
        r#"var a = [1, 2, 3];
a.length = 100;"#,
        r#"dense(len=3,cap=3,length=100,obj=3,pvc=0)"#,
    ),
    (
        r#"var a = [];
var b = [];
for (var i = 0; i < 300; i++) b.push(i);
a = b.slice();
a.push(1);"#,
        r#"dense(len=301,cap=512,length=301,obj=301,pvc=0)"#,
    ),
    (
        r#"var b = [];
for (var i = 0; i < 300; i++) b.push(i);
var a = b.filter(function () { return true; });"#,
        r#"dense(len=300,cap=303,length=300,obj=300,pvc=0)"#,
    ),
    (
        r#"var a = [];
for (var i = 0; i < 20; i++) a.push(i);
a.length = 16;
a.length = 4;
"#,
        r#"dense(len=4,cap=32,length=4,obj=20,pvc=0)"#,
    ),
];
