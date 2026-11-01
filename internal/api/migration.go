package api_storage

import (
	"encoding/json"
	"fmt"
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

func ExecuteMigrations(migrationQueries []MigrationQuery) error {
	_, err := ExecuteMigrationsCount(migrationQueries)

	return err
}

func ExecuteMigrationsCount(migrationQueries []MigrationQuery) (int, error) {
	updated := 0
	for _, query := range migrationQueries {
		switch query.Action {
		case ACTION_SET:
			n, err := executeMigrationSetAction(query.Entity, query.Properties)
			updated += n
			if err != nil {
				return updated, err
			}
		default:
			return updated, fmt.Errorf("unsupported action: %s", query.Action)
		}
	}

	return updated, nil
}

func executeMigrationSetAction(entity string, properties map[string]any) (int, error) {
	entities := ListEntities(entity, 0, 0, "", true, map[string]map[string]string{}, "", "all")
	updated := 0

	for _, ent := range entities {
		for key, value := range properties {
			SetNestedField(ent, key, value)
		}

		id, _ := ent["id"].(string)
		if id == "" {
			if v, ok := ent["_id"].(string); ok {
				id = v
			}
		}

		if id == "" {
			return updated, fmt.Errorf("missing id")
		}

		UpdateEntityById(entity, id, ent)
		updated++
	}

	return updated, nil
}
