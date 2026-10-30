package mongodb

import (
	"regexp"
	"slices"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
	"go.mongodb.org/mongo-driver/v2/bson"
)

const (
	bsonTypeString = "string"
	bsonTypeBool   = "bool"
	bsonTypeArray  = "array"
)

var bsonNumberTypes = []string{"double", "int"}

var (
	lowerInverseOnce sync.Once
	lowerInverse     map[rune][]rune
)

func BuildListPrefilter(filters map[string]map[string]string, narrowArrays bool) bson.M {
	fields := make([]string, 0, len(filters))
	for field := range filters {
		fields = append(fields, field)
	}
	sort.Strings(fields)

	clauses := make([]bson.M, 0, len(fields))
	for _, field := range fields {
		path, ok := MongoFieldPath(field)
		if !ok {
			continue
		}

		ops := make([]string, 0, len(filters[field]))
		for op := range filters[field] {
			ops = append(ops, op)
		}
		sort.Strings(ops)

		for _, op := range ops {
			clauses = append(clauses, operatorPrefilter(path, op, filters[field][op], narrowArrays))
		}
	}

	return andPrefilters(clauses)
}

func BuildQueryPrefilter(node query.FilterNode) bson.M {
	if node.Leaf != nil {
		return BuildListPrefilter(node.Leaf, true)
	}

	if len(node.And) > 0 {
		clauses := make([]bson.M, 0, len(node.And))
		for _, child := range node.And {
			if c := BuildQueryPrefilter(child); len(c) > 0 {
				clauses = append(clauses, c)
			}
		}

		return andPrefilters(clauses)
	}

	if len(node.Or) > 0 {
		clauses := make([]bson.M, 0, len(node.Or))
		for _, child := range node.Or {
			c := BuildQueryPrefilter(child)
			if len(c) == 0 {
				return bson.M{}
			}

			clauses = append(clauses, c)
		}

		if len(clauses) == 1 {
			return clauses[0]
		}

		return bson.M{"$or": clauses}
	}

	return bson.M{}
}

func MongoFieldPath(field string) (string, bool) {
	segments := strings.Split(field, ".")
	for _, s := range segments {
		if s == "" || strings.HasPrefix(s, "$") || strings.ContainsRune(s, 0) {
			return "", false
		}
	}

	switch segments[0] {
	case "id":
		segments[0] = "_id"
	case "_id":
		segments[0] = StoredUserIdField
	}

	return strings.Join(segments, "."), true
}

func andPrefilters(clauses []bson.M) bson.M {
	switch len(clauses) {
	case 0:
		return bson.M{}
	case 1:
		return clauses[0]
	}

	return bson.M{"$and": clauses}
}

func operatorPrefilter(path, op, cmp string, narrowArrays bool) bson.M {
	var checked []string
	branches := make([]bson.M, 0, 8)
	num, isNumber := parseFilterNumber(cmp)

	switch op {
	case "eq", "neq":
		checked = append(slices.Clone(bsonNumberTypes), bsonTypeString, bsonTypeBool)
		branches = append(branches, scalarEqualityBranches(path, op, cmp, num, isNumber)...)
	case "gt", "gte", "lt", "lte":
		checked = slices.Clone(bsonNumberTypes)
		if isNumber {
			branches = append(branches, bson.M{path: bson.M{"$" + op: num}})
		}
	case "contains", "any", "all":
		checked = slices.Clone(bsonNumberTypes)
		if isNumber {
			branches = append(branches, bson.M{path: bson.M{"$type": bsonNumberTypes}})
		}
		if narrowArrays {
			checked = append(checked, bsonTypeArray)
			branches = append(branches, arrayMembershipBranch(path, op, cmp))
		}
	default:
		checked = slices.Clone(bsonNumberTypes)
		if isNumber {
			branches = append(branches, bson.M{path: bson.M{"$type": bsonNumberTypes}})
		}
	}

	branches = append(branches, bson.M{path: bson.M{"$not": bson.M{"$type": checked}}})
	if !slices.Contains(checked, bsonTypeArray) {
		branches = append(branches, bson.M{path: bson.M{"$type": bsonTypeArray}})
	}

	branches = append(branches, unresolvedPathBranches(path)...)

	return bson.M{"$or": branches}
}

func scalarEqualityBranches(path, op, cmp string, num float64, isNumber bool) []bson.M {
	branches := make([]bson.M, 0, 3)
	expected := cmp == "true" || cmp == "1"

	if op == "eq" {
		if isNumber {
			branches = append(branches, bson.M{path: num})
		}

		if api_storage.IsDate(cmp) {
			branches = append(branches, bson.M{path: bson.M{"$type": bsonTypeString}})
		} else if pattern, ok := GlobToRegex(cmp); ok {
			branches = append(branches, bson.M{path: bson.Regex{Pattern: pattern, Options: "s"}})
		}

		return append(branches, bson.M{path: expected})
	}

	if isNumber {
		branches = append(branches, bson.M{path: bson.M{"$type": bsonNumberTypes, "$ne": num}})
	}

	pattern, ok := GlobToRegex(cmp)
	if api_storage.IsDate(cmp) || !ok {
		branches = append(branches, bson.M{path: bson.M{"$type": bsonTypeString}})
	} else {
		branches = append(branches, bson.M{path: bson.M{
			"$type": bsonTypeString,
			"$not":  bson.Regex{Pattern: pattern, Options: "s"},
		}})
	}

	return append(branches, bson.M{path: !expected})
}

func arrayMembershipBranch(path, op, cmp string) bson.M {
	values := []string{cmp}
	if strings.Contains(cmp, ",") {
		values = strings.Split(cmp, ",")
	}

	anyArray := bson.M{path: bson.M{"$type": bsonTypeArray}}

	switch op {
	case "contains":
		candidates, ok := arrayElementCandidates(values[0])
		if !ok {
			return anyArray
		}

		return bson.M{path: bson.M{"$type": bsonTypeArray, "$in": candidates}}
	case "any":
		all := make([]any, 0, len(values)*2)
		for _, v := range values {
			candidates, ok := arrayElementCandidates(v)
			if !ok {
				return anyArray
			}

			all = append(all, candidates...)
		}

		return bson.M{path: bson.M{"$type": bsonTypeArray, "$in": all}}
	}

	clauses := []bson.M{anyArray}
	for _, v := range values {
		if candidates, ok := arrayElementCandidates(v); ok {
			clauses = append(clauses, bson.M{path: bson.M{"$in": candidates}})
		}
	}

	return andPrefilters(clauses)
}

func arrayElementCandidates(value string) ([]any, bool) {
	if strings.HasPrefix(value, "map[") || strings.HasPrefix(value, "[") {
		return nil, false
	}

	candidates := []any{value}
	if f, err := strconv.ParseFloat(value, 64); err == nil && strconv.FormatFloat(f, 'f', -1, 64) == value {
		candidates = append(candidates, f)
	}

	if d, err := bson.ParseDecimal128(value); err == nil && d.String() == value {
		candidates = append(candidates, d)
	}

	if t, err := time.Parse(time.RFC3339, value); err == nil && t.UTC().Format(time.RFC3339Nano) == value {
		candidates = append(candidates, bson.NewDateTimeFromTime(t))
	}

	switch value {
	case "true":
		candidates = append(candidates, true)
	case "false":
		candidates = append(candidates, false)
	case "<nil>":
		candidates = append(candidates, nil)
	}

	return candidates, true
}

func unresolvedPathBranches(path string) []bson.M {
	segments := strings.Split(path, ".")
	branches := make([]bson.M, 0, 2*(len(segments)-1))
	for i := 1; i < len(segments); i++ {
		prefix := strings.Join(segments[:i], ".")
		branches = append(branches,
			bson.M{prefix: bson.M{"$type": bsonTypeArray}},
			bson.M{prefix + ".@entity": bson.M{"$exists": true}},
		)
	}

	return branches
}

func parseFilterNumber(v string) (float64, bool) {
	f, err := strconv.ParseFloat(v, 64)

	return f, err == nil
}

func GlobToRegex(pattern string) (string, bool) {
	var b strings.Builder
	b.WriteString("^")

	runes := []rune(strings.ToLower(pattern))
	for i := 0; i < len(runes); i++ {
		r := runes[i]
		switch r {
		case '\\':
			if i+1 >= len(runes) {
				return "", false
			}

			i++
			writeCaseInsensitiveRune(&b, runes[i])
		case '*':
			b.WriteString(".*")
		default:
			writeCaseInsensitiveRune(&b, r)
		}
	}

	b.WriteString(`\z`)

	return b.String(), true
}

func writeCaseInsensitiveRune(b *strings.Builder, lower rune) {
	variants := append([]rune{lower}, lowerInverseOf(lower)...)
	if len(variants) == 1 {
		b.WriteString(regexp.QuoteMeta(string(lower)))
		return
	}

	b.WriteString("(?:")
	for i, v := range variants {
		if i > 0 {
			b.WriteString("|")
		}
		b.WriteString(regexp.QuoteMeta(string(v)))
	}
	b.WriteString(")")
}

func lowerInverseOf(r rune) []rune {
	lowerInverseOnce.Do(func() {
		lowerInverse = map[rune][]rune{}
		for _, cr := range unicode.CaseRanges {
			for x := rune(cr.Lo); x <= rune(cr.Hi); x++ {
				if l := unicode.ToLower(x); l != x {
					lowerInverse[l] = append(lowerInverse[l], x)
				}
			}
		}
	})

	return lowerInverse[r]
}
