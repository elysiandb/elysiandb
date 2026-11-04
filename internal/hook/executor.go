package hook

import (
	"errors"
	"fmt"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	engine "github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	defaultScriptTimeout = 2 * time.Second
	maxCompiledScripts   = 1024
)

func scriptTimeout() time.Duration {
	if cfg := globals.GetConfig(); cfg != nil && cfg.Api.Hooks.TimeoutMs > 0 {
		return time.Duration(cfg.Api.Hooks.TimeoutMs) * time.Millisecond
	}

	return defaultScriptTimeout
}

var (
	compiledMu sync.Mutex
	compiled   = map[string]*goja.Program{}
)

func compile(script string) (*goja.Program, error) {
	compiledMu.Lock()
	p, ok := compiled[script]
	compiledMu.Unlock()
	if ok {
		return p, nil
	}

	p, err := goja.Compile("", script, false)
	if err != nil {
		return nil, err
	}

	compiledMu.Lock()
	if len(compiled) >= maxCompiledScripts {
		compiled = map[string]*goja.Program{}
	}
	compiled[script] = p
	compiledMu.Unlock()

	return p, nil
}

type Request struct {
	username string
	mu       sync.Mutex
	memo     map[string]*result
	budget   time.Duration
	spent    atomic.Int64
	skipped  atomic.Bool
	guard    *api_storage.Guard
}

func NewRequest(username string) *Request {
	r := &Request{username: username, memo: map[string]*result{}}
	if cfg := globals.GetConfig(); cfg != nil && cfg.Api.Hooks.RequestBudgetMs > 0 {
		r.budget = time.Duration(cfg.Api.Hooks.RequestBudgetMs) * time.Millisecond
	}

	return r
}

func (r *Request) WithGuard(guard *api_storage.Guard) *Request {
	r.guard = guard

	return r
}

func (r *Request) Stopped() bool {
	return r.guard.Stopped()
}

func (r *Request) overBudget() bool {
	if r.budget <= 0 {
		return false
	}

	if time.Duration(r.spent.Load()) < r.budget {
		return false
	}

	if r.skipped.CompareAndSwap(false, true) {
		hookLog.Warn("hook budget of the request exceeded, the remaining hooks are skipped", "budget_ms", r.budget.Milliseconds())
	}

	return true
}

func filtersKey(filters map[string]map[string]string) string {
	fields := make([]string, 0, len(filters))
	for f := range filters {
		fields = append(fields, f)
	}
	sort.Strings(fields)

	var b strings.Builder
	for _, f := range fields {
		ops := make([]string, 0, len(filters[f]))
		for op := range filters[f] {
			ops = append(ops, op)
		}
		sort.Strings(ops)
		for _, op := range ops {
			fmt.Fprintf(&b, "%q[%q]=%q;", f, op, filters[f][op])
		}
	}

	return b.String()
}

func entityVersion(entity string) string {
	e := storage.Engine()
	if e == nil || !engine.IsEngineInternal() {
		return ""
	}

	snap := e.Snapshot(entity)

	return fmt.Sprintf("%d.%d.%d", e.Instance(), snap.Generation(), snap.Version())
}

func listForHook(targetEntity string, filters map[string]map[string]string, guard *api_storage.Guard) ([]map[string]any, error) {
	if !engine.IsEngineInternal() {
		return engine.ListEntities(targetEntity, 0, 0, "", true, filters, "", ""), nil
	}

	return api_storage.ListEntitiesGuarded(targetEntity, 0, 0, "", true, filters, "", "", guard)
}

func (r *Request) owner(bypassAcl bool) string {
	if bypassAcl {
		return "\x00bypass"
	}

	return r.username
}

func queryVersion(targetEntity string, bypassAcl bool) string {
	version := entityVersion(targetEntity)
	if version != "" && !bypassAcl {
		version += "/" + entityVersion(acl.ACLEntity)
	}

	return version
}

func (r *Request) query(h Hook, targetEntity string, filters map[string]map[string]string, bypassAcl bool, guard *api_storage.Guard) (*result, string, error) {
	prepared.observe(h.Script, targetEntity, filters)

	key := targetEntity + "\x00" + r.owner(bypassAcl) + "\x00" + filtersKey(filters)

	r.mu.Lock()
	res, ok := r.memo[key]
	r.mu.Unlock()
	if ok {
		metrics.ObserveHookQuery(h.ID, h.Entity, h.Event, len(res.rows), true)
		return res, key, nil
	}

	version := queryVersion(targetEntity, bypassAcl)
	res, hit, err := results.get(key, version, guard, func(prev *result) (*result, error) {
		return buildResult(targetEntity, filters, r.username, bypassAcl, guard, prev)
	})
	if err != nil {
		return nil, key, err
	}

	metrics.ObserveHookQuery(h.ID, h.Entity, h.Event, len(res.rows), hit)

	r.mu.Lock()
	if cur, ok := r.memo[key]; ok {
		res = cur
	} else {
		r.memo[key] = res
	}
	r.mu.Unlock()

	return res, key, nil
}

func (r *Request) stillValid(o *output) bool {
	for _, d := range o.deps {
		if d.version == "" {
			return false
		}

		if d.owner != r.owner(d.bypass) {
			return false
		}

		r.mu.Lock()
		pinned, ok := r.memo[d.key]
		r.mu.Unlock()

		current := queryVersion(d.target, d.bypass)
		if ok {
			current = pinned.version
		}

		if current != d.version {
			return false
		}
	}

	return true
}

func (r *Request) runDeadline(timeout time.Duration) (time.Time, string) {
	stopAt := time.Now().Add(timeout)
	reason := fmt.Sprintf("hook script stopped after %s", timeout)
	if r.guard != nil && !r.guard.Deadline.IsZero() && r.guard.Deadline.Before(stopAt) {
		stopAt = r.guard.Deadline
		reason = api_storage.ErrDeadline.Error()
	}

	return stopAt, reason
}

func (r *Request) stopReason(err error, timeoutReason string) error {
	if errors.Is(err, api_storage.ErrDeadline) && r.guard.Err() == nil {
		return errors.New(timeoutReason)
	}

	return err
}

func (r *Request) applyScript(h Hook, fnName string, entity map[string]any) error {
	program, err := compile(h.Script)
	if err != nil {
		return err
	}

	var key string
	var before map[string][]byte
	if input, ok := canonical(entity); ok {
		key = memoKey(h.Script, fnName, append(input, boolByte(h.ByPassACL)))
		if o := outputs.find(key, r.stillValid); o != nil {
			o.apply(entity)
			return nil
		}

		if done, leader := outputs.begin(key); leader {
			defer done()
		} else if o := outputs.find(key, r.stillValid); o != nil {
			o.apply(entity)
			return nil
		}

		before = fieldBytes(entity)
	}

	laned := false
	if costs.heavy(h.Script) {
		release, err := api_storage.AcquireHeavyLane(r.guard)
		if err != nil {
			return err
		}
		defer release()
		laned = true
	}

	stopAt, reason := r.runDeadline(scriptTimeout())
	run := r.guard.Until(stopAt)
	run.Laned = laned

	start := time.Now()
	defer func() { costs.observe(h.Script, time.Since(start)) }()

	vm := goja.New()
	timer := time.AfterFunc(time.Until(stopAt), func() {
		vm.Interrupt(reason)
	})
	defer timer.Stop()

	det := &determinism{}
	var deps []queryDep
	var depsMu sync.Mutex
	if key != "" {
		det.install(vm)
	}

	fetch := func(name string, call goja.FunctionCall, args int) *result {
		if len(call.Arguments) < args {
			panic(vm.ToValue(name + " expected"))
		}

		targetEntity := call.Arguments[0].String()
		if engine.IsCoreEntityType(targetEntity) {
			return &result{}
		}

		params, _ := call.Arguments[1].Export().(map[string]any)

		filters := make(map[string]map[string]string, len(params))
		for field, cond := range params {
			typed, ok := cond.(map[string]any)
			if !ok {
				continue
			}

			condMap := make(map[string]string, len(typed))
			for op, val := range typed {
				condMap[op] = fmt.Sprintf("%v", val)
			}

			filters[field] = condMap
		}

		res, qkey, err := r.query(h, targetEntity, filters, h.ByPassACL, run)
		if err != nil {
			panic(vm.NewGoError(r.stopReason(err, reason)))
		}

		depsMu.Lock()
		deps = append(deps, queryDep{target: targetEntity, owner: r.owner(h.ByPassACL), key: qkey, version: res.version, bypass: h.ByPassACL})
		depsMu.Unlock()

		return res
	}

	ctx := map[string]any{
		"entity": entity,
		"query": func(call goja.FunctionCall) goja.Value {
			return newArrayView(vm, fetch("query(entity, params)", call, 2).rows)
		},
		"count": func(call goja.FunctionCall) goja.Value {
			return vm.ToValue(len(fetch("count(entity, params)", call, 2).rows))
		},
		"sum": func(call goja.FunctionCall) goja.Value {
			res := fetch("sum(entity, params, field)", call, 3)
			return vm.ToValue(res.sum(call.Arguments[2].String()))
		},
		"group": func(call goja.FunctionCall) goja.Value {
			res := fetch("group(entity, params, field)", call, 3)
			sumField := ""
			if len(call.Arguments) > 3 && !goja.IsUndefined(call.Arguments[3]) && !goja.IsNull(call.Arguments[3]) {
				sumField = call.Arguments[3].String()
			}
			return groupObject(vm, res.groupBy(call.Arguments[2].String(), sumField), sumField != "")
		},
	}

	if err := vm.Set("ctx", ctx); err != nil {
		return err
	}

	if _, err := vm.RunProgram(program); err != nil {
		return err
	}

	fn, ok := goja.AssertFunction(vm.Get(fnName))
	if !ok {
		return nil
	}

	_, err = fn(goja.Undefined(), vm.Get("ctx"))
	if err != nil || key == "" || det.broken.Load() {
		return err
	}

	for _, d := range deps {
		if d.version == "" {
			return nil
		}
	}

	if o, ok := diffOutput(before, entity); ok {
		o.deps = deps
		outputs.store(key, o)
	}

	return nil
}

func boolByte(b bool) byte {
	if b {
		return 1
	}

	return 0
}

func ApplyPostReadScript(
	username string,
	script string,
	entity map[string]any,
	bypassAcl bool,
) error {
	return NewRequest(username).applyScript(Hook{Script: script, ByPassACL: bypassAcl}, "postRead", entity)
}

func ApplyPreReadScript(
	username string,
	script string,
	entity map[string]any,
	bypassAcl bool,
) error {
	return NewRequest(username).applyScript(Hook{Script: script, ByPassACL: bypassAcl}, "preRead", entity)
}

const heavyHookAfter = 5 * time.Millisecond

type hookCost struct {
	mu   sync.Mutex
	ewma time.Duration
}

type costTable struct {
	m sync.Map
}

var costs = &costTable{}

func (c *costTable) heavy(script string) bool {
	v, ok := c.m.Load(script)
	if !ok {
		return false
	}

	hc := v.(*hookCost)
	hc.mu.Lock()
	defer hc.mu.Unlock()

	return hc.ewma >= heavyHookAfter
}

func (c *costTable) observe(script string, d time.Duration) {
	v, _ := c.m.LoadOrStore(script, &hookCost{ewma: d})
	hc := v.(*hookCost)
	hc.mu.Lock()
	hc.ewma = (hc.ewma*7 + d) / 8
	hc.mu.Unlock()
}
