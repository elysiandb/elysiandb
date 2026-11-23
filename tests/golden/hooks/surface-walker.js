function postRead(ctx) {
  var seen = new Map();
  var records = [];
  var display = function (k) { return typeof k === "symbol" ? "[" + k.description + "]" : k; };
  var keysOf = function (o, path) {
    var names = Object.getOwnPropertyNames(o).filter(function (k) { return !(path === "globalThis" && (k === "ctx" || k === "postRead")); });
    var syms = Object.getOwnPropertySymbols(o);
    var all = names.map(function (k) { return [display(k), k]; }).concat(syms.map(function (k) { return [display(k), k]; }));
    all.sort(function (a, b) { return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0; });
    return all;
  };
  var visit = function (o, path) {
    if (seen.has(o)) { records.push({path: path, alias: seen.get(o)}); return; }
    seen.set(o, path);
    var keys = keysOf(o, path);
    var children = [];
    for (var i = 0; i < keys.length; i++) {
      var name = keys[i][0], key = keys[i][1];
      var d = Object.getOwnPropertyDescriptor(o, key);
      var r = {path: path, key: name, enumerable: d.enumerable, configurable: d.configurable};
      if ("value" in d) {
        r.kind = "data";
        r.writable = d.writable;
        var v = d.value;
        r.type = v === null ? "null" : typeof v;
        if (typeof v === "function") {
          r.length = v.length;
          r.name = v.name;
        } else if (v !== null && typeof v !== "object" && typeof v !== "undefined") {
          r.value = typeof v === "symbol" ? v.toString() : String(v);
        }
        if (v !== null && (typeof v === "object" || typeof v === "function")) { children.push([v, path + "." + name]); }
      } else {
        r.kind = "accessor";
        r.get = typeof d.get;
        r.set = typeof d.set;
        if (d.get) { children.push([d.get, path + "." + name + ".get"]); }
        if (d.set) { children.push([d.set, path + "." + name + ".set"]); }
      }
      records.push(r);
    }
    var proto = Object.getPrototypeOf(o);
    records.push({path: path, prototype: proto === null ? "null" : seen.has(proto) ? seen.get(proto) : path + ".[[Prototype]]"});
    for (var j = 0; j < children.length; j++) { visit(children[j][0], children[j][1]); }
    if (proto !== null && !seen.has(proto)) { visit(proto, path + ".[[Prototype]]"); }
  };
  visit(globalThis, "globalThis");
  var gen = function* () {};
  var asyncFn = async function () {};
  var roots = [
    ["%IteratorPrototype%", Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))],
    ["%ArrayIteratorPrototype%", Object.getPrototypeOf([][Symbol.iterator]())],
    ["%StringIteratorPrototype%", Object.getPrototypeOf(""[Symbol.iterator]())],
    ["%MapIteratorPrototype%", Object.getPrototypeOf(new Map()[Symbol.iterator]())],
    ["%SetIteratorPrototype%", Object.getPrototypeOf(new Set()[Symbol.iterator]())],
    ["%RegExpStringIteratorPrototype%", Object.getPrototypeOf(/a/[Symbol.matchAll]("a"))],
    ["%GeneratorFunction%", Object.getPrototypeOf(gen).constructor],
    ["%GeneratorFunction.prototype%", Object.getPrototypeOf(gen)],
    ["%GeneratorPrototype%", Object.getPrototypeOf(gen).prototype],
    ["%AsyncFunction%", Object.getPrototypeOf(asyncFn).constructor],
    ["%AsyncFunction.prototype%", Object.getPrototypeOf(asyncFn)],
    ["%TypedArray%", Object.getPrototypeOf(Int8Array)],
    ["%ThrowTypeError%", Object.getOwnPropertyDescriptor(Function.prototype, "arguments").get]
  ];
  for (var k = 0; k < roots.length; k++) { visit(roots[k][1], roots[k][0]); }
  ctx.entity.surface = JSON.stringify(records, null, 1);
}
