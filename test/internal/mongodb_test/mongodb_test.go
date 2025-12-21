package mongodb_test

import (
	"reflect"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/mongodb"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func TestParseIncludes(t *testing.T) {
	all, paths := mongodb.ParseIncludes("")
	if all || paths != nil {
		t.Fatal()
	}

	all, paths = mongodb.ParseIncludes("all")
	if !all || paths != nil {
		t.Fatal()
	}

	all, paths = mongodb.ParseIncludes("author,comments.user")
	if all {
		t.Fatal()
	}

	if len(paths) != 2 {
		t.Fatal()
	}

	if !reflect.DeepEqual(paths[0], []string{"author"}) {
		t.Fatal()
	}
	if !reflect.DeepEqual(paths[1], []string{"comments", "user"}) {
		t.Fatal()
	}
}

func TestSingularFallback(t *testing.T) {
	if mongodb.SingularFallback("stories") != "story" {
		t.Fatal()
	}
	if mongodb.SingularFallback("boxes") != "boxe" {
		t.Fatal()
	}
	if mongodb.SingularFallback("users") != "user" {
		t.Fatal()
	}
	if mongodb.SingularFallback("user") != "user" {
		t.Fatal()
	}
}

func TestGetRefId(t *testing.T) {
	id, ok := mongodb.GetRefId(map[string]any{"id": "1"})
	if !ok || id != "1" {
		t.Fatal()
	}

	id, ok = mongodb.GetRefId(map[string]any{"_id": "2"})
	if !ok || id != "2" {
		t.Fatal()
	}

	if _, ok := mongodb.GetRefId(map[string]any{}); ok {
		t.Fatal()
	}
}

func TestIsRefMap(t *testing.T) {
	if !mongodb.IsRefMap(map[string]any{"@entity": "user", "id": "1"}) {
		t.Fatal()
	}

	if mongodb.IsRefMap(map[string]any{"@entity": "user", "id": "1", "x": "y"}) {
		t.Fatal()
	}
}

func TestRefEntityFromValue(t *testing.T) {
	if ent, ok := mongodb.RefEntityFromValue(map[string]any{"@entity": "user"}); !ok || ent != "user" {
		t.Fatal()
	}

	if ent, ok := mongodb.RefEntityFromValue(bson.M{"@entity": "post"}); !ok || ent != "post" {
		t.Fatal()
	}

	if ent, ok := mongodb.RefEntityFromValue(bson.D{{Key: "@entity", Value: "comment"}}); !ok || ent != "comment" {
		t.Fatal()
	}

	if ent, ok := mongodb.RefEntityFromValue([]any{map[string]any{"@entity": "x"}}); !ok || ent != "x" {
		t.Fatal()
	}
}

func TestGlobHelpers(t *testing.T) {
	if !mongodb.IsGlobPattern("a*") {
		t.Fatal()
	}
	if mongodb.IsGlobPattern("abc") {
		t.Fatal()
	}

	if mongodb.GlobToRegex("a*b") != "^a.*b$" {
		t.Fatal()
	}
}

func TestParseFilterValue(t *testing.T) {
	if mongodb.ParseFilterValue("true") != true {
		t.Fatal()
	}
	if mongodb.ParseFilterValue("10").(int64) != 10 {
		t.Fatal()
	}
	if mongodb.ParseFilterValue("1.5").(float64) != 1.5 {
		t.Fatal()
	}

	d := mongodb.ParseFilterValue("2024-01-01").(time.Time)
	if d.Hour() != 0 {
		t.Fatal()
	}
}

func TestParseArrayValues(t *testing.T) {
	arr, ok := mongodb.ParseArrayValues("a,b,c")
	if !ok || len(arr) != 3 {
		t.Fatal()
	}

	if _, ok := mongodb.ParseArrayValues("a"); ok {
		t.Fatal()
	}
}

func TestParseArrayValuesTyped(t *testing.T) {
	arr, ok := mongodb.ParseArrayValues("1,2,3")
	if !ok {
		t.Fatal()
	}

	if arr[0] != true || arr[1] != int64(2) || arr[2] != int64(3) {
		t.Fatal()
	}
}

func TestBuildMongoEq(t *testing.T) {
	if _, ok := mongodb.BuildMongoEq("a*")["$regex"]; !ok {
		t.Fatal()
	}

	if mongodb.BuildMongoEq("x")["$eq"] != "x" {
		t.Fatal()
	}
}

func TestBuildMongoEqDate(t *testing.T) {
	if mongodb.BuildMongoEq("2024-01-01")["$eq"] == nil {
		t.Fatal()
	}
}

func TestNormalizeMongoValue(t *testing.T) {
	if _, ok := mongodb.NormalizeMongoValue(time.Now()).(string); !ok {
		t.Fatal()
	}

	v := mongodb.NormalizeMongoValue(bson.M{"a": "b"})
	switch m := v.(type) {
	case bson.M:
		if m["a"] != "b" {
			t.Fatal()
		}
	case map[string]any:
		if m["a"] != "b" {
			t.Fatal()
		}
	default:
		t.Fatal()
	}

	arr := mongodb.NormalizeMongoValue(bson.A{"x"}).([]any)
	if arr[0] != "x" {
		t.Fatal()
	}
}

func TestToMongoValue(t *testing.T) {
	if _, ok := mongodb.ToMongoValue(time.Now().UTC().Format(time.RFC3339)).(time.Time); !ok {
		t.Fatal()
	}

	m := mongodb.ToMongoValue(map[string]any{"a": "b"}).(bson.M)
	if m["a"] != "b" {
		t.Fatal()
	}
}

func TestFromMongoDocument(t *testing.T) {
	out := mongodb.FromMongoDocument(map[string]any{"_id": "1", "a": "b"})
	if out["id"] != "1" || out["a"] != "b" {
		t.Fatal()
	}
}

func TestFromMongoDocumentObjectID(t *testing.T) {
	id := bson.NewObjectID()
	out := mongodb.FromMongoDocument(map[string]any{"_id": id})
	if out["id"] != id.Hex() {
		t.Fatal()
	}
}

func TestRegexpEscape(t *testing.T) {
	if mongodb.RegexpEscape("a+b*c(d)") != "a\\+b*c\\(d\\)" {
		t.Fatal()
	}
}

func TestGlobToRegexFull(t *testing.T) {
	if mongodb.GlobToRegex("a?c") != "^a.c$" {
		t.Fatal()
	}
}

func TestIsGlobPatternFull(t *testing.T) {
	if mongodb.IsGlobPattern("abc") {
		t.Fatal()
	}
	if !mongodb.IsGlobPattern("a?c") {
		t.Fatal()
	}
}

func TestBuildMongoFilters(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"name": {"eq": "john*"},
		"age":  {"gte": "18"},
	})

	if _, ok := q["name"].(bson.M)["$regex"]; !ok {
		t.Fatal()
	}
	if q["age"].(bson.M)["$gte"].(int64) != 18 {
		t.Fatal()
	}
}

func TestNormalizeMongoDocument(t *testing.T) {
	out := mongodb.NormalizeMongoDocument(map[string]any{
		"_id": "abc",
		"n":   bson.A{bson.M{"x": 1}},
	})

	if out["id"] != "abc" {
		t.Fatal()
	}

	v := out["n"].([]any)[0]
	switch m := v.(type) {
	case bson.M:
		if m["x"] != 1 {
			t.Fatal()
		}
	case map[string]any:
		if m["x"] != 1 {
			t.Fatal()
		}
	default:
		t.Fatal()
	}
}

func TestToMongoDocument(t *testing.T) {
	doc := mongodb.ToMongoDocument(map[string]any{"id": "1", "name": "x"})
	if doc["_id"] != "1" || doc["name"] != "x" {
		t.Fatal()
	}
}

func TestCollectRefsAtPath(t *testing.T) {
	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{}

	mongodb.CollectRefsAtPath(map[string]any{
		"author": map[string]any{"@entity": "user", "id": "1"},
	}, []string{"author"}, &refs)

	if len(refs) != 1 || refs[0].Entity != "user" || refs[0].Id != "1" {
		t.Fatal()
	}
}

func TestCollectAllRefMaps(t *testing.T) {
	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{}

	mongodb.CollectAllRefMaps(map[string]any{
		"a": map[string]any{"@entity": "x", "id": "1"},
		"b": []any{map[string]any{"@entity": "y", "id": "2"}},
	}, &refs)

	if len(refs) != 2 {
		t.Fatal()
	}
}

func TestApplyLoadedRefs(t *testing.T) {
	parent := map[string]any{
		"a": map[string]any{"@entity": "x", "id": "1"},
	}

	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{
		{
			Loc:    mongodb.RefLoc{ParentMap: parent, Key: "a", Idx: -1},
			Entity: "x",
			Id:     "1",
		},
	}

	mongodb.ApplyLoadedRefs(refs, map[string]map[string]map[string]any{
		"x": {"1": {"id": "1", "name": "ok"}},
	})

	if parent["a"].(map[string]any)["name"] != "ok" {
		t.Fatal()
	}
}

func TestIsDateOnly(t *testing.T) {
	if !mongodb.IsDateOnly("2024-01-01") {
		t.Fatal()
	}
	if mongodb.IsDateOnly("2024-01-01T10:00:00Z") {
		t.Fatal()
	}
	if mongodb.IsDateOnly("abc") {
		t.Fatal()
	}
}

func TestParseFilterValueBooleans(t *testing.T) {
	if mongodb.ParseFilterValue("false") != false {
		t.Fatal()
	}
	if mongodb.ParseFilterValue("1") != true {
		t.Fatal()
	}
	if mongodb.ParseFilterValue("0") != false {
		t.Fatal()
	}
}

func TestParseFilterValueFallback(t *testing.T) {
	if mongodb.ParseFilterValue("abc") != "abc" {
		t.Fatal()
	}
}

func TestBuildMongoFiltersNeq(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"a": {"neq": "x"},
	})
	if q["a"].(bson.M)["$ne"] != "x" {
		t.Fatal()
	}
}

func TestBuildMongoFiltersNeqGlob(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"a": {"neq": "x*"},
	})
	if _, ok := q["a"].(bson.M)["$not"]; !ok {
		t.Fatal()
	}
}

func TestBuildMongoFiltersGtLt(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"a": {"gt": "10"},
		"b": {"lt": "5"},
	})
	if q["a"].(bson.M)["$gt"].(int64) != 10 {
		t.Fatal()
	}
	if q["b"].(bson.M)["$lt"].(int64) != 5 {
		t.Fatal()
	}
}

func TestBuildMongoFiltersGteLte(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"a": {"gte": "10"},
		"b": {"lte": "5"},
	})
	if q["a"].(bson.M)["$gte"].(int64) != 10 {
		t.Fatal()
	}
	if q["b"].(bson.M)["$lte"].(int64) != 5 {
		t.Fatal()
	}
}

func TestBuildMongoFiltersDateOnly(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"d": {"eq": "2024-01-01"},
	})
	if _, ok := q["d"].(bson.M)["$gte"]; !ok {
		t.Fatal()
	}
	if _, ok := q["d"].(bson.M)["$lt"]; !ok {
		t.Fatal()
	}
}

func TestBuildMongoFiltersArrays(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"a": {"any": "1,2"},
		"b": {"all": "x,y"},
		"c": {"none": "3,4"},
	})
	if _, ok := q["a"].(bson.M)["$in"]; !ok {
		t.Fatal()
	}
	if _, ok := q["b"].(bson.M)["$all"]; !ok {
		t.Fatal()
	}
	if _, ok := q["c"].(bson.M)["$nin"]; !ok {
		t.Fatal()
	}
}

func TestNormalizeMongoValueNested(t *testing.T) {
	v := mongodb.NormalizeMongoValue(bson.M{
		"a": bson.A{
			bson.M{"b": 1},
		},
	})

	root, ok := v.(bson.M)
	if !ok {
		t.Fatal()
	}

	arr, ok := root["a"].(bson.A)
	if !ok || len(arr) != 1 {
		t.Fatal()
	}

	inner, ok := arr[0].(bson.M)
	if !ok {
		t.Fatal()
	}

	if inner["b"] != 1 {
		t.Fatal()
	}
}

func TestToMongoValueArray(t *testing.T) {
	v := mongodb.ToMongoValue([]any{"1", "2"}).(bson.A)
	if v[0] != "1" {
		t.Fatal()
	}
}

func TestToMongoValueFallback(t *testing.T) {
	if mongodb.ToMongoValue(10).(int) != 10 {
		t.Fatal()
	}
}

func TestToMongoDocumentDate(t *testing.T) {
	now := time.Now().UTC().Truncate(time.Second)
	doc := mongodb.ToMongoDocument(map[string]any{
		"id": "1",
		"d":  now.Format(time.RFC3339),
	})
	if _, ok := doc["d"].(time.Time); !ok {
		t.Fatal()
	}
}

func TestCollectRefsAtPathArray(t *testing.T) {
	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{}

	mongodb.CollectRefsAtPath(map[string]any{
		"a": []any{
			map[string]any{"@entity": "x", "id": "1"},
			map[string]any{"@entity": "x", "id": "2"},
		},
	}, []string{"a"}, &refs)

	if len(refs) != 2 {
		t.Fatal()
	}
}

func TestCollectRefsAtPathNested(t *testing.T) {
	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{}

	mongodb.CollectRefsAtPath(map[string]any{
		"a": map[string]any{
			"b": map[string]any{"@entity": "x", "id": "1"},
		},
	}, []string{"a", "b"}, &refs)

	if len(refs) != 1 {
		t.Fatal()
	}
}

func TestCollectAllRefMapsDeep(t *testing.T) {
	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{}

	mongodb.CollectAllRefMaps(map[string]any{
		"a": map[string]any{
			"b": []any{
				map[string]any{"@entity": "x", "id": "1"},
			},
		},
	}, &refs)

	if len(refs) != 1 {
		t.Fatal()
	}
}

func TestApplyLoadedRefsArray(t *testing.T) {
	parent := map[string]any{
		"a": []any{
			map[string]any{"@entity": "x", "id": "1"},
		},
	}

	refs := []struct {
		Loc    mongodb.RefLoc
		Entity string
		Id     string
	}{
		{
			Loc:    mongodb.RefLoc{ParentArr: parent["a"].([]any), Idx: 0},
			Entity: "x",
			Id:     "1",
		},
	}

	mongodb.ApplyLoadedRefs(refs, map[string]map[string]map[string]any{
		"x": {"1": {"id": "1", "name": "ok"}},
	})

	if parent["a"].([]any)[0].(map[string]any)["name"] != "ok" {
		t.Fatal()
	}
}

func TestRegexpEscapeFull(t *testing.T) {
	s := mongodb.RegexpEscape(".+?()[]{}^$|")
	if s == ".+?()[]{}^$|" {
		t.Fatal()
	}
}

func TestGlobToRegexEscaping(t *testing.T) {
	r := mongodb.GlobToRegex("a.b")
	if r != "^a\\.b$" {
		t.Fatal()
	}
}
