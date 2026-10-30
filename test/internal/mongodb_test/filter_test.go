package mongodb_test

import (
	"math/rand"
	"reflect"
	"regexp"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/storage"
	"go.mongodb.org/mongo-driver/v2/bson"
)

var globAlphabet = []string{"a", "A", "b", "B", "*", "\\", "i", "I", "İ", "k", "K", "K", "é", "É", "ß", "ẞ", ".", "?", "(", ")", "[", "]", "$", "^", "+", "|", "{", "}", "\n", " ", "1"}

func randomGlobString(r *rand.Rand, maxLen int) string {
	var b strings.Builder
	for n := r.Intn(maxLen + 1); n > 0; n-- {
		b.WriteString(globAlphabet[r.Intn(len(globAlphabet))])
	}

	return b.String()
}

func TestGlobToRegexMatchesMatchGlob(t *testing.T) {
	r := rand.New(rand.NewSource(42))

	patterns := []string{"", "*", "**", "a*b", "*li*", `\*`, `a\`, `\\`, "İstanbul", "kate", "KATE", "straße", "a.b", "(x)", "$^", "x\ny"}
	for i := 0; i < 400; i++ {
		patterns = append(patterns, randomGlobString(r, 6))
	}

	values := []string{"", "a", "A", "ab", "aXb", "istanbul", "İstanbul", "ISTANBUL", "Kate", "kate", "straße", "STRASSE", "a.b", "axb", "(x)", "$^", "x\ny", "*", `\`, "a\\"}
	for i := 0; i < 400; i++ {
		values = append(values, randomGlobString(r, 6))
	}

	for _, p := range patterns {
		pattern, ok := mongodb.GlobToRegex(p)

		var re *regexp.Regexp
		if ok {
			re = regexp.MustCompile("(?s)" + pattern)
		}

		for _, v := range values {
			want := storage.MatchGlob(p, v)
			got := ok && re.MatchString(v)
			if got != want {
				t.Fatalf("pattern %q value %q: regex %q matched=%v, MatchGlob=%v", p, v, pattern, got, want)
			}
		}
	}
}

func TestMongoFieldPath(t *testing.T) {
	cases := map[string]struct {
		path string
		ok   bool
	}{
		"name":        {"name", true},
		"id":          {"_id", true},
		"id.x":        {"_id.x", true},
		"_id":         {"_elysiandb_core__id", true},
		"_id.x":       {"_elysiandb_core__id.x", true},
		"author.id":   {"author.id", true},
		"a..b":        {"", false},
		"$where":      {"", false},
		"a.$b":        {"", false},
		"":            {"", false},
		"with space":  {"with space", true},
		"author.name": {"author.name", true},
	}

	for field, want := range cases {
		path, ok := mongodb.MongoFieldPath(field)
		if path != want.path || ok != want.ok {
			t.Fatalf("%q: got (%q, %v), want (%q, %v)", field, path, ok, want.path, want.ok)
		}
	}
}

func TestBuildListPrefilterEmpty(t *testing.T) {
	if got := mongodb.BuildListPrefilter(nil, true); len(got) != 0 {
		t.Fatalf("expected empty filter, got %#v", got)
	}

	if got := mongodb.BuildListPrefilter(map[string]map[string]string{"$bad": {"eq": "x"}}, true); len(got) != 0 {
		t.Fatalf("expected unsupported paths to be ignored, got %#v", got)
	}
}

func TestBuildListPrefilterMapsIdAndKeepsUntypedValues(t *testing.T) {
	got := mongodb.BuildListPrefilter(map[string]map[string]string{"id": {"eq": "abc"}}, true)

	branches := got["$or"].([]bson.M)
	if !containsBranch(branches, bson.M{"_id": bson.M{"$not": bson.M{"$type": []string{"double", "int", "string", "bool"}}}}) {
		t.Fatalf("missing untyped branch on _id: %#v", branches)
	}

	if !containsBranch(branches, bson.M{"_id": bson.M{"$type": "array"}}) {
		t.Fatalf("missing array branch on _id: %#v", branches)
	}
}

func TestBuildListPrefilterNestedPathAdmitsUnresolvedReferences(t *testing.T) {
	got := mongodb.BuildListPrefilter(map[string]map[string]string{"author.company.name": {"eq": "acme"}}, true)
	branches := got["$or"].([]bson.M)

	for _, want := range []bson.M{
		{"author": bson.M{"$type": "array"}},
		{"author.@entity": bson.M{"$exists": true}},
		{"author.company": bson.M{"$type": "array"}},
		{"author.company.@entity": bson.M{"$exists": true}},
	} {
		if !containsBranch(branches, want) {
			t.Fatalf("missing branch %#v in %#v", want, branches)
		}
	}
}

func TestBuildListPrefilterArrayNarrowingCanBeDisabled(t *testing.T) {
	narrowed := mongodb.BuildListPrefilter(map[string]map[string]string{"tags": {"contains": "go"}}, true)
	if !containsBranch(narrowed["$or"].([]bson.M), bson.M{"tags": bson.M{"$type": "array", "$in": []any{"go"}}}) {
		t.Fatalf("expected array membership branch, got %#v", narrowed)
	}

	wide := mongodb.BuildListPrefilter(map[string]map[string]string{"tags": {"contains": "go"}}, false)
	if !containsBranch(wide["$or"].([]bson.M), bson.M{"tags": bson.M{"$type": "array"}}) {
		t.Fatalf("expected every array to be kept, got %#v", wide)
	}
}

func TestBuildQueryPrefilterCombinesNodes(t *testing.T) {
	leaf := query.FilterNode{Leaf: map[string]map[string]string{"age": {"gt": "3"}}}

	if got := mongodb.BuildQueryPrefilter(query.FilterNode{}); len(got) != 0 {
		t.Fatalf("expected empty node to be unconstrained, got %#v", got)
	}

	and := mongodb.BuildQueryPrefilter(query.FilterNode{And: []query.FilterNode{leaf, {Leaf: map[string]map[string]string{}}}})
	if !reflect.DeepEqual(and, mongodb.BuildQueryPrefilter(leaf)) {
		t.Fatalf("expected unconstrained children to be dropped from and, got %#v", and)
	}

	or := mongodb.BuildQueryPrefilter(query.FilterNode{Or: []query.FilterNode{leaf, {Leaf: map[string]map[string]string{}}}})
	if len(or) != 0 {
		t.Fatalf("expected or with an unconstrained child to be unconstrained, got %#v", or)
	}

	both := mongodb.BuildQueryPrefilter(query.FilterNode{Or: []query.FilterNode{leaf, leaf}})
	if len(both["$or"].([]bson.M)) != 2 {
		t.Fatalf("expected two alternatives, got %#v", both)
	}
}

func containsBranch(branches []bson.M, want bson.M) bool {
	for _, b := range branches {
		if reflect.DeepEqual(b, want) {
			return true
		}
	}

	return false
}
