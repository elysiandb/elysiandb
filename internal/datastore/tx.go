package datastore

import (
	"encoding/json"
	"errors"
	"github.com/taymour/elysiandb/internal/log"
	"sort"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
)

const (
	maxCommitAttempts = 64
	failedWriteWait   = 2 * time.Second
)

type txOp struct {
	kind   journal.Kind
	entity string
	id     string
	value  []byte
	doc    map[string]any
}

type Tx struct {
	e       *Engine
	reads   map[string]uint64
	ops     []txOp
	overlay map[string]int
	resets  map[string]bool
	clear   bool
	decoded map[string]decodedDoc
}

type decodedDoc struct {
	value []byte
	doc   map[string]any
}

func docKey(entity, id string) string {
	return entity + "\x00" + id
}

func (e *Engine) newTx() *Tx {
	return &Tx{e: e, reads: map[string]uint64{}, overlay: map[string]int{}, resets: map[string]bool{}}
}

var errAbort = errors.New("datastore: aborted")

func (e *Engine) Update(fn func(tx *Tx) error) error {
	if e.closed.Load() {
		return ErrClosed
	}

	e.writers.Add(1)
	defer e.writers.Done()

	e.waitForDirty()
	e.waitForBacklog()

	for attempt := 0; ; attempt++ {
		tx := e.newTx()
		if err := fn(tx); err != nil {
			return err
		}

		ticket, err := e.commit(tx)
		if errors.Is(err, ErrConflict) && attempt < maxCommitAttempts {
			e.stats.conflicts.Add(1)
			if attempt > 4 {
				time.Sleep(time.Duration(attempt) * 50 * time.Microsecond)
			}
			continue
		}

		if err != nil {
			return err
		}

		return e.waitDurable(ticket)
	}
}

const (
	retryFirst = 20 * time.Millisecond
	retryMax   = time.Second
)

func (e *Engine) waitDurable(ticket journal.Ticket) error {
	err := ticket.Wait()
	if err == nil {
		return nil
	}

	if suppressed, ok := writeFailures.Allow("log"); ok {
		engineLog.Error("log write failed, writes are refused until the disk accepts them", log.WithSuppressed([]any{"err", err}, suppressed)...)
	}

	delay := retryFirst
	deadline := time.Now().Add(failedWriteWait)
	for !e.closed.Load() && time.Now().Before(deadline) {
		time.Sleep(delay)
		delay = min(2*delay, retryMax)

		if err = e.j.Flush(); err == nil {
			if err = ticket.Wait(); err == nil {
				return nil
			}
		}
	}

	if e.closed.Load() {
		return ErrClosed
	}

	if errors.Is(err, syscall.ENOSPC) {
		return e.refuse(ErrDiskFull)
	}

	return e.refuse(ErrWriteFailed)
}

var writeFailures = log.NewThrottle(time.Minute)

func (tx *Tx) lookup(entity, id string) (*txOp, bool) {
	if i, ok := tx.overlay[docKey(entity, id)]; ok {
		return &tx.ops[i], true
	}

	if tx.resets[entity] || tx.clear {
		return nil, true
	}

	return nil, false
}

func (tx *Tx) ReadBytes(entity, id string) ([]byte, bool) {
	if op, ok := tx.lookup(entity, id); ok {
		if op == nil || op.kind == journal.KindDocDelete {
			return nil, false
		}

		return op.value, true
	}

	key := docKey(entity, id)
	doc, ok := tx.e.Snapshot(entity).Get(id)
	if !ok {
		if _, seen := tx.reads[key]; !seen {
			tx.reads[key] = 0
		}
		return nil, false
	}

	if _, seen := tx.reads[key]; !seen {
		tx.reads[key] = doc.LSN()
	}

	b, err := doc.Bytes()
	if err != nil {
		return nil, false
	}

	return b, true
}

func (tx *Tx) ReadShared(entity, id string) map[string]any {
	b, ok := tx.ReadBytes(entity, id)
	if !ok {
		return nil
	}

	key := docKey(entity, id)
	if c, ok := tx.decoded[key]; ok && len(c.value) == len(b) && len(b) > 0 && &c.value[0] == &b[0] {
		return c.doc
	}

	doc := decodeDoc(b)
	if tx.decoded == nil {
		tx.decoded = map[string]decodedDoc{}
	}
	tx.decoded[key] = decodedDoc{value: b, doc: doc}

	return doc
}

func (tx *Tx) Read(entity, id string) map[string]any {
	b, ok := tx.ReadBytes(entity, id)
	if !ok {
		return nil
	}

	return decodeDoc(b)
}

func (tx *Tx) Exists(entity, id string) bool {
	_, ok := tx.ReadBytes(entity, id)
	return ok
}

func (tx *Tx) Put(entity, id string, doc map[string]any) error {
	value, err := json.Marshal(doc)
	if err != nil {
		return err
	}

	return tx.PutBytes(entity, id, value, decodeDoc(value))
}

func (tx *Tx) PutBytes(entity, id string, value []byte, doc map[string]any) error {
	tx.record(txOp{kind: journal.KindDocPut, entity: entity, id: id, value: value, doc: doc})
	return nil
}

func (tx *Tx) Delete(entity, id string) {
	tx.record(txOp{kind: journal.KindDocDelete, entity: entity, id: id})
}

func (tx *Tx) record(op txOp) {
	tx.ops = append(tx.ops, op)
	tx.overlay[docKey(op.entity, op.id)] = len(tx.ops) - 1
}

func (tx *Tx) ResetEntity(entity string) {
	for key, i := range tx.overlay {
		if tx.ops[i].entity == entity {
			delete(tx.overlay, key)
		}
	}

	tx.ops = append(tx.ops, txOp{kind: journal.KindEntityReset, entity: entity})
	tx.resets[entity] = true
}

func (tx *Tx) EnsureType(entity string) {
	tx.ops = append(tx.ops, txOp{kind: journal.KindEntityCreate, entity: entity})
}

func (tx *Tx) DropType(entity string) {
	tx.ops = append(tx.ops, txOp{kind: journal.KindEntityDrop, entity: entity})
}

func (tx *Tx) ClearAll() {
	tx.ops = append(tx.ops, txOp{kind: journal.KindClearAll})
	tx.overlay = map[string]int{}
	tx.clear = true
}

func (tx *Tx) TypeExists(entity string) bool {
	exists := tx.e.TypeExists(entity)
	for _, op := range tx.ops {
		switch {
		case op.kind == journal.KindEntityCreate && op.entity == entity:
			exists = true
		case op.kind == journal.KindEntityDrop && op.entity == entity:
			exists = false
		case op.kind == journal.KindClearAll:
			exists = false
		}
	}

	return exists
}

func (tx *Tx) Empty() bool {
	return len(tx.ops) == 0
}

func (tx *Tx) addsData() bool {
	for _, op := range tx.ops {
		if op.kind == journal.KindDocPut || op.kind == journal.KindEntityCreate {
			return true
		}
	}

	return false
}

func (e *Engine) commit(tx *Tx) (journal.Ticket, error) {
	if len(tx.ops) == 0 {
		return journal.Ticket{}, nil
	}

	if err := e.WriteRefusal(); err != nil && (tx.addsData() || e.j.Failure() != nil) {
		return journal.Ticket{}, e.refuse(err)
	}

	names := map[string]bool{}
	for _, op := range tx.ops {
		if op.entity != "" && op.kind != journal.KindEntityCreate && op.kind != journal.KindEntityDrop {
			names[op.entity] = true
		}
	}

	if tx.clear {
		e.catalogMu.RLock()
		for name := range e.entities {
			names[name] = true
		}
		e.catalogMu.RUnlock()
	}

	sorted := make([]string, 0, len(names))
	for name := range names {
		sorted = append(sorted, name)
	}
	sort.Strings(sorted)

	if e.opts.Background && len(tx.ops) >= bulkBatchRecords {
		e.readAhead(tx)
	}

	ents := make([]*entity, len(sorted))
	for i, name := range sorted {
		ents[i] = e.entity(name, true)
		ents[i].apply.Lock()
	}

	typeOps := tx.clear
	for _, op := range tx.ops {
		if op.kind == journal.KindEntityCreate || op.kind == journal.KindEntityDrop {
			typeOps = true
			break
		}
	}

	if typeOps {
		e.typesMu.Lock()
	}

	unlock := func() {
		if typeOps {
			e.typesMu.Unlock()
		}

		for i := len(ents) - 1; i >= 0; i-- {
			ents[i].apply.Unlock()
		}
	}

	for key, lsn := range tx.reads {
		entity, id := splitDocKey(key)
		current := uint64(0)
		if ent := e.entity(entity, false); ent != nil {
			if s, ok := ent.root.Load().slotByID(id, true); ok {
				current = s.lsn
			}
		}

		if current != lsn {
			unlock()
			return journal.Ticket{}, ErrConflict
		}
	}

	gens := map[string]uint64{}
	gen := func(name string) uint64 {
		if g, ok := gens[name]; ok {
			return g
		}

		g := e.entity(name, true).root.Load().gen
		gens[name] = g

		return g
	}

	recs := make([]journal.Record, 0, len(tx.ops))
	docs := make([]map[string]any, 0, len(tx.ops))
	types := e.Types()
	hasType := func(name string) bool {
		for _, t := range types {
			if t == name {
				return true
			}
		}
		return false
	}

	for _, op := range tx.ops {
		switch op.kind {
		case journal.KindDocPut:
			recs = append(recs, journal.Record{Kind: op.kind, Entity: op.entity, Key: op.id, Value: op.value, Gen: gen(op.entity)})
			docs = append(docs, op.doc)
		case journal.KindDocDelete:
			recs = append(recs, journal.Record{Kind: op.kind, Entity: op.entity, Key: op.id, Gen: gen(op.entity)})
			docs = append(docs, nil)
		case journal.KindEntityReset:
			g := gen(op.entity) + 1
			gens[op.entity] = g
			recs = append(recs, journal.Record{Kind: op.kind, Entity: op.entity, Gen: g})
			docs = append(docs, nil)
		case journal.KindEntityCreate:
			if hasType(op.entity) {
				continue
			}
			types = append(types, op.entity)
			recs = append(recs, journal.Record{Kind: op.kind, Entity: op.entity})
			docs = append(docs, nil)
		case journal.KindEntityDrop:
			if !hasType(op.entity) {
				continue
			}
			kept := types[:0:0]
			for _, t := range types {
				if t != op.entity {
					kept = append(kept, t)
				}
			}
			types = kept
			recs = append(recs, journal.Record{Kind: op.kind, Entity: op.entity})
			docs = append(docs, nil)
		case journal.KindClearAll:
			recs = append(recs, journal.Record{Kind: journal.KindKVReset})
			docs = append(docs, nil)
			for _, name := range sorted {
				g := gen(name) + 1
				gens[name] = g
				recs = append(recs, journal.Record{Kind: journal.KindEntityReset, Entity: name, Gen: g})
				docs = append(docs, nil)
			}
			for _, t := range types {
				recs = append(recs, journal.Record{Kind: journal.KindEntityDrop, Entity: t})
				docs = append(docs, nil)
			}
			types = nil
		}
	}

	if len(recs) == 0 {
		unlock()
		return journal.Ticket{}, nil
	}

	if tx.clear {
		e.kv.lockAll()
		defer e.kv.unlockAll()
	}

	ticket, err := e.j.Append(recs)
	if err != nil {
		unlock()
		return journal.Ticket{}, err
	}

	e.applyBatch(recs, docs, false)
	unlock()

	return ticket, nil
}

func splitDocKey(key string) (string, string) {
	for i := 0; i < len(key); i++ {
		if key[i] == 0 {
			return key[:i], key[i+1:]
		}
	}

	return key, ""
}
