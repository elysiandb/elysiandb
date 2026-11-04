package hook

import (
	"bytes"
	"sort"
	"sync"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/storage"
)

type registeredHook struct {
	hook    Hook
	sortKey []byte
}

type registry struct {
	mu       sync.Mutex
	engine   *datastore.Engine
	gen      uint64
	version  uint64
	loaded   bool
	hooks    []registeredHook
	byEntity map[string][]Hook
}

var hooks = &registry{}

func (r *registry) forEntity(entity string) ([]Hook, bool) {
	e := storage.Engine()
	if e == nil {
		return nil, false
	}

	gen, ver := e.EntityVersion(HookEntity)

	r.mu.Lock()
	defer r.mu.Unlock()

	if !r.loaded || r.engine != e || r.gen != gen || r.version != ver {
		snap := e.Snapshot(HookEntity)
		r.load(e, snap)
		snap.Release()
	}

	if list, ok := r.byEntity[entity]; ok {
		return list, true
	}

	list := []Hook{}
	for _, h := range r.hooks {
		if docpath.MatchGlob(entity, h.hook.Entity) {
			list = append(list, h.hook)
		}
	}

	r.byEntity[entity] = list

	return list, true
}

func (r *registry) load(e *datastore.Engine, snap *datastore.Snapshot) {
	list := []registeredHook{}
	snap.Ascend(0, func(d datastore.Doc) bool {
		data, err := d.Decode()
		if err != nil {
			return true
		}

		var h Hook
		h.FromDataMap(data)
		list = append(list, registeredHook{hook: h, sortKey: datastore.SortKey(data, "priority")})

		return true
	})

	sort.SliceStable(list, func(i, j int) bool {
		return bytes.Compare(list[i].sortKey, list[j].sortKey) < 0
	})

	r.engine = e
	r.gen = snap.Generation()
	r.version = snap.Version()
	r.hooks = list
	r.byEntity = map[string][]Hook{}
	r.loaded = true
}

func hooksForEntity(entity string) []Hook {
	if list, ok := hooks.forEntity(entity); ok {
		return list
	}

	return listHooksFromEngine(entity)
}

func listHooksFromEngine(entity string) []Hook {
	data := api_storage.ListEntities(
		HookEntity,
		0,
		0,
		"priority",
		true,
		map[string]map[string]string{"entity": {"eq": entity}},
		"",
		"",
	)

	out := make([]Hook, 0, len(data))
	for _, item := range data {
		var h Hook
		h.FromDataMap(item)
		out = append(out, h)
	}

	return out
}

func hooksForEvent(entity, event string) []Hook {
	out := []Hook{}
	for _, h := range hooksForEntity(entity) {
		if h.Event == event {
			out = append(out, h)
		}
	}

	return out
}
