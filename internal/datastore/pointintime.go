package datastore

import (
	"sort"
	"sync"
	"time"
)

type KVItem struct {
	Key       string
	Value     []byte
	ExpiresAt int64
}

type PointInTime struct {
	Snapshots map[string]*Snapshot
	Types     []string
	KV        []KVItem
	LSN       uint64
	At        time.Time

	e    *Engine
	once sync.Once
}

func (e *Engine) PointInTime() *PointInTime {
	e.j.Hold()
	e.pages.Hold()

	for {
		ents := e.allEntities()
		sort.Slice(ents, func(i, j int) bool { return ents[i].name < ents[j].name })
		for _, ent := range ents {
			ent.apply.Lock()
		}
		e.typesMu.Lock()

		if len(e.allEntities()) != len(ents) {
			e.typesMu.Unlock()
			for i := len(ents) - 1; i >= 0; i-- {
				ents[i].apply.Unlock()
			}
			continue
		}

		p := &PointInTime{e: e, Snapshots: make(map[string]*Snapshot, len(ents)), Types: e.Types(), LSN: e.j.NextLSN() - 1, At: time.Now()}
		for _, ent := range ents {
			p.Snapshots[ent.name] = &Snapshot{e: e, ent: ent, r: ent.root.Load(), name: ent.name}
		}

		e.kv.lockAll()
		p.KV = make([]KVItem, 0, e.kv.count.Load())
		for _, sh := range e.kv.shards {
			for key, entry := range sh.m {
				p.KV = append(p.KV, KVItem{Key: key, Value: entry.value, ExpiresAt: entry.expiresAt})
			}
		}
		e.kv.unlockAll()

		e.typesMu.Unlock()
		for i := len(ents) - 1; i >= 0; i-- {
			ents[i].apply.Unlock()
		}

		sort.Slice(p.KV, func(i, j int) bool { return p.KV[i].Key < p.KV[j].Key })

		return p
	}
}

func (p *PointInTime) Release() {
	p.once.Do(func() {
		p.e.j.Release()
		p.e.pages.Release()
	})
}
