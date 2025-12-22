package mongodb

import (
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
)

func NormalizeMongoDocument(doc map[string]any) map[string]any {
	return FromMongoDocument(doc)
}

func NormalizeMongoValue(v any) any {
	switch t := v.(type) {

	case bson.D:
		m := map[string]any{}
		for _, e := range t {
			m[e.Key] = NormalizeMongoValue(e.Value)
		}
		return m

	case bson.A:
		arr := make([]any, 0, len(t))
		for _, v := range t {
			arr = append(arr, NormalizeMongoValue(v))
		}
		return arr

	case map[string]any:
		if _, has := t["_id"]; has {
			return FromMongoDocument(t)
		}
		m := map[string]any{}
		for k, v := range t {
			m[k] = NormalizeMongoValue(v)
		}
		return m

	case []any:
		arr := make([]any, 0, len(t))
		for _, v := range t {
			arr = append(arr, NormalizeMongoValue(v))
		}
		return arr

	case time.Time:
		return t.Format(time.RFC3339)

	default:
		return v
	}
}

func ToMongoValue(v any) any {
	switch t := v.(type) {

	case string:
		if tm, err := time.Parse(time.RFC3339, t); err == nil {
			return tm
		}
		return t

	case map[string]any:
		m := bson.M{}
		for k, v := range t {
			m[k] = ToMongoValue(v)
		}
		return m

	case []any:
		arr := make(bson.A, 0, len(t))
		for _, v := range t {
			arr = append(arr, ToMongoValue(v))
		}
		return arr

	default:
		return v
	}
}

func ToMongoDocument(data map[string]interface{}) bson.M {
	doc := bson.M{}
	for k, v := range data {
		if k == "id" {
			if s, ok := v.(string); ok {
				doc["_id"] = s
			}
			continue
		}

		if s, ok := v.(string); ok {
			if t, err := time.Parse(time.RFC3339, s); err == nil {
				doc[k] = t
				continue
			}
		}

		doc[k] = v
	}
	return doc
}

func FromMongoDocument(doc map[string]any) map[string]any {
	out := map[string]any{}
	for k, v := range doc {
		if k == "_id" {
			switch t := v.(type) {
			case string:
				out["id"] = t
			case bson.ObjectID:
				out["id"] = t.Hex()
			}
			continue
		}
		out[k] = NormalizeMongoValue(v)
	}
	return out
}
