package main

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"strings"

	"github.com/dop251/goja/parser"
	"github.com/go-sourcemap/sourcemap"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type smCase struct {
	name string
	url  string
	data string
}

var smCases = []smCase{
	{"basic", "", `{"version":3,"file":"out.js","sources":["a.js","b.js"],"names":["foo","bar"],"mappings":"AAAA,EAAE;ACAA,IAAIA;AACC,GAAGC"}`},
	{"basic-url", "http://host/dir/map.js.map", `{"version":3,"file":"out.js","sources":["a.js","sub/b.js","/abs.js","http://cdn/x.js"],"names":["foo"],"mappings":"AAAA,EAAE;ACAA,IAAIA;AACC;AACA,EEAA;AGAA"}`},
	{"root-abs", "", `{"version":3,"sourceRoot":"http://example.com/src/","sources":["a.js","../b.js","/c.js"],"mappings":"AAAA;AACA;ACAA;ACAA"}`},
	{"root-rel", "map.js.map", `{"version":3,"sourceRoot":"src","sources":["a.js","../b.js","./c.js"],"mappings":"AAAA;AACA;ACAA;ACAA"}`},
	{"root-rel-url", "http://h/x/y.map", `{"version":3,"sourceRoot":"src/","sources":["a.js"],"mappings":"AAAA;AACA"}`},
	{"root-bad", "", `{"version":3,"sourceRoot":"%zz","sources":["a.js"],"mappings":"AAAA"}`},
	{"url-bad", "%zz", `{"version":3,"sources":["a.js"],"mappings":"AAAA"}`},
	{"url-rel", "a/b/c.map", `{"version":3,"sources":["a.js"],"mappings":"AAAA;AACA"}`},
	{"version0", "", `{"sources":["a.js"],"mappings":"AACA"}`},
	{"version2", "", `{"version":2,"sources":["a.js"],"mappings":"AAAA"}`},
	{"version-str", "", `{"version":"3","sources":["a.js"],"mappings":"AAAA"}`},
	{"version-float", "", `{"version":3.0,"sources":["a.js"],"mappings":"AAAA"}`},
	{"version-big", "", `{"version":99999999999999999999,"sources":["a.js"],"mappings":"AAAA"}`},
	{"version-null", "", `{"version":null,"sources":["a.js"],"mappings":"AACA"}`},
	{"mappings-empty", "", `{"version":3,"sources":["a.js"],"mappings":""}`},
	{"mappings-missing", "", `{"version":3,"sources":["a.js"]}`},
	{"empty-object", "", `{}`},
	{"null", "", `null`},
	{"array", "", `[]`},
	{"string", "", `"x"`},
	{"number", "", `3`},
	{"syntax", "", `{"version":3,`},
	{"syntax2", "", `{bad}`},
	{"syntax3", "", ``},
	{"syntax4", "", `{"version":3} x`},
	{"vlq-eof", "", `{"version":3,"sources":["a.js"],"mappings":"AAAg"}`},
	{"vlq-eof2", "", `{"version":3,"sources":["a.js"],"mappings":"g"}`},
	{"vlq-bad-chars", "", `{"version":3,"sources":["a.js","b.js"],"mappings":"A*A!,C@CC;;$A$A"}`},
	{"vlq-big", "", `{"version":3,"sources":["a.js"],"mappings":"AA+/////D,AAgggggggA,A//////////D"}`},
	{"negative", "", `{"version":3,"sources":["a.js","b.js"],"names":["x","y"],"mappings":"UAAU,LAAL,DACDA,ACADC;KAAK,FAAF,aAAa"}`},
	{"unsorted", "", `{"version":3,"sources":["a.js"],"mappings":"gBAAA,UAAC,LAAD,BAAA,oBAAA;AAAA,UAAU,PAAP"}`},
	{"line1col0", "", `{"version":3,"sources":["a.js"],"mappings":"AAAA,CAAA,CAAC,CAAD;AAAA"}`},
	{"names-mixed", "", `{"version":3,"sources":["a.js"],"names":["s",1.50,null,true,{"a" : [1, 2]},[ 3 ],"A\n"],"mappings":"AACAA,CAACC,CAACC,CAACC,CAACC,CAACC,CAACC,CAACC"}`},
	{"names-string", "", `{"version":3,"sources":["a.js"],"names":"abc","mappings":"AACA"}`},
	{"sources-null-elem", "", `{"version":3,"sources":[null,"b.js"],"mappings":"AACA,CCAA"}`},
	{"sources-num", "", `{"version":3,"sources":[1],"mappings":"AACA"}`},
	{"sources-null", "", `{"version":3,"sources":null,"mappings":"AACA"}`},
	{"sources-dup", "", `{"version":3,"sources":["a.js","b.js","c.js"],"sources":["x.js"],"sources":[null,null,null,null],"mappings":"AACA,CCAA,CCAA,CCAA"}`},
	{"sources-dup2", "", `{"version":3,"sources":["a","b","c","d","e"],"sources":["x"],"sources":[null,null,null,null,null,null,null,null,null],"mappings":"AACA,CCAA,CCAA,CCAA,CCAA,CCAA,CCAA,CCAA,CCAA"}`},
	{"sources-content", "", `{"version":3,"sources":["a.js","b.js","c.js"],"sourcesContent":["var a;",null],"mappings":"AACA,CCAA"}`},
	{"case-keys", "", `{"VERSION":3,"Sources":["a.js"],"MAPPINGS":"AACA;ACAA","FILE":"f.js"}`},
	{"fold-keys", "", "{\"version\":3,\"ſources\":[\"long-s.js\"],\"mappingſ\":\"AACA\",\"Key\":1}"},
	{"exact-wins", "", `{"version":3,"Sources":["upper.js"],"sources":["lower.js"],"mappings":"AACA"}`},
	{"unknown-keys", "", `{"version":3,"x":[1,{"y":null}],"sources":["a.js"],"mappings":"AACA","extra":"z"}`},
	{"file-num", "", `{"version":3,"file":5,"sources":["a.js"],"mappings":"AACA"}`},
	{"sections", "", `{"version":3,"file":"all.js","sections":[{"offset":{"line":0,"column":0},"map":{"version":3,"sources":["one.js"],"names":["n1"],"mappings":"AACA,CAACA;AACA"}},{"offset":{"line":2,"column":5},"map":{"version":3,"sources":["two.js"],"mappings":"AACA,EAAE;AACA"}}]}`},
	{"sections-root", "http://h/m/map.map", `{"version":3,"sections":[{"offset":{"line":1,"column":0},"map":{"version":3,"sourceRoot":"r","sources":["one.js"],"mappings":"AACA"}}]}`},
	{"sections-nil-map", "", `{"version":3,"sections":[{"offset":{"line":0,"column":0}}]}`},
	{"sections-null-map", "", `{"version":3,"sections":[{"offset":{"line":0,"column":0},"map":null}]}`},
	{"sections-bad-version", "", `{"version":3,"sections":[{"offset":{"line":0,"column":0},"map":{"version":4,"sources":["a"],"mappings":"AACA"}}]}`},
	{"sections-top-version", "", `{"version":5,"sections":[{"offset":{"line":0,"column":0},"map":{"version":3,"sources":["a"],"mappings":"AACA"}}]}`},
	{"sections-offset-str", "", `{"version":3,"sections":[{"offset":{"line":"0","column":0},"map":{"version":3,"sources":["a"],"mappings":"AACA"}}]}`},
	{"sections-nested", "", `{"version":3,"sections":[{"offset":{"line":0,"column":0},"map":{"version":3,"sources":["a"],"mappings":"AACA","sections":[{"map":null}]}}]}`},
	{"sections-dup-map", "", `{"version":3,"sections":[{"offset":{"line":0,"column":1},"map":{"version":3,"sources":["x.js","y.js"],"mappings":"AAAA"},"map":{"mappings":"AACA,CCAA"},"offset":{"line":1}}]}`},
	{"sections-dup", "", `{"version":3,"sections":[{"offset":{"line":0,"column":0},"map":{"version":3,"sources":["a.js"],"mappings":"AACA"}},{"offset":{"line":3,"column":0},"map":{"version":3,"sources":["b.js"],"mappings":"AACA"}}],"sections":[{"map":{"sources":["c.js"]}},null]}`},
	{"sections-empty", "", `{"version":3,"sources":["a.js"],"mappings":"AACA","sections":[]}`},
	{"sections-elem-num", "", `{"version":3,"sections":[1]}`},
	{"names-index-out", "", `{"version":3,"sources":["a.js"],"names":["x"],"mappings":"AACAE,CACAD"}`},
	{"source-index-out", "", `{"version":3,"sources":["a.js"],"mappings":"AECA"}`},
	{"semicolons", "", `{"version":3,"sources":["a.js"],"mappings":";;;AACA;;CACA,,CACA;"}`},
	{"whitespace", "", " \n\t{ \"version\" : 3 , \"sources\" : [ \"a.js\" ] , \"mappings\" : \"AACA\" } \r\n"},
	{"deep", "", `{"version":3,"x":` + strings.Repeat("[", 9000) + strings.Repeat("]", 9000) + `,"sources":["a.js"],"mappings":"AACA"}`},
	{"too-deep", "", `{"version":3,"x":` + strings.Repeat("[", 10001) + strings.Repeat("]", 10001) + `}`},
}

func smErrClass(err error) string {
	var se *json.SyntaxError
	if errors.As(err, &se) {
		return "json-syntax"
	}
	var te *json.UnmarshalTypeError
	if errors.As(err, &te) {
		return "json-type"
	}
	var ue *url.Error
	if errors.As(err, &ue) {
		return "url"
	}
	if err == io.EOF {
		return "vlq-eof"
	}
	msg := err.Error()
	switch {
	case msg == "sourcemap: mappings are empty":
		return "mappings-empty"
	case strings.HasPrefix(msg, "sourcemap: got version="):
		return "version"
	}
	return "other"
}

func smParse(u string, data []byte) (c *sourcemap.Consumer, err error, panicked bool) {
	defer func() {
		if r := recover(); r != nil {
			panicked = true
		}
	}()
	c, err = sourcemap.Parse(u, data)
	return
}

func smQuery(c *sourcemap.Consumer, line, col int) (q []any) {
	defer func() {
		if r := recover(); r != nil {
			q = []any{line, col, "panic"}
		}
	}()
	src, name, sl, sc, ok := c.Source(line, col)
	return []any{line, col, src, name, sl, sc, ok}
}

func genSourcemap(out string) error {
	var lines goldenio.Lines
	for _, tc := range smCases {
		c, err, panicked := smParse(tc.url, []byte(tc.data))
		m := map[string]any{"name": tc.name, "url": tc.url, "data": tc.data}
		switch {
		case panicked:
			m["err"] = "panic"
		case err != nil:
			m["err"] = smErrClass(err)
			var se *json.SyntaxError
			if errors.As(err, &se) {
				m["offset"] = se.Offset
			}
		default:
			m["file"] = c.File()
			m["sourcemap_url"] = c.SourcemapURL()
			var queries []any
			for line := 0; line <= 5; line++ {
				for _, col := range []int{-1, 0, 1, 2, 3, 4, 5, 6, 8, 10, 13, 20, 40, 100} {
					queries = append(queries, smQuery(c, line, col))
				}
			}
			m["queries"] = queries
			contents := map[string]string{}
			for _, s := range []string{"a.js", "b.js", "c.js", "one.js", "x.js", "", "missing.js"} {
				contents[s] = c.SourceContent(s)
			}
			m["contents"] = contents
		}
		lines.Add(m)
	}
	if err := lines.Write(out, "sourcemap/consumer.jsonl"); err != nil {
		return err
	}
	return genSourcemapScripts(out)
}

type smScript struct {
	name     string
	filename string
	src      string
}

func dataURL(m string) string {
	return "data:application/json;base64," + base64.StdEncoding.EncodeToString([]byte(m))
}

func genSourcemapScripts(out string) error {
	body := "function f(a) {\n  return a + 1;\n}\nvar x = f(2);\nthrow new Error(\"x\");\n"
	mapA := `{"version":3,"file":"out.js","sources":["src/a.ts"],"names":["f","a"],"mappings":"AAAA,SAASA,EAAEC;EACT,OAAOA;AACT;AACA,IAAI;AACJ"}`
	mapB := `{"version":3,"sourceRoot":"http://cdn.example/root/","sources":["../lib/b.ts","c.ts"],"mappings":"AAAA;ACAA,EAAE;ADAA;ACCA;AAAA"}`
	mapC := `{"version":3,"sections":[{"offset":{"line":0,"column":0},"map":{"version":3,"sources":["one.js"],"mappings":"AAAA;AACA"}},{"offset":{"line":3,"column":0},"map":{"version":3,"sources":["two.js"],"mappings":"AAAA;AACA"}}]}`
	scripts := []smScript{
		{"data-a", "", body + "//# sourceMappingURL=" + dataURL(mapA)},
		{"data-a-named", "/srv/app/out.js", body + "//# sourceMappingURL=" + dataURL(mapA)},
		{"data-a-url", "https://site.example/js/out.js", body + "//# sourceMappingURL=" + dataURL(mapA)},
		{"data-a-rel", "js/out.js", body + "//# sourceMappingURL=" + dataURL(mapA)},
		{"data-b", "", body + "//# sourceMappingURL=" + dataURL(mapB)},
		{"data-c", "file:///x/y.js", body + "//# sourceMappingURL=" + dataURL(mapC)},
		{"data-trailing", "", body + "//# sourceMappingURL=" + dataURL(mapA) + "\n})\n\n"},
		{"data-crlf", "", strings.ReplaceAll(body, "\n", "\r\n") + "//# sourceMappingURL=" + dataURL(mapA) + "\r"},
		{"data-empty", "", body + "//# sourceMappingURL=data:application/json;base64,"},
		{"data-bad-b64", "", body + "//# sourceMappingURL=data:application/json;base64,e30"},
		{"data-no-comma", "", body + "//# sourceMappingURL=data:application/json"},
		{"data-utf8", "", body + "//# sourceMappingURL=data:application/json;charset=utf-8;base64," + base64.StdEncoding.EncodeToString([]byte(mapA))},
		{"bad-data", "", body + "//# sourceMappingURL=data:application/json;base64,e30="},
		{"bad-json", "", body + "//# sourceMappingURL=" + dataURL("{x")},
		{"http-scheme", "", body + "//# sourceMappingURL=http://example.com/x.map"},
		{"url-unparsable", "", body + "//# sourceMappingURL=%zz"},
		{"url-colon", "", body + "//# sourceMappingURL=a:b"},
		{"not-last", "", "//# sourceMappingURL=" + dataURL(mapA) + "\nvar z = 1"},
		{"spaces", "", body + "//# sourceMappingURL= " + dataURL(mapA)},
		{"prefix-only", "", body + "//# sourceMappingURL="},
		{"double-slash", "", body + "// # sourceMappingURL=" + dataURL(mapA)},
		{"syntax-error-too", "", "var = 1;\n//# sourceMappingURL=" + dataURL(mapA)},
		{"missing-file", "", body + "//# sourceMappingURL=missing.js.map"},
	}
	var lines goldenio.Lines
	for _, s := range scripts {
		prg, err := parser.ParseFile(nil, s.filename, s.src, 0)
		m := map[string]any{"name": s.name, "filename": s.filename, "src": s.src}
		if err != nil {
			list, ok := err.(parser.ErrorList)
			if !ok {
				m["err"] = "other"
			} else {
				classes := []string{}
				for _, e := range list {
					switch {
					case strings.HasPrefix(e.Message, "Could not load source map: "):
						classes = append(classes, "load")
					case strings.HasPrefix(e.Message, "Could not parse source map: "):
						classes = append(classes, "parse")
					default:
						classes = append(classes, e.Error())
					}
				}
				m["err"] = classes
			}
		}
		if prg != nil && prg.File != nil {
			var pos []any
			for off := 0; off <= len(s.src); off += 3 {
				p := prg.File.Position(off)
				pos = append(pos, []any{off, p.Filename, p.Line, p.Column})
			}
			m["positions"] = pos
		}
		lines.Add(m)
	}
	return lines.Write(out, "sourcemap/scripts.jsonl")
}
