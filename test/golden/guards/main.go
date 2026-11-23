package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var removedByD17 = []string{
	"key\tmonitoring.pprof",
	"key\tmonitoring.pprof.enabled",
	"route\tGET /debug/pprof/{profile:*}",
}

func main() {
	goldenio.Main("guards", run)
}

func run(out string) error {
	keys := configKeys(reflect.TypeOf(configuration.Config{}), "")
	routes := registeredRoutes()
	commands := commandNames()
	if len(keys) != 81 || len(routes) != 63 || len(commands) != 9 {
		return fmt.Errorf("Go capabilities: %d keys, %d routes, %d commands", len(keys), len(routes), len(commands))
	}
	var goRows []string
	goRows = appendKind(goRows, "key", keys)
	goRows = appendKind(goRows, "route", routes)
	goRows = appendKind(goRows, "command", commands)
	if err := goldenio.WriteFile(out, "capabilities-go.tsv", tsv(goRows)); err != nil {
		return err
	}
	removed := map[string]bool{}
	for _, row := range removedByD17 {
		removed[row] = true
	}
	var rustRows []string
	for _, row := range goRows {
		if removed[row] {
			delete(removed, row)
			continue
		}
		rustRows = append(rustRows, row)
	}
	if len(removed) != 0 || len(rustRows) != 79+62+9 {
		return fmt.Errorf("D17 removals not found in the Go capabilities")
	}
	if err := goldenio.WriteFile(out, "capabilities.tsv", tsv(rustRows)); err != nil {
		return err
	}
	tests, err := goTests(".")
	if err != nil {
		return err
	}
	if err := goldenio.WriteFile(out, "go-tests.tsv", tsv(tests)); err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "guards: %d keys, %d routes, %d commands, %d test rows\n", len(keys), len(routes), len(commands), len(tests))
	return nil
}

func appendKind(rows []string, kind string, values []string) []string {
	for _, v := range values {
		rows = append(rows, kind+"\t"+v)
	}
	return rows
}

func tsv(rows []string) []byte {
	var b strings.Builder
	for _, row := range rows {
		b.WriteString(row)
		b.WriteByte('\n')
	}
	return []byte(b.String())
}

func sortedSet(m map[string]bool) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

func configKeys(t reflect.Type, prefix string) []string {
	set := map[string]bool{}
	collectKeys(t, prefix, set)
	return sortedSet(set)
}

func collectKeys(t reflect.Type, prefix string, set map[string]bool) {
	for t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	if t.Kind() != reflect.Struct {
		return
	}
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		tag := strings.Split(f.Tag.Get("yaml"), ",")[0]
		if tag == "" || tag == "-" {
			continue
		}
		key := tag
		if prefix != "" {
			key = prefix + "." + tag
		}
		set[key] = true
		collectKeys(f.Type, key, set)
	}
}

func registeredRoutes() []string {
	cfg := &configuration.Config{}
	cfg.Stats.Enabled = true
	cfg.Api.Schema.Enabled = true
	cfg.Api.Hooks.Enabled = true
	cfg.Monitoring.Pprof.Enabled = true
	cfg.Monitoring.Prometheus.Enabled = true
	cfg.AdminUI.Enabled = true
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	cfg.Log.Level = "error"
	globals.SetConfig(cfg)
	r := router.New()
	routing.RegisterRoutes(r)
	set := map[string]bool{}
	for method, paths := range r.List() {
		for _, p := range paths {
			set[method+" "+p] = true
		}
	}
	return sortedSet(set)
}

func commandNames() []string {
	set := map[string]bool{}
	for name := range cmd.GetHandlers() {
		set[name] = true
	}
	return sortedSet(set)
}

func isTestName(name, prefix string) bool {
	if !strings.HasPrefix(name, prefix) {
		return false
	}
	if len(name) == len(prefix) {
		return true
	}
	r, _ := utf8.DecodeRuneInString(name[len(prefix):])
	return !unicode.IsLower(r)
}

func paramIs(fn *ast.FuncDecl, typeName string) bool {
	params := fn.Type.Params.List
	if fn.Recv != nil || len(params) != 1 || len(params[0].Names) > 1 || fn.Type.Results != nil {
		return false
	}
	star, ok := params[0].Type.(*ast.StarExpr)
	if !ok {
		return false
	}
	sel, ok := star.X.(*ast.SelectorExpr)
	if !ok {
		return false
	}
	pkg, ok := sel.X.(*ast.Ident)
	return ok && pkg.Name == "testing" && sel.Sel.Name == typeName
}

func goTests(root string) ([]string, error) {
	var rows []string
	fset := token.NewFileSet()
	base := filepath.Join(root, "test")
	err := filepath.WalkDir(base, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		if d.IsDir() {
			if rel == "test/golden" || d.Name() == "testdata" {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(path, "_test.go") {
			return nil
		}
		file, err := parser.ParseFile(fset, path, nil, parser.SkipObjectResolution)
		if err != nil {
			return err
		}
		dir := filepath.ToSlash(filepath.Dir(rel))
		for _, decl := range file.Decls {
			fn, ok := decl.(*ast.FuncDecl)
			if !ok {
				continue
			}
			name := fn.Name.Name
			switch {
			case name == "TestMain":
			case isTestName(name, "Test") && paramIs(fn, "T"):
				rows = append(rows, dir+"\t"+name)
			case isTestName(name, "Fuzz") && paramIs(fn, "F"):
				rows = append(rows, dir+"\t"+name)
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Strings(rows)
	for i := 1; i < len(rows); i++ {
		if rows[i] == rows[i-1] {
			return nil, fmt.Errorf("duplicate test %s", rows[i])
		}
	}
	return rows, nil
}
