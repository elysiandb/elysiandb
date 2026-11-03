package hook

import (
	"fmt"

	"github.com/google/uuid"
	"github.com/taymour/elysiandb/internal/docpath"
	engine "github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
)

func GetPostReadHooksForEntity(entity string) []Hook {
	return hooksForEvent(entity, HookEventPostRead)
}

func GetPreReadHooksForEntity(entity string) []Hook {
	return hooksForEvent(entity, HookEventPreRead)
}

func CreateHook(hook *Hook) error {
	if hook.ID == "" {
		hook.ID = uuid.New().String()
	}

	if engine.EntityExists(HookEntity, hook.ID) {
		return fmt.Errorf("the hook '%s' already exists", hook.Name)
	}

	if hook.Script == "" {
		hook.Script = defaultScriptForEvent(hook.Event)
	}

	err := engine.WriteEntity(HookEntity, hook.ToDataMap())
	if len(err) > 0 {
		return fmt.Errorf("an error occurred while creating the hook: %v", err)
	}

	return nil
}

func UpdateHook(hook *Hook) error {
	if hook.Script == "" {
		hook.Script = defaultScriptForEvent(hook.Event)
	}

	if err := hook.Validate(); err != nil {
		return err
	}

	if engine.UpdateEntityById(HookEntity, hook.ID, hook.ToDataMap()) == nil {
		return fmt.Errorf("the hook '%s' does not exist", hook.Name)
	}

	return nil
}

func defaultScriptForEvent(event string) string {
	switch event {
	case HookEventPreRead:
		return GetDefaultHookScriptJSForPreRead()
	case HookEventPostRead:
		return GetDefaultHookScriptJSForPostRead()
	}

	return ""
}

func DeleteHook(hookId string) error {
	if !engine.EntityExists(HookEntity, hookId) {
		return fmt.Errorf("the hook with id '%s' does not exist", hookId)
	}

	engine.DeleteEntityById(HookEntity, hookId)

	return nil
}

func GetHookById(hookId string) (*Hook, error) {
	if !engine.EntityExists(HookEntity, hookId) {
		return nil, fmt.Errorf("the hook with id '%s' does not exist", hookId)
	}

	data := engine.ReadEntityById(HookEntity, hookId)
	if data == nil {
		return nil, fmt.Errorf("an error occurred while retrieving the hook with id '%s'", hookId)
	}

	hook := &Hook{}
	hook.FromDataMap(data)

	return hook, nil
}

func EntityHasHooks(entity string) bool {
	if !globals.GetConfig().Api.Hooks.Enabled {
		return false
	}

	return len(hooksForEntity(entity)) > 0
}

func entityHasHooksFor(entity, event string) bool {
	for _, h := range hooksForEntity(entity) {
		if docpath.MatchGlob(event, h.Event) {
			return true
		}
	}

	return false
}

func EntityHasPostReadHooks(entity string) bool {
	return entityHasHooksFor(entity, HookEventPostRead)
}

func EntityHasPreReadHooks(entity string) bool {
	return entityHasHooksFor(entity, HookEventPreRead)
}

func GetHooksForEntity(entity string) ([]Hook, error) {
	list := hooksForEntity(entity)

	return append(make([]Hook, 0, len(list)), list...), nil
}
