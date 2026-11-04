package hook

import (
	"reflect"
	"sort"
	"sync"

	"github.com/dop251/goja/ast"
	"github.com/dop251/goja/parser"
	"github.com/taymour/elysiandb/internal/datastore"
	engine "github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/storage"
)

type indexNeed struct {
	entity string
	field  string
}

func scriptNeeds(script string) []indexNeed {
	program, err := parser.ParseFile(nil, "", script, 0)
	if err != nil {
		return nil
	}

	seen := map[indexNeed]bool{}
	walkAST(reflect.ValueOf(program), func(call *ast.CallExpression) {
		dot, ok := call.Callee.(*ast.DotExpression)
		if !ok || dot.Identifier.Name != "query" || len(call.ArgumentList) < 2 {
			return
		}

		if id, ok := dot.Left.(*ast.Identifier); !ok || id.Name != "ctx" {
			return
		}

		target, ok := call.ArgumentList[0].(*ast.StringLiteral)
		if !ok {
			return
		}

		object, ok := call.ArgumentList[1].(*ast.ObjectLiteral)
		if !ok {
			return
		}

		for _, prop := range object.Value {
			keyed, ok := prop.(*ast.PropertyKeyed)
			if !ok || keyed.Computed {
				continue
			}

			var field string
			switch k := keyed.Key.(type) {
			case *ast.StringLiteral:
				field = k.Value.String()
			case *ast.Identifier:
				field = k.Name.String()
			}

			if field != "" && !engine.IsCoreEntityType(target.Value.String()) {
				seen[indexNeed{entity: target.Value.String(), field: field}] = true
			}
		}
	})

	out := make([]indexNeed, 0, len(seen))
	for n := range seen {
		out = append(out, n)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].entity != out[j].entity {
			return out[i].entity < out[j].entity
		}
		return out[i].field < out[j].field
	})

	return out
}

var callType = reflect.TypeOf(&ast.CallExpression{})

func walkAST(v reflect.Value, fn func(*ast.CallExpression)) {
	switch v.Kind() {
	case reflect.Interface, reflect.Pointer:
		if v.IsNil() {
			return
		}
		if v.Type() == callType {
			fn(v.Interface().(*ast.CallExpression))
		}
		walkAST(v.Elem(), fn)
	case reflect.Struct:
		for i := 0; i < v.NumField(); i++ {
			if v.Type().Field(i).IsExported() {
				walkAST(v.Field(i), fn)
			}
		}
	case reflect.Slice:
		for i := 0; i < v.Len(); i++ {
			walkAST(v.Index(i), fn)
		}
	}
}

type preparedIndexes struct {
	seen     sync.Map
	mu       sync.Mutex
	engine   *datastore.Engine
	pinned   map[indexNeed]bool
	observed map[string]map[indexNeed]bool
}

var prepared = &preparedIndexes{pinned: map[indexNeed]bool{}, observed: map[string]map[indexNeed]bool{}}

func filterSpec(field string) datastore.IndexSpec {
	return datastore.IndexSpec{Kind: datastore.IndexFilter, Field: field}
}

func (p *preparedIndexes) sync(e *datastore.Engine, list []registeredHook) {
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.engine != e {
		p.engine = e
		p.pinned = map[indexNeed]bool{}
		p.observed = map[string]map[indexNeed]bool{}
		p.seen.Clear()
	}

	scripts := map[string]bool{}
	want := map[indexNeed]bool{}
	for _, h := range list {
		if !h.hook.Enabled {
			continue
		}
		scripts[h.hook.Script] = true
		for _, n := range scriptNeeds(h.hook.Script) {
			want[n] = true
		}
	}

	for script, needs := range p.observed {
		if !scripts[script] {
			delete(p.observed, script)
			continue
		}
		for n := range needs {
			want[n] = true
		}
	}

	p.apply(e, want)
}

func (p *preparedIndexes) apply(e *datastore.Engine, want map[indexNeed]bool) {
	for n := range p.pinned {
		if !want[n] {
			e.UnpinIndex(n.entity, filterSpec(n.field))
			delete(p.pinned, n)
		}
	}

	var fresh []indexNeed
	for n := range want {
		if !p.pinned[n] {
			e.PinIndex(n.entity, filterSpec(n.field))
			p.pinned[n] = true
			fresh = append(fresh, n)
		}
	}

	if len(fresh) == 0 {
		return
	}

	go func() {
		for _, n := range fresh {
			if e.TypeExists(n.entity) || e.Snapshot(n.entity).Len() > 0 {
				if !hasFilterIndex(e, n) {
					e.EnsureIndexWith(n.entity, filterSpec(n.field), datastore.OriginHook)
				}
			}
		}
	}()
}

func hasFilterIndex(e *datastore.Engine, n indexNeed) bool {
	snap := e.Snapshot(n.entity)

	return snap.HasIndex(filterSpec(n.field)) && !snap.IndexDormant(filterSpec(n.field)) ||
		snap.HasIndex(datastore.IndexSpec{Kind: datastore.IndexBitmap, Field: n.field})
}

func (p *preparedIndexes) observe(script, entity string, filters map[string]map[string]string) {
	known := true
	for field := range filters {
		if _, ok := p.seen.Load(script + "\x00" + entity + "\x00" + field); !ok {
			known = false
			break
		}
	}
	if known {
		return
	}

	e := storage.Engine()
	if e == nil || !engine.IsEngineInternal() {
		return
	}

	for field := range filters {
		p.seen.Store(script+"\x00"+entity+"\x00"+field, true)
	}

	p.mu.Lock()
	defer p.mu.Unlock()

	if p.engine != e {
		return
	}

	needs := p.observed[script]
	added := map[indexNeed]bool{}
	for field := range filters {
		n := indexNeed{entity: entity, field: field}
		if needs[n] {
			continue
		}
		if needs == nil {
			needs = map[indexNeed]bool{}
			p.observed[script] = needs
		}
		needs[n] = true
		added[n] = true
	}

	if len(added) == 0 {
		return
	}

	want := map[indexNeed]bool{}
	for n := range p.pinned {
		want[n] = true
	}
	for n := range added {
		want[n] = true
	}

	p.apply(e, want)
}
