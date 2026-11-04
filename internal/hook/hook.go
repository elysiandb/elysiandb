package hook

import (
	"fmt"
	"log/slog"
	"sort"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

const HookEntity = "_elysiandb_core_hook"

const (
	HookEventPostRead = "post_read"
	HookEventPreRead  = "pre_read"
)

var HookEntitySchema = map[string]any{
	"name": map[string]any{
		"name":     "name",
		"required": true,
		"type":     "string",
	},
	"entity": map[string]any{
		"name":     "entity",
		"required": true,
		"type":     "string",
	},
	"script": map[string]any{
		"name":     "script",
		"required": true,
		"type":     "string",
	},
	"event": map[string]any{
		"name":     "event",
		"required": true,
		"type":     "string",
	},
	"language": map[string]any{
		"name":     "language",
		"required": true,
		"type":     "string",
	},
	"priority": map[string]any{
		"name":     "priority",
		"required": false,
		"type":     "number",
	},
	"bypass_acl": map[string]any{
		"name":     "bypass_acl",
		"required": false,
		"type":     "boolean",
	},
	"enabled": map[string]any{
		"name":     "enabled",
		"required": false,
		"type":     "boolean",
	},
}

type Hook struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Script    string `json:"script"`
	Entity    string `json:"entity"`
	Event     string `json:"event"`
	Language  string `json:"language"`
	Priority  int    `json:"priority"`
	ByPassACL bool   `json:"bypass_acl"`
	Enabled   bool   `json:"enabled"`
}

func (h *Hook) ToDataMap() map[string]any {
	return map[string]any{
		"id":         h.ID,
		"name":       h.Name,
		"script":     h.Script,
		"entity":     h.Entity,
		"event":      h.Event,
		"language":   h.Language,
		"priority":   h.Priority,
		"bypass_acl": h.ByPassACL,
		"enabled":    h.Enabled,
	}
}

func (h *Hook) Validate() error {
	requiredFields := []struct {
		name  string
		value string
	}{
		{"name", h.Name},
		{"entity", h.Entity},
		{"event", h.Event},
		{"language", h.Language},
		{"script", h.Script},
	}

	for _, field := range requiredFields {
		if field.value == "" {
			return fmt.Errorf("the hook field '%s' is required", field.name)
		}
	}

	return nil
}

func (h *Hook) FromDataMap(data map[string]any) {
	if val, ok := data["id"].(string); ok {
		h.ID = val
	}

	if val, ok := data["name"].(string); ok {
		h.Name = val
	}

	if val, ok := data["script"].(string); ok {
		h.Script = val
	}

	if val, ok := data["entity"].(string); ok {
		h.Entity = val
	}

	if val, ok := data["event"].(string); ok {
		h.Event = val
	}

	if val, ok := data["language"].(string); ok {
		h.Language = val
	}

	if v, ok := data["priority"]; ok {
		switch n := v.(type) {
		case int:
			h.Priority = n
		case int64:
			h.Priority = int(n)
		case float64:
			h.Priority = int(n)
		}
	}

	if val, ok := data["bypass_acl"].(bool); ok {
		h.ByPassACL = val
	}

	if val, ok := data["enabled"].(bool); ok {
		h.Enabled = val
	}
}

const (
	PhasePreRead  = "pre_read"
	PhasePostRead = "post_read"
)

var (
	hookLog          = log.For("hook")
	hookFailThrottle = log.NewThrottle(time.Minute)
)

func observeHook(h Hook, entity, phase string, start time.Time, err error) {
	metrics.ObserveHook(h.ID, entity, phase, time.Since(start), err)

	if err == nil {
		if log.Enabled(slog.LevelDebug) {
			hookLog.Debug("hook executed", "hook_id", h.ID, "entity", entity, "phase", phase, "duration_ms", log.Since(start))
		}
		return
	}

	if suppressed, ok := hookFailThrottle.Allow(h.ID); ok {
		hookLog.Warn("hook script failed", log.WithSuppressed([]any{"hook_id", h.ID, "entity", entity, "phase", phase, "err", err}, suppressed)...)
	}
}

func InitHooks() {
	if !globals.GetConfig().Api.Hooks.Enabled {
		return
	}

	if api_storage.EntityTypeExists(HookEntity) {
		return
	}

	err := CreateHookEntity()
	if err != nil {
		hookLog.Error("cannot create the hooks entity type", "err", err)
	}
}

func CreateHookEntity() error {
	err := api_storage.CreateEntityType(HookEntity)
	if err != nil {
		return err
	}

	api_storage.UpdateEntitySchema(HookEntity, HookEntitySchema)

	return nil
}

func ApplyPostReadHooksForEntity(username, entity string, data map[string]any) map[string]any {
	return NewRequest(username).PostRead(entity, data)
}

func ApplyPreReadHooksForEntity(username, entity string, data map[string]any) map[string]any {
	return NewRequest(username).PreRead(entity, data)
}

func (r *Request) PostRead(entity string, data map[string]any) map[string]any {
	return r.apply(entity, data, HookEventPostRead, PhasePostRead, "postRead")
}

func (r *Request) PreRead(entity string, data map[string]any) map[string]any {
	return r.apply(entity, data, HookEventPreRead, PhasePreRead, "preRead")
}

func (r *Request) apply(entity string, data map[string]any, event, phase, fnName string) map[string]any {
	if !globals.GetConfig().Api.Hooks.Enabled {
		return data
	}

	list := append([]Hook(nil), hooksForEvent(entity, event)...)
	sort.SliceStable(list, func(i, j int) bool {
		return list[i].Priority > list[j].Priority
	})

	for _, h := range list {
		if !h.Enabled {
			continue
		}

		if r.overBudget() || r.guard.Stopped() {
			break
		}

		start := time.Now()
		err := r.applyScript(h, fnName, data)
		r.spent.Add(int64(time.Since(start)))
		observeHook(h, entity, phase, start, err)
	}

	return data
}
