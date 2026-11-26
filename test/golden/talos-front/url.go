package main

import (
	"errors"
	"net/url"
	"path"
	"strings"

	"github.com/dop251/goja/file"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var urlInputs = []string{
	"", "*", "a", "a.js", "./a.js", "../a.js", "/a.js", "//a.js", "///a.js", "a/b/../c.js", "/a/./b/../../c",
	"http://example.com", "http://example.com/", "http://example.com/a/b.js", "HTTP://Example.com/A", "https://user@host/p",
	"https://user:pass@host:8080/p?q=1#frag", "http://[::1]:80/x", "http://[fe80::1%25en0]/x", "http://[::1", "http://host:port/",
	"http://host:80x/", "http://ho st/", "http://h%41st/", "http://h%c3%a9st/", "http://%zz/", "http://a@b@c/", "http://us er@h/",
	"file:///tmp/a.js.map", "file:a.js", "file:/a.js", "file://a.js", "z:/file.map", "C:\\dir\\file.map", "mailto:x@y.z",
	"data:application/json;base64,e30=", ":foo", "1http://x", "a:b", "a/b:c", "./a:b", "%zz", "%2", "%41%42", "a%20b.map",
	"a b.map", "a\tb", "a\x7fb", "?", "a?", "a??", "a?b?c", "?x", "#", "a#", "a#b#c", "#%zz", "#%41", "a#b c", "/a%2Fb/c",
	"/a%2fb", "/%7e", "/~", "/a;b,c", "/a?b", "http://x/a?b?#c", "javascript:void(0)", "http:", "http:/", "http://",
	"http:///", "http:///a", "http:a/b", "foo://bar/../baz", "/a/b/c/./../../g", "mid/content=5/../6", "é.js", "/é/ü.js",
	"http://héllo/", "http://x/é", "//host/path", "//user@host", "//[::1]:8080/x", "\x00", "a\nb", "-x:y", "+x:y", ".x:y",
	"x1+-.:y", "X1:y", "http://x/%2e%2e/a", "../../..", "..", ".", "/..", "/.", "a/..", "a/.", "a//b", "//", "/a//",
}

var resolveBases = []string{
	"", "base.js", "../base.js", "/somewhere/base.js", "file:///somewhere/base.js", "https://example.com/somewhere/else/base.js",
	"http://a/b/c/d;p?q", "http://a/b/c/d;p?q#f", "http://a", "mailto:x", "/", "a/b/", "http://u:p@h:1/x/y?z#w", "//h/p",
}

var resolveRefs = []string{
	"", "g", "./g", "g/", "/g", "//g", "?y", "g?y", "#s", "g#s", "g?y#s", ";x", "g;x", "g;x?y#s", ".", "./", "..", "../", "../g",
	"../..", "../../", "../../g", "../../../g", "/./g", "/../g", "g.", ".g", "g..", "..g", "./../g", "./g/.", "g/./h", "g/../h",
	"g;x=1/./y", "g;x=1/../y", "http:g", "http://x/y", "mailto:z", "a%2Fb", "%zz", "?", "#", "test.js", "../test.js", "/test.js",
	"file:///test.js", "z:/file.map", "a b", "é",
}

func urlFields(u *url.URL) map[string]any {
	m := map[string]any{
		"scheme":       u.Scheme,
		"opaque":       u.Opaque,
		"host":         u.Host,
		"path":         u.Path,
		"raw_path":     u.RawPath,
		"omit_host":    u.OmitHost,
		"force_query":  u.ForceQuery,
		"raw_query":    u.RawQuery,
		"fragment":     u.Fragment,
		"raw_fragment": u.RawFragment,
		"string":       u.String(),
		"escaped_path": u.EscapedPath(),
		"is_abs":       u.IsAbs(),
		"user":         nil,
	}
	if u.User != nil {
		p, set := u.User.Password()
		m["user"] = map[string]any{"username": u.User.Username(), "password": p, "password_set": set, "string": u.User.String()}
	}
	return m
}

func urlErrClass(err error) string {
	var ue *url.Error
	if !errors.As(err, &ue) {
		return "other"
	}
	var ee url.EscapeError
	if errors.As(ue.Err, &ee) {
		return "escape:" + string(ee)
	}
	var he url.InvalidHostError
	if errors.As(ue.Err, &he) {
		return "host:" + string(he)
	}
	msg := ue.Err.Error()
	switch {
	case msg == "net/url: invalid control character in URL":
		return "control"
	case msg == "missing protocol scheme":
		return "scheme"
	case msg == "first path segment in URL cannot contain colon":
		return "colon"
	case strings.HasPrefix(msg, "invalid port "):
		return "port"
	case msg == "net/url: invalid userinfo":
		return "userinfo"
	case msg == "missing ']' in host":
		return "bracket"
	}
	return "other"
}

func genURL(out string) error {
	var parsed goldenio.Lines
	for _, s := range urlInputs {
		u, err := url.Parse(s)
		m := map[string]any{"in": s}
		if err != nil {
			m["err"] = urlErrClass(err)
		} else {
			m["url"] = urlFields(u)
		}
		parsed.Add(m)
	}
	if err := parsed.Write(out, "url/parse.jsonl"); err != nil {
		return err
	}

	var res goldenio.Lines
	for _, b := range resolveBases {
		bu, err := url.Parse(b)
		if err != nil {
			continue
		}
		for _, r := range resolveRefs {
			ru, err := url.Parse(r)
			if err != nil {
				continue
			}
			res.Add(map[string]any{"base": b, "ref": r, "url": urlFields(bu.ResolveReference(ru))})
		}
	}
	if err := res.Write(out, "url/resolve.jsonl"); err != nil {
		return err
	}

	var smu goldenio.Lines
	sources := append([]string{"\ntest.js", "\rtest2.js\t\n  ", " a.map ", "\x00.map"}, resolveRefs...)
	bases := append([]string{"base123.js", "\tbase.js", "a%zz/b.js", "%zz"}, resolveBases...)
	for _, b := range bases {
		for _, s := range sources {
			u := file.ResolveSourcemapURL(b, s)
			m := map[string]any{"basename": b, "source": s, "url": nil}
			if u != nil {
				m["url"] = urlFields(u)
			}
			smu.Add(m)
		}
	}
	if err := smu.Write(out, "url/resolve_sourcemap.jsonl"); err != nil {
		return err
	}

	var paths goldenio.Lines
	pathInputs := []string{"", ".", "..", "/", "//", "a", "/a", "a/", "a/b", "a//b", "/a/b/", "./a", "../a", "a/../b", "a/./b", "/../a", "../../a/b/..", "a/b/c/../../..", "/a/b/c/../../../..", "abc/def/../../..", "a/.b", "a/..b", ".../a", "~/x"}
	for _, a := range pathInputs {
		paths.Add(map[string]any{"op": "clean", "a": a, "r": path.Clean(a)})
		paths.Add(map[string]any{"op": "dir", "a": a, "r": path.Dir(a)})
		paths.Add(map[string]any{"op": "isabs", "a": a, "r": path.IsAbs(a)})
		for _, b := range []string{"", "x", "/x", "../x", "x/", "."} {
			paths.Add(map[string]any{"op": "join", "a": a, "b": b, "r": path.Join(a, b)})
		}
	}
	return paths.Write(out, "url/path.jsonl")
}
