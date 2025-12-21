package mongodb

import (
	"strconv"
	"strings"
	"time"

	"go.mongodb.org/mongo-driver/v2/bson"
)

func GlobToRegex(glob string) string {
	var b strings.Builder
	b.WriteString("^")

	for _, c := range glob {
		switch c {
		case '*':
			b.WriteString(".*")
		case '?':
			b.WriteString(".")
		case '.', '+', '(', ')', '|', '^', '$', '[', ']', '{', '}', '\\':
			b.WriteString("\\")
			b.WriteRune(c)
		default:
			b.WriteRune(c)
		}
	}

	b.WriteString("$")
	return b.String()
}

func IsGlobPattern(s string) bool {
	return strings.ContainsAny(s, "*?")
}

func BuildMongoFilters(filters map[string]map[string]string) bson.M {
	q := bson.M{}

	for field, ops := range filters {
		for op, raw := range ops {
			val := ParseFilterValue(raw)

			switch op {

			case "eq":
				if s, ok := val.(string); ok && IsGlobPattern(s) {
					q[field] = bson.M{"$regex": GlobToRegex(s), "$options": "i"}
					continue
				}
				if t, ok := val.(time.Time); ok {
					if IsDateOnly(raw) {
						q[field] = bson.M{"$gte": t, "$lt": t.Add(24 * time.Hour)}
					} else {
						q[field] = t
					}
					continue
				}
				if arr, ok := ParseArrayValues(raw); ok {
					q[field] = bson.M{"$all": arr, "$size": len(arr)}
					continue
				}
				q[field] = val

			case "neq":
				if s, ok := val.(string); ok && IsGlobPattern(s) {
					q[field] = bson.M{"$not": bson.M{"$regex": GlobToRegex(s), "$options": "i"}}
					continue
				}
				if t, ok := val.(time.Time); ok {
					if IsDateOnly(raw) {
						q[field] = bson.M{"$lt": t, "$gte": t.Add(24 * time.Hour)}
					} else {
						q[field] = bson.M{"$ne": t}
					}
					continue
				}
				q[field] = bson.M{"$ne": val}

			case "gt":
				if t, ok := val.(time.Time); ok && IsDateOnly(raw) {
					q[field] = bson.M{"$gt": t.Add(24 * time.Hour)}
					continue
				}
				q[field] = bson.M{"$gt": val}

			case "gte":
				q[field] = bson.M{"$gte": val}

			case "lt":
				if t, ok := val.(time.Time); ok && IsDateOnly(raw) {
					q[field] = bson.M{"$lt": t}
					continue
				}
				q[field] = bson.M{"$lt": val}

			case "lte":
				if t, ok := val.(time.Time); ok && IsDateOnly(raw) {
					q[field] = bson.M{"$lt": t.Add(24 * time.Hour)}
					continue
				}
				q[field] = bson.M{"$lte": val}

			case "contains":
				q[field] = val

			case "not_contains":
				q[field] = bson.M{"$ne": val}

			case "any":
				if arr, ok := ParseArrayValues(raw); ok {
					q[field] = bson.M{"$in": arr}
				}

			case "all":
				if arr, ok := ParseArrayValues(raw); ok {
					q[field] = bson.M{"$all": arr}
				}

			case "none":
				if arr, ok := ParseArrayValues(raw); ok {
					q[field] = bson.M{"$nin": arr}
				}
			}
		}
	}

	return q
}

func ParseArrayValues(v string) ([]any, bool) {
	if !strings.Contains(v, ",") {
		return nil, false
	}

	parts := strings.Split(v, ",")
	out := make([]any, 0, len(parts))
	for _, p := range parts {
		out = append(out, ParseFilterValue(strings.TrimSpace(p)))
	}

	return out, true
}

func ParseFilterValue(v string) any {
	if v == "true" || v == "false" || v == "1" || v == "0" {
		return v == "true" || v == "1"
	}

	if i, err := strconv.ParseInt(v, 10, 64); err == nil {
		return i
	}

	if f, err := strconv.ParseFloat(v, 64); err == nil {
		return f
	}

	if t, err := time.Parse(time.RFC3339, v); err == nil {
		return t
	}

	if t, err := time.Parse("2006-01-02", v); err == nil {
		return t.Truncate(24 * time.Hour)
	}

	return v
}

func IsDateOnly(v string) bool {
	_, err := time.Parse("2006-01-02", v)
	return err == nil
}
