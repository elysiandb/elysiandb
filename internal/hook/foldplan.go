package hook

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"sync"

	"github.com/dop251/goja"
	"github.com/dop251/goja/ast"
	"github.com/dop251/goja/parser"
	"github.com/dop251/goja/token"
)

const (
	foldGlobal      = "__elysian_hook"
	foldResultVar   = "__elysian_r"
	maxHelperDepth  = 8
	maxScriptPlans  = 1024
	entityFreeLabel = "\x00entity."
)

var errNotFoldable = errors.New("not foldable")

var foldDenied = []string{
	"prototype", "__proto__", "constructor", "eval", "Function", "globalThis", "Reflect", "Proxy",
	"arguments", "setPrototypeOf", "getPrototypeOf", "defineProperty", "__elysian",
}

var objectMembers = map[string]bool{
	"constructor": true, "hasOwnProperty": true, "isPrototypeOf": true, "propertyIsEnumerable": true,
	"toLocaleString": true, "toString": true, "valueOf": true, "__proto__": true,
	"__defineGetter__": true, "__defineSetter__": true, "__lookupGetter__": true, "__lookupSetter__": true,
}

type foldDef struct {
	id           string
	target       string
	partField    string
	baseFilters  map[string]map[string]string
	staticBase   bool
	fields       []string
	regs         []string
	frees        []string
	lits         []string
	locals       int
	body         stmt
	accumulating bool
}

type scriptPlan struct {
	program *goja.Program
	folds   []*foldDef
}

type planKey struct {
	script string
	fn     string
}

var (
	plansMu sync.Mutex
	plans   = map[planKey]*scriptPlan{}
)

func planFor(script, fnName string) *scriptPlan {
	key := planKey{script: script, fn: fnName}

	plansMu.Lock()
	p, ok := plans[key]
	plansMu.Unlock()
	if ok {
		return p
	}

	p = buildPlan(script, fnName)

	plansMu.Lock()
	if len(plans) >= maxScriptPlans {
		plans = map[planKey]*scriptPlan{}
	}
	plans[key] = p
	plansMu.Unlock()

	return p
}

func buildPlan(script, fnName string) *scriptPlan {
	for _, word := range foldDenied {
		if strings.Contains(script, word) {
			return nil
		}
	}

	program, err := parser.ParseFile(nil, "", script, 0)
	if err != nil {
		return nil
	}

	helpers := map[string]*ast.FunctionLiteral{}
	var entry *ast.FunctionLiteral
	for _, st := range program.Body {
		decl, ok := st.(*ast.FunctionDeclaration)
		if !ok {
			return nil
		}

		name := decl.Function.Name.Name.String()
		if _, dup := helpers[name]; dup {
			return nil
		}
		helpers[name] = decl.Function
		if name == fnName {
			entry = decl.Function
		}
	}

	if entry == nil || len(entry.ParameterList.List) != 1 || entry.ParameterList.Rest != nil {
		return nil
	}

	param, ok := entry.ParameterList.List[0].Target.(*ast.Identifier)
	if !ok || entry.ParameterList.List[0].Initializer != nil {
		return nil
	}

	if !safeAssignments(program) {
		return nil
	}

	a := &analysis{script: script, fn: fnName, helpers: helpers, entry: entry, param: param.Name.String()}

	return a.plan()
}

type edit struct {
	from, to int
	text     string
}

type analysis struct {
	script  string
	fn      string
	helpers map[string]*ast.FunctionLiteral
	entry   *ast.FunctionLiteral
	param   string
}

type queryDecl struct {
	name    string
	call    *ast.CallExpression
	target  string
	base    map[string]map[string]string
	static  bool
	part    string
	binding *ast.Binding
}

func (a *analysis) plan() *scriptPlan {
	queries := map[string]*queryDecl{}
	for _, st := range a.entry.Body.List {
		vs, ok := st.(*ast.VariableStatement)
		if !ok {
			continue
		}

		for _, b := range vs.List {
			id, ok := b.Target.(*ast.Identifier)
			if !ok {
				continue
			}

			call, ok := b.Initializer.(*ast.CallExpression)
			if !ok {
				continue
			}

			if q := a.queryCall(id.Name.String(), call); q != nil {
				if _, dup := queries[q.name]; dup {
					delete(queries, q.name)
					continue
				}
				q.binding = b
				queries[q.name] = q
			}
		}
	}

	if len(queries) == 0 {
		return nil
	}

	var defs []*foldDef
	var loops []*ast.ForStatement
	loopsByQuery := map[string][]int{}
	for _, st := range a.entry.Body.List {
		loop, ok := st.(*ast.ForStatement)
		if !ok {
			continue
		}

		name, idx, ok := loopShape(loop)
		if !ok {
			continue
		}

		q, ok := queries[name]
		if !ok {
			continue
		}

		def, err := a.compileLoop(q, loop, idx, len(defs))
		if err != nil {
			continue
		}

		loopsByQuery[name] = append(loopsByQuery[name], len(defs))
		defs = append(defs, def)
		loops = append(loops, loop)
	}

	if len(defs) == 0 {
		return nil
	}

	var edits []edit
	kept := make([]*foldDef, 0, len(defs))
	for name, q := range queries {
		list := loopsByQuery[name]
		if len(list) == 0 || !a.usedOnlyInLoops(name, list, loops, q) {
			continue
		}

		callee := q.call.Callee
		edits = append(edits, edit{from: int(callee.Idx0()) - 1, to: int(callee.Idx1()) - 1, text: foldGlobal + ".view"})
		for _, i := range list {
			def := defs[i]
			loop := loops[i]
			start, end := int(loop.Idx0())-1, int(loop.Idx1())-1
			edits = append(edits, edit{from: start, to: end, text: foldCallText(len(kept), name, def, a.script[start:end], a.param)})
			def.id = foldID(a.script, a.fn, len(kept))
			kept = append(kept, def)
		}
	}

	if len(kept) == 0 {
		return nil
	}

	sort.Slice(edits, func(i, j int) bool { return edits[i].from > edits[j].from })
	src := a.script
	for _, e := range edits {
		src = src[:e.from] + e.text + src[e.to:]
	}

	program, err := goja.Compile("", src, false)
	if err != nil {
		return nil
	}

	return &scriptPlan{program: program, folds: kept}
}

func foldID(script, fn string, index int) string {
	h := sha256.Sum256([]byte(script + "\x00" + fn + "\x00" + strconv.Itoa(index)))

	return string(h[:16])
}

func foldCallText(index int, query string, def *foldDef, loop, param string) string {
	args := []string{strconv.Itoa(index), query}
	args = append(args, def.regs...)
	for _, f := range def.frees {
		if rest, ok := strings.CutPrefix(f, entityFreeLabel); ok {
			args = append(args, param+".entity["+strconv.Quote(rest)+"]")
		} else {
			args = append(args, f)
		}
	}

	result := foldResultVar + strconv.Itoa(index)

	var b strings.Builder
	fmt.Fprintf(&b, "var %s = %s.fold(%s); if (%s) { ", result, foldGlobal, strings.Join(args, ", "), result)
	for i, r := range def.regs {
		fmt.Fprintf(&b, "%s = %s[%d]; ", r, result, i)
	}
	fmt.Fprintf(&b, "} else { %s = %s.rows(%s); %s }", query, foldGlobal, query, loop)

	return b.String()
}

func (a *analysis) queryCall(name string, call *ast.CallExpression) *queryDecl {
	dot, ok := call.Callee.(*ast.DotExpression)
	if !ok || dot.Identifier.Name != "query" || len(call.ArgumentList) != 2 {
		return nil
	}

	if id, ok := dot.Left.(*ast.Identifier); !ok || id.Name.String() != a.param {
		return nil
	}

	target, ok := call.ArgumentList[0].(*ast.StringLiteral)
	if !ok {
		return nil
	}

	object, ok := call.ArgumentList[1].(*ast.ObjectLiteral)
	if !ok {
		return nil
	}

	q := &queryDecl{name: name, call: call, target: target.Value.String(), base: map[string]map[string]string{}, static: true}
	for _, prop := range object.Value {
		keyed, ok := prop.(*ast.PropertyKeyed)
		if !ok || keyed.Computed {
			return nil
		}

		field := literalKey(keyed.Key)
		cond, ok := keyed.Value.(*ast.ObjectLiteral)
		if field == "" || !ok || strings.Contains(field, ".") {
			return nil
		}

		ops := map[string]string{}
		dynamic := false
		for _, op := range cond.Value {
			kv, ok := op.(*ast.PropertyKeyed)
			if !ok || kv.Computed {
				return nil
			}

			name := literalKey(kv.Key)
			if name == "" {
				return nil
			}

			switch lit := kv.Value.(type) {
			case *ast.StringLiteral:
				ops[name] = lit.Value.String()
			case *ast.NumberLiteral, *ast.BooleanLiteral:
				q.static = false
			default:
				if !a.pureExpression(kv.Value) {
					return nil
				}
				if name != "eq" || len(cond.Value) != 1 {
					return nil
				}
				dynamic = true
			}
		}

		if dynamic {
			if q.part != "" {
				return nil
			}
			q.part = field
			continue
		}

		q.base[field] = ops
	}

	return q
}

func (a *analysis) pureExpression(e ast.Expression) bool {
	switch t := e.(type) {
	case *ast.Identifier:
		return true
	case *ast.StringLiteral, *ast.NumberLiteral, *ast.BooleanLiteral, *ast.NullLiteral:
		return true
	case *ast.DotExpression:
		return a.pureExpression(t.Left)
	}

	return false
}

func literalKey(k ast.Expression) string {
	switch t := k.(type) {
	case *ast.StringLiteral:
		return t.Value.String()
	case *ast.Identifier:
		return t.Name.String()
	}

	return ""
}

func loopShape(loop *ast.ForStatement) (string, string, bool) {
	if _, ok := loop.Body.(*ast.BlockStatement); !ok {
		return "", "", false
	}

	idx := ""
	switch init := loop.Initializer.(type) {
	case *ast.ForLoopInitializerVarDeclList:
		if len(init.List) != 1 {
			return "", "", false
		}
		id, ok := init.List[0].Target.(*ast.Identifier)
		if !ok || !isZero(init.List[0].Initializer) {
			return "", "", false
		}
		idx = id.Name.String()
	default:
		return "", "", false
	}

	test, ok := loop.Test.(*ast.BinaryExpression)
	if !ok || test.Operator != token.LESS {
		return "", "", false
	}

	left, ok := test.Left.(*ast.Identifier)
	if !ok || left.Name.String() != idx {
		return "", "", false
	}

	length, ok := test.Right.(*ast.DotExpression)
	if !ok || length.Identifier.Name != "length" {
		return "", "", false
	}

	query, ok := length.Left.(*ast.Identifier)
	if !ok {
		return "", "", false
	}

	update, ok := loop.Update.(*ast.UnaryExpression)
	if !ok || update.Operator != token.INCREMENT {
		return "", "", false
	}

	if id, ok := update.Operand.(*ast.Identifier); !ok || id.Name.String() != idx {
		return "", "", false
	}

	return query.Name.String(), idx, true
}

func isZero(e ast.Expression) bool {
	n, ok := e.(*ast.NumberLiteral)
	if !ok {
		return false
	}

	switch v := n.Value.(type) {
	case int64:
		return v == 0
	case float64:
		return v == 0
	}

	return false
}

func walkNodes(v reflect.Value, fn func(any) bool) {
	switch v.Kind() {
	case reflect.Interface, reflect.Pointer:
		if v.IsNil() {
			return
		}
		if v.Kind() == reflect.Pointer && !fn(v.Interface()) {
			return
		}
		walkNodes(v.Elem(), fn)
	case reflect.Struct:
		for i := 0; i < v.NumField(); i++ {
			if v.Type().Field(i).IsExported() {
				walkNodes(v.Field(i), fn)
			}
		}
	case reflect.Slice:
		for i := 0; i < v.Len(); i++ {
			walkNodes(v.Index(i), fn)
		}
	}
}

func rootIdentifier(e ast.Expression) string {
	for {
		switch t := e.(type) {
		case *ast.Identifier:
			return t.Name.String()
		case *ast.DotExpression:
			e = t.Left
		case *ast.BracketExpression:
			e = t.Left
		default:
			return ""
		}
	}
}

func safeAssignments(program *ast.Program) bool {
	safe := true
	for _, st := range program.Body {
		fn := st.(*ast.FunctionDeclaration).Function
		locals := map[string]bool{}
		for _, b := range fn.ParameterList.List {
			if id, ok := b.Target.(*ast.Identifier); ok {
				locals[id.Name.String()] = true
			}
		}
		walkNodes(reflect.ValueOf(fn.Body), func(n any) bool {
			switch t := n.(type) {
			case *ast.VariableStatement:
				for _, b := range t.List {
					if id, ok := b.Target.(*ast.Identifier); ok {
						locals[id.Name.String()] = true
					}
				}
			case *ast.ForLoopInitializerVarDeclList:
				for _, b := range t.List {
					if id, ok := b.Target.(*ast.Identifier); ok {
						locals[id.Name.String()] = true
					}
				}
			case *ast.FunctionLiteral, *ast.ArrowFunctionLiteral:
				safe = false
			case *ast.WithStatement, *ast.ThisExpression, *ast.TryStatement, *ast.YieldExpression, *ast.AwaitExpression:
				safe = false
			case *ast.BracketExpression:
				switch t.Member.(type) {
				case *ast.StringLiteral, *ast.NumberLiteral, *ast.Identifier:
				default:
					safe = false
				}
			}
			return true
		})

		walkNodes(reflect.ValueOf(fn.Body), func(n any) bool {
			switch t := n.(type) {
			case *ast.AssignExpression:
				if !locals[rootIdentifier(t.Left)] {
					safe = false
				}
			case *ast.UnaryExpression:
				if t.Operator == token.INCREMENT || t.Operator == token.DECREMENT || t.Operator == token.DELETE {
					if !locals[rootIdentifier(t.Operand)] {
						safe = false
					}
				}
			}
			return true
		})
	}

	return safe
}

func collectIdentifiers(node any, into map[string]int) {
	walkNodes(reflect.ValueOf(node), func(n any) bool {
		switch t := n.(type) {
		case *ast.Identifier:
			into[t.Name.String()]++
		case *ast.PropertyKeyed:
			if !t.Computed {
				collectIdentifiers(t.Value, into)
				return false
			}
		}
		return true
	})
}

func (a *analysis) usedOnlyInLoops(name string, list []int, loops []*ast.ForStatement, q *queryDecl) bool {
	inLoop := map[*ast.ForStatement]bool{}
	for _, i := range list {
		inLoop[loops[i]] = true
	}

	for _, st := range a.entry.Body.List {
		if loop, ok := st.(*ast.ForStatement); ok && inLoop[loop] {
			continue
		}

		used := map[string]int{}
		if vs, ok := st.(*ast.VariableStatement); ok {
			for _, b := range vs.List {
				if b == q.binding {
					continue
				}
				collectIdentifiers(b.Initializer, used)
			}
		} else {
			collectIdentifiers(st, used)
		}

		if used[name] > 0 {
			return false
		}
	}

	return true
}

func (a *analysis) compileLoop(q *queryDecl, loop *ast.ForStatement, idx string, index int) (*foldDef, error) {
	outside := map[string]int{}
	outer := map[string]bool{}
	for _, st := range a.entry.Body.List {
		if st == ast.Statement(loop) {
			continue
		}
		walkNodes(reflect.ValueOf(st), func(n any) bool {
			switch t := n.(type) {
			case *ast.VariableStatement:
				for _, b := range t.List {
					if id, ok := b.Target.(*ast.Identifier); ok {
						outer[id.Name.String()] = true
					}
				}
			case *ast.ForLoopInitializerVarDeclList:
				for _, b := range t.List {
					if id, ok := b.Target.(*ast.Identifier); ok {
						outer[id.Name.String()] = true
					}
				}
			}
			return true
		})
		collectIdentifiers(st, outside)
	}

	body := loop.Body.(*ast.BlockStatement)
	declared := map[string]bool{}
	assigned := map[string]bool{}
	walkNodes(reflect.ValueOf(body), func(n any) bool {
		switch t := n.(type) {
		case *ast.VariableStatement:
			for _, b := range t.List {
				if id, ok := b.Target.(*ast.Identifier); ok {
					declared[id.Name.String()] = true
				}
			}
		case *ast.AssignExpression:
			if id, ok := t.Left.(*ast.Identifier); ok {
				assigned[id.Name.String()] = true
			}
		case *ast.UnaryExpression:
			if t.Operator == token.INCREMENT || t.Operator == token.DECREMENT {
				if id, ok := t.Operand.(*ast.Identifier); ok {
					assigned[id.Name.String()] = true
				}
			}
		}
		return true
	})

	for name := range declared {
		if outside[name] > 0 || outer[name] || name == idx || name == q.name || name == a.param {
			return nil, errNotFoldable
		}
	}

	if outside[idx] > 0 || outer[idx] || assigned[idx] || assigned[q.name] || assigned[a.param] {
		return nil, errNotFoldable
	}

	c := &foldCompiler{
		a:        a,
		query:    q.name,
		idx:      idx,
		outer:    outer,
		declared: declared,
		assigned: assigned,
		regs:     map[string]int{},
		frees:    map[string]int{},
		fields:   map[string]int{},
		litIndex: map[string]int{},
		scopes:   []*foldScope{{slots: map[string]int{}, set: map[string]bool{}, aliases: map[string]bool{}}},
	}

	fields := map[string]bool{}
	collectRowFields(body, q.name, idx, fields)
	for f := range fields {
		c.fieldNames = append(c.fieldNames, f)
	}
	sort.Strings(c.fieldNames)
	for i, f := range c.fieldNames {
		c.fields[f] = i
	}

	block, _, err := c.stmtList(body.List)
	if err != nil {
		return nil, err
	}

	def := &foldDef{
		target:       q.target,
		partField:    q.part,
		baseFilters:  q.base,
		staticBase:   q.static,
		fields:       c.fieldNames,
		regs:         c.regNames,
		frees:        c.freeNames,
		lits:         c.lits,
		locals:       c.nextLocal,
		body:         block,
		accumulating: !c.regRead && !c.regAssign,
	}

	if len(def.regs) == 0 {
		return nil, errNotFoldable
	}

	return def, nil
}

func collectRowFields(body *ast.BlockStatement, query, idx string, into map[string]bool) {
	isRow := func(e ast.Expression) bool {
		b, ok := e.(*ast.BracketExpression)
		if !ok {
			return false
		}
		left, ok := b.Left.(*ast.Identifier)
		if !ok || left.Name.String() != query {
			return false
		}
		m, ok := b.Member.(*ast.Identifier)
		return ok && m.Name.String() == idx
	}

	aliases := map[string]bool{}
	for _, st := range body.List {
		vs, ok := st.(*ast.VariableStatement)
		if !ok {
			continue
		}
		for _, b := range vs.List {
			if id, ok := b.Target.(*ast.Identifier); ok && isRow(b.Initializer) {
				aliases[id.Name.String()] = true
			}
		}
	}

	isRef := func(e ast.Expression) bool {
		if id, ok := e.(*ast.Identifier); ok {
			return aliases[id.Name.String()]
		}
		return isRow(e)
	}

	walkNodes(reflect.ValueOf(body), func(n any) bool {
		switch t := n.(type) {
		case *ast.DotExpression:
			if isRef(t.Left) {
				into[t.Identifier.Name.String()] = true
			}
		case *ast.BracketExpression:
			if s, ok := t.Member.(*ast.StringLiteral); ok && isRef(t.Left) {
				into[s.Value.String()] = true
			}
		}
		return true
	})
}

type foldScope struct {
	slots   map[string]int
	set     map[string]bool
	aliases map[string]bool
	inner   bool
}

type foldCompiler struct {
	a          *analysis
	query      string
	idx        string
	outer      map[string]bool
	declared   map[string]bool
	assigned   map[string]bool
	regs       map[string]int
	regNames   []string
	frees      map[string]int
	freeNames  []string
	fields     map[string]int
	fieldNames []string
	lits       []string
	litIndex   map[string]int
	nextLocal  int
	regRead    bool
	regAssign  bool
	depth      int
	nested     int
	scopes     []*foldScope
}

func (c *foldCompiler) scope() *foldScope {
	return c.scopes[len(c.scopes)-1]
}

func (c *foldCompiler) localSlot(name string) int {
	s := c.scope()
	if i, ok := s.slots[name]; ok {
		return i
	}

	i := c.nextLocal
	c.nextLocal++
	s.slots[name] = i

	return i
}

func (c *foldCompiler) lit(s string) int {
	if i, ok := c.litIndex[s]; ok {
		return i
	}

	i := len(c.lits)
	c.lits = append(c.lits, s)
	c.litIndex[s] = i

	return i
}

func (c *foldCompiler) reg(name string) int {
	if i, ok := c.regs[name]; ok {
		return i
	}

	i := len(c.regNames)
	c.regs[name] = i
	c.regNames = append(c.regNames, name)

	return i
}

func (c *foldCompiler) free(name string) int {
	if i, ok := c.frees[name]; ok {
		return i
	}

	i := len(c.freeNames)
	c.frees[name] = i
	c.freeNames = append(c.freeNames, name)

	return i
}

func (c *foldCompiler) isLocal(name string) bool {
	s := c.scope()
	if s.inner {
		_, ok := s.slots[name]
		return ok
	}

	return c.declared[name]
}

func (c *foldCompiler) isReg(name string) bool {
	return !c.scope().inner && c.outer[name] && c.assigned[name]
}

func (c *foldCompiler) isFree(name string) bool {
	return !c.scope().inner && c.outer[name] && !c.assigned[name]
}

func (c *foldCompiler) stmtList(list []ast.Statement) (stmt, bool, error) {
	out := make([]stmt, 0, len(list))
	ends := false
	for _, st := range list {
		if ends {
			return nil, false, errNotFoldable
		}

		s, stop, err := c.statement(st)
		if err != nil {
			return nil, false, err
		}

		if s != nil {
			out = append(out, s)
		}
		ends = stop
	}

	switch len(out) {
	case 0:
		return func(*frame) ctl { return ctlNext }, ends, nil
	case 1:
		return out[0], ends, nil
	}

	return func(fr *frame) ctl {
		for _, s := range out {
			if r := s(fr); r != ctlNext {
				return r
			}
		}
		return ctlNext
	}, ends, nil
}

func (c *foldCompiler) statement(st ast.Statement) (stmt, bool, error) {
	switch t := st.(type) {
	case *ast.EmptyStatement:
		return nil, false, nil
	case *ast.BlockStatement:
		return c.stmtList(t.List)
	case *ast.VariableStatement:
		return c.variables(t)
	case *ast.ExpressionStatement:
		s, err := c.effect(t.Expression)
		return s, false, err
	case *ast.BranchStatement:
		if t.Token != token.CONTINUE || t.Label != nil || c.scope().inner {
			return nil, false, errNotFoldable
		}
		return func(*frame) ctl { return ctlContinue }, true, nil
	case *ast.ReturnStatement:
		if !c.scope().inner {
			return nil, false, errNotFoldable
		}
		if t.Argument == nil {
			return func(fr *frame) ctl {
				fr.ret = undefVal
				return ctlReturn
			}, true, nil
		}
		e, err := c.expression(t.Argument)
		if err != nil {
			return nil, false, err
		}
		return func(fr *frame) ctl {
			fr.ret = e(fr)
			return ctlReturn
		}, true, nil
	case *ast.IfStatement:
		return c.ifStatement(t)
	}

	return nil, false, errNotFoldable
}

func (c *foldCompiler) ifStatement(t *ast.IfStatement) (stmt, bool, error) {
	test, err := c.expression(t.Test)
	if err != nil {
		return nil, false, err
	}

	c.nested++
	defer func() { c.nested-- }()

	before := copySet(c.scope().set)
	cons, consEnds, err := c.statement(t.Consequent)
	if err != nil {
		return nil, false, err
	}
	afterCons := c.scope().set

	c.scope().set = copySet(before)
	var alt stmt
	altEnds := false
	if t.Alternate != nil {
		alt, altEnds, err = c.statement(t.Alternate)
		if err != nil {
			return nil, false, err
		}
	}
	afterAlt := c.scope().set

	switch {
	case consEnds && altEnds:
		c.scope().set = before
	case consEnds:
		c.scope().set = afterAlt
	case altEnds:
		c.scope().set = afterCons
	default:
		merged := map[string]bool{}
		for k := range afterCons {
			if afterAlt[k] {
				merged[k] = true
			}
		}
		c.scope().set = merged
	}

	if cons == nil {
		cons = func(*frame) ctl { return ctlNext }
	}

	if alt == nil {
		return func(fr *frame) ctl {
			if fr.truthy(test(fr)) {
				return cons(fr)
			}
			return ctlNext
		}, consEnds && altEnds, nil
	}

	return func(fr *frame) ctl {
		if fr.truthy(test(fr)) {
			return cons(fr)
		}
		return alt(fr)
	}, consEnds && altEnds, nil
}

func copySet(in map[string]bool) map[string]bool {
	out := make(map[string]bool, len(in))
	for k, v := range in {
		out[k] = v
	}

	return out
}

func (c *foldCompiler) variables(t *ast.VariableStatement) (stmt, bool, error) {
	var out []stmt
	for _, b := range t.List {
		id, ok := b.Target.(*ast.Identifier)
		if !ok {
			return nil, false, errNotFoldable
		}

		name := id.Name.String()
		if b.Initializer == nil {
			continue
		}

		if !c.scope().inner && c.isRowExpr(b.Initializer) {
			if c.nested > 0 || c.scope().aliases[name] || c.scope().set[name] {
				return nil, false, errNotFoldable
			}
			c.scope().aliases[name] = true
			continue
		}

		if c.scope().aliases[name] {
			return nil, false, errNotFoldable
		}

		e, err := c.expression(b.Initializer)
		if err != nil {
			return nil, false, err
		}

		slot := c.localSlot(name)
		c.scope().set[name] = true
		out = append(out, func(fr *frame) ctl {
			fr.local[slot] = e(fr)
			return ctlNext
		})
	}

	switch len(out) {
	case 0:
		return nil, false, nil
	case 1:
		return out[0], false, nil
	}

	return func(fr *frame) ctl {
		for _, s := range out {
			s(fr)
		}
		return ctlNext
	}, false, nil
}

func (c *foldCompiler) isRowExpr(e ast.Expression) bool {
	b, ok := e.(*ast.BracketExpression)
	if !ok {
		return false
	}

	left, ok := b.Left.(*ast.Identifier)
	if !ok || left.Name.String() != c.query {
		return false
	}

	m, ok := b.Member.(*ast.Identifier)

	return ok && m.Name.String() == c.idx
}

func (c *foldCompiler) effect(e ast.Expression) (stmt, error) {
	switch t := e.(type) {
	case *ast.AssignExpression:
		return c.assignment(t)
	case *ast.UnaryExpression:
		if t.Operator != token.INCREMENT && t.Operator != token.DECREMENT {
			return nil, errNotFoldable
		}
		id, ok := t.Operand.(*ast.Identifier)
		if !ok {
			return nil, errNotFoldable
		}
		delta := 1.0
		if t.Operator == token.DECREMENT {
			delta = -1
		}
		return c.update(id.Name.String(), delta)
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) update(name string, delta float64) (stmt, error) {
	if c.isReg(name) {
		i := c.reg(name)
		return func(fr *frame) ctl {
			r := &fr.regs[i]
			if r.k != vNum {
				return ctlBail
			}
			r.n += delta
			fr.touched = true
			return ctlNext
		}, nil
	}

	if c.isLocal(name) && c.scope().set[name] {
		slot := c.localSlot(name)
		return func(fr *frame) ctl {
			r := &fr.local[slot]
			if r.k != vNum {
				return ctlBail
			}
			r.n += delta
			return ctlNext
		}, nil
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) assignment(t *ast.AssignExpression) (stmt, error) {
	id, ok := t.Left.(*ast.Identifier)
	if !ok {
		return nil, errNotFoldable
	}

	name := id.Name.String()
	if c.scope().aliases[name] {
		return nil, errNotFoldable
	}

	var op byte
	switch t.Operator {
	case token.ASSIGN:
	case token.PLUS:
		op = '+'
	case token.MINUS:
		op = '-'
	case token.MULTIPLY:
		op = '*'
	default:
		return nil, errNotFoldable
	}

	right, err := c.expression(t.Right)
	if err != nil {
		return nil, err
	}

	if c.isReg(name) {
		i := c.reg(name)
		if op == 0 || op == '*' {
			c.regAssign = true
		}
		return func(fr *frame) ctl {
			v := right(fr)
			if fr.bail {
				return ctlBail
			}
			if op != 0 {
				v = fr.arith(op, fr.regs[i], v)
				if fr.bail {
					return ctlBail
				}
			} else if v.k == vTmp || v.k == vArr {
				return ctlBail
			}
			fr.regs[i] = v
			fr.touched = true
			return ctlNext
		}, nil
	}

	if c.isLocal(name) {
		if op != 0 && !c.scope().set[name] {
			return nil, errNotFoldable
		}
		slot := c.localSlot(name)
		c.scope().set[name] = true
		return func(fr *frame) ctl {
			v := right(fr)
			if op != 0 {
				v = fr.arith(op, fr.local[slot], v)
			}
			fr.local[slot] = v
			return ctlNext
		}, nil
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) rowField(field string) (expr, int, error) {
	if field == "" || objectMembers[field] {
		return nil, 0, errNotFoldable
	}

	col, ok := c.fields[field]
	if !ok {
		return nil, 0, errNotFoldable
	}

	return func(fr *frame) val {
		v := fr.row[col]
		if v.k == vOpaque {
			fr.bail = true
		}
		return v
	}, col, nil
}

func (c *foldCompiler) isRowRef(e ast.Expression) bool {
	if c.isRowExpr(e) {
		return true
	}

	id, ok := e.(*ast.Identifier)

	return ok && !c.scope().inner && c.scope().aliases[id.Name.String()]
}

func (c *foldCompiler) expression(e ast.Expression) (expr, error) {
	switch t := e.(type) {
	case *ast.NumberLiteral:
		var n float64
		switch v := t.Value.(type) {
		case int64:
			n = float64(v)
		case float64:
			n = v
		default:
			return nil, errNotFoldable
		}
		out := numVal(n)
		return func(*frame) val { return out }, nil
	case *ast.StringLiteral:
		i := c.lit(t.Value.String())
		return func(fr *frame) val { return fr.lits[i] }, nil
	case *ast.BooleanLiteral:
		out := boolVal(t.Value)
		return func(*frame) val { return out }, nil
	case *ast.NullLiteral:
		return func(*frame) val { return nullVal }, nil
	case *ast.Identifier:
		return c.identifier(t.Name.String())
	case *ast.DotExpression:
		return c.member(t.Left, t.Identifier.Name.String())
	case *ast.BracketExpression:
		return c.bracket(t)
	case *ast.UnaryExpression:
		return c.unary(t)
	case *ast.BinaryExpression:
		return c.binary(t)
	case *ast.ConditionalExpression:
		return c.conditional(t)
	case *ast.CallExpression:
		return c.call(t)
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) identifier(name string) (expr, error) {
	s := c.scope()
	if s.aliases[name] || (!s.inner && (name == c.query || name == c.idx)) {
		return nil, errNotFoldable
	}

	if c.isLocal(name) {
		if !s.set[name] {
			return nil, errNotFoldable
		}
		slot := c.localSlot(name)
		return func(fr *frame) val { return fr.local[slot] }, nil
	}

	if c.isReg(name) {
		i := c.reg(name)
		c.regRead = true
		return func(fr *frame) val { return fr.regs[i] }, nil
	}

	if c.isFree(name) {
		i := c.free(name)
		return func(fr *frame) val { return fr.free[i] }, nil
	}

	if name == "undefined" {
		return func(*frame) val { return undefVal }, nil
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) entityField(left ast.Expression) bool {
	dot, ok := left.(*ast.DotExpression)
	if !ok || dot.Identifier.Name != "entity" {
		return false
	}

	id, ok := dot.Left.(*ast.Identifier)

	return ok && id.Name.String() == c.a.param
}

func (c *foldCompiler) member(left ast.Expression, name string) (expr, error) {
	if c.isRowRef(left) {
		e, _, err := c.rowField(name)
		return e, err
	}

	if c.entityField(left) && !c.scope().inner {
		if objectMembers[name] || !asciiOnly(name) {
			return nil, errNotFoldable
		}
		i := c.free(entityFreeLabel + name)
		return func(fr *frame) val { return fr.free[i] }, nil
	}

	if name != "length" {
		return nil, errNotFoldable
	}

	e, err := c.expression(left)
	if err != nil {
		return nil, err
	}

	return func(fr *frame) val { return fr.length(e(fr)) }, nil
}

func (c *foldCompiler) bracket(t *ast.BracketExpression) (expr, error) {
	if s, ok := t.Member.(*ast.StringLiteral); ok {
		return c.member(t.Left, s.Value.String())
	}

	n, ok := t.Member.(*ast.NumberLiteral)
	if !ok {
		return nil, errNotFoldable
	}

	idx, ok := n.Value.(int64)
	if !ok {
		return nil, errNotFoldable
	}

	e, err := c.expression(t.Left)
	if err != nil {
		return nil, err
	}

	return func(fr *frame) val { return fr.element(e(fr), int(idx)) }, nil
}

func (c *foldCompiler) unary(t *ast.UnaryExpression) (expr, error) {
	if t.Operator == token.INCREMENT || t.Operator == token.DECREMENT {
		return nil, errNotFoldable
	}

	e, err := c.expression(t.Operand)
	if err != nil {
		return nil, err
	}

	switch t.Operator {
	case token.NOT:
		return func(fr *frame) val { return boolVal(!fr.truthy(e(fr))) }, nil
	case token.MINUS:
		return func(fr *frame) val {
			v := e(fr)
			if v.k != vNum {
				fr.bail = true
				return undefVal
			}
			return numVal(-v.n)
		}, nil
	case token.PLUS:
		return func(fr *frame) val {
			v := e(fr)
			if v.k != vNum {
				fr.bail = true
				return undefVal
			}
			return v
		}, nil
	case token.TYPEOF:
		return func(fr *frame) val { return fr.typeName(e(fr)) }, nil
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) binary(t *ast.BinaryExpression) (expr, error) {
	if t.Operator == token.LOGICAL_AND || t.Operator == token.LOGICAL_OR {
		return c.logical(t)
	}

	left, err := c.expression(t.Left)
	if err != nil {
		return nil, err
	}

	right, err := c.expression(t.Right)
	if err != nil {
		return nil, err
	}

	switch t.Operator {
	case token.PLUS:
		return func(fr *frame) val { return fr.arith('+', left(fr), right(fr)) }, nil
	case token.MINUS:
		return func(fr *frame) val { return fr.arith('-', left(fr), right(fr)) }, nil
	case token.MULTIPLY:
		return func(fr *frame) val { return fr.arith('*', left(fr), right(fr)) }, nil
	case token.SLASH:
		return func(fr *frame) val { return fr.arith('/', left(fr), right(fr)) }, nil
	case token.REMAINDER:
		return func(fr *frame) val { return fr.arith('%', left(fr), right(fr)) }, nil
	case token.STRICT_EQUAL:
		return func(fr *frame) val { return boolVal(fr.strictEq(left(fr), right(fr))) }, nil
	case token.STRICT_NOT_EQUAL:
		return func(fr *frame) val { return boolVal(!fr.strictEq(left(fr), right(fr))) }, nil
	case token.EQUAL:
		return func(fr *frame) val { return boolVal(fr.looseEq(left(fr), right(fr))) }, nil
	case token.NOT_EQUAL:
		return func(fr *frame) val { return boolVal(!fr.looseEq(left(fr), right(fr))) }, nil
	case token.LESS:
		return func(fr *frame) val { return boolVal(fr.less(left(fr), right(fr), true, false)) }, nil
	case token.LESS_OR_EQUAL:
		return func(fr *frame) val { return boolVal(fr.less(left(fr), right(fr), false, false)) }, nil
	case token.GREATER:
		return func(fr *frame) val { return boolVal(fr.less(left(fr), right(fr), true, true)) }, nil
	case token.GREATER_OR_EQUAL:
		return func(fr *frame) val { return boolVal(fr.less(left(fr), right(fr), false, true)) }, nil
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) logical(t *ast.BinaryExpression) (expr, error) {
	left, err := c.expression(t.Left)
	if err != nil {
		return nil, err
	}

	before := copySet(c.scope().set)
	right, err := c.expression(t.Right)
	if err != nil {
		return nil, err
	}
	c.scope().set = before

	if t.Operator == token.LOGICAL_AND {
		return func(fr *frame) val {
			v := left(fr)
			if !fr.truthy(v) {
				return v
			}
			return right(fr)
		}, nil
	}

	return func(fr *frame) val {
		v := left(fr)
		if fr.truthy(v) {
			return v
		}
		return right(fr)
	}, nil
}

func (c *foldCompiler) conditional(t *ast.ConditionalExpression) (expr, error) {
	test, err := c.expression(t.Test)
	if err != nil {
		return nil, err
	}

	cons, err := c.expression(t.Consequent)
	if err != nil {
		return nil, err
	}

	alt, err := c.expression(t.Alternate)
	if err != nil {
		return nil, err
	}

	return func(fr *frame) val {
		if fr.truthy(test(fr)) {
			return cons(fr)
		}
		return alt(fr)
	}, nil
}

func (c *foldCompiler) call(t *ast.CallExpression) (expr, error) {
	switch callee := t.Callee.(type) {
	case *ast.Identifier:
		name := callee.Name.String()
		if c.isLocal(name) || c.outer[name] {
			return nil, errNotFoldable
		}
		if fn, ok := c.a.helpers[name]; ok {
			return c.helperCall(fn, t.ArgumentList)
		}
		if name == "parseInt" && len(t.ArgumentList) == 2 {
			radix, ok := t.ArgumentList[1].(*ast.NumberLiteral)
			if !ok || radix.Value != int64(10) {
				return nil, errNotFoldable
			}
			e, err := c.expression(t.ArgumentList[0])
			if err != nil {
				return nil, err
			}
			return func(fr *frame) val { return fr.parseInt10(e(fr)) }, nil
		}
	case *ast.DotExpression:
		if callee.Identifier.Name == "split" && len(t.ArgumentList) == 1 {
			sep, ok := t.ArgumentList[0].(*ast.StringLiteral)
			if !ok || sep.Value.String() == "" {
				return nil, errNotFoldable
			}
			e, err := c.expression(callee.Left)
			if err != nil {
				return nil, err
			}
			s := sep.Value.String()
			return func(fr *frame) val { return fr.split(e(fr), s) }, nil
		}
	}

	return nil, errNotFoldable
}

func (c *foldCompiler) helperCall(fn *ast.FunctionLiteral, args []ast.Expression) (expr, error) {
	if c.depth >= maxHelperDepth || fn.Name == nil || fn.Name.Name.String() == c.a.fn || fn.ParameterList.Rest != nil {
		return nil, errNotFoldable
	}

	params := fn.ParameterList.List
	if len(args) != len(params) {
		return nil, errNotFoldable
	}

	compiled := make([]expr, len(args))
	for i, a := range args {
		e, err := c.expression(a)
		if err != nil {
			return nil, err
		}
		compiled[i] = e
	}

	inner := &foldScope{slots: map[string]int{}, set: map[string]bool{}, aliases: map[string]bool{}, inner: true}
	c.scopes = append(c.scopes, inner)
	c.depth++
	defer func() {
		c.scopes = c.scopes[:len(c.scopes)-1]
		c.depth--
	}()

	slots := make([]int, len(params))
	for i, p := range params {
		id, ok := p.Target.(*ast.Identifier)
		if !ok || p.Initializer != nil {
			return nil, errNotFoldable
		}
		slots[i] = c.localSlot(id.Name.String())
		inner.set[id.Name.String()] = true
	}

	walkNodes(reflect.ValueOf(fn.Body), func(n any) bool {
		if vs, ok := n.(*ast.VariableStatement); ok {
			for _, b := range vs.List {
				if id, ok := b.Target.(*ast.Identifier); ok {
					c.localSlot(id.Name.String())
				}
			}
		}
		return true
	})

	body, _, err := c.stmtList(fn.Body.List)
	if err != nil {
		return nil, err
	}

	return func(fr *frame) val {
		for i, e := range compiled {
			fr.local[slots[i]] = e(fr)
		}
		if body(fr) == ctlReturn {
			return fr.ret
		}
		return undefVal
	}, nil
}
