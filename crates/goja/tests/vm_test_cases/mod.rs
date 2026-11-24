pub const ORACLE_CASES: &[(&str, &str)] = &[
    (r#"1 + 2 * 3 - 4 / 2"#, r#"RESULT goja.valueInt "5""#),
    (
        r#"var a = 5; var b = "x"; a + b + (a - 1) + typeof a + typeof b + typeof null + typeof undefined + typeof {} + typeof function(){}"#,
        r#"RESULT goja.asciiString "5x4numberstringobjectundefinedobjectfunction""#,
    ),
    (
        r#"function f(x, y) { return x * y + arguments.length; } f(3, 4, 5)"#,
        r#"RESULT goja.valueInt "15""#,
    ),
    (
        r#"function f(x) { arguments[0] = 10; return x; } f(1)"#,
        r#"RESULT goja.valueInt "10""#,
    ),
    (
        r#"function f(x) { "use strict"; arguments[0] = 10; return x; } f(1)"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"function f(a, ...rest) { return rest; } typeof f(1,2,3)"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"var s = ""; for (var i = 0; i < 10; i++) { if (i % 2) continue; if (i > 7) break; s += i; } s"#,
        r#"RESULT goja.asciiString "0246""#,
    ),
    (
        r#"outer: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { if (j == 1) continue outer; if (i == 2) break outer; } } i + "-" + j"#,
        r#"RESULT goja.asciiString "2-0""#,
    ),
    (
        r#"function f() { try { throw new Error("boom"); } catch (e) { return e.message; } finally { } } f()"#,
        r#"RESULT goja.asciiString "boom""#,
    ),
    (
        r#"function f() { try { return 1; } finally { return 2; } } f()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function f() { var r = ""; try { try { throw 1; } finally { r += "a"; } } catch (e) { r += "b" + e; } return r; } f()"#,
        r#"RESULT goja.asciiString "ab1""#,
    ),
    (
        r#"function f() { throw new TypeError("x"); } function g() { f(); } g()"#,
        r#"EXCEPTION "TypeError: x\n\tat f (<eval>:1:22(3))\n\tat g (<eval>:1:60(2))\n\tat <eval>:1:67(4)\n"
ERROR "TypeError: x at f (<eval>:1:22(3))""#,
    ),
    (
        r#"function f() { null.x; } f()"#,
        r#"EXCEPTION "TypeError: Cannot read property 'x' of undefined\n\tat f (<eval>:1:21(2))\n\tat <eval>:1:27(3)\n"
ERROR "TypeError: Cannot read property 'x' of undefined at f (<eval>:1:21(2))""#,
    ),
    (
        r#"var o = {get x() { return 42; }, set x(v) { this.y = v; }}; o.x = 5; o.x + o.y"#,
        r#"RESULT goja.valueInt "47""#,
    ),
    (
        r#"class A { constructor(x) { this.x = x; } get double() { return this.x * 2; } static make() { return new A(3); } } A.make().double"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"class A { #p = 1; #m() { return this.#p + 1; } get v() { return this.#m(); } static #s = 10; static s() { return A.#s; } } new A().v + A.s()"#,
        r#"RESULT goja.valueInt "12""#,
    ),
    (
        r#"class A { constructor() { this.a = 1; } m() { return "A"; } } class B extends A { constructor() { super(); this.b = 2; } m() { return super.m() + "B"; } } var b = new B(); b.m() + b.a + b.b"#,
        r#"RESULT goja.asciiString "AB12""#,
    ),
    (
        r#"class A { x = 1; } class B extends A { y = this.x + 1; } new B().y"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function* g() { var x = yield 1; yield x + 1; return 5; } var it = g(); var r1 = it.next(); var r2 = it.next(10); var r3 = it.next(); var r4 = it.next(); "" + r1.value + r1.done + r2.value + r3.value + r3.done + r4.value + r4.done"#,
        r#"RESULT goja.asciiString "1false115trueundefinedtrue""#,
    ),
    (
        r#"function* g() { try { yield 1; } catch (e) { yield "caught " + e; } } var it = g(); it.next(); it.throw("x").value"#,
        r#"RESULT goja.asciiString "caught x""#,
    ),
    (
        r#"function* inner() { yield 1; yield 2; return 3; } function* outer() { var r = yield* inner(); yield r; } var it = outer(); var s = ""; for (var v of it) s += v; s"#,
        r#"RESULT goja.asciiString "123""#,
    ),
    (
        r#"var iterable = { [Symbol.iterator]() { var i = 0; return { next() { i++; return { value: i, done: i > 3 }; } }; } }; var s = 0; for (var v of iterable) s += v; s"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var {a: {b: c = 5} = {}} = {}; c"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function f() { var x = 1; return eval("x + 1"); } f()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var x = 10; function f() { var x = 1; return (0, eval)("x"); } f()"#,
        r#"RESULT goja.valueInt "10""#,
    ),
    (
        r#"var o = {a: 1}; with (o) { a = 2; } o.a"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var o = {a: {b: null}}; (o?.a?.b?.c ?? "def") + (o.x?.y)"#,
        r#"RESULT goja.asciiString "defundefined""#,
    ),
    (
        r#"function F() { return new.target === F; } new F() + "," + F()"#,
        r#"RESULT goja.asciiString "[object Object],false""#,
    ),
    (
        r#"var s = Symbol("desc"); var o = {[s]: 1}; o[s] + s.toString() + s.description"#,
        r#"RESULT goja.asciiString "1Symbol(desc)desc""#,
    ),
    (r#"`a${1+1}b${"c"}`"#, r#"RESULT goja.asciiString "a2bc""#),
    (
        r#"function f(a, b, c) { return a + b + c; } var it = { [Symbol.iterator]() { var i = 0; return { next() { i++; return { value: i, done: i > 3 }; } }; } }; f(...it)"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var o = Object.create(null); o.x = 1; Object.getPrototypeOf(o) === null"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#""use strict"; undeclared = 5"#,
        r#"EXCEPTION "ReferenceError: undeclared is not defined\n\tat <eval>:1:15(2)\n"
ERROR "ReferenceError: undeclared is not defined at <eval>:1:15(2)""#,
    ),
    (
        r#"const c = 1; c = 2"#,
        r#"EXCEPTION "TypeError: Assignment to constant variable.\n\tat <eval>:1:14(5)\n"
ERROR "TypeError: Assignment to constant variable. at <eval>:1:14(5)""#,
    ),
    (
        r#"let x = x + 1"#,
        r#"EXCEPTION "ReferenceError: Cannot access a variable before initialization\n\tat <eval>:1:9(1)\n"
ERROR "ReferenceError: Cannot access a variable before initialization at <eval>:1:9(1)""#,
    ),
    (
        r#"function f() { return this; } typeof f()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"var o = { f() { return this; } }; o.f() === o"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function() { "use strict"; return typeof this; })()"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (r#"var x = 1; delete x"#, r#"RESULT goja.valueBool "false""#),
    (
        r#"var o = {a:1}; delete o.a; "a" in o"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (r#"void 0"#, r#"RESULT goja.valueUndefined "undefined""#),
    (r#"1n + 2n"#, r#"RESULT *goja.valueBigInt "3""#),
    (
        r#"1n + 1"#,
        r#"EXCEPTION "TypeError: Cannot mix BigInt and other types, use explicit conversions\n\tat <eval>:1:1(2)\n"
ERROR "TypeError: Cannot mix BigInt and other types, use explicit conversions at <eval>:1:1(2)""#,
    ),
    (
        r#"var x = 0; x++; x++; x--; ++x; x"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (r#""abc" < "abd""#, r#"RESULT goja.valueBool "true""#),
    (
        r#"({}) instanceof Object"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"undefined.foo()"#,
        r#"EXCEPTION "TypeError: Cannot read property 'foo' of undefined or null\n\tat <eval>:1:1(1)\n"
ERROR "TypeError: Cannot read property 'foo' of undefined or null at <eval>:1:1(1)""#,
    ),
    (
        r#"var o = {}; o.foo()"#,
        r#"EXCEPTION "TypeError: Object has no member 'foo'\n\tat <eval>:1:18(6)\n"
ERROR "TypeError: Object has no member 'foo' at <eval>:1:18(6)""#,
    ),
    (
        r#"var o = {}; o["foo"]()"#,
        r#"EXCEPTION "TypeError: Object has no member 'foo'\n\tat <eval>:1:21(6)\n"
ERROR "TypeError: Object has no member 'foo' at <eval>:1:21(6)""#,
    ),
    (
        r#"x()"#,
        r#"EXCEPTION "ReferenceError: x is not defined\n\tat <eval>:1:2(1)\n"
ERROR "ReferenceError: x is not defined at <eval>:1:2(1)""#,
    ),
    (
        r#"Symbol() + """#,
        r#"EXCEPTION "TypeError: Cannot convert a Symbol value to a string\n\tat <eval>:1:1(3)\n"
ERROR "TypeError: Cannot convert a Symbol value to a string at <eval>:1:1(3)""#,
    ),
    (
        r#"new (function() {}).x"#,
        r#"EXCEPTION "TypeError: Value is not an object: undefined\n\tat <eval>:1:1(2)\n"
ERROR "TypeError: Value is not an object: undefined at <eval>:1:1(2)""#,
    ),
    (
        r#"new 5"#,
        r#"EXCEPTION "TypeError: Value is not an object: 5\n\tat <eval>:1:1(1)\n"
ERROR "TypeError: Value is not an object: 5 at <eval>:1:1(1)""#,
    ),
    (
        r#"class A {} A()"#,
        r#"EXCEPTION "TypeError: Class constructor cannot be invoked without 'new'\n\tat <eval>:1:13(6)\n"
ERROR "TypeError: Class constructor cannot be invoked without 'new' at <eval>:1:13(6)""#,
    ),
    (
        r#"class A extends null {} typeof A"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"class B extends 5 {}"#,
        r#"EXCEPTION "TypeError: Class extends value is not a constructor or null\n\tat <eval>:1:1(3)\n"
ERROR "TypeError: Class extends value is not a constructor or null at <eval>:1:1(3)""#,
    ),
    (
        r#"function f() { return f.caller; } f()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var e = new Error("m"); e.name + ": " + e.message"#,
        r#"RESULT goja.asciiString "Error: m""#,
    ),
    (
        r#"try { throw {a: 1} } catch ({a}) { a }"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"switch (3) { case 1: "a"; break; case 3: "c"; default: "d" }"#,
        r#"RESULT goja.asciiString "d""#,
    ),
    (
        r#"do { var k = 1; } while (false); k"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var n = 0; while (n < 5) n++; n"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"var o = {valueOf() { return 42; }}; o + 1"#,
        r#"RESULT goja.valueInt "43""#,
    ),
    (
        r#"var o = {toString() { return "s"; }}; o + "!""#,
        r#"RESULT goja.asciiString "s!""#,
    ),
    (
        r#"(function f(n) { return n ? f(n - 1) + 1 : 0; })(100)"#,
        r#"RESULT goja.valueInt "100""#,
    ),
    (
        r#"function f() { return arguments; } var a = f(1, 2); a.length + a[0] + a[1]"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function f() { "use strict"; return arguments.callee; } f()"#,
        r#"EXCEPTION "TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them\n\tat f (<eval>:1:47(4))\n\tat <eval>:1:58(3)\n"
ERROR "TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them at f (<eval>:1:47(4))""#,
    ),
    (
        r#"var getter = Object.getOwnPropertyDescriptor(function(){ "use strict"; return arguments; }(), "callee"); typeof getter.get"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"label: { break label; }"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var c = 0; for (let i = 0; i < 3; i++) { setTimeout = function() {}; c += i; } c"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var fns = {}; for (let i = 0; i < 3; i++) { fns[i] = () => i; } fns[0]() + fns[1]() + fns[2]()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(() => { return this; })() === this"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = { m() { return () => this; } }; o.m()() === o"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function Outer() { this.v = 1; var self = this; return { f: () => this.v } } new Outer().f()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A { static #x = 1; static has(o) { return #x in o; } } A.has(A) + "," + A.has({})"#,
        r#"RESULT goja.asciiString "true,false""#,
    ),
    (
        r#"class A { #x; constructor() { this.#x = 1; } static get(o) { return o.#x; } } A.get({})"#,
        r#"EXCEPTION "TypeError: Cannot read private member #x from an object whose class did not declare it\n\tat get (<eval>:1:71(2))\n\tat <eval>:1:84(11)\n"
ERROR "TypeError: Cannot read private member #x from an object whose class did not declare it at get (<eval>:1:71(2))""#,
    ),
    (
        r#"class A { get #g() { return 1; } set(v) { this.#g = v; } } new A().set(2)"#,
        r#"EXCEPTION "TypeError: Cannot assign to read only property '#g'\n\tat set (<eval>:1:48(3))\n\tat <eval>:1:71(16)\n"
ERROR "TypeError: Cannot assign to read only property '#g' at set (<eval>:1:48(3))""#,
    ),
    (
        r#"class A { #m() {} set() { this.#m = 1; } } new A().set()"#,
        r#"EXCEPTION "TypeError: Private method '#m' is not writable\n\tat set (<eval>:1:32(3))\n\tat <eval>:1:55(15)\n"
ERROR "TypeError: Private method '#m' is not writable at set (<eval>:1:32(3))""#,
    ),
    (
        r#"function f() { for (var i = 0; i < 3; i++) { try { if (i == 1) return "r" + i; } finally { if (i == 1) continue; } } return "end"; } f()"#,
        r#"RESULT goja.asciiString "end""#,
    ),
    (
        r#"function f() { var s = ""; for (var i = 0; i < 3; i++) { try { s += i; continue; } finally { s += "f"; } } return s; } f()"#,
        r#"RESULT goja.asciiString "0f1f2f""#,
    ),
    (
        r#"function f() { L: try { break L; } finally { return "fin"; } } f()"#,
        r#"RESULT goja.asciiString "fin""#,
    ),
    (
        r#"function* g() { var i = 0; while (true) { try { yield i++; } finally { if (i > 2) return; } } } var s = ""; for (var v of g()) { s += v; if (v == 4) break; } s"#,
        r#"RESULT goja.asciiString "012""#,
    ),
    (
        r#"function* g() { yield 1; } var it = g(); it.return(5).value + "," + it.next().done"#,
        r#"RESULT goja.asciiString "5,true""#,
    ),
    (
        r#"function* g() { yield 1; } var it = g(); try { it.throw(new Error("e")); } catch (e) { e.message + it.next().done }"#,
        r#"RESULT goja.asciiString "etrue""#,
    ),
    (
        r#"function* g() { yield* { [Symbol.iterator]() { return { next(v) { return { value: v, done: false }; }, return(v) { return { value: "ret" + v, done: true }; } }; } }; } var it = g(); it.next(); it.next(1).value + it.return(9).value"#,
        r#"RESULT goja.asciiString "1ret9""#,
    ),
    (
        r#"function* g() { yield* { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; } }; } }; } var it = g(); it.next(); try { it.throw(5) } catch (e) { e.message }"#,
        r#"RESULT goja.asciiString "The iterator does not provide a 'throw' method""#,
    ),
    (
        r#"function* g() { const x = yield; return x * 2; } var it = g(); it.next(); it.next(21).value"#,
        r#"RESULT goja.valueInt "42""#,
    ),
    (
        r#"function* g() {} var it = g(); typeof it.next + Object.getPrototypeOf(it) === g.prototype"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"function* g() { yield this; } var o = { g }; o.g().next().value === o"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var it = (function*() { var r = yield 1; try { yield r; } catch (e) { yield "c:" + e; } })(); it.next(); it.next("x"); it.throw("t").value"#,
        r#"RESULT goja.asciiString "c:t""#,
    ),
    (
        r#"function* fib() { var a = 0, b = 1; while (true) { yield a; [a, b] = [b, a + b]; } } typeof fib"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"function* g() { yield 1; } var it = g(); it.next(); it.next(); it.next().done"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function* g() { var it2 = yield; it2.next(); } var it = g(); it.next(); try { it.next(it); } catch (e) { e.message }"#,
        r#"RESULT goja.asciiString "Illegal generator state""#,
    ),
    (
        r#"class A { static x = 1; static y = A.x + 1; } A.y"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class A { static { this.z = 3; } } A.z"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"class A { ["computed" + 1]() { return 1; } static ["s" + 2] = 5; } new A().computed1() + A.s2"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var C = class Named { who() { return Named.name; } }; new C().who()"#,
        r#"RESULT goja.asciiString "Named""#,
    ),
    (
        r#"class A { constructor() { return {o: 1}; } } class B extends A { constructor() { super(); } } new B().o"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A {} class B extends A { constructor() { } } new B()"#,
        r#"EXCEPTION "ReferenceError: Must call super constructor in derived class before returning from derived constructor\n\tat <eval>:1:52(11)\n"
ERROR "ReferenceError: Must call super constructor in derived class before returning from derived constructor at <eval>:1:52(11)""#,
    ),
    (
        r#"class A {} class B extends A { constructor() { super(); super(); } } new B()"#,
        r#"EXCEPTION "ReferenceError: Super constructor may only be called once\n\tat <eval>:1:62(5)\n\tat <eval>:1:70(11)\n"
ERROR "ReferenceError: Super constructor may only be called once at <eval>:1:62(5)""#,
    ),
    (
        r#"class A {} class B extends A { constructor() { this.x = 1; } } new B()"#,
        r#"EXCEPTION "ReferenceError: Must call super constructor in derived class before accessing 'this'\n\tat <eval>:1:48(1)\n\tat <eval>:1:64(11)\n"
ERROR "ReferenceError: Must call super constructor in derived class before accessing 'this' at <eval>:1:48(1)""#,
    ),
    (
        r#"class A { #x = 1; static check(o) { return o.#x; } } class B extends A { #y = 2; } A.check(new B())"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A { m() { return "A.m"; } static sm() { return "A.sm"; } } class B extends A { m() { var f = () => super.m(); return f(); } static sm() { return super.sm() + "!"; } } new B().m() + B.sm()"#,
        r#"RESULT goja.asciiString "A.mA.sm!""#,
    ),
    (
        r#"var o = { __proto__: { m() { return "proto"; } }, m() { return super.m() + "!"; } }; o.m()"#,
        r#"RESULT goja.asciiString "proto!""#,
    ),
    (
        r#"class A { constructor() { this.t = new.target.name; } } class B extends A {} new B().t"#,
        r#"RESULT goja.asciiString "B""#,
    ),
    (
        r#"function F() { if (!new.target) return "no"; return "yes"; } F() + new F()"#,
        r#"RESULT goja.asciiString "no[object Object]""#,
    ),
    (
        r#"class Counter { #c = 0; inc() { return ++this.#c; } } var c = new Counter(); c.inc(); c.inc()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class A { #a = 1; get a() { return this.#a; } set a(v) { this.#a = v; } } var x = new A(); x.a = 5; x.a"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"class A { static #priv() { return 7; } static call() { return A.#priv(); } } A.call()"#,
        r#"RESULT goja.valueInt "7""#,
    ),
    (
        r#"class A { #x = 1; m() { return eval("this.#x"); } } new A().m()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {a: 1, b: {c: 2}}; var {b: {c}} = o; c"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var {x = 1, y = x + 1} = {}; x + y"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"function f({a, b} = {a: 1, b: 2}) { return a + b; } f() + f({a: 3, b: 4})"#,
        r#"RESULT goja.valueInt "10""#,
    ),
    (
        r#"function f(a = 1, b = a + 1) { return a * b; } f() + f(2) + f(2, 5)"#,
        r#"RESULT goja.valueInt "18""#,
    ),
    (
        r#"function f(a, b = () => a) { var a = 5; return b(); } f(1)"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var {a, ...r} = Object.create({inh: 1}, {own: {value: 2, enumerable: true}}); a === undefined && r.own === 2 && r.inh === undefined"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var src = {a: 1, b: 2}; var {...copy} = src; copy !== src && copy.a === 1 && copy.b === 2"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = {x: 1, ...{y: 2, z: 3}}; o.x + o.y + o.z"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var o = {...null, ...undefined, a: 1}; o.a"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var t = 0; var o = {get g() { t++; return 1; }}; var c = {...o}; t + c.g"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"eval("var evalVar = 5"); evalVar"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function f() { eval("var inner = 1"); return inner; } f()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"function f() { "use strict"; eval("var inner2 = 1"); return typeof inner2; } f()"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"eval("let lex = 1"); typeof lex"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"eval("function evalFn() { return 3; }"); evalFn()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"eval("syntax error here")"#,
        r#"EXCEPTION "SyntaxError: SyntaxError: <eval>: Line 1:8 Unexpected identifier\n\tat <eval>:1:5(2)\n\tat <eval>:1:5(2)\n"
ERROR "SyntaxError: SyntaxError: <eval>: Line 1:8 Unexpected identifier at <eval>:1:5(2)""#,
    ),
    (
        r#"eval("throw new Error('from eval')")"#,
        r#"EXCEPTION "Error: from eval\n\tat <eval>:1:7(2)\n\tat <eval>:1:5(2)\n"
ERROR "Error: from eval at <eval>:1:7(2)""#,
    ),
    (
        r#"function f() { return eval("arguments.length"); } f(1, 2, 3)"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"function f() { return eval("this"); } f.call({v: 1}).v"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (r#"eval()"#, r#"RESULT goja.valueUndefined "undefined""#),
    (r#"eval(5)"#, r#"RESULT goja.valueInt "5""#),
    (r#"var e = eval; e("1 + 1")"#, r#"RESULT goja.valueInt "2""#),
    (
        r#"var o = {}; with (o) { var w = 1; } w + "," + o.w"#,
        r#"RESULT goja.asciiString "1,undefined""#,
    ),
    (
        r#"var o = {p: 1}; with (o) { p = 2; var q = p; } q + "," + o.p"#,
        r#"RESULT goja.asciiString "2,2""#,
    ),
    (
        r#"var o = {[Symbol.unscopables]: {u: true}, u: "hidden"}; var u = "visible"; with (o) { u }"#,
        r#"RESULT goja.asciiString "visible""#,
    ),
    (
        r#"function f() { with ({}) { return typeof arguments; } } f()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"var x = 1; { let x = 2; { let x = 3; } } x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"let a = 1; { a = 2; let b = a; } a"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"{ function blockFn() { return 1; } } blockFn()"#,
        r#"EXCEPTION "ReferenceError: blockFn is not defined\n\tat <eval>:1:45(5)\n"
ERROR "ReferenceError: blockFn is not defined at <eval>:1:45(5)""#,
    ),
    (
        r#"switch (1) { case 1: let z = 5; z; }"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"try { (function() { "use strict"; delete Object.prototype; })(); } catch (e) { e.message }"#,
        r#"RESULT goja.asciiString "Cannot delete property 'prototype' of [object Function]""#,
    ),
    (
        r#"(function() { "use strict"; var o = Object.freeze({a: 1}); o.a = 2; })()"#,
        r#"EXCEPTION "TypeError: Cannot assign to read only property 'a'\n\tat <eval>:1:62(10)\n\tat <eval>:1:71(2)\n"
ERROR "TypeError: Cannot assign to read only property 'a' at <eval>:1:62(10)""#,
    ),
    (
        r#"(function() { var o = Object.freeze({a: 1}); o.a = 2; return o.a; })()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"Object.defineProperty({}, "x", {get: 5})"#,
        r#"EXCEPTION "TypeError: Value is not an object: 5\n\tat defineProperty (native)\n\tat <eval>:1:22(7)\n"
ERROR "TypeError: Value is not an object: 5 at defineProperty (native)""#,
    ),
    (
        r#"var o = {}; Object.defineProperty(o, "x", {value: 1}); o.x = 2; o.x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (r#"true + 1"#, r#"RESULT goja.valueInt "2""#),
    (r#"null + 1"#, r#"RESULT goja.valueInt "1""#),
    (r#"undefined + 1"#, r#"RESULT goja.valueFloat "NaN""#),
    (r#""5" * "2""#, r#"RESULT goja.valueInt "10""#),
    (r#""5" - - "2""#, r#"RESULT goja.valueInt "7""#),
    (
        r#"1 / 0 + "," + -1 / 0 + "," + 0 / 0"#,
        r#"RESULT goja.asciiString "Infinity,-Infinity,NaN""#,
    ),
    (r#"-0 === 0"#, r#"RESULT goja.valueBool "true""#),
    (r#"Object.is(-0, 0)"#, r#"RESULT goja.valueBool "false""#),
    (
        r#"5 % 3 + "," + -5 % 3 + "," + 5.5 % 2"#,
        r#"RESULT goja.asciiString "2,-2,1.5""#,
    ),
    (r#"2 ** 10"#, r#"RESULT goja.valueInt "1024""#),
    (r#"(-2) ** 2"#, r#"RESULT goja.valueInt "4""#),
    (r#"1 << 31"#, r#"RESULT goja.valueInt "-2147483648""#),
    (r#"-1 >>> 0"#, r#"RESULT goja.valueInt "4294967295""#),
    (r#"-8 >> 1"#, r#"RESULT goja.valueInt "-4""#),
    (r#"~5"#, r#"RESULT goja.valueInt "-6""#),
    (
        r#"!!"" + "," + !!"a""#,
        r#"RESULT goja.asciiString "false,true""#,
    ),
    (r#"null == undefined"#, r#"RESULT goja.valueBool "true""#),
    (r#""1" == 1"#, r#"RESULT goja.valueBool "true""#),
    (r#"NaN == NaN"#, r#"RESULT goja.valueBool "false""#),
    (
        r#"({}) == "[object Object]""#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var i = 0; i += 5; i -= 2; i *= 3; i /= 9; i"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var s = "a"; s += 1; s"#,
        r#"RESULT goja.asciiString "a1""#,
    ),
    (
        r#"var u; u ??= 5; var v = 1; v ||= 7; var w = 1; w &&= 9; u + v + w"#,
        r#"RESULT goja.valueInt "15""#,
    ),
    (
        r#"typeof undeclaredVariable"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (r#"typeof typeof 1"#, r#"RESULT goja.asciiString "string""#),
    (
        r#"var o = {}; o.x?.(); "ok""#,
        r#"RESULT goja.asciiString "ok""#,
    ),
    (
        r#"var o = {f: null}; o.f?.()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var o = {}; o.method()"#,
        r#"EXCEPTION "TypeError: Object has no member 'method'\n\tat <eval>:1:21(6)\n"
ERROR "TypeError: Object has no member 'method' at <eval>:1:21(6)""#,
    ),
    (
        r#"var o = {f() { return this === o; }}; (o.f)()"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = {f() { return typeof this; }}; (0, o.f)()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"(function(){ "use strict"; return this; }).call(5)"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function f() {} f.prototype.constructor === f"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function(a, b) {}).length + (function(...a) {}).length + (function(a, b = 1) {}).length"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(function named() {}).name + (() => {}).name + ({m() {}}).m.name"#,
        r#"RESULT goja.asciiString "namedm""#,
    ),
    (
        r#"var anon = function() {}; anon.name"#,
        r#"RESULT goja.asciiString "anon""#,
    ),
    (
        r#"var arrow = () => {}; arrow.name"#,
        r#"RESULT goja.asciiString "arrow""#,
    ),
    (
        r#"var o = {get p() {}}; Object.getOwnPropertyDescriptor(o, "p").get.name"#,
        r#"RESULT goja.asciiString "get p""#,
    ),
    (
        r#"var o = {[Symbol.iterator]() {}}; o[Symbol.iterator].name"#,
        r#"RESULT goja.asciiString "[Symbol.iterator]""#,
    ),
    (r#"class K {} K.name"#, r#"RESULT goja.asciiString "K""#),
    (
        r#"(function() { return arguments.length; })(1, 2, 3)"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(function(a) { a = 2; return arguments[0]; })(1)"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"(function(a) { "use strict"; a = 2; return arguments[0]; })(1)"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"(function(a) { arguments[0] = 9; return a; })(1)"#,
        r#"RESULT goja.valueInt "9""#,
    ),
    (
        r#"(function(a) { delete arguments[0]; arguments[0] = 9; return a; })(1)"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"(function(a) { Object.defineProperty(arguments, "0", {value: 3}); return a; })(1)"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(function(a) { Object.defineProperty(arguments, "0", {value: 3, writable: false}); a = 4; return arguments[0]; })(1)"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(function() { var s = ""; for (var k in arguments) s += k; return s; })(1, 2)"#,
        r#"RESULT goja.asciiString "01""#,
    ),
    (
        r#"(function(a) { var s = ""; for (var k in arguments) s += k + arguments[k]; return s; })(1, 2)"#,
        r#"RESULT goja.asciiString "0112""#,
    ),
    (
        r#"(function(a) { return typeof arguments[Symbol.iterator]; })(1)"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"(function(a) { return Object.prototype.toString.call(arguments); })(1)"#,
        r#"RESULT goja.asciiString "[object Arguments]""#,
    ),
    (
        r#"(function(a) { "use strict"; return Object.prototype.toString.call(arguments); })(1)"#,
        r#"RESULT goja.asciiString "[object Arguments]""#,
    ),
    (
        r#"(function(a) { return arguments.callee.name; })(1)"#,
        r#"RESULT goja.asciiString """#,
    ),
    (
        r#"(() => arguments)()"#,
        r#"EXCEPTION "ReferenceError: arguments is not defined\n\tat <eval>:1:8(1)\n\tat <eval>:1:18(2)\n"
ERROR "ReferenceError: arguments is not defined at <eval>:1:8(1)""#,
    ),
    (
        r#"function f() { null.x } f.call()"#,
        r#"EXCEPTION "TypeError: Cannot read property 'x' of undefined\n\tat f (<eval>:1:21(2))\n\tat call (native)\n\tat <eval>:1:31(4)\n"
ERROR "TypeError: Cannot read property 'x' of undefined at f (<eval>:1:21(2))""#,
    ),
    (
        r#"function f() { throw new Error("in f"); } function g() { return f.call(null); } g()"#,
        r#"EXCEPTION "Error: in f\n\tat f (<eval>:1:22(3))\n\tat call (native)\n\tat g (<eval>:1:71(4))\n\tat <eval>:1:82(4)\n"
ERROR "Error: in f at f (<eval>:1:22(3))""#,
    ),
    (
        r#"function f() { throw "plain"; } function g() { f(); } g()"#,
        r#"EXCEPTION "plain\n\tat f (<eval>:1:16(2))\n\tat g (<eval>:1:49(2))\n\tat <eval>:1:56(4)\n"
ERROR "plain at f (<eval>:1:16(2))""#,
    ),
    (
        r#"var o = { get x() { throw new TypeError("getter"); } }; function h() { return o.x; } h()"#,
        r#"EXCEPTION "TypeError: getter\n\tat x (<eval>:1:27(3))\n\tat h (<eval>:1:81(2))\n\tat <eval>:1:87(8)\n"
ERROR "TypeError: getter at x (<eval>:1:27(3))""#,
    ),
    (
        r#"function rec(n) { return n === 0 ? 0 : 1 + rec(n - 1); } rec(100000)"#,
        r#"RESULT goja.valueInt "100000""#,
    ),
    (
        r#"Function("a", "b", "return a + b")(2, 3)"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"new Function("return this")() === this"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"Function("syntax(")"#,
        r#"EXCEPTION "SyntaxError: SyntaxError: <eval>: Line 4:1 Unexpected token } (and 3 more errors)\n\tat Function (native)\n\tat Function (native)\n\tat <eval>:1:9(2)\n"
ERROR "SyntaxError: SyntaxError: <eval>: Line 4:1 Unexpected token } (and 3 more errors) at Function (native)""#,
    ),
    (
        r#"(function() { try { throw 1 } catch (e) { return typeof e } finally { } })()"#,
        r#"RESULT goja.asciiString "number""#,
    ),
    (
        r#"var log = ""; var it = { [Symbol.iterator]() { return { i: 0, next() { this.i++; return { value: this.i, done: this.i > 5 }; }, return() { log += "ret"; return {}; } }; } }; for (var v of it) { if (v == 2) break; } log"#,
        r#"RESULT goja.asciiString "ret""#,
    ),
    (
        r#"var log = ""; var it = { [Symbol.iterator]() { return { i: 0, next() { this.i++; return { value: this.i, done: this.i > 5 }; }, return() { log += "ret"; return {}; } }; } }; try { for (var v of it) { throw "x"; } } catch (e) { log += e } log"#,
        r#"RESULT goja.asciiString "retx""#,
    ),
    (
        r#"var it = { [Symbol.iterator]() { return { next() { throw new Error("next err"); } }; } }; for (var v of it) {}"#,
        r#"EXCEPTION "Error: next err\n\tat next (<eval>:1:58(3))\n\tat <eval>:1:105(12)\n"
ERROR "Error: next err at next (<eval>:1:58(3))""#,
    ),
    (
        r#"var it = { [Symbol.iterator]() { return { next() { return 5; } }; } }; for (var v of it) {}"#,
        r#"EXCEPTION "TypeError: Value is not an object: 5\n\tat <eval>:1:86(12)\n"
ERROR "TypeError: Value is not an object: 5 at <eval>:1:86(12)""#,
    ),
    (
        r#"var it = { [Symbol.iterator]() { return 5; } }; for (var v of it) {}"#,
        r#"EXCEPTION "TypeError: Value is not an object: 5\n\tat <eval>:1:63(10)\n"
ERROR "TypeError: Value is not an object: 5 at <eval>:1:63(10)""#,
    ),
    (
        r#"for (var v of {}) {}"#,
        r#"EXCEPTION "TypeError: object is not iterable\n\tat <eval>:1:15(2)\n"
ERROR "TypeError: object is not iterable at <eval>:1:15(2)""#,
    ),
    (
        r#"for (var v of 5) {}"#,
        r#"EXCEPTION "TypeError: object is not iterable\n\tat <eval>:1:1(2)\n"
ERROR "TypeError: object is not iterable at <eval>:1:1(2)""#,
    ),
    (
        r#"function f(a, b) { return a + "," + b; } var o = { [Symbol.iterator]() { var n = 0; return { next() { n++; return { done: n > 2, value: n }; } }; } }; f(...o, ...o)"#,
        r#"RESULT goja.asciiString "1,2""#,
    ),
    (
        r#"var s = 0; for (const x of { [Symbol.iterator]() { var n = 0; return { next() { n++; return { done: n > 3, value: n }; } }; } }) { s += x; } s"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var o = {}; for (o.k in {a: 1}); o.k"#,
        r#"RESULT goja.asciiString "a""#,
    ),
    (
        r#"var count = 0; for (var k in {a:1, b:2, c:3}) { count++; } count"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var p = {inh: 1}; var o = Object.create(p); o.own = 2; var s = ""; for (var k in o) s += k; s"#,
        r#"RESULT goja.asciiString "owninh""#,
    ),
    (
        r#"var o = {a: 1, b: 2}; var s = ""; for (var k in o) { delete o.b; s += k; } s"#,
        r#"RESULT goja.asciiString "a""#,
    ),
    (
        r#"var o = {}; Object.defineProperty(o, "hidden", {value: 1, enumerable: false}); o.vis = 2; var s = ""; for (var k in o) s += k; s"#,
        r#"RESULT goja.asciiString "vis""#,
    ),
    (
        r#"for (var k in null) {} "ok""#,
        r#"RESULT goja.asciiString "ok""#,
    ),
    (
        r#"var s = Symbol("x"); var o = {[s]: 1, a: 2}; var r = ""; for (var k in o) r += k; r"#,
        r#"RESULT goja.asciiString "a""#,
    ),
    (
        r#"function* g() { yield 1; yield 2; } var s = ""; for (var v of g()) s += v; s"#,
        r#"RESULT goja.asciiString "12""#,
    ),
    (
        r#"function* g() { try { yield 1; } finally { throw new Error("fin"); } } var it = g(); it.next(); it.return(1)"#,
        r#"EXCEPTION "Error: fin\n\tat g (<eval>:1:50(8))\n\tat return (native)\n\tat <eval>:1:106(13)\n"
ERROR "Error: fin at g (<eval>:1:50(8))""#,
    ),
    (
        r#"function* g() { yield 1; } var it = g(); it.next(); it.next(); it.throw(new Error("after done"))"#,
        r#"EXCEPTION "Error: after done\n\tat <eval>:1:73(18)\n"
ERROR "Error: after done at <eval>:1:73(18)""#,
    ),
    (
        r#"function* g() { it.next(); } var it = g(); it.next()"#,
        r#"EXCEPTION "TypeError: Illegal generator state\n\tat next (native)\n\tat g (<eval>:1:24(4))\n\tat next (native)\n\tat <eval>:1:51(8)\n"
ERROR "TypeError: Illegal generator state at next (native)""#,
    ),
    (
        r#"var genFn = function*(){}; Object.getPrototypeOf(genFn) === Object.getPrototypeOf(function*(){})"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function* g() {} new g()"#,
        r#"EXCEPTION "TypeError: Value is not a constructor\n\tat <eval>:1:18(3)\n"
ERROR "TypeError: Value is not a constructor at <eval>:1:18(3)""#,
    ),
    (
        r#"var o = { *gm() { yield 1; } }; o.gm().next().value"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class C { *gm() { yield this; } } var c = new C(); c.gm().next().value === c"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function* g(a, b = a * 2) { yield a + b; } g(5).next().value"#,
        r#"RESULT goja.valueInt "15""#,
    ),
    (
        r#"function* g() { var x = yield* (function*() { yield 1; return "inner"; })(); yield x; } var it = g(); it.next().value + it.next().value"#,
        r#"RESULT goja.asciiString "1inner""#,
    ),
    (
        r#"function* g() { yield* 5; } g().next()"#,
        r#"EXCEPTION "TypeError: object is not iterable\n\tat next (native)\n\tat <eval>:1:37(5)\n"
ERROR "TypeError: object is not iterable at next (native)""#,
    ),
    (
        r#"function* g() { return yield yield 1; } var it = g(); it.next().value + "," + it.next(2).value + "," + it.next(3).value + it.next().done"#,
        r#"RESULT goja.asciiString "1,2,3true""#,
    ),
    (
        r#"(function*() { yield 1; }).constructor.name"#,
        r#"RESULT goja.asciiString "GeneratorFunction""#,
    ),
    (
        r#"typeof Object.getPrototypeOf(function*(){}).prototype.next"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"var x = 0; function* g() { x++; yield; } var it = g(); x"#,
        r#"RESULT goja.valueInt "0""#,
    ),
    (
        r#"class A { static m() { return this; } } A.m() === A"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"class A { constructor() { this.f = () => this; } } var a = new A(); a.f() === a"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"class A { m() { return super.toString === Object.prototype.toString; } } new A().m()"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"class A extends Object { constructor() { super(); this.x = 1; } } new A().x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A extends Function {} typeof new A("return 1")"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"class A extends Error { constructor(m) { super(m); this.name = "AErr"; } } var e = new A("msg"); e.message + "," + (e instanceof Error) + "," + e.name"#,
        r#"RESULT goja.asciiString "msg,true,AErr""#,
    ),
    (
        r#"class A { constructor() { this.x = 1 } } class B extends A { constructor() { var f = () => super(); f(); } } new B().x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class B extends (class {}) { constructor() { return 5; } } new B()"#,
        r#"EXCEPTION "TypeError: Derived constructors may only return object or undefined\n\tat <eval>:1:60(9)\n"
ERROR "TypeError: Derived constructors may only return object or undefined at <eval>:1:60(9)""#,
    ),
    (
        r#"class B extends (class {}) { constructor() { return undefined; } } typeof new B()"#,
        r#"EXCEPTION "ReferenceError: Must call super constructor in derived class before returning from derived constructor\n\tat <eval>:1:75(9)\n"
ERROR "ReferenceError: Must call super constructor in derived class before returning from derived constructor at <eval>:1:75(9)""#,
    ),
    (
        r#"class A { #x = 1; static f(o) { o.#x = 2; } } A.f({})"#,
        r#"EXCEPTION "TypeError: Cannot write private member #x from an object whose class did not declare it\n\tat f (<eval>:1:35(3))\n\tat <eval>:1:50(11)\n"
ERROR "TypeError: Cannot write private member #x from an object whose class did not declare it at f (<eval>:1:35(3))""#,
    ),
    (
        r#"class A { #x; #y = this.#x + 1; static get(a) { return a.#y; } } A.get(new A())"#,
        r#"RESULT goja.valueFloat "NaN""#,
    ),
    (
        r#"class A { static #c = 0; static inc() { return ++A.#c; } } A.inc() + A.inc()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"class A { #f = 1; m() { const o = {f: () => this.#f}; return o.f(); } } new A().m()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A { #x = 1; static has(o) { try { o.#x; return true; } catch (e) { return e.constructor.name; } } } A.has({})"#,
        r#"RESULT goja.asciiString "TypeError""#,
    ),
    (
        r#"class A { a = this.constructor.name; } class B extends A {} new B().a"#,
        r#"RESULT goja.asciiString "B""#,
    ),
    (
        r#"class A { ["x"] = 1; [Symbol.iterator] = 2; } var a = new A(); a.x + a[Symbol.iterator]"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var k = "dyn"; class A { static [k] = 3; } A.dyn"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"class A { get [Symbol.toStringTag]() { return "Tagged"; } } Object.prototype.toString.call(new A())"#,
        r#"RESULT goja.asciiString "[object Tagged]""#,
    ),
    (
        r#"class A { static name() { return "fn"; } } typeof A.name"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"(class { constructor(a, b) {} }).length"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class A {} A.prototype.constructor === A && Object.getOwnPropertyDescriptor(A, "prototype").writable"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"class A { m() {} } Object.getOwnPropertyDescriptor(A.prototype, "m").enumerable"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"var C = class {}; C.name"#,
        r#"RESULT goja.asciiString "C""#,
    ),
    (
        r#"var o = { C: class {} }; o.C.name"#,
        r#"RESULT goja.asciiString "C""#,
    ),
    (
        r#"let D = class { static n = this.name; }; D.n"#,
        r#"RESULT goja.asciiString "D""#,
    ),
    (
        r#"(function() { "use strict"; return function() { return this; }(); })()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var a = 1; var b = a++ + ++a; a + "," + b"#,
        r#"RESULT goja.asciiString "3,4""#,
    ),
    (
        r#"var o = {n: 1}; o.n++; ++o.n; o.n-- + "," + o.n"#,
        r#"RESULT goja.asciiString "3,2""#,
    ),
    (
        r#"var o = {n: "5"}; o.n++; o.n"#,
        r#"RESULT goja.valueFloat "6""#,
    ),
    (
        r#"var x = "10"; x--; typeof x"#,
        r#"RESULT goja.asciiString "number""#,
    ),
    (
        r#"var o = {}; o[Symbol.toPrimitive] = function(h) { return h; }; `${o}` + (o + "") + (+{[Symbol.toPrimitive]() { return 3; }})"#,
        r#"RESULT goja.asciiString "stringdefault3""#,
    ),
    (
        r#"var n = 0; var o = { valueOf() { n++; return 1.5; } }; -o; n"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var n = 0; var o = { valueOf() { n++; return 1; } }; -o; n"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"1 < {valueOf() { return 2; }}"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (r#""10" < "9""#, r#"RESULT goja.valueBool "true""#),
    (r#"10 < "9""#, r#"RESULT goja.valueBool "false""#),
    (r#"1n < 2"#, r#"RESULT goja.valueBool "true""#),
    (r#"2n > "1""#, r#"RESULT goja.valueBool "true""#),
    (r#"undefined < 1"#, r#"RESULT goja.valueBool "false""#),
    (r#"null >= 0"#, r#"RESULT goja.valueBool "true""#),
    (r#"NaN <= NaN"#, r#"RESULT goja.valueBool "false""#),
    (
        r#"var a = {}; var b = {}; a < b"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"Symbol() < 1"#,
        r#"EXCEPTION "TypeError: Cannot convert a Symbol value to a number\n\tat <eval>:1:1(3)\n"
ERROR "TypeError: Cannot convert a Symbol value to a number at <eval>:1:1(3)""#,
    ),
    (
        r#"5n ** -1n"#,
        r#"EXCEPTION "RangeError: exponent must be positive\n\tat <eval>:1:1(2)\n"
ERROR "RangeError: exponent must be positive at <eval>:1:1(2)""#,
    ),
    (
        r#"5n / 0n"#,
        r#"EXCEPTION "RangeError: Division by zero\n\tat <eval>:1:1(2)\n"
ERROR "RangeError: Division by zero at <eval>:1:1(2)""#,
    ),
    (
        r#"5n % 0n"#,
        r#"EXCEPTION "RangeError: Division by zero\n\tat <eval>:1:1(2)\n"
ERROR "RangeError: Division by zero at <eval>:1:1(2)""#,
    ),
    (
        r#"1n >>> 1n"#,
        r#"EXCEPTION "TypeError: BigInts have no unsigned right shift, use >> instead\n\tat <eval>:1:1(2)\n"
ERROR "TypeError: BigInts have no unsigned right shift, use >> instead at <eval>:1:1(2)""#,
    ),
    (r#"-5n % 3n"#, r#"RESULT *goja.valueBigInt "-2""#),
    (r#"7n / 2n"#, r#"RESULT *goja.valueBigInt "3""#),
    (r#"1n << 3n"#, r#"RESULT *goja.valueBigInt "8""#),
    (r#"-1n >> 1n"#, r#"RESULT *goja.valueBigInt "-1""#),
    (r#"~1n"#, r#"RESULT *goja.valueBigInt "-2""#),
    (r#"-(1n)"#, r#"RESULT *goja.valueBigInt "-1""#),
    (r#"var b = 1n; b++; b"#, r#"RESULT *goja.valueBigInt "2""#),
    (r#"typeof 1n"#, r#"RESULT goja.asciiString "bigint""#),
    (r#"1n == 1"#, r#"RESULT goja.valueBool "true""#),
    (r#"1n === 1"#, r#"RESULT goja.valueBool "false""#),
    (r#""1" == 1n"#, r#"RESULT goja.valueBool "true""#),
    (
        r#"0.1 + 0.2"#,
        r#"RESULT goja.valueFloat "0.30000000000000004""#,
    ),
    (
        r#"9007199254740993"#,
        r#"RESULT goja.valueFloat "9007199254740992""#,
    ),
    (
        r#"2 ** 53 + 1"#,
        r#"RESULT goja.valueFloat "9007199254740992""#,
    ),
    (r#"1e21 + """#, r#"RESULT goja.asciiString "1e+21""#),
    (r#"-0 + """#, r#"RESULT goja.asciiString "0""#),
    (r#"1 / -0"#, r#"RESULT goja.valueFloat "-Infinity""#),
    (
        r#"(-0).toString"#,
        r#"RESULT *goja.Object "function toString() { [native code] }""#,
    ),
    (
        r#"var x = -0; x === 0 && 1 / x"#,
        r#"RESULT goja.valueFloat "-Infinity""#,
    ),
    (r#"0 * -1"#, r#"RESULT goja.valueFloat "0""#),
    (r#"-1 * 0"#, r#"RESULT goja.valueFloat "0""#),
    (r#"-5 % 5"#, r#"RESULT goja.valueFloat "0""#),
    (r#"5 % -5"#, r#"RESULT goja.valueInt "0""#),
    (r#"Infinity % 2"#, r#"RESULT goja.valueFloat "NaN""#),
    (r#"2 % Infinity"#, r#"RESULT goja.valueInt "2""#),
    (r#"(2 ** 31) | 0"#, r#"RESULT goja.valueInt "-2147483648""#),
    (r#"(2 ** 32 + 5) >>> 0"#, r#"RESULT goja.valueInt "5""#),
    (r#"1.9 | 0"#, r#"RESULT goja.valueInt "1""#),
    (r#"-1.9 | 0"#, r#"RESULT goja.valueInt "-1""#),
    (r#"1 << 32"#, r#"RESULT goja.valueInt "1""#),
    (r#"1 << -1"#, r#"RESULT goja.valueInt "-2147483648""#),
    (
        r#"var o = { v: 1, f() { return this.v; } }; with (o) { f() }"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = { v: 1, f() { return this === o; } }; with (o) { f() }"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function outer() { var x = 1; with ({}) { return function() { return x; }; } } outer()()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var x = 1; function f() { with ({x: 2}) { var x = 3; } return x; } f() + "," + x"#,
        r#"RESULT goja.asciiString "undefined,1""#,
    ),
    (
        r#"function f() { var fns = {}; for (let i = 0; i < 3; i++) { fns[i] = function() { return i; }; } return fns[0]() + fns[1]() + fns[2](); } f()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"function f() { var fns = {}; for (let i = 0; i < 3; i++) { fns[i] = () => i; i++; } return fns[0]() + "," + fns[2](); } f()"#,
        r#"RESULT goja.asciiString "1,3""#,
    ),
    (
        r#"function f() { var s = ""; for (let k in {a: 1, b: 2}) { setTimeout = () => k; s += k; } return s; } f()"#,
        r#"RESULT goja.asciiString "ab""#,
    ),
    (
        r#"function f(a, b = eval("a + 1")) { return b; } f(1)"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function f(a = () => b, b = 2) { return a(); } f()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function f(a = b, b = 2) { return a; } f()"#,
        r#"EXCEPTION "ReferenceError: Cannot access a variable before initialization\n\tat f (<eval>:1:16(3))\n\tat <eval>:1:41(3)\n"
ERROR "ReferenceError: Cannot access a variable before initialization at f (<eval>:1:16(3))""#,
    ),
    (
        r#"function f(x) { { function x() {} } return typeof x; } f(1)"#,
        r#"RESULT goja.asciiString "number""#,
    ),
    (
        r#"function f() { return typeof g; function g() {} } f()"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"function f() { g(); function g() { h = 1; } return h; } f()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#""use strict"; function f() { g(); function g() { h2 = 1; } } f()"#,
        r#"EXCEPTION "ReferenceError: h2 is not defined\n\tat g (<eval>:1:50(3))\n\tat f (<eval>:1:31(5))\n\tat <eval>:1:63(3)\n"
ERROR "ReferenceError: h2 is not defined at g (<eval>:1:50(3))""#,
    ),
    (
        r#"var r = ""; for (var i = 0; i < 3; i++) { r += (function() { return i; })(); } r"#,
        r#"RESULT goja.asciiString "012""#,
    ),
    (
        r#"function counter() { let c = 0; return { inc: () => ++c, get: () => c }; } var c1 = counter(); c1.inc(); c1.inc(); c1.get()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function f() { eval("var e1 = 1"); delete e1; return typeof e1; } f()"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"function f() { var v = 1; return delete v; } f()"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"function f() { eval("let l1 = 1"); return typeof l1; } f()"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"function f() { var x = 1; { let x = 2; eval("x = 3"); } return x; } f()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"function f() { "use strict"; var x = 1; eval("var x = 2"); return x; } f()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var globalThisRef = this; function f() { return eval("this") === globalThisRef; } f()"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function() { return eval("new.target"); })()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"class A { m() { return eval("super.toString") === Object.prototype.toString; } } new A().m()"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"class A { constructor() { eval("this.x = 1"); } } new A().x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class A {} class B extends A { constructor() { eval("super()"); this.y = 2; } } new B().y"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class A { static #s = 5; static m() { return eval("A.#s"); } } A.m()"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"class O { #a = 1; m() { class I { #b = 2; get(o) { return o.#a; } } return new I().get(this); } } new O().m()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"class O { #a = 1; m() { class I { #a = 2; get() { return this.#a; } } return new I().get(); } } new O().m()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class A { #x = 1; static m(o) { return #x in o; } } A.m(new A()) + "," + A.m(5)"#,
        r#"EXCEPTION "TypeError: Value is not an object: 5\n\tat m (<eval>:1:46(2))\n\tat <eval>:1:77(18)\n"
ERROR "TypeError: Value is not an object: 5 at m (<eval>:1:46(2))""#,
    ),
    (
        r#"var o = { get g() { return super.constructor === Object; } }; o.g"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = { m() { return super.m2; } }; Object.setPrototypeOf(o, { m2: "p" }); o.m()"#,
        r#"RESULT goja.asciiString "p""#,
    ),
    (
        r#"var o = { m() { super.v = 1; return this.v; } }; o.m()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"function F(a, b) { this.s = a + b; } var it = { [Symbol.iterator]() { var n = 0; return { next() { n++; return { value: n, done: n > 2 }; } }; } }; new F(...it).s"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var o = { f(...a) { return this === o; } }; var it = { [Symbol.iterator]() { return { next() { return { done: true }; } }; } }; o.f(...it)"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = { m() { return 1; } }; o?.m?.()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = null; o?.m()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var o = {}; o.a?.b.c.d"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var f = null; f?.()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (r#"(void 0)?.x ?? "u""#, r#"RESULT goja.asciiString "u""#),
    (
        r#"var o = { m() { return this; } }; o?.m() === o"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var o = { m() { return this; } }; (o?.m)() === o"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"0 || null ?? "x""#,
        r#"EXCEPTION "SyntaxError: SyntaxError: (anonymous): Line 1:11 Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses (and 1 more errors)\n"
ERROR "SyntaxError: SyntaxError: (anonymous): Line 1:11 Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses (and 1 more errors)""#,
    ),
    (
        r#"var a = 0; a ||= 1; var b = 1; b &&= 0; var c = null; c ??= 3; a + b + c"#,
        r#"RESULT goja.valueInt "4""#,
    ),
    (
        r#"var o = { get x() { return 1; } }; o.x ||= 2; o.x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#""use strict"; var o = { get x() { return 0; } }; o.x ||= 2"#,
        r#"EXCEPTION "TypeError: Cannot assign to read only property 'x'\n\tat <eval>:1:50(11)\n"
ERROR "TypeError: Cannot assign to read only property 'x' at <eval>:1:50(11)""#,
    ),
    (
        r#"var o = {}; o.a ??= {}; o.a.b ??= 5; o.a.b"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"var o = {n: 1}; with (o) { n += 1; } o.n"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var cnt = 0; var o = { get p() { cnt++; return 1; }, set p(v) {} }; o.p += 1; o.p++; cnt"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var cnt = 0; var k = { toString() { cnt++; return "a"; } }; var o = {a: 1}; o[k] += 1; o[k]++; cnt + "," + o.a"#,
        r#"RESULT goja.asciiString "4,3""#,
    ),
    (
        r#"var cnt = 0; var k = { toString() { cnt++; return "a"; } }; var o = {}; o[k] = 1; cnt"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {}; var k = Symbol(); o[k] = 1; o[k] += 1; o[k]"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var s = "abc"; s.x = 1; s.x"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#""use strict"; var s = "abc"; s.x = 1"#,
        r#"EXCEPTION "TypeError: Receiver is not an object: abc\n\tat <eval>:1:32(6)\n"
ERROR "TypeError: Receiver is not an object: abc at <eval>:1:32(6)""#,
    ),
    (r#"true.toString()"#, r#"RESULT goja.asciiString "true""#),
    (
        r#"Symbol.iterator.toString()"#,
        r#"RESULT goja.asciiString "Symbol(Symbol.iterator)""#,
    ),
    (
        r#"Symbol("x").description"#,
        r#"RESULT goja.asciiString "x""#,
    ),
    (
        r#"var s = Symbol("y"); s.foo = 1; s.foo"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"null.x = 1"#,
        r#"EXCEPTION "TypeError: Cannot convert undefined or null to object\n\tat <eval>:1:6(2)\n"
ERROR "TypeError: Cannot convert undefined or null to object at <eval>:1:6(2)""#,
    ),
    (
        r#"undefined[0]"#,
        r#"EXCEPTION "TypeError: Cannot read property '0' of undefined\n\tat <eval>:1:10(2)\n"
ERROR "TypeError: Cannot read property '0' of undefined at <eval>:1:10(2)""#,
    ),
    (
        r#"var o = {}; o[undefined] = 1; o["undefined"]"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {}; o[null] = 1; o.null"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {}; o[1.5] = 1; o["1.5"]"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {}; o[-0] = 1; o["0"]"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {}; o[1e21] = 1; o["1e+21"]"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = {1: "a", 0: "b", x: "c"}; var s = ""; for (var k in o) s += k; s"#,
        r#"RESULT goja.asciiString "01x""#,
    ),
    (r#"delete 5"#, r#"RESULT goja.valueBool "true""#),
    (r#"var x; delete x"#, r#"RESULT goja.valueBool "false""#),
    (r#"delete undeclared123"#, r#"RESULT goja.valueBool "true""#),
    (
        r#""use strict"; var o = {}; Object.defineProperty(o, "x", {value: 1}); delete o.x"#,
        r#"EXCEPTION "TypeError: Cannot delete property 'x' of [object Object]\n\tat <eval>:1:79(14)\n"
ERROR "TypeError: Cannot delete property 'x' of [object Object] at <eval>:1:79(14)""#,
    ),
    (
        r#"var o = {}; Object.defineProperty(o, "x", {value: 1}); delete o.x"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"typeof delete {}.a"#,
        r#"RESULT goja.asciiString "boolean""#,
    ),
    (
        r#"var a = 1; a = a++ + a++; a"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var i = 0; var o = {}; o[i++] = i++; i + "," + o[0]"#,
        r#"RESULT goja.asciiString "2,1""#,
    ),
    (
        r#"function f() { return f.length; } f()"#,
        r#"RESULT goja.valueInt "0""#,
    ),
    (
        r#"function f(a, b) { return arguments.length; } f(...{ [Symbol.iterator]() { var n = 0; return { next() { n++; return { value: n, done: n > 5 }; } }; } })"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function f() { return this; } var bound = f.bind(5); typeof bound()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"function f(a, b) { return a + b; } var b = f.bind(null, 1); b(2) + "," + b.length + "," + b.name"#,
        r#"RESULT goja.asciiString "3,1,bound f""#,
    ),
    (
        r#"function F(a) { this.a = a; } var B = F.bind(null, 7); new B().a + "," + (new B() instanceof F)"#,
        r#"RESULT goja.asciiString "7,true""#,
    ),
    (
        r#"Function.prototype.apply.call(function(a, b) { return a + b; }, null, {length: 2, 0: 1, 1: 2})"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var f = function() {}; f.prototype = 5; typeof new f()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"function F() { return 5; } typeof new F()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"function F() { return {x: 1}; } new F().x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var o = new function() { this.v = 9; }; o.v"#,
        r#"RESULT goja.valueInt "9""#,
    ),
    (
        r#"new Object(1) instanceof Object"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"Object(Symbol()) instanceof Symbol"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"({}).toString.call(null)"#,
        r#"RESULT goja.asciiString "[object Null]""#,
    ),
    (
        r#"({}).toString.call(undefined)"#,
        r#"RESULT goja.asciiString "[object Undefined]""#,
    ),
    (
        r#"Object.prototype.toString.call(function*(){})"#,
        r#"RESULT goja.asciiString "[object GeneratorFunction]""#,
    ),
    (
        r#"Object.prototype.toString.call((function*(){})())"#,
        r#"RESULT goja.asciiString "[object Generator]""#,
    ),
    (
        r#"Object.prototype.toString.call(class {})"#,
        r#"RESULT goja.asciiString "[object Function]""#,
    ),
    (
        r#"typeof (async function() {})"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"var x = 10; { var x = 20; } x"#,
        r#"RESULT goja.valueInt "20""#,
    ),
    (
        r#"let y = 1; try { y = 2; throw 0; } catch { y += 1; } y"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"try { throw 1 } catch { "c" } finally { "f" }"#,
        r#"RESULT goja.asciiString "c""#,
    ),
    (
        r#"(function() { try { return "t"; } finally { "f"; } })()"#,
        r#"RESULT goja.asciiString "t""#,
    ),
    (
        r#"(function() { try { throw "x"; } catch (e) { return e; } finally { } })()"#,
        r#"RESULT goja.asciiString "x""#,
    ),
    (
        r#"(function() { label: try { return 1; } finally { break label; } return 2; })()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"(function() { for (var i = 0; i < 2; i++) { try { break; } finally { continue; } } return i; })()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var s = ""; for (var i = 0; i < 3; i++) { try { try { if (i == 1) throw i; s += "a"; } finally { s += "f"; } } catch (e) { s += "c"; } } s"#,
        r#"RESULT goja.asciiString "affcaf""#,
    ),
    (
        r#"function* g() { try { yield 1; } finally { console; } } var it = g(); it.next(); it.return(2).value"#,
        r#"EXCEPTION "ReferenceError: console is not defined\n\tat g (<eval>:1:44(6))\n\tat return (native)\n\tat <eval>:1:91(13)\n"
ERROR "ReferenceError: console is not defined at g (<eval>:1:44(6))""#,
    ),
    (
        r#"function* g() { for (var i = 0; i < 3; i++) { try { yield i; } finally { } } } var s = ""; var it = g(); for (var v of it) { s += v; } s"#,
        r#"RESULT goja.asciiString "012""#,
    ),
    (
        r#"function* g() { yield 1; yield 2; } var [a, b] = g()"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"function* g() { yield* { [Symbol.iterator]() { return { i: 0, next() { return { done: this.i > 1, value: this.i++ }; } }; } }; } var s = ""; for (var v of g()) s += v; s"#,
        r#"RESULT goja.asciiString "01""#,
    ),
    (
        r#"function* g() { try { yield* inner(); } catch (e) { yield "caught:" + e; } } function* inner() { yield 1; throw "E"; } var it = g(); it.next().value + it.next().value"#,
        r#"RESULT goja.asciiString "1caught:E""#,
    ),
    (
        r#"function* g() { yield* inner(); } function* inner() { try { yield 1; } finally { log += "innerfin"; } } var log = ""; var it = g(); it.next(); it.return(5); log"#,
        r#"RESULT goja.asciiString "innerfin""#,
    ),
    (
        r#"function* g() { yield* inner(); } function* inner() { try { yield 1; } catch (e) { yield "c" + e; } } var it = g(); it.next(); it.throw("T").value"#,
        r#"RESULT goja.asciiString "cT""#,
    ),
    (
        r#"var gen = (function*() { yield 1; })(); gen[Symbol.iterator]() === gen"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"class A { set x(v) { this._x = v; } } class B extends A { m() { super.x = 5; return this._x; } } new B().m()"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"class A { } class B extends A { m(k) { super[k] = 7; return this[k]; } } new B().m("z")"#,
        r#"RESULT goja.valueInt "7""#,
    ),
    (
        r#"class A { get g() { return this.t; } } class B extends A { constructor() { super(); this.t = "T"; } m() { return super.g; } } new B().m()"#,
        r#"RESULT goja.asciiString "T""#,
    ),
    (
        r#"class A { static s() { return "As"; } } class B extends A { static s() { return super.s() + "Bs"; } } B.s()"#,
        r#"RESULT goja.asciiString "AsBs""#,
    ),
    (
        r#"class A { constructor(v) { this.v = v; } } class B extends A { constructor() { const f = () => super(9); f(); } } new B().v"#,
        r#"RESULT goja.valueInt "9""#,
    ),
    (
        r#"class A { #p = 1; static #sp = 2; #m() { return this.#p; } static #sm() { return A.#sp; } t() { return this.#m() + A.#sm(); } } new A().t()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"class A { static #count = 0; static { A.#count = 5; } static get c() { return A.#count; } } A.c"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"class A { #x = 1; getX() { return this.#x; } } class B extends A { #x = 2; getBX() { return this.#x; } } var b = new B(); b.getX() + "," + b.getBX()"#,
        r#"RESULT goja.asciiString "1,2""#,
    ),
    (
        r#"class A { #f() {} static t(o) { try { o.#f(); return "ok"; } catch (e) { return e.message; } } } A.t({})"#,
        r#"RESULT goja.asciiString "Cannot read private member #f from an object whose class did not declare it""#,
    ),
    (
        r#"class A { #v = 0; set #s(x) { this.#v = x; } get v() { return this.#v; } m() { this.#s = 3; return this.v; } } new A().m()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"class A { get #g() { return 1; } m() { try { this.#g = 2; } catch (e) { return e.message; } } } new A().m()"#,
        r#"RESULT goja.asciiString "Cannot assign to read only property '#g'""#,
    ),
    (
        r#"class A { set #s(v) {} m() { try { return this.#s; } catch (e) { return e.message; } } } new A().m()"#,
        r#"RESULT goja.asciiString "'#s' was defined without a getter""#,
    ),
    (
        r#"class A { #m() {} n() { try { this.#m = 1; } catch (e) { return e.message; } } } new A().n()"#,
        r#"RESULT goja.asciiString "Private method '#m' is not writable""#,
    ),
    (
        r#"class A { #x = this.#y; #y = 1; } try { new A(); } catch (e) { e.message }"#,
        r#"RESULT goja.asciiString "Private member #y is accessed before it is initialized""#,
    ),
    (
        r#"class C { static #p = 1; static m() { return eval("C.#p + 1"); } } C.m()"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"class C { #p = 5; m() { return (() => eval("this.#p"))(); } } new C().m()"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"function* g() { var x = yield* [1, 2]; } typeof g"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"function* g() { yield 1; yield 2; yield 3; } function f(a, b, c) { return a + b + c; } f(...g())"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"function* g() { L: { yield 1; break L; } yield 2; } var s = ""; for (var v of g()) s += v; s"#,
        r#"RESULT goja.asciiString "12""#,
    ),
    (
        r#"function* g() { try { yield 1; } catch (e) {} yield 2; } var it = g(); it.next(); it.throw(1).value"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"function* g() { var a = arguments; yield a[0]; yield a.length; } var it = g(5, 6); it.next().value + "," + it.next().value"#,
        r#"RESULT goja.asciiString "5,2""#,
    ),
    (
        r#"function* g() { yield new.target; } g().next().value"#,
        r#"RESULT goja.valueUndefined "undefined""#,
    ),
    (
        r#"var o = { *g() { yield super.toString === Object.prototype.toString; } }; o.g().next().value"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"function* g() { var x = yield; var y = yield; return x + y; } var it = g(); it.next(); it.next(1); it.next(2).value"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"function* g(x) { while (x > 0) { x = yield x; } return "end"; } var it = g(3); var s = ""; var r = it.next(); while (!r.done) { s += r.value; r = it.next(r.value - 1); } s + r.value"#,
        r#"RESULT goja.asciiString "321end""#,
    ),
    (
        r#"function* g() { try { yield 1; } finally { yield 2; } } var it = g(); it.next(); var r = it.return(9); r.value + "," + r.done + "," + it.next().value + "," + it.next().done"#,
        r#"RESULT goja.asciiString "9,true,undefined,true""#,
    ),
    (
        r#"function* g() { yield* g2(); } function* g2() { return yield 1; } var it = g(); it.next(); it.next(5).done"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var log = ""; function* g() { try { yield 1; } finally { log += "F"; } } for (var v of g()) { break; } log"#,
        r#"RESULT goja.asciiString "F""#,
    ),
    (
        r#"var log = ""; function* g() { try { yield 1; } finally { log += "F"; } } try { for (var v of g()) { throw 1; } } catch (e) {} log"#,
        r#"RESULT goja.asciiString "F""#,
    ),
    (
        r#"function* g() { yield 1; return 2; } var a = []; typeof a"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"var it = (function*() { yield 1; })(); var n = it.next; n.call({})"#,
        r#"EXCEPTION "TypeError: Method [Generator].prototype.next called on incompatible receiver\n\tat call (native)\n\tat <eval>:1:63(13)\n"
ERROR "TypeError: Method [Generator].prototype.next called on incompatible receiver at call (native)""#,
    ),
    (
        r#"function* g() {} g.prototype.x = 1; g().x"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"function* g() {} var p = g.prototype; g.prototype = null; Object.getPrototypeOf(g()) === Object.getPrototypeOf(p)"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var { a = (() => { throw new Error("default") })() } = { a: 0 }; a"#,
        r#"RESULT goja.valueInt "0""#,
    ),
    (
        r#"var { [Symbol.iterator]: it } = { [Symbol.iterator]: 5 }; it"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"var cnt = 0; var { [(cnt++, "k")]: k } = { k: 1 }; cnt + k"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"var { a: { b } } = { a: null }"#,
        r#"EXCEPTION "TypeError: Value is not object coercible\n\tat <eval>:1:20(9)\n"
ERROR "TypeError: Value is not object coercible at <eval>:1:20(9)""#,
    ),
    (
        r#"var {} = null"#,
        r#"EXCEPTION "TypeError: Value is not object coercible\n\tat <eval>:1:1(1)\n"
ERROR "TypeError: Value is not object coercible at <eval>:1:1(1)""#,
    ),
    (
        r#"var { x } = undefined"#,
        r#"EXCEPTION "TypeError: Value is not object coercible\n\tat <eval>:1:13(2)\n"
ERROR "TypeError: Value is not object coercible at <eval>:1:13(2)""#,
    ),
    (
        r#"function f({ a, b: { c } = { c: 3 } }) { return a + c; } f({ a: 1 })"#,
        r#"RESULT goja.valueInt "4""#,
    ),
    (
        r#"var o = {}; ({ a: o.p, b: o["q"] } = { a: 1, b: 2 }); o.p + o.q"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var { a, ...r } = "abc"; typeof r"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"var { length: l, ...r2 } = "xy"; l"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"try { throw { m: "msg" }; } catch ({ m }) { m }"#,
        r#"RESULT goja.asciiString "msg""#,
    ),
    (
        r#"for (var { k, v } of { [Symbol.iterator]() { var i = 0; return { next() { i++; return { done: i > 2, value: { k: i, v: i * 2 } }; } }; } }) {} k + v"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var x = 1; var o = { x: 2, f() { var x = 3; return eval("x"); } }; o.f()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"var f = new Function("a", "return a * 2"); f(21)"#,
        r#"RESULT goja.valueInt "42""#,
    ),
    (
        r#"new Function("return typeof arguments")()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"Function("return this")() === this"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function() { "use strict"; return Function("return this")(); })() === this"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var ev = eval; (function() { var local = 1; return ev("typeof local"); })()"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"(function() { var local = 1; return eval("typeof local"); })()"#,
        r#"RESULT goja.asciiString "number""#,
    ),
    (
        r#"(function() { return eval("(function() { return typeof arguments; })()"); })()"#,
        r#"RESULT goja.asciiString "object""#,
    ),
    (
        r#"(function(a) { eval("var a = 2"); return a; })(1)"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"(function(a) { eval("a = 3"); return arguments[0]; })(1)"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"eval("'use strict'; var sv = 1"); typeof sv"#,
        r#"RESULT goja.asciiString "undefined""#,
    ),
    (
        r#"eval("function ef() {}"); delete ef"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"eval("var ev2 = 1"); delete ev2"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"var gv = 1; delete gv"#,
        r#"RESULT goja.valueBool "false""#,
    ),
    (
        r#"this.tv = 1; delete tv"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function() { eval("var x1 = 1; var x2 = 2"); return x1 + x2; })()"#,
        r#"RESULT goja.valueInt "3""#,
    ),
    (
        r#"(function() { { eval("var bx = 1"); } return bx; })()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"(function() { let l = 1; { eval("var l = 2"); } })()"#,
        r#"EXCEPTION "SyntaxError: Identifier 'l' has already been declared\n\tat <eval>:1:1(0)\n\tat <eval>:1:32(9)\n\tat <eval>:1:51(2)\n"
ERROR "SyntaxError: Identifier 'l' has already been declared at <eval>:1:1(0)""#,
    ),
    (
        r#"(function() { var v; { let v2; eval("var v2 = 1"); } })()"#,
        r#"EXCEPTION "SyntaxError: Identifier 'v2' has already been declared\n\tat <eval>:1:1(0)\n\tat <eval>:1:36(10)\n\tat <eval>:1:56(2)\n"
ERROR "SyntaxError: Identifier 'v2' has already been declared at <eval>:1:1(0)""#,
    ),
    (
        r#"with ({ a: 1 }) { eval("a") }"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#"var x = 1; with ({}) { eval("var x = 5"); } x"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"(function() { with ({}) { var w2 = 1; } return w2; })()"#,
        r#"RESULT goja.valueInt "1""#,
    ),
    (
        r#""use strict"; eval("with ({}) {}")"#,
        r#"EXCEPTION "SyntaxError: SyntaxError: Strict mode code may not include a with statement at <eval>:1:1\n\tat <eval>:1:19(2)\n\tat <eval>:1:19(2)\n"
ERROR "SyntaxError: SyntaxError: Strict mode code may not include a with statement at <eval>:1:1 at <eval>:1:19(2)""#,
    ),
    (
        r#"(function f() { return eval("f.name"); })()"#,
        r#"RESULT goja.asciiString "f""#,
    ),
    (
        r#"(function f() { f = 1; return typeof f; })()"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"(function f() { "use strict"; f = 1; })()"#,
        r#"EXCEPTION "TypeError: Assignment to constant variable.\n\tat f (<eval>:1:31(6))\n\tat <eval>:1:40(2)\n"
ERROR "TypeError: Assignment to constant variable. at f (<eval>:1:31(6))""#,
    ),
    (
        r#"var fe = function g() { return typeof g; }; fe() + typeof g"#,
        r#"RESULT goja.asciiString "functionundefined""#,
    ),
    (
        r#"typeof (function() { return arguments; })().callee"#,
        r#"RESULT goja.asciiString "function""#,
    ),
    (
        r#"(function() { return arguments.callee === arguments.callee; })()"#,
        r#"RESULT goja.valueBool "true""#,
    ),
    (
        r#"(function(a, a) { return a; })(1, 2)"#,
        r#"RESULT goja.valueInt "2""#,
    ),
    (
        r#"(function(a, b) { arguments.length = 5; return arguments.length; })(1)"#,
        r#"RESULT goja.valueInt "5""#,
    ),
    (
        r#"(function() { arguments[5] = 1; return arguments.length; })()"#,
        r#"RESULT goja.valueInt "0""#,
    ),
    (
        r#"var s = 0; var it = { [Symbol.iterator]() { var n = 0; return { next() { n++; var junk = {a: {b: {}}}; return { value: { v: n }, done: n > 50 }; } }; } }; for (var x of it) { s += x.v; } s"#,
        r#"RESULT goja.valueInt "1275""#,
    ),
    (
        r#"function* g() { for (var i = 0; i < 30; i++) { var o = { i: i, nested: { k: [i] } }; yield o; } } var s = 0; for (var o of g()) { s += o.i; } s"#,
        r#"RESULT goja.valueInt "435""#,
    ),
    (
        r#"var o = { get a() { var t = {}; for (var i = 0; i < 10; i++) t = { prev: t }; return { val: 7 }; } }; var r = 0; for (var i = 0; i < 20; i++) r += o.a.val; r"#,
        r#"RESULT goja.valueInt "140""#,
    ),
    (
        r#"var k = { toString() { var junk = {x: {}}; return "key"; } }; var o = {}; for (var i = 0; i < 10; i++) { o[k] = { n: i }; } o.key.n"#,
        r#"RESULT goja.valueInt "9""#,
    ),
    (
        r#"function f() { return arguments; } var a; for (var i = 0; i < 20; i++) { a = f({ x: i }, { y: i }); } a[0].x + a[1].y"#,
        r#"RESULT goja.valueInt "38""#,
    ),
    (
        r#"var r = 0; for (var i = 0; i < 20; i++) { var { a, ...rest } = { a: { v: i }, b: { w: i }, c: {} }; r += a.v + rest.b.w; } r"#,
        r#"RESULT goja.valueInt "380""#,
    ),
    (
        r#"var r = ""; for (var i = 0; i < 10; i++) { with ({ p: { q: "x" } }) { r += p.q; } } r"#,
        r#"RESULT goja.asciiString "xxxxxxxxxx""#,
    ),
    (
        r#"class P { #priv = { v: 1 }; get v() { var junk = {}; return this.#priv.v; } } var t = 0; for (var i = 0; i < 20; i++) t += new P().v; t"#,
        r#"RESULT goja.valueInt "20""#,
    ),
    (
        r#"function mk(i) { return () => ({ i: i }); } var fs = {}; for (var i = 0; i < 20; i++) fs[i] = mk(i); var t = 0; for (var i = 0; i < 20; i++) t += fs[i]().i; t"#,
        r#"RESULT goja.valueInt "190""#,
    ),
    (
        r#"var t = 0; for (var i = 0; i < 20; i++) { try { throw { e: { v: i } }; } catch (x) { t += x.e.v; } } t"#,
        r#"RESULT goja.valueInt "190""#,
    ),
    (
        r#"function* inner() { yield { a: 1 }; yield { a: 2 }; } function* outer() { yield* inner(); yield* inner(); } var t = 0; for (var v of outer()) t += v.a; t"#,
        r#"RESULT goja.valueInt "6""#,
    ),
    (
        r#"var t = 0; var o = { valueOf() { var junk = { j: {} }; return 3; } }; for (var i = 0; i < 20; i++) t += o * 2; t"#,
        r#"RESULT goja.valueInt "120""#,
    ),
    (
        r#"function f(a, b, c) { return a.x + b.x + c.x; } var it = { [Symbol.iterator]() { var n = 0; return { next() { n++; return { value: { x: n }, done: n > 3 }; } }; } }; var t = 0; for (var i = 0; i < 10; i++) t += f(...it); t"#,
        r#"RESULT goja.valueInt "60""#,
    ),
    (
        r#"var r = 0; for (var i = 0; i < 10; i++) { r += eval("var tmp = { v: " + i + " }; tmp.v"); } r"#,
        r#"RESULT goja.valueInt "45""#,
    ),
    (
        r#"class A { constructor(x) { this.x = { v: x }; } } class B extends A { constructor(x) { super(x); this.y = { v: x * 2 }; } } var t = 0; for (var i = 0; i < 20; i++) { var b = new B(i); t += b.x.v + b.y.v; } t"#,
        r#"RESULT goja.valueInt "570""#,
    ),
    (
        r#"var gen = (function*() { var i = 0; while (true) { var o = yield { i: i++ }; if (o && o.stop) return { done: 1 }; } })(); var t = 0; for (var i = 0; i < 20; i++) t += gen.next({}).value.i; t + gen.next({ stop: true }).value.done"#,
        r#"RESULT goja.valueInt "191""#,
    ),
    (
        r#"var t = 0; for (var i = 0; i < 20; i++) { var f = new Function("a", "return { r: a * 2 }"); t += f(i).r; } t"#,
        r#"RESULT goja.valueInt "380""#,
    ),
    (
        r#"function t(s){return s} var a = t`x`; var b = t`x`; (a === b) + "," + (a == b) + "," + (a.raw === b.raw)"#,
        r#"RESULT goja.asciiString "false,false,false""#,
    ),
    (
        r#"var { a, ...rest } = { a: 1, b: 2, c: 3 }; var s = ""; for (var k in rest) s += k + rest[k]; s + ("a" in rest)"#,
        r#"RESULT goja.asciiString "b2c3false""#,
    ),
    (
        r#"var log = ""; var src = { get a() { log += "a"; return 1; }, get b() { log += "b"; return 2; } }; var { a, ...rest } = src; log + rest.b + log"#,
        r#"RESULT goja.asciiString "ab2ab""#,
    ),
    (
        r#"var sym = Symbol("s"); var { [sym]: v, ...rest } = { [sym]: 1, x: 2 }; v + "," + (sym in rest) + "," + rest.x"#,
        r#"RESULT goja.asciiString "1,false,2""#,
    ),
    (
        r#"var sym = Symbol("s"); var { x, ...rest } = { [sym]: 1, x: 2 }; rest[sym] + "," + ("x" in rest)"#,
        r#"RESULT goja.asciiString "1,false""#,
    ),
    (
        r#"var { 0: first, ...rest } = "xyz"; var s = first; for (var k in rest) s += k + rest[k]; s"#,
        r#"RESULT goja.asciiString "x1y2z""#,
    ),
    (
        r#"var { valueOf, ...rest } = 5; typeof valueOf + "," + (Object.getPrototypeOf(rest) === Object.prototype)"#,
        r#"RESULT goja.asciiString "function,true""#,
    ),
    (
        r#"var p = { inherited: 1 }; var o = Object.create(p); o.own = 2; var { ...rest } = o; ("inherited" in rest) + "," + rest.own"#,
        r#"RESULT goja.asciiString "false,2""#,
    ),
    (
        r#"var o = {}; Object.defineProperty(o, "hidden", { value: 1, enumerable: false }); o.vis = 2; var { ...rest } = o; ("hidden" in rest) + "," + rest.vis"#,
        r#"RESULT goja.asciiString "false,2""#,
    ),
    (
        r#"var { a, b, ...rest } = { a: 1, b: 2 }; var n = 0; for (var k in rest) n++; n"#,
        r#"RESULT goja.valueInt "0""#,
    ),
    (
        r#"var key = "dyn"; var { [key]: d, ...rest } = { dyn: 1, other: 2 }; d + "," + ("dyn" in rest) + "," + rest.other"#,
        r#"RESULT goja.asciiString "1,false,2""#,
    ),
    (
        r#"var { a: { x }, ...rest } = { a: { x: 9 }, b: 1 }; x + "," + ("a" in rest) + "," + rest.b"#,
        r#"RESULT goja.asciiString "9,false,1""#,
    ),
    (
        r#"function f({ a, ...rest }) { var s = ""; for (var k in rest) s += k; return a + s; } f({ a: "A", b: 1, c: 2 })"#,
        r#"RESULT goja.asciiString "Abc""#,
    ),
    (
        r#"var { a, ...rest } = null"#,
        r#"EXCEPTION "TypeError: Value is not object coercible\n\tat <eval>:1:1(2)\n"
ERROR "TypeError: Value is not object coercible at <eval>:1:1(2)""#,
    ),
    (
        r#"var log = ""; var o = { a: 1 }; Object.defineProperty(o, "b", { get() { log += "B"; return 2; }, enumerable: true }); var { ...rest } = o; log + rest.b"#,
        r#"RESULT goja.asciiString "B2""#,
    ),
    (
        r#"var { x = 5, ...rest } = { y: 1 }; x + "," + rest.y + "," + ("x" in rest)"#,
        r#"RESULT goja.asciiString "5,1,false""#,
    ),
];
