package datastore

import (
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	bulkBatchRecords  = 64
	settleInlineOps   = 4096
	settleMaxPasses   = 3
	backlogIdleAfter  = 300 * time.Millisecond
	backlogTick       = 100 * time.Millisecond
	backlogOpOverhead = 48
	backlogMaxWait    = 30 * time.Second
	backlogWaitPoll   = 10 * time.Millisecond
)

type opKind uint8

const (
	opPut opKind = iota
	opDelete
	opBitmapAdd
	opBitmapRemove
	opPostingAdd
	opIrregularUp
	opIrregularDown
)

type indexOp struct {
	key  string
	seq  uint32
	kind opKind
}

type indexBacklog struct {
	mu      sync.Mutex
	ops     map[string][]indexOp
	bytes   int64
	running map[string]bool
	dropped map[string]bool
	pending atomic.Int64
	lastOp  atomic.Int64
	lagging atomic.Bool
	passMu  sync.Mutex
}

func changeOps(spec IndexSpec, seq uint32, oldDoc, newDoc map[string]any) []indexOp {
	chunk := seq >> postingChunkBits

	switch spec.Kind {
	case IndexSearch:
		_, added := diffGrams(searchGrams(oldDoc), searchGrams(newDoc))
		ops := make([]indexOp, 0, len(added))
		for _, g := range added {
			ops = append(ops, indexOp{key: postingKey(g, chunk), seq: seq, kind: opPostingAdd})
		}
		return ops
	case IndexComposite:
		oldKeys, oldIrregular := compositeKeys(spec, oldDoc)
		newKeys, newIrregular := compositeKeys(spec, newDoc)
		removed, added := diffSorted(oldKeys, newKeys)
		ops := make([]indexOp, 0, len(removed)+len(added)+1)
		sk := seqKey(seq)
		for _, k := range removed {
			ops = append(ops, indexOp{key: k + sk, seq: seq, kind: opDelete})
		}
		for _, k := range added {
			ops = append(ops, indexOp{key: k + sk, seq: seq, kind: opPut})
		}
		switch {
		case oldIrregular && !newIrregular:
			ops = append(ops, indexOp{key: irregularKey, seq: seq, kind: opIrregularDown})
		case !oldIrregular && newIrregular:
			ops = append(ops, indexOp{key: irregularKey, seq: seq, kind: opIrregularUp})
		}
		return ops
	}

	oldKeys := uniqueKeys(extractKeys(spec, oldDoc))
	newKeys := uniqueKeys(extractKeys(spec, newDoc))
	if equalStrings(oldKeys, newKeys) {
		return nil
	}

	removed, added := diffSorted(oldKeys, newKeys)
	ops := make([]indexOp, 0, len(removed)+len(added))
	if spec.Kind == IndexBitmap {
		for _, v := range removed {
			ops = append(ops, indexOp{key: bitmapKey(v, chunk), seq: seq, kind: opBitmapRemove})
		}
		for _, v := range added {
			ops = append(ops, indexOp{key: bitmapKey(v, chunk), seq: seq, kind: opBitmapAdd})
		}
		return ops
	}

	sk := seqKey(seq)
	for _, k := range removed {
		ops = append(ops, indexOp{key: k + sk, seq: seq, kind: opDelete})
	}
	for _, k := range added {
		ops = append(ops, indexOp{key: k + sk, seq: seq, kind: opPut})
	}

	return ops
}

func applyOps(tree pagetree.Tree, ops []indexOp) pagetree.Tree {
	sort.SliceStable(ops, func(i, j int) bool { return ops[i].key < ops[j].key })

	for i := range ops {
		op := &ops[i]
		off := uint16(op.seq & (postingChunk - 1))
		switch op.kind {
		case opPut:
			tree, _ = tree.Put(op.key, nil)
		case opDelete:
			tree, _ = tree.Delete(op.key)
		case opBitmapAdd:
			tree = tree.Upsert(op.key, func(c []byte, found bool) []byte {
				if !found {
					return postingAdd(nil, off)
				}
				return postingAdd(c, off)
			})
		case opBitmapRemove:
			cur, ok := tree.Get(op.key)
			if !ok {
				continue
			}
			next := postingRemove(cur, off)
			if next == nil {
				tree, _ = tree.Delete(op.key)
				continue
			}
			tree, _ = tree.Update(op.key, func([]byte) []byte { return next })
		case opPostingAdd:
			tree = tree.Upsert(op.key, func(c []byte, found bool) []byte {
				if !found {
					return postingAdd(nil, off)
				}
				if c[0] == 1 {
					c[1+off/8] |= 1 << (off % 8)
					return c
				}
				return postingAdd(c, off)
			})
		case opIrregularUp:
			tree = addIrregular(tree, 1)
		case opIrregularDown:
			tree = addIrregular(tree, -1)
		}
	}

	return tree
}

func (d *draft) queue(name string, ops []indexOp) {
	if len(ops) == 0 {
		return
	}

	if d.ops == nil {
		d.ops = map[string][]indexOp{}
	}

	d.ops[name] = append(d.ops[name], ops...)
}

func opsBytes(ops []indexOp) int64 {
	var n int64
	for i := range ops {
		n += int64(len(ops[i].key)) + backlogOpOverhead
	}

	return n
}

func (e *Engine) mergeBacklog(ent *entity, queued map[string][]indexOp) {
	b := &ent.backlog
	b.mu.Lock()
	defer b.mu.Unlock()

	if b.ops == nil {
		b.ops = map[string][]indexOp{}
	}

	count := 0
	var size int64
	for name, ops := range queued {
		b.ops[name] = append(b.ops[name], ops...)
		count += len(ops)
		size += opsBytes(ops)
	}

	b.bytes += size
	b.pending.Add(int64(count))
	b.lastOp.Store(time.Now().UnixNano())
	b.lagging.Store(true)
	e.backlogBytes.Add(size)
}

func (b *indexBacklog) active() bool {
	b.mu.Lock()
	defer b.mu.Unlock()

	return len(b.ops) > 0 || b.running != nil
}

func (e *Engine) forgetBacklog(ent *entity, name string) {
	b := &ent.backlog
	b.mu.Lock()
	defer b.mu.Unlock()

	if ops, ok := b.ops[name]; ok {
		size := opsBytes(ops)
		b.bytes -= size
		b.pending.Add(-int64(len(ops)))
		e.backlogBytes.Add(-size)
		delete(b.ops, name)
	}

	if b.running[name] {
		if b.dropped == nil {
			b.dropped = map[string]bool{}
		}
		b.dropped[name] = true
	}
}

func (e *Engine) forgetAllBacklog(ent *entity) {
	b := &ent.backlog
	b.mu.Lock()
	defer b.mu.Unlock()

	e.backlogBytes.Add(-b.bytes)
	b.ops = nil
	b.bytes = 0
	b.pending.Store(0)
	if b.running != nil {
		if b.dropped == nil {
			b.dropped = map[string]bool{}
		}
		for name := range b.running {
			b.dropped[name] = true
		}
	}
}

func (b *indexBacklog) isRunning(name string) bool {
	b.mu.Lock()
	defer b.mu.Unlock()

	return b.running[name]
}

func (b *indexBacklog) lagNames() map[string]bool {
	b.mu.Lock()
	defer b.mu.Unlock()

	if len(b.ops) == 0 && len(b.running) == 0 {
		return nil
	}

	out := make(map[string]bool, len(b.ops)+len(b.running))
	for name := range b.ops {
		out[name] = true
	}
	for name := range b.running {
		out[name] = true
	}

	return out
}

func (b *indexBacklog) take() (map[string][]indexOp, int64) {
	b.mu.Lock()
	defer b.mu.Unlock()

	work, size := b.ops, b.bytes
	b.ops = nil
	b.bytes = 0
	b.pending.Store(0)
	b.running = make(map[string]bool, len(work))
	b.dropped = nil
	for name := range work {
		b.running[name] = true
	}

	return work, size
}

func (e *Engine) runIndexPass(ent *entity) bool {
	b := &ent.backlog
	b.passMu.Lock()
	defer b.passMu.Unlock()

	if b.pending.Load() == 0 {
		return false
	}

	ent.apply.Lock()
	work, size := b.take()
	base := ent.root.Load()
	ent.apply.Unlock()

	defer e.backlogBytes.Add(-size)

	results := make(map[string]pagetree.Tree, len(work))
	failed := false
	func() {
		defer func() {
			if r := recover(); r != nil {
				failed = true
				engineLog.Error("an index could not be brought up to date, it will be rebuilt", "entity", ent.name, "err", r)
			}
		}()

		tok := pagetree.NewToken()
		defer tok.Freeze()

		for name, ops := range work {
			idx, ok := base.indexes[name]
			if !ok || !idx.ready {
				continue
			}

			results[name] = applyOps(idx.tree.WithToken(tok), ops).WithToken(nil)
		}
	}()

	ent.apply.Lock()
	defer ent.apply.Unlock()

	cur := ent.root.Load()
	nr := *cur
	nr.indexes = cloneIndexes(cur.indexes)

	b.mu.Lock()
	dropped := b.dropped
	b.running = nil
	b.dropped = nil
	still := len(b.ops) > 0
	b.mu.Unlock()

	if cur.gen == base.gen {
		for name := range work {
			idx, ok := cur.indexes[name]
			if !ok || !idx.ready || dropped[name] {
				continue
			}

			if failed {
				nr.indexes[name] = &index{spec: idx.spec, dormant: true}
				continue
			}

			if tree, done := results[name]; done {
				nr.indexes[name] = &index{spec: idx.spec, ready: true, tree: tree}
			}
		}
	}

	nr.lagging = still
	b.lagging.Store(still)
	ent.root.Store(&nr)

	return true
}

func (e *Engine) IndexedSnapshot(name string) *Snapshot {
	ent := e.entity(name, false)
	if ent == nil || !ent.backlog.lagging.Load() {
		return e.Snapshot(name)
	}

	return e.settle(ent, name)
}

func (e *Engine) settle(ent *entity, name string) *Snapshot {
	b := &ent.backlog
	for i := 0; i < settleMaxPasses && b.pending.Load() >= settleInlineOps; i++ {
		e.runIndexPass(ent)
	}

	b.passMu.Lock()
	defer b.passMu.Unlock()

	ent.apply.Lock()
	defer ent.apply.Unlock()

	cur := ent.root.Load()
	if !cur.lagging {
		return &Snapshot{e: e, ent: ent, r: cur, name: name}
	}

	work, size := b.take()
	e.backlogBytes.Add(-size)

	nr := *cur
	nr.indexes = cloneIndexes(cur.indexes)

	tok := pagetree.NewToken()
	for name, ops := range work {
		idx, ok := cur.indexes[name]
		if !ok || !idx.ready {
			continue
		}

		nr.indexes[name] = &index{spec: idx.spec, dormant: true}
		func() {
			defer func() {
				if r := recover(); r != nil {
					engineLog.Error("an index could not be brought up to date, it will be rebuilt", "entity", ent.name, "err", r)
				}
			}()

			nr.indexes[name] = &index{spec: idx.spec, ready: true, tree: applyOps(idx.tree.WithToken(tok), ops).WithToken(nil)}
		}()
	}
	tok.Freeze()

	b.mu.Lock()
	b.running = nil
	b.dropped = nil
	b.mu.Unlock()

	nr.lagging = false
	b.lagging.Store(false)
	ent.root.Store(&nr)

	return &Snapshot{e: e, ent: ent, r: &nr, name: name}
}

func (e *Engine) backlogLimit() int64 {
	return e.dirtyFlushBytes() / 2
}

func (e *Engine) waitForBacklog() {
	if !e.opts.Background {
		return
	}

	limit := 2 * e.backlogLimit()
	if e.backlogBytes.Load() < limit {
		return
	}

	deadline := time.Now().Add(backlogMaxWait)
	for e.backlogBytes.Load() >= limit && !e.closed.Load() && time.Now().Before(deadline) {
		time.Sleep(backlogWaitPoll)
	}
}

func (e *Engine) backlogLoop() {
	defer e.wg.Done()

	ticker := time.NewTicker(backlogTick)
	defer ticker.Stop()

	for {
		select {
		case <-e.stop:
			return
		case <-ticker.C:
		}

		e.guarded("index upkeep", e.drainBacklogs)
	}
}

func (e *Engine) drainBacklogs() {
	idleSince := time.Now().Add(-backlogIdleAfter).UnixNano()
	var largest *entity
	var largestBytes int64
	for _, ent := range e.allEntities() {
		b := &ent.backlog
		if b.pending.Load() == 0 {
			continue
		}

		if b.lastOp.Load() <= idleSince {
			e.runIndexPass(ent)
			continue
		}

		b.mu.Lock()
		size := b.bytes
		b.mu.Unlock()
		if size > largestBytes {
			largest, largestBytes = ent, size
		}
	}

	if largest != nil && e.backlogBytes.Load() >= e.backlogLimit() {
		e.runIndexPass(largest)
	}
}

func (e *Engine) settleAll() {
	for _, ent := range e.allEntities() {
		e.guarded("index upkeep", func() {
			for ent.backlog.pending.Load() > 0 {
				if !e.runIndexPass(ent) {
					return
				}
			}
		})
	}
}

func (e *Engine) IndexBacklogBytes() int64 {
	return e.backlogBytes.Load()
}

func (e *Engine) readAhead(tx *Tx) {
	ids := map[string][]string{}
	for _, op := range tx.ops {
		if op.kind == journal.KindDocPut || op.kind == journal.KindDocDelete {
			ids[op.entity] = append(ids[op.entity], op.id)
		}
	}

	for name, list := range ids {
		ent := e.entity(name, false)
		if ent == nil {
			continue
		}

		sort.Strings(list)
		r := ent.root.Load()
		for _, id := range list {
			r.slotByID(id, true)
		}
	}
}
