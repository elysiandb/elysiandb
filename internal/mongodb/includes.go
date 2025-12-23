package mongodb

import (
	"context"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func ApplyIncludes(data []map[string]interface{}, includesParam string) []map[string]interface{} {
	return data
}

func ParseIncludes(includesParam string) (bool, [][]string) {
	includesParam = strings.TrimSpace(includesParam)
	if includesParam == "" {
		return false, nil
	}

	if includesParam == "all" {
		return true, nil
	}

	parts := strings.Split(includesParam, ",")
	out := make([][]string, 0, len(parts))
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if p == "" {
			continue
		}

		chunks := strings.Split(p, ".")
		clean := make([]string, 0, len(chunks))
		for _, c := range chunks {
			c = strings.TrimSpace(c)
			if c != "" {
				clean = append(clean, c)
			}
		}

		if len(clean) > 0 {
			out = append(out, clean)
		}
	}

	return false, out
}

func BuildSpecsFromSample(entity string, includeAll bool, paths [][]string) []IncludeSpec {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	var sample map[string]any
	_ = globals.MongoDB.Collection(entity).FindOne(ctx, bson.M{}).Decode(&sample)

	if includeAll {
		out := make([]IncludeSpec, 0)
		for k, v := range sample {
			if k == "_id" {
				continue
			}

			ent, ok := RefEntityFromValue(v)
			if !ok || ent == "" {
				continue
			}

			as := k
			out = append(out, IncludeSpec{
				Path: []string{k},
				From: ent,
				As:   as,
				Tmp:  "__ely_ref_" + as,
			})
		}

		return out
	}

	out := make([]IncludeSpec, 0)
	for _, p := range paths {
		if len(p) == 0 {
			continue
		}

		if len(p) != 1 {
			continue
		}

		field := p[0]
		from := ""
		if v, ok := sample[field]; ok {
			if ent, ok := RefEntityFromValue(v); ok {
				from = ent
			}
		}

		if from == "" {
			from = SingularFallback(field)
		}

		as := field
		out = append(out, IncludeSpec{
			Path: []string{field},
			From: from,
			As:   as,
			Tmp:  "__ely_ref_" + as,
		})
	}

	return out
}

func SingularFallback(s string) string {
	s = strings.TrimSpace(s)
	if s == "" {
		return s
	}

	if strings.HasSuffix(s, "ies") && len(s) > 3 {
		return s[:len(s)-3] + "y"
	}

	if strings.HasSuffix(s, "ses") && len(s) > 3 {
		return s[:len(s)-2]
	}

	if strings.HasSuffix(s, "s") && len(s) > 1 {
		return s[:len(s)-1]
	}

	return s
}

func GetRefId(m map[string]any) (string, bool) {
	if v, ok := m["id"].(string); ok && v != "" {
		return v, true
	}

	if v, ok := m["_id"].(string); ok && v != "" {
		return v, true
	}

	return "", false
}

func IsRefMap(m map[string]any) bool {
	if m == nil {
		return false
	}

	_, ok := m["@entity"].(string)
	if !ok {
		return false
	}

	_, ok = GetRefId(m)
	if !ok {
		return false
	}

	for k := range m {
		if k != "@entity" && k != "id" && k != "_id" {
			return false
		}
	}

	return true
}

func RefEntityFromValue(v any) (string, bool) {
	switch t := v.(type) {
	case map[string]any:
		if ent, ok := t["@entity"].(string); ok && ent != "" {
			return ent, true
		}
	case bson.M:
		if ent, ok := t["@entity"].(string); ok && ent != "" {
			return ent, true
		}
	case bson.D:
		for _, e := range t {
			if e.Key == "@entity" {
				if s, ok := e.Value.(string); ok && s != "" {
					return s, true
				}
			}
		}
	case []any:
		for _, it := range t {
			if ent, ok := RefEntityFromValue(it); ok {
				return ent, true
			}
		}
	case bson.A:
		for _, it := range t {
			if ent, ok := RefEntityFromValue(it); ok {
				return ent, true
			}
		}
	}

	return "", false
}

func AddIncludeEntityTags(items []map[string]any, specs []IncludeSpec) []map[string]any {
	if len(specs) == 0 {
		return items
	}

	for _, it := range items {
		for _, sp := range specs {
			if arr, ok := it[sp.As].([]any); ok {
				for _, v := range arr {
					if m, ok := v.(map[string]any); ok {
						m["@entity"] = sp.From
					}
				}
			}
		}
	}

	return items
}

func CollectRefsAtPath(root any, path []string, out *[]struct {
	Loc    RefLoc
	Entity string
	Id     string
},
) {
	if len(path) == 0 {
		return
	}

	switch t := root.(type) {
	case map[string]any:
		seg := path[0]
		v, ok := t[seg]
		if !ok {
			return
		}

		if len(path) == 1 {
			switch vv := v.(type) {
			case map[string]any:
				if IsRefMap(vv) {
					ent, _ := vv["@entity"].(string)
					id, ok := GetRefId(vv)
					if ok && ent != "" {
						*out = append(*out, struct {
							Loc    RefLoc
							Entity string
							Id     string
						}{
							Loc:    RefLoc{ParentMap: t, Key: seg},
							Entity: ent,
							Id:     id,
						})
					}
				}
			case []any:
				for i, it := range vv {
					if m, ok := it.(map[string]any); ok && IsRefMap(m) {
						ent, _ := m["@entity"].(string)
						id, ok := GetRefId(m)
						if ok && ent != "" {
							*out = append(*out, struct {
								Loc    RefLoc
								Entity string
								Id     string
							}{
								Loc:    RefLoc{ParentArr: vv, Idx: i},
								Entity: ent,
								Id:     id,
							})
						}
					}
				}
			}

			return
		}

		switch vv := v.(type) {
		case map[string]any:
			CollectRefsAtPath(vv, path[1:], out)
		case []any:
			for _, it := range vv {
				CollectRefsAtPath(it, path[1:], out)
			}
		}
	case []any:
		for _, it := range t {
			CollectRefsAtPath(it, path, out)
		}
	}
}

func CollectAllRefMaps(root any, out *[]struct {
	Loc    RefLoc
	Entity string
	Id     string
},
) {
	switch t := root.(type) {
	case map[string]any:
		for k, v := range t {
			if m, ok := v.(map[string]any); ok && IsRefMap(m) {
				ent, _ := m["@entity"].(string)
				id, ok := GetRefId(m)
				if ok && ent != "" {
					*out = append(*out, struct {
						Loc    RefLoc
						Entity string
						Id     string
					}{
						Loc:    RefLoc{ParentMap: t, Key: k},
						Entity: ent,
						Id:     id,
					})
				}
				continue
			}

			if arr, ok := v.([]any); ok {
				for i, it := range arr {
					if m, ok := it.(map[string]any); ok && IsRefMap(m) {
						ent, _ := m["@entity"].(string)
						id, ok := GetRefId(m)
						if ok && ent != "" {
							*out = append(*out, struct {
								Loc    RefLoc
								Entity string
								Id     string
							}{
								Loc:    RefLoc{ParentArr: arr, Idx: i},
								Entity: ent,
								Id:     id,
							})
						}
					} else {
						CollectAllRefMaps(it, out)
					}
				}

				continue
			}

			CollectAllRefMaps(v, out)
		}
	case []any:
		for _, it := range t {
			CollectAllRefMaps(it, out)
		}
	}
}

func LoadDocsByIds(entity string, ids []string) map[string]map[string]any {
	if len(ids) == 0 {
		return map[string]map[string]any{}
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)

	defer cancel()

	q := bson.M{"_id": bson.M{"$in": ids}}
	cur, err := globals.MongoDB.Collection(entity).Find(ctx, q)
	if err != nil || cur == nil {
		return map[string]map[string]any{}
	}

	defer cur.Close(ctx)

	out := map[string]map[string]any{}
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) == nil {
			n := NormalizeMongoDocument(raw)
			if id, ok := n["id"].(string); ok && id != "" {
				out[id] = n
			}
		}
	}

	return out
}

func ApplyLoadedRefs(refs []struct {
	Loc    RefLoc
	Entity string
	Id     string
}, loaded map[string]map[string]map[string]any,
) {
	for _, r := range refs {
		entMap, ok := loaded[r.Entity]
		if !ok {
			continue
		}

		doc, ok := entMap[r.Id]
		if !ok || doc == nil {
			continue
		}

		doc["@entity"] = r.Entity

		if r.Loc.ParentMap != nil {
			r.Loc.ParentMap[r.Loc.Key] = doc
		} else if r.Loc.ParentArr != nil && r.Loc.Idx >= 0 && r.Loc.Idx < len(r.Loc.ParentArr) {
			r.Loc.ParentArr[r.Loc.Idx] = doc
		}
	}
}

func ResolveIncludesPaths(items []map[string]any, paths [][]string) {
	if len(paths) == 0 || len(items) == 0 {
		return
	}

	for _, p := range paths {
		refs := make([]struct {
			Loc    RefLoc
			Entity string
			Id     string
		}, 0)

		for _, it := range items {
			CollectRefsAtPath(it, p, &refs)
		}

		if len(refs) == 0 {
			continue
		}

		byEntity := map[string]map[string]struct{}{}
		for _, r := range refs {
			m, ok := byEntity[r.Entity]
			if !ok {
				m = map[string]struct{}{}
				byEntity[r.Entity] = m
			}

			m[r.Id] = struct{}{}
		}

		loaded := map[string]map[string]map[string]any{}
		for ent, set := range byEntity {
			ids := make([]string, 0, len(set))
			for id := range set {
				ids = append(ids, id)
			}

			loaded[ent] = LoadDocsByIds(ent, ids)
		}

		ApplyLoadedRefs(refs, loaded)
	}
}
