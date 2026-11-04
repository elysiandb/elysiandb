package api_storage

import (
	"math"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/query"
)

const maxIndexesPerEntity = 64

type ListQuery struct {
	Entity    string
	Limit     int
	Offset    int
	SortField string
	SortAsc   bool
	Sorts     []query.Sort
	Filters   map[string]map[string]string
	Node      *query.FilterNode
	Search    string
	Includes  string
	CountOnly bool
	Snapshot  *datastore.Snapshot
	Guard     *Guard
}

type ListResult struct {
	snap  *datastore.Snapshot
	guard *Guard
	Docs  []datastore.Doc
	Total int
	Err   error
}

func (r *ListResult) Snapshot() *datastore.Snapshot {
	return r.snap
}

func (r *ListResult) IDs() []string {
	ids := make([]string, 0, len(r.Docs))
	for _, d := range r.Docs {
		ids = append(ids, d.ID())
	}

	return ids
}

const decodedOverhead = 4

func (r *ListResult) Decode() []map[string]any {
	out := make([]map[string]any, 0, len(r.Docs))
	for i, d := range r.Docs {
		raw, err := d.Bytes()
		if err != nil {
			continue
		}

		if err := r.guard.Charge(int64(len(raw)) * decodedOverhead); err != nil {
			return out[:0]
		}

		if i%guardInterval == guardInterval-1 && r.guard.Check() != nil {
			return out[:0]
		}

		doc, err := datastore.DecodeMap(raw)
		if err == nil && doc != nil {
			out = append(out, doc)
		}
	}

	return out
}

func (r *ListResult) reader() func(entity, id string) map[string]any {
	return includeReader()
}

func includeReader() func(entity, id string) map[string]any {
	if ReadEntityByIdFunc != nil {
		return ReadEntityByIdFunc
	}

	return newDocReader()
}

func newDocReader() func(entity, id string) map[string]any {
	snaps := map[string]*datastore.Snapshot{}
	cache := map[string][]byte{}

	return func(entity, id string) map[string]any {
		key := entity + "\x00" + id
		if b, ok := cache[key]; ok {
			if b == nil {
				return nil
			}
			return decodeMap(b)
		}

		e := db()
		if e == nil {
			return nil
		}

		snap := snaps[entity]
		if snap == nil {
			snap = e.Snapshot(entity)
			snaps[entity] = snap
		}

		doc, ok := snap.Get(id)
		if !ok {
			cache[key] = nil
			return nil
		}

		b, err := doc.Bytes()
		if err != nil {
			return nil
		}

		cache[key] = b

		return decodeMap(b)
	}
}

type candidates struct {
	all   bool
	set   *datastore.Bitset
	exact *datastore.Bitset
}

func universe() candidates {
	return candidates{all: true}
}

func (c candidates) isEmpty() bool {
	return !c.all && (c.set == nil || c.set.Count() == 0)
}

func intersect(a, b candidates) candidates {
	if a.all && b.all {
		return candidates{all: true, exact: andExact(a.exact, b.exact, a, b)}
	}

	if a.all {
		return candidates{set: b.set, exact: andExact(a.exact, b.exact, a, b)}
	}

	if b.all {
		return candidates{set: a.set, exact: andExact(a.exact, b.exact, a, b)}
	}

	set := a.set.Clone()
	set.And(b.set)

	return candidates{set: set, exact: andExact(a.exact, b.exact, a, b)}
}

func andExact(ea, eb *datastore.Bitset, a, b candidates) *datastore.Bitset {
	if ea == nil || eb == nil {
		return nil
	}

	out := ea.Clone()
	out.And(eb)

	return out
}

func union(a, b candidates) candidates {
	exact := orExact(a.exact, b.exact)
	if a.all || b.all {
		return candidates{all: true, exact: exact}
	}

	set := a.set.Clone()
	set.Or(b.set)

	return candidates{set: set, exact: exact}
}

func orExact(a, b *datastore.Bitset) *datastore.Bitset {
	switch {
	case a == nil && b == nil:
		return nil
	case a == nil:
		return b.Clone()
	case b == nil:
		return a.Clone()
	}

	out := a.Clone()
	out.Or(b)

	return out
}

type planner struct {
	e     *datastore.Engine
	snap  *datastore.Snapshot
	refs  []datastore.RefPath
	inc   bool
	depth int
}

func (p *planner) affectedByIncludes(path string) bool {
	if !p.inc {
		return false
	}

	for _, ref := range p.refs {
		if path == ref.Path+".id" || path == ref.Path+".@entity" {
			continue
		}

		if path == ref.Path || strings.HasPrefix(path, ref.Path+".") {
			return true
		}
	}

	return false
}

func (p *planner) index(spec datastore.IndexSpec) (*datastore.IndexView, bool) {
	if view, ok := p.snap.Index(spec); ok {
		return view, true
	}

	registered := p.snap.HasIndex(spec)
	if ((!registered && len(p.e.IndexNames(p.snap.Name())) < maxIndexesPerEntity) || p.snap.IndexDormant(spec)) && p.snap.Len() > 0 {
		if p.e.EnsureIndex(p.snap.Name(), spec) {
			p.snap = p.e.Snapshot(p.snap.Name())
			return p.snap.Index(spec)
		}
	}

	return nil, false
}

func (p *planner) filterIndex(field string) (*datastore.IndexView, bool) {
	if view, ok := p.snap.Index(datastore.IndexSpec{Kind: datastore.IndexBitmap, Field: field}); ok {
		return view, true
	}

	return p.index(datastore.IndexSpec{Kind: datastore.IndexFilter, Field: field})
}

func (p *planner) leaf(field string, ops map[string]string) candidates {
	if len(ops) == 0 {
		return universe()
	}

	if p.affectedByIncludes(field) {
		return p.throughReferences(field, ops)
	}

	view, ok := p.filterIndex(field)
	if !ok {
		return universe()
	}

	result := universe()
	first := true
	for op, cmp := range ops {
		c := opCandidates(view, op, cmp, ops)
		if first {
			result = c
			first = false
		} else {
			result = intersect(result, c)
		}
	}

	if len(ops) > 1 {
		result.exact = nil
	}

	if refined, ok := evaluateLeaf(view, ops, result); ok {
		return refined
	}

	return result
}

const maxReferenceDepth = 4

func (p *planner) throughReferences(field string, ops map[string]string) candidates {
	if p.depth >= maxReferenceDepth {
		return universe()
	}

	var path string
	for _, ref := range p.refs {
		if strings.HasPrefix(field, ref.Path+".") && (path == "" || len(ref.Path) < len(path)) {
			path = ref.Path
		}
	}

	rest := strings.TrimPrefix(field, path+".")
	if path == "" || rest == "" || rest == "id" || rest == "@entity" {
		return universe()
	}

	refsView, ok := p.index(datastore.IndexSpec{Kind: datastore.IndexRefs})
	if !ok {
		return universe()
	}

	result := &datastore.Bitset{}
	if direct, ok := p.filterIndex(field); ok {
		key := []byte{datastore.KeyPresent}
		collect(direct, key, afterTerm(key), result, nil)
	} else {
		return universe()
	}

	for _, ref := range p.refs {
		if ref.Path != path || IsCoreEntityType(ref.Target) {
			continue
		}

		targetSnap := p.e.Snapshot(ref.Target)
		target := &planner{e: p.e, snap: targetSnap, refs: targetSnap.Refs(), inc: true, depth: p.depth + 1}
		matched := target.leaf(rest, ops)
		if matched.all {
			return universe()
		}

		prefix := datastore.RefPrefix(path, ref.Target)
		matched.set.ForEach(func(seq uint32) bool {
			d, ok := target.snap.BySeq(seq)
			if !ok {
				return true
			}

			key := datastore.AppendTerminated(append([]byte(nil), prefix...), d.ID())
			refsView.ScanPrefix(key, func(_ string, source uint32) bool {
				result.Set(source)
				return true
			})

			return true
		})
	}

	return candidates{set: result}
}

func unescapePayload(key string) string {
	body := key[1 : len(key)-6]
	if strings.IndexByte(body, 0) < 0 {
		return body
	}

	return strings.ReplaceAll(body, "\x00\xff", "\x00")
}

func prefixOf(kind byte, s string) []byte {
	return datastore.AppendEscaped([]byte{kind}, s)
}

func exactOf(kind byte, s string) []byte {
	return datastore.AppendTerminated([]byte{kind}, s)
}

func collect(view *datastore.IndexView, lo, hi []byte, set *datastore.Bitset, keep func(key string) bool) {
	view.Collect(lo, hi, set, keep)
}

func afterTerm(term []byte) []byte {
	return append(append([]byte(nil), term...), 0xff, 0xff, 0xff, 0xff, 0xff)
}

func kindBounds(kind byte) ([]byte, []byte) {
	return []byte{kind}, []byte{kind + 1}
}

func allComparisonDates(ops map[string]string) bool {
	for op, cmp := range ops {
		if !isComparisonOperator(op) {
			return false
		}

		if _, ok, _ := docpath.ParseDate(cmp); !ok {
			return false
		}
	}

	return true
}

func dayStart(t time.Time) time.Time {
	return t.Truncate(24 * time.Hour)
}

func opCandidates(view *datastore.IndexView, op, cmp string, ops map[string]string) candidates {
	exact := &datastore.Bitset{}
	maybe := &datastore.Bitset{}

	addDateRange := func(lo, hi *time.Time) {
		l, h := kindBounds(datastore.KeyDate)
		if lo != nil {
			l = datastore.AppendTime([]byte{datastore.KeyDate}, *lo)
		}
		if hi != nil {
			h = datastore.AppendTime([]byte{datastore.KeyDate}, *hi)
		}
		collect(view, l, h, maybe, nil)
	}

	switch op {
	case "eq":
		lower := strings.ToLower(cmp)
		literal := lower
		if i := strings.IndexAny(lower, "*\\"); i >= 0 {
			literal = lower[:i]
		}

		if literal == lower {
			term := exactOf(datastore.KeyLower, lower)
			collect(view, term, afterTerm(term), exact, nil)
		} else {
			prefix := prefixOf(datastore.KeyLower, literal)
			_, hi := kindBounds(datastore.KeyLower)
			view.ScanRange(prefix, hi, func(key string, seq uint32) bool {
				if !strings.HasPrefix(key, string(prefix)) {
					return false
				}
				if docpath.MatchGlob(cmp, unescapePayload(key)) {
					exact.Set(seq)
				}
				return true
			})
		}

		if f, err := strconv.ParseFloat(cmp, 64); err == nil {
			key := datastore.AppendNumber([]byte{datastore.KeyNumber}, f)
			collect(view, key, afterTerm(key), exact, nil)
		}

		b := byte(0)
		if cmp == "true" || cmp == "1" {
			b = 1
		}
		boolKey := []byte{datastore.KeyBool, b}
		collect(view, boolKey, afterTerm(boolKey), exact, nil)

		if t, ok, _ := docpath.ParseDate(cmp); ok && allComparisonDates(ops) {
			lo := dayStart(t)
			hi := lo.Add(24 * time.Hour)
			addDateRange(&lo, &hi)
		}

		first := strings.Split(cmp, ",")[0]
		term := exactOf(datastore.KeyElement, first)
		collect(view, term, afterTerm(term), maybe, nil)
	case "lt", "lte", "gt", "gte":
		if f, err := strconv.ParseFloat(cmp, 64); err == nil {
			key := datastore.AppendNumber([]byte{datastore.KeyNumber}, f)
			lo, hi := kindBounds(datastore.KeyNumber)
			switch op {
			case "lt":
				hi = key
			case "lte":
				hi = afterTerm(key)
			case "gt":
				lo = afterTerm(key)
			case "gte":
				lo = key
			}
			collect(view, lo, hi, exact, nil)
		}

		term := exactOf(datastore.KeyRaw, cmp)
		lo, hi := kindBounds(datastore.KeyRaw)
		switch op {
		case "lt":
			hi = term
		case "lte":
			hi = afterTerm(term)
		case "gt":
			lo = afterTerm(term)
		case "gte":
			lo = term
		}
		collect(view, lo, hi, maybe, nil)

		if t, ok, _ := docpath.ParseDate(cmp); ok && allComparisonDates(ops) {
			day := dayStart(t)
			if op == "gt" || op == "gte" {
				addDateRange(&day, nil)
			} else {
				next := day.Add(24 * time.Hour)
				addDateRange(nil, &next)
			}
		}
	case "contains":
		needle := strings.ToLower(cmp)
		lo, hi := kindBounds(datastore.KeyLower)
		collect(view, lo, hi, exact, func(key string) bool {
			return strings.Contains(unescapePayload(key), needle)
		})

		term := exactOf(datastore.KeyElement, cmp)
		collect(view, term, afterTerm(term), exact, nil)
	case "any":
		for _, v := range strings.Split(cmp, ",") {
			term := exactOf(datastore.KeyElement, v)
			collect(view, term, afterTerm(term), exact, nil)
		}
	case "all":
		first := strings.Split(cmp, ",")[0]
		term := exactOf(datastore.KeyElement, first)
		collect(view, term, afterTerm(term), maybe, nil)
	default:
		return universe()
	}

	set := exact.Clone()
	set.Or(maybe)

	return candidates{set: set, exact: exact}
}

const (
	selectiveRatio   = 16
	selectiveMinimum = 4096
	verifyDirectly   = 256
)

func (p *planner) estimate(field string, ops map[string]string) int {
	if len(ops) != 1 || p.affectedByIncludes(field) {
		return math.MaxInt
	}

	cmp, ok := ops["eq"]
	if !ok || strings.ContainsAny(cmp, "*\\") {
		return math.MaxInt
	}

	view, ok := p.filterIndex(field)
	if !ok {
		return math.MaxInt
	}

	n := 0
	count := func(term []byte) {
		n += view.Rank(afterTerm(term)) - view.Rank(term)
	}

	count(exactOf(datastore.KeyLower, strings.ToLower(cmp)))
	count(exactOf(datastore.KeyElement, strings.Split(cmp, ",")[0]))
	if f, err := strconv.ParseFloat(cmp, 64); err == nil {
		count(datastore.AppendNumber([]byte{datastore.KeyNumber}, f))
	}
	b := byte(0)
	if cmp == "true" || cmp == "1" {
		b = 1
	}
	count([]byte{datastore.KeyBool, b})
	if _, ok, _ := docpath.ParseDate(cmp); ok {
		return math.MaxInt
	}

	return n
}

func (p *planner) restFilters(filters map[string]map[string]string) candidates {
	if len(filters) == 0 {
		return candidates{all: true}
	}

	type leafPlan struct {
		field string
		ops   map[string]string
		size  int
	}

	order := make([]leafPlan, 0, len(filters))
	for field, ops := range filters {
		size := math.MaxInt
		if len(filters) > 1 {
			size = p.estimate(field, ops)
		}
		order = append(order, leafPlan{field: field, ops: ops, size: size})
	}
	sort.SliceStable(order, func(i, j int) bool {
		if order[i].size != order[j].size {
			return order[i].size < order[j].size
		}
		return order[i].field < order[j].field
	})

	result := p.leaf(order[0].field, order[0].ops)
	for _, l := range order[1:] {
		if !result.all {
			pending := result.set.Count()
			if pending <= verifyDirectly || (l.size != math.MaxInt && l.size > selectiveMinimum && l.size/selectiveRatio > pending) {
				result.exact = nil
				continue
			}
		}
		result = intersect(result, p.leaf(l.field, l.ops))
	}

	return result
}

func (p *planner) node(n query.FilterNode) candidates {
	if n.Leaf != nil {
		if len(n.Leaf) == 0 {
			return candidates{all: true, exact: nil}
		}

		return p.restFilters(n.Leaf)
	}

	if len(n.And) > 0 {
		result := p.node(n.And[0])
		for _, child := range n.And[1:] {
			result = intersect(result, p.node(child))
		}
		return result
	}

	if len(n.Or) > 0 {
		result := p.node(n.Or[0])
		for _, child := range n.Or[1:] {
			result = union(result, p.node(child))
		}
		return result
	}

	return candidates{set: &datastore.Bitset{}, exact: &datastore.Bitset{}}
}

func nodeIsEverything(n query.FilterNode) bool {
	return n.Leaf != nil && len(n.Leaf) == 0
}

type matchFunc func(doc map[string]any) bool

func Plan(q ListQuery) *ListResult {
	e := db()
	if e == nil {
		return &ListResult{}
	}

	snap := q.Snapshot
	if snap == nil {
		snap = e.Snapshot(q.Entity)
	}

	p := &planner{e: e, snap: snap, refs: snap.Refs(), inc: q.Includes != ""}
	if docs, total, ok := p.compositePage(q); ok {
		return &ListResult{snap: p.snap, guard: q.Guard, Docs: docs, Total: total}
	}

	hasFilter := len(q.Filters) > 0
	hasNode := q.Node != nil && !nodeIsEverything(*q.Node)
	needsMatch := hasFilter || hasNode || q.Search != ""

	cand := universe()
	everythingExact := !needsMatch
	switch {
	case hasNode:
		cand = p.node(*q.Node)
	case hasFilter:
		cand = p.restFilters(q.Filters)
	}

	if q.Search != "" {
		searchAffected := p.inc && len(p.refs) > 0
		if !searchAffected {
			if set, ok := e.SearchCandidates(q.Entity, SearchPattern(q.Search)); ok {
				cand = intersect(cand, candidates{set: set})
			}
		}
		cand.exact = nil
	}

	snap = p.snap

	var match, matchFields matchFunc
	if needsMatch {
		filters, node, search := q.Filters, q.Node, q.Search
		matchFields = func(doc map[string]any) bool {
			if node != nil && !matchFilterNode(*node, doc) {
				return false
			}
			return len(filters) == 0 || FiltersMatchEntity(doc, filters)
		}
		match = func(doc map[string]any) bool {
			return matchFields(doc) && (search == "" || SearchMatchesEntity(doc, search))
		}
	}

	evalIncludes := ""
	if p.inc {
		for field := range q.Filters {
			if p.affectedByIncludes(field) {
				evalIncludes = q.Includes
			}
		}
		if q.Search != "" && len(p.refs) > 0 {
			evalIncludes = q.Includes
		}
	}

	res := &ListResult{snap: snap, guard: q.Guard}
	reader := includeReader()

	heavy := needsMatch && heavyPlan(snap, cand, evalIncludes)
	searchOnBytes := q.Search != "" && evalIncludes == ""
	onlySearch := searchOnBytes && !hasFilter && !hasNode
	loweredSearch := LoweredSearchPattern(q.Search)
	var release func()
	defer func() {
		if release != nil {
			release()
		}
	}()

	verified := 0
	expired := false
	var started time.Time
	verify := func(d datastore.Doc) bool {
		if match == nil || expired {
			return !expired
		}

		if !cand.all && !cand.set.Has(d.Seq()) {
			return false
		}

		if cand.exact != nil && cand.exact.Has(d.Seq()) {
			return true
		}

		if verified == 0 {
			started = time.Now()
		}

		verified++
		if verified%guardInterval == 0 {
			if heavy && release == nil && time.Since(started) >= heavyLaneAfter {
				r, err := acquireHeavyLane(q.Guard)
				if err != nil {
					expired = true
					return false
				}
				release = r
			}

			if q.Guard.Check() != nil {
				expired = true
				return false
			}
		}

		check := match
		if searchOnBytes {
			if raw, err := d.Bytes(); err == nil {
				if found, ok := searchBytesLowered(raw, loweredSearch); ok {
					if !found || onlySearch {
						return found
					}
					check = matchFields
				}
			}
		}

		doc, err := d.Decode()
		if err != nil {
			return false
		}

		if evalIncludes != "" {
			doc = applyIncludesWith([]map[string]any{doc}, evalIncludes, reader)[0]
		}

		return check(doc)
	}

	if q.CountOnly && len(q.Sorts) == 0 && q.SortField == "" && q.Limit == 0 && q.Offset == 0 {
		res.Total = countMatches(snap, cand, needsMatch, everythingExact, verify)
	} else if docs, total, ok := p.directPageIf(!needsMatch, q); ok {
		res.Docs, res.Total = docs, total
	} else {
		order := p.orderFor(q, cand, needsMatch, verify)
		res.Docs, res.Total = paginate(order, q.Offset, q.Limit, q.CountOnly)
	}

	if expired || q.Guard.Err() != nil {
		res.Err = q.Guard.Err()
		if res.Err == nil {
			res.Err = ErrDeadline
		}
		res.Docs, res.Total = nil, 0
	}

	return res
}

const (
	heavyCandidates       = 2048
	guardInterval         = 64
	defaultQueuePerWorker = 4
	defaultQueueTimeout   = 5 * time.Second
	heavyLanePollInterval = 20 * time.Millisecond
	heavyLaneAfter        = 5 * time.Millisecond
	heavyEnvelope         = 8 << 20
)

var (
	heavyMu      sync.Mutex
	heavySlots   chan struct{}
	heavySize    int
	heavyWaiting atomic.Int64
)

func HeavyLaneSize() int {
	if cfg := globals.GetConfig(); cfg != nil && cfg.Api.Query.Workers > 0 {
		return cfg.Api.Query.Workers
	}

	return max(1, runtime.GOMAXPROCS(0)/2)
}

func heavyQueueLimit(size int) int64 {
	if cfg := globals.GetConfig(); cfg != nil {
		switch {
		case cfg.Api.Query.MaxQueued > 0:
			return int64(cfg.Api.Query.MaxQueued)
		case cfg.Api.Query.MaxQueued < 0:
			return -1
		}
	}

	return int64(size * defaultQueuePerWorker)
}

func heavyQueueTimeout() time.Duration {
	if cfg := globals.GetConfig(); cfg != nil {
		switch {
		case cfg.Api.Query.QueueTimeoutMs > 0:
			return time.Duration(cfg.Api.Query.QueueTimeoutMs) * time.Millisecond
		case cfg.Api.Query.QueueTimeoutMs < 0:
			return -1
		}
	}

	return defaultQueueTimeout
}

func HeavyWaiting() int {
	return int(heavyWaiting.Load())
}

func heavyLane() chan struct{} {
	size := HeavyLaneSize()

	heavyMu.Lock()
	defer heavyMu.Unlock()

	if heavySlots == nil || heavySize != size {
		heavySlots = make(chan struct{}, size)
		heavySize = size
	}

	return heavySlots
}

func heavyPlan(snap *datastore.Snapshot, cand candidates, evalIncludes string) bool {
	if evalIncludes != "" {
		return snap.Len() > heavyCandidates/8
	}

	if cand.all {
		return snap.Len() > heavyCandidates
	}

	maybe := cand.set.Count()
	if cand.exact != nil {
		maybe -= cand.exact.Count()
	}

	return maybe > heavyCandidates
}

func AcquireHeavyLane(guard *Guard) (func(), error) {
	return acquireHeavyLane(guard)
}

func acquireHeavyLane(guard *Guard) (func(), error) {
	if guard != nil && guard.Laned {
		return func() {}, nil
	}

	slots := heavyLane()
	release := func() { <-slots }

	if err := guard.Reserve(heavyEnvelope); err != nil {
		return nil, err
	}

	select {
	case slots <- struct{}{}:
		metrics.HeavyQuery(0)
		return release, nil
	default:
	}

	patient := guard == nil || guard.Admitted
	if limit := heavyQueueLimit(cap(slots)); !patient && limit >= 0 && heavyWaiting.Load() >= limit {
		metrics.HeavyRejected()
		return nil, guard.fail(ErrBusy)
	}

	heavyWaiting.Add(1)
	defer heavyWaiting.Add(-1)

	start := time.Now()
	timeout := heavyQueueTimeout()
	if patient {
		timeout = -1
	}
	ticker := time.NewTicker(heavyLanePollInterval)
	defer ticker.Stop()

	for {
		select {
		case slots <- struct{}{}:
			metrics.HeavyQuery(time.Since(start))
			return release, nil
		case <-ticker.C:
		}

		if err := guard.Check(); err != nil {
			metrics.HeavyQuery(time.Since(start))
			return nil, err
		}

		if timeout >= 0 && time.Since(start) >= timeout {
			metrics.HeavyRejected()
			return nil, guard.fail(ErrBusy)
		}
	}
}

func DeadlineFromConfig() time.Time {
	if cfg := globals.GetConfig(); cfg != nil && cfg.Api.Query.TimeoutMs > 0 {
		return time.Now().Add(time.Duration(cfg.Api.Query.TimeoutMs) * time.Millisecond)
	}

	return time.Time{}
}

func (p *planner) directPageIf(enabled bool, q ListQuery) ([]datastore.Doc, int, bool) {
	if !enabled {
		return nil, 0, false
	}

	return p.directPage(q)
}

func (p *planner) directPage(q ListQuery) ([]datastore.Doc, int, bool) {
	sorts := q.Sorts
	if len(sorts) == 0 && q.SortField != "" {
		sorts = []query.Sort{{Field: q.SortField, Ascending: q.SortAsc}}
	}

	if len(sorts) > 1 || q.Offset < 0 {
		return nil, 0, false
	}

	snap := p.snap
	total := snap.Len()
	docs := []datastore.Doc{}
	take := func(d datastore.Doc) bool {
		docs = append(docs, d)
		return q.Limit <= 0 || len(docs) < q.Limit
	}

	if len(sorts) == 0 {
		snap.Ascend(q.Offset, take)
		return docs, total, true
	}

	if len(p.refs) > 0 && p.sortPathCrossesReference(sorts[0].Field) {
		return nil, 0, false
	}

	view, ok := p.index(datastore.IndexSpec{Kind: datastore.IndexSort, Field: sorts[0].Field})
	if !ok {
		return nil, 0, false
	}

	snap = p.snap
	walkSortIndexFrom(view, sorts[0].Ascending, q.Offset, func(seq uint32) bool {
		d, ok := snap.BySeq(seq)
		if !ok {
			return true
		}
		return take(d)
	})

	return docs, snap.Len(), true
}

func countMatches(snap *datastore.Snapshot, cand candidates, needsMatch, everythingExact bool, verify func(datastore.Doc) bool) int {
	if !needsMatch || everythingExact {
		return snap.Len()
	}

	if !cand.all && cand.exact != nil {
		set := cand.set.Clone()
		set.AndNot(cand.exact)
		if set.Count() == 0 {
			return cand.exact.Count()
		}
	}

	n := 0
	iterateCandidates(snap, cand, func(d datastore.Doc) bool {
		if verify(d) {
			n++
		}
		return true
	})

	return n
}

func iterateCandidates(snap *datastore.Snapshot, cand candidates, fn func(datastore.Doc) bool) {
	if cand.all || cand.set.Count()*16 > snap.Len() {
		snap.Ascend(0, fn)
		return
	}

	cand.set.ForEach(func(seq uint32) bool {
		d, ok := snap.BySeq(seq)
		if !ok {
			return true
		}
		return fn(d)
	})
}

type orderedSource func(fn func(datastore.Doc) bool)

func paginate(src orderedSource, offset, limit int, countAll bool) ([]datastore.Doc, int) {
	if offset < 0 {
		offset = 0
	}

	docs := []datastore.Doc{}
	n := 0
	src(func(d datastore.Doc) bool {
		if n >= offset && (limit <= 0 || len(docs) < limit) {
			docs = append(docs, d)
		}
		n++

		return countAll || limit <= 0 || len(docs) < limit || n < offset
	})

	return docs, n
}

func (p *planner) orderFor(q ListQuery, cand candidates, needsMatch bool, verify func(datastore.Doc) bool) orderedSource {
	snap := p.snap
	sorts := q.Sorts
	if len(sorts) == 0 && q.SortField != "" {
		sorts = []query.Sort{{Field: q.SortField, Ascending: q.SortAsc}}
	}

	if len(sorts) == 0 {
		return func(fn func(datastore.Doc) bool) {
			if !needsMatch {
				snap.Ascend(0, fn)
				return
			}

			iterateCandidates(snap, cand, func(d datastore.Doc) bool {
				if !verify(d) {
					return true
				}
				return fn(d)
			})
		}
	}

	primary := sorts[0]
	sortAffected := len(p.refs) > 0 && p.sortPathCrossesReference(primary.Field)
	if !sortAffected {
		if view, ok := p.index(datastore.IndexSpec{Kind: datastore.IndexSort, Field: primary.Field}); ok {
			snap = p.snap
			accept := func(seq uint32) (datastore.Doc, bool) {
				if needsMatch && !cand.all && !cand.set.Has(seq) {
					return datastore.Doc{}, false
				}

				if !needsMatch || (cand.exact != nil && cand.exact.Has(seq)) {
					return snap.Indexed(seq), true
				}

				d, ok := snap.BySeq(seq)
				if !ok || (needsMatch && !verify(d)) {
					return datastore.Doc{}, false
				}

				return d, true
			}

			if len(sorts) == 1 {
				return func(fn func(datastore.Doc) bool) {
					walkSortIndex(view, primary.Ascending, func(seq uint32) bool {
						d, ok := accept(seq)
						if !ok {
							return true
						}
						return fn(d)
					})
				}
			}

			return func(fn func(datastore.Doc) bool) {
				walkSortGroups(view, primary.Ascending, func(seqs []uint32) bool {
					docs := make([]datastore.Doc, 0, len(seqs))
					for _, seq := range seqs {
						if d, ok := accept(seq); ok {
							docs = append(docs, d)
						}
					}

					for _, d := range sortGroup(docs, sorts[1:]) {
						if !fn(d) {
							return false
						}
					}

					return true
				})
			}
		}
	}

	return func(fn func(datastore.Doc) bool) {
		type row struct {
			doc  datastore.Doc
			keys [][]byte
			vals []any
		}

		reader := includeReader()
		rows := []row{}
		iterateCandidates(snap, cand, func(d datastore.Doc) bool {
			if needsMatch && !verify(d) {
				return true
			}

			doc, err := d.Decode()
			if err != nil {
				return true
			}

			if sortAffected {
				doc = applyIncludesWith([]map[string]any{doc}, "all", reader)[0]
			}

			r := row{doc: d}
			for _, s := range sorts {
				v := docpath.SortValue(doc, s.Field)
				r.vals = append(r.vals, v)
				r.keys = append(r.keys, datastore.SortValueKey(v))
			}
			rows = append(rows, r)

			return true
		})

		sort.SliceStable(rows, func(i, j int) bool {
			for k, s := range sorts {
				a, b := rows[i].vals[k], rows[j].vals[k]
				if a == nil || b == nil {
					if a == nil && b == nil {
						continue
					}
					return b == nil
				}

				c := compareSortValues(a, b)
				if c == 0 {
					continue
				}

				if s.Ascending {
					return c < 0
				}

				return c > 0
			}

			return false
		})

		for _, r := range rows {
			if !fn(r.doc) {
				return
			}
		}
	}
}

func sortGroup(docs []datastore.Doc, sorts []query.Sort) []datastore.Doc {
	if len(docs) < 2 || len(sorts) == 0 {
		return docs
	}

	type row struct {
		doc  datastore.Doc
		vals []any
	}

	rows := make([]row, 0, len(docs))
	for _, d := range docs {
		data, err := d.Decode()
		if err != nil {
			continue
		}

		r := row{doc: d}
		for _, s := range sorts {
			r.vals = append(r.vals, docpath.SortValue(data, s.Field))
		}
		rows = append(rows, r)
	}

	sort.SliceStable(rows, func(i, j int) bool {
		for k, s := range sorts {
			a, b := rows[i].vals[k], rows[j].vals[k]
			if a == nil || b == nil {
				if a == nil && b == nil {
					continue
				}
				return b == nil
			}

			c := compareSortValues(a, b)
			if c == 0 {
				continue
			}

			if s.Ascending {
				return c < 0
			}

			return c > 0
		}

		return false
	})

	out := make([]datastore.Doc, len(rows))
	for i, r := range rows {
		out[i] = r.doc
	}

	return out
}

func (p *planner) sortPathCrossesReference(field string) bool {
	for _, ref := range p.refs {
		if field == ref.Path+".id" || field == ref.Path+".@entity" {
			continue
		}

		if field == ref.Path || strings.HasPrefix(field, ref.Path+".") {
			return true
		}
	}

	return false
}

func groupEnd(view *datastore.IndexView, i int) int {
	key, _, ok := view.At(i)
	if !ok {
		return i
	}

	return view.Rank([]byte(datastore.KeyPayload(key)+"\xff\xff\xff\xff\xff")) - 1
}

func groupStart(view *datastore.IndexView, i int) int {
	key, _, ok := view.At(i)
	if !ok {
		return i
	}

	return view.Rank([]byte(datastore.KeyPayload(key)))
}

func emitRange(view *datastore.IndexView, start, end int, fn func(seq uint32) bool) bool {
	i := start
	stopped := false
	view.AscendAt(start, func(_ string, seq uint32) bool {
		if i > end {
			return false
		}
		i++
		if !fn(seq) {
			stopped = true
			return false
		}
		return true
	})

	return !stopped
}

func walkSortIndexFrom(view *datastore.IndexView, ascending bool, skip int, fn func(seq uint32) bool) {
	missingStart := view.Rank([]byte{datastore.SortRankMissing})
	total := view.Len()

	if ascending {
		view.AscendAt(skip, func(_ string, seq uint32) bool { return fn(seq) })
		return
	}

	end := missingStart - 1
	for end >= 0 {
		start := groupStart(view, end)
		size := end - start + 1
		if skip >= size {
			skip -= size
			end = start - 1
			continue
		}

		if !emitRange(view, start+skip, end, fn) {
			return
		}

		skip = 0
		end = start - 1
	}

	if missingStart+skip < total {
		view.AscendAt(missingStart+skip, func(_ string, seq uint32) bool { return fn(seq) })
	}
}

func walkSortIndex(view *datastore.IndexView, ascending bool, fn func(seq uint32) bool) {
	walkSortIndexFrom(view, ascending, 0, fn)
}

func walkSortGroups(view *datastore.IndexView, ascending bool, fn func(seqs []uint32) bool) {
	missingStart := view.Rank([]byte{datastore.SortRankMissing})
	total := view.Len()

	group := func(start, end int) bool {
		seqs := make([]uint32, 0, end-start+1)
		emitRange(view, start, end, func(seq uint32) bool {
			seqs = append(seqs, seq)
			return true
		})
		return fn(seqs)
	}

	if ascending {
		for i := 0; i < missingStart; {
			end := groupEnd(view, i)
			if !group(i, end) {
				return
			}
			i = end + 1
		}
	} else {
		for end := missingStart - 1; end >= 0; {
			start := groupStart(view, end)
			if !group(start, end) {
				return
			}
			end = start - 1
		}
	}

	if missingStart < total {
		group(missingStart, total-1)
	}
}

func decodeMap(b []byte) map[string]any {
	doc, err := datastore.DecodeMap(b)
	if err != nil {
		return nil
	}

	return doc
}

type SeqFilter func(seq uint32) bool

func (p *planner) storedLeaf(field string, ops map[string]string) SeqFilter {
	if p.affectedByIncludes(field) {
		return nil
	}

	view, ok := p.filterIndex(field)
	if !ok {
		return nil
	}

	present := &datastore.Bitset{}
	key := []byte{datastore.KeyPresent}
	collect(view, key, afterTerm(key), present, nil)

	c := p.leaf(field, ops)
	if c.all {
		return nil
	}

	return func(seq uint32) bool {
		return !present.Has(seq) || c.set.Has(seq)
	}
}

func (p *planner) storedFields(filters map[string]map[string]string) SeqFilter {
	var parts []SeqFilter
	for field, ops := range filters {
		if f := p.storedLeaf(field, ops); f != nil {
			parts = append(parts, f)
		}
	}

	if len(parts) == 0 {
		return nil
	}

	return func(seq uint32) bool {
		for _, f := range parts {
			if !f(seq) {
				return false
			}
		}
		return true
	}
}

func (p *planner) storedNode(n query.FilterNode) SeqFilter {
	if n.Leaf != nil {
		return p.storedFields(n.Leaf)
	}

	children := n.And
	isAnd := true
	if len(children) == 0 {
		children = n.Or
		isAnd = false
	}

	if len(children) == 0 {
		return func(uint32) bool { return false }
	}

	parts := make([]SeqFilter, 0, len(children))
	for _, child := range children {
		f := p.storedNode(child)
		if f == nil && !isAnd {
			return nil
		}
		if f != nil {
			parts = append(parts, f)
		}
	}

	if len(parts) == 0 {
		return nil
	}

	return func(seq uint32) bool {
		for _, f := range parts {
			if f(seq) == !isAnd {
				return !isAnd
			}
		}
		return isAnd
	}
}

func StoredCandidates(snap *datastore.Snapshot, filters map[string]map[string]string, node *query.FilterNode, includes string) SeqFilter {
	e := db()
	if e == nil || snap == nil {
		return nil
	}

	p := &planner{e: e, snap: snap, refs: snap.Refs(), inc: includes != ""}
	if node != nil {
		return p.storedNode(*node)
	}

	return p.storedFields(filters)
}
