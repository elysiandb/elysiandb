package hook

import (
	"fmt"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
)

func GetPostReadHooksForEntity(entity string) []Hook {
	hooks, err := GetHooksForEntity(entity)
	if err != nil {
		return []Hook{}
	}

	postReadHooks := make([]Hook, 0)
	for _, hook := range hooks {
		if hook.Event == HookEventBeforeCreate {
			postReadHooks = append(postReadHooks, hook)
		}
	}

	return postReadHooks
}

func CreateHook(hook Hook) error {
	if hook.ID == "" {
		hook.ID = uuid.New().String()
	}

	if api_storage.EntityExists(HookEntity, hook.ID) {
		return fmt.Errorf("the hook '%s' already exists", hook.Name)
	}

	if hook.Script == "" {
		hook.Script = GetDefaultHookScriptJSForPostRead()
	}

	err := api_storage.WriteEntity(HookEntity, hook.ToDataMap())
	if len(err) > 0 {
		return fmt.Errorf("an error occured while creating the hook: %v", err)
	}

	return nil
}

func UpdateHook(hook *Hook) error {
	if !api_storage.EntityExists(HookEntity, hook.ID) {
		return fmt.Errorf("the hook '%s' does not exist", hook.Name)
	}

	err := api_storage.UpdateEntityById(HookEntity, hook.ID, hook.ToDataMap())
	if err != nil {
		return fmt.Errorf("an error occured while updating the hook: %v", err)
	}

	return nil
}

func DeleteHook(hookId string) error {
	if !api_storage.EntityExists(HookEntity, hookId) {
		return fmt.Errorf("the hook with id '%s' does not exist", hookId)
	}

	api_storage.DeleteEntityById(HookEntity, hookId)

	return nil
}

func GetHookById(hookId string) (*Hook, error) {
	if !api_storage.EntityExists(HookEntity, hookId) {
		return nil, fmt.Errorf("the hook with id '%s' does not exist", hookId)
	}

	data := api_storage.ReadEntityById(HookEntity, hookId)
	if data == nil {
		return nil, fmt.Errorf("an error occured while retrieving the hook with id '%s'", hookId)
	}

	hook := &Hook{}
	hook.FromDataMap(data)

	return hook, nil
}

func EntityHasHooks(entity string) bool {
	filters := map[string]map[string]string{
		"entity": {
			"eq": entity,
		},
	}

	data := api_storage.ListEntities(
		HookEntity,
		0,
		0,
		"priority",
		true,
		filters,
		"",
		"",
	)

	return len(data) > 0
}

func GetHooksForEntity(entity string) ([]Hook, error) {
	filters := map[string]map[string]string{
		"entity": {
			"eq": entity,
		},
	}

	data := api_storage.ListEntities(
		HookEntity,
		0,
		0,
		"priority",
		true,
		filters,
		"",
		"",
	)

	hooks := make([]Hook, 0, len(data))
	for _, item := range data {
		var hook Hook
		hook.FromDataMap(item)
		hooks = append(hooks, hook)
	}

	return hooks, nil
}
