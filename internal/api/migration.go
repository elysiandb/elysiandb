package api_storage

import (
	"encoding/json"
	"fmt"

	"github.com/taymour/elysiandb/internal/log"
)

type MigrationQuery struct {
	Entity     string
	Action     string
	Properties map[string]any
}

const ACTION_SET = "set"

func ParseMigrationQuery(s, entity string) []MigrationQuery {
	var rawItems []map[string]json.RawMessage
	if err := json.Unmarshal([]byte(s), &rawItems); err != nil {
		return nil
	}

	out := make([]MigrationQuery, 0, len(rawItems))
	for _, item := range rawItems {
		for action, payload := range item {
			var propsList []map[string]any
			if err := json.Unmarshal(payload, &propsList); err == nil {
				for _, props := range propsList {
					out = append(out, MigrationQuery{
						Entity:     entity,
						Action:     action,
						Properties: props,
					})
				}

				continue
			}

			var props map[string]any
			if err := json.Unmarshal(payload, &props); err == nil {
				out = append(out, MigrationQuery{
					Entity:     entity,
					Action:     action,
					Properties: props,
				})
			}
		}
	}

	return out
}

type EntityLister func(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	search string,
	includesParam string,
) []map[string]any

type EntityUpdater func(entity, id string, updated map[string]any) map[string]any

type EntitiesIncluder func(data []map[string]any, includesParam string) []map[string]any

func ExecuteMigrations(migrationQueries []MigrationQuery) error {
	return ApplyMigrations(migrationQueries, ListEntities, ReadEntityById, includeFromStorage, UpdateEntityById)
}

func includeFromStorage(data []map[string]any, includesParam string) []map[string]any {
	return ApplyIncludes(data, includesParam, ReadEntityById)
}

func ApplyMigrations(
	migrationQueries []MigrationQuery,
	list EntityLister,
	read EntityReader,
	include EntitiesIncluder,
	update EntityUpdater,
) error {
	for _, query := range migrationQueries {
		switch query.Action {
		case ACTION_SET:
			log.Info("Executing migration set action on entity:", query.Entity)
			executeMigrationSetAction(query.Entity, query.Properties, list, read, include, update)
		default:
			return fmt.Errorf("unsupported action: %s", query.Action)
		}
	}

	return nil
}

func executeMigrationSetAction(
	entity string,
	properties map[string]any,
	list EntityLister,
	read EntityReader,
	include EntitiesIncluder,
	update EntityUpdater,
) error {
	for _, listed := range list(entity, 0, 0, "", true, map[string]map[string]string{}, "", "") {
		id, _ := listed["id"].(string)
		if id == "" {
			if v, ok := listed["_id"].(string); ok {
				id = v
			}
		}

		if id == "" {
			return fmt.Errorf("missing id")
		}

		ent := read(entity, id)
		if ent == nil {
			continue
		}

		ent = include([]map[string]any{ent}, "all")[0]
		for key, value := range properties {
			SetNestedField(ent, key, value)
		}

		update(entity, id, ent)
	}

	return nil
}
