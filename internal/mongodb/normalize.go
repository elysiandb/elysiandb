package mongodb

import (
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
)

const StoredUserIdField = globals.CoreFieldsPrefix + "_id"

func NormalizeMongoDocument(doc map[string]any) map[string]any {
	return FromMongoDocument(doc)
}

func NormalizeMongoValue(v any) any {
	switch t := v.(type) {
	case bson.D:
		m := make(map[string]any, len(t))
		for _, e := range t {
			m[e.Key] = NormalizeMongoValue(e.Value)
		}

		return m

	case bson.M:
		m := make(map[string]any, len(t))
		for k, v := range t {
			m[k] = NormalizeMongoValue(v)
		}

		return m

	case map[string]any:
		m := make(map[string]any, len(t))
		for k, v := range t {
			m[k] = NormalizeMongoValue(v)
		}

		return m

	case bson.A:
		arr := make([]any, 0, len(t))
		for _, v := range t {
			arr = append(arr, NormalizeMongoValue(v))
		}

		return arr

	case []any:
		arr := make([]any, 0, len(t))
		for _, v := range t {
			arr = append(arr, NormalizeMongoValue(v))
		}

		return arr

	case int32:
		return float64(t)

	case int64:
		return float64(t)

	case bson.DateTime:
		return t.Time().UTC().Format(time.RFC3339Nano)

	case time.Time:
		return t.UTC().Format(time.RFC3339Nano)

	default:
		return v
	}
}

func ToMongoDocument(data map[string]any) bson.M {
	doc := make(bson.M, len(data))
	for k, v := range data {
		switch k {
		case "id":
		case "_id":
			doc[StoredUserIdField] = v
		default:
			doc[k] = v
		}
	}

	if id, ok := data["id"].(string); ok {
		doc["_id"] = id
	}

	return doc
}

func FromMongoDocument(doc map[string]any) map[string]any {
	out := make(map[string]any, len(doc))
	for k, v := range doc {
		switch k {
		case "_id":
			switch t := v.(type) {
			case string:
				out["id"] = t
			case bson.ObjectID:
				out["id"] = t.Hex()
			}
		case StoredUserIdField:
			out["_id"] = NormalizeMongoValue(v)
		default:
			out[k] = NormalizeMongoValue(v)
		}
	}

	return out
}

func cloneDocument(doc map[string]any) map[string]any {
	if doc == nil {
		return nil
	}

	return cloneValue(doc).(map[string]any)
}

func cloneValue(v any) any {
	switch t := v.(type) {
	case map[string]any:
		m := make(map[string]any, len(t))
		for k, v := range t {
			m[k] = cloneValue(v)
		}

		return m
	case []any:
		arr := make([]any, len(t))
		for i, v := range t {
			arr[i] = cloneValue(v)
		}

		return arr
	default:
		return v
	}
}
