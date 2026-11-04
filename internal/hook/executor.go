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
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	defaultScriptTimeout = 2 * time.Second
	maxCompiledScripts   = 1024
	maxCachedQueries     = 1024
	maxCachedQueryRows   = 20000
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

type cachedQuery struct {
	version string
	rows    []map[string]any
}

var (
	queryCacheMu sync.Mutex
	queryCache   = map[string]cachedQuery{}
)

type Request struct {
	username string
	mu       sync.Mutex
	memo     map[string][]map[string]any
	budget   time.Duration
	spent    atomic.Int64
	skipped  atomic.Bool
	guard    *api_storage.Guard
}

func NewRequest(username string) *Request {
	r := &Request{username: username, memo: map[string][]map[string]any{}}
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
	if e == nil {
		return ""
	}

	snap := e.Snapshot(entity)

	return fmt.Sprintf("%d.%d.%d", e.Instance(), snap.Generation(), snap.Version())
}

func copyRows(rows []map[string]any) []map[string]any {
	out := make([]map[string]any, len(rows))
	for i, row := range rows {
		out[i] = storage.DeepCopyDocument(row)
	}

	return out
}

func listForHook(targetEntity string, filters map[string]map[string]string, guard *api_storage.Guard) ([]map[string]any, error) {
	return api_storage.ListEntitiesGuarded(targetEntity, 0, 0, "", true, filters, "", "", guard)
}

func (r *Request) query(h Hook, targetEntity string, filters map[string]map[string]string, bypassAcl bool, guard *api_storage.Guard) ([]map[string]any, error) {
	owner := r.username
	if bypassAcl {
		owner = "\x00bypass"
	}

	key := targetEntity + "\x00" + owner + "\x00" + filtersKey(filters)

	r.mu.Lock()
	rows, ok := r.memo[key]
	r.mu.Unlock()
	if ok {
		metrics.ObserveHookQuery(h.ID, h.Entity, h.Event, len(rows), true)
		return copyRows(rows), nil
	}

	version := entityVersion(targetEntity)
	if version != "" && !bypassAcl {
		version += "/" + entityVersion(acl.ACLEntity)
	}

	if version != "" {
		queryCacheMu.Lock()
		cached, hit := queryCache[key]
		queryCacheMu.Unlock()
		if hit && cached.version == version {
			r.mu.Lock()
			r.memo[key] = cached.rows
			r.mu.Unlock()
			metrics.ObserveHookQuery(h.ID, h.Entity, h.Event, len(cached.rows), true)
			return copyRows(cached.rows), nil
		}
	}

	results, err := listForHook(targetEntity, filters, guard)
	if err != nil {
		return nil, err
	}

	if !bypassAcl {
		results = acl.FilterListOfEntities(r.username, targetEntity, results)
	}

	metrics.ObserveHookQuery(h.ID, h.Entity, h.Event, len(results), false)

	r.mu.Lock()
	r.memo[key] = results
	r.mu.Unlock()

	if version != "" && len(results) <= maxCachedQueryRows {
		queryCacheMu.Lock()
		if len(queryCache) >= maxCachedQueries {
			queryCache = map[string]cachedQuery{}
		}
		queryCache[key] = cachedQuery{version: version, rows: results}
		queryCacheMu.Unlock()
	}

	return copyRows(results), nil
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

	stopAt, reason := r.runDeadline(scriptTimeout())
	run := r.guard.Until(stopAt)
	vm := goja.New()
	timer := time.AfterFunc(time.Until(stopAt), func() {
		vm.Interrupt(reason)
	})
	defer timer.Stop()

	ctx := map[string]any{
		"entity": entity,
		"query": func(call goja.FunctionCall) goja.Value {
			if len(call.Arguments) < 2 {
				panic(vm.ToValue("query(entity, params) expected"))
			}

			targetEntity := call.Arguments[0].String()
			if api_storage.IsCoreEntityType(targetEntity) {
				return vm.ToValue([]map[string]any{})
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

			rows, err := r.query(h, targetEntity, filters, h.ByPassACL, run)
			if err != nil {
				panic(vm.NewGoError(r.stopReason(err, reason)))
			}

			return vm.ToValue(rows)
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

	return err
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
