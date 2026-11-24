package main

import (
	"encoding/base64"
	"fmt"
	"math"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const (
	longTagPrefix = "tag:yaml.org,2002:"
	tagNull       = longTagPrefix + "null"
	tagBool       = longTagPrefix + "bool"
	tagStr        = longTagPrefix + "str"
	tagInt        = longTagPrefix + "int"
	tagFloat      = longTagPrefix + "float"
	tagTimestamp  = longTagPrefix + "timestamp"
	tagSeq        = longTagPrefix + "seq"
	tagMap        = longTagPrefix + "map"
	tagBinary     = longTagPrefix + "binary"
	tagMerge      = longTagPrefix + "merge"
)

type fatalErr struct {
	data   map[string]any
	msg    string
	prefix bool
}

type issue struct {
	data map[string]any
	msg  string
}

type outcome struct {
	msg    string
	prefix bool
	fatal  map[string]any
	issues []any
}

type replay struct {
	strict      bool
	aliases     map[*xnode]bool
	issues      []issue
	decodeCount int
	aliasCount  int
	aliasDepth  int
	path        []any
	inKey       bool
}

var (
	ifaceType    = reflect.TypeOf((*interface{})(nil)).Elem()
	stringType   = reflect.TypeOf("")
	durationType = reflect.TypeOf(time.Duration(0))
	defaultMap   = reflect.TypeOf(map[interface{}]interface{}{})
	sliceIface   = reflect.TypeOf([]interface{}{})
)

func newReplay(strict bool) *replay {
	return &replay{strict: strict, aliases: map[*xnode]bool{}}
}

func (r *replay) run(doc *xnode, t reflect.Type) (o outcome) {
	defer func() {
		if v := recover(); v != nil {
			f, ok := v.(fatalErr)
			if !ok {
				panic(v)
			}
			o = outcome{msg: "yaml: " + f.msg, prefix: f.prefix, fatal: f.data}
		}
	}()
	if doc != nil {
		r.unmarshal(doc, t, nil)
	}
	o.issues = []any{}
	if len(r.issues) == 0 {
		return o
	}
	msgs := make([]string, len(r.issues))
	for i, is := range r.issues {
		msgs[i] = is.msg
		o.issues = append(o.issues, is.data)
	}
	o.msg = "yaml: unmarshal errors:\n  " + strings.Join(msgs, "\n  ")
	return o
}

func fail(data map[string]any, msg string) {
	panic(fatalErr{data: data, msg: msg})
}

func allowedAliasRatio(decodeCount int) float64 {
	switch {
	case decodeCount <= 400000:
		return 0.99
	case decodeCount >= 4000000:
		return 0.10
	default:
		return 0.99 - 0.89*(float64(decodeCount-400000)/float64(4000000-400000))
	}
}

func (r *replay) pathCopy() []any {
	p := make([]any, len(r.path))
	copy(p, r.path)
	return p
}

func isNull(n *xnode) bool {
	return n.tag == tagNull || n.kind == scalarNode && n.tag == "" && (n.value == "null" || n.value == "~" || n.value == "" && n.implicit)
}

func (r *replay) unmarshal(n *xnode, t reflect.Type, keys *[]any) bool {
	r.decodeCount++
	if r.aliasDepth > 0 {
		r.aliasCount++
	}
	if r.aliasCount > 100 && r.decodeCount > 1000 && float64(r.aliasCount)/float64(r.decodeCount) > allowedAliasRatio(r.decodeCount) {
		fail(map[string]any{"class": "aliasing"}, "document contains excessive aliasing")
	}
	switch n.kind {
	case documentNode:
		if len(n.children) == 1 {
			r.unmarshal(n.children[0], t, keys)
			return true
		}
		return false
	case aliasNode:
		return r.alias(n, t, keys)
	}
	if !isNull(n) {
		for t.Kind() == reflect.Ptr {
			t = t.Elem()
		}
	}
	switch n.kind {
	case scalarNode:
		return r.scalar(n, t)
	case mappingNode:
		return r.mapping(n, t, keys)
	case sequenceNode:
		return r.sequence(n, t)
	}
	panic("unknown node kind")
}

func (r *replay) alias(n *xnode, t reflect.Type, keys *[]any) bool {
	if r.aliases[n] {
		fail(map[string]any{"class": "recursive-anchor", "anchor": n.value}, fmt.Sprintf("anchor '%s' value contains itself", n.value))
	}
	r.aliases[n] = true
	r.aliasDepth++
	good := r.unmarshal(n.alias, t, keys)
	r.aliasDepth--
	delete(r.aliases, n)
	return good
}

func expectedOf(t reflect.Type) string {
	for t.Kind() == reflect.Ptr {
		t = t.Elem()
	}
	switch t.Kind() {
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return "integer"
	case reflect.Bool:
		return "boolean"
	case reflect.String:
		return "string"
	case reflect.Slice:
		if t.Elem().Kind() == reflect.String {
			return "strings"
		}
		return "array"
	case reflect.Struct, reflect.Map:
		return "mapping"
	}
	panic("no expected kind for " + t.String())
}

func gotOf(n *xnode) string {
	switch n.kind {
	case mappingNode:
		return "mapping"
	case sequenceNode:
		return "sequence"
	}
	return "scalar"
}

func (r *replay) terror(n *xnode, tag string, t reflect.Type, resolved any) {
	if n.tag != "" {
		tag = n.tag
	}
	value := n.value
	if tag != tagSeq && tag != tagMap {
		if len(value) > 10 {
			value = " `" + value[:7] + "...`"
		} else {
			value = " `" + value + "`"
		}
	}
	msg := fmt.Sprintf("line %d: cannot unmarshal %s%s into %s", n.line+1, shortTag(tag), value, t)
	kind := "type"
	if r.inKey {
		kind = "key"
	} else if isIntKind(t) && n.kind == scalarNode {
		switch v := resolved.(type) {
		case uint64:
			kind = "range"
		case float64:
			if !math.IsNaN(v) && !math.IsInf(v, 0) {
				kind = "range"
			}
		}
	}
	data := map[string]any{"line": n.line + 1, "kind": kind, "path": r.pathCopy(), "expected": expectedOf(t), "got": gotOf(n)}
	if kind == "key" {
		data["expected"] = "string"
	}
	if n.kind == scalarNode {
		data["text"] = n.value
	}
	r.issues = append(r.issues, issue{data: data, msg: msg})
}

func isIntKind(t reflect.Type) bool {
	switch t.Kind() {
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return true
	}
	return false
}

func (r *replay) scalar(n *xnode, t reflect.Type) bool {
	var tag string
	var resolved any
	if n.tag == "" && !n.implicit {
		tag = tagStr
		resolved = n.value
	} else {
		tag, resolved = resolve(n.tag, n.value)
		if tag == tagBinary {
			data, err := base64.StdEncoding.DecodeString(resolved.(string))
			if err != nil {
				fail(map[string]any{"class": "binary"}, "!!binary value contains invalid base64 data")
			}
			resolved = string(data)
		}
	}
	if resolved == nil {
		return true
	}
	if reflect.TypeOf(resolved) == t {
		return true
	}
	switch t.Kind() {
	case reflect.String:
		return true
	case reflect.Interface:
		return true
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		zero := reflect.New(t).Elem()
		switch v := resolved.(type) {
		case int:
			if !zero.OverflowInt(int64(v)) {
				return true
			}
		case int64:
			if !zero.OverflowInt(v) {
				return true
			}
		case uint64:
			if v <= math.MaxInt64 && !zero.OverflowInt(int64(v)) {
				return true
			}
		case float64:
			if v <= math.MaxInt64 && !zero.OverflowInt(int64(v)) {
				return true
			}
		case string:
			if t == durationType {
				if _, err := time.ParseDuration(v); err == nil {
					return true
				}
			}
		}
	case reflect.Bool:
		if _, ok := resolved.(bool); ok {
			return true
		}
	case reflect.Struct:
		if reflect.TypeOf(resolved) == t {
			return true
		}
	case reflect.Ptr:
		if t.Elem() == reflect.TypeOf(resolved) {
			return true
		}
	}
	r.terror(n, tag, t, resolved)
	return false
}

func (r *replay) sequence(n *xnode, t reflect.Type) bool {
	switch t.Kind() {
	case reflect.Slice:
	case reflect.Interface:
		t = sliceIface
	default:
		r.terror(n, tagSeq, t, nil)
		return false
	}
	et := t.Elem()
	for i, c := range n.children {
		r.path = append(r.path, i)
		r.unmarshal(c, et, nil)
		r.path = r.path[:len(r.path)-1]
	}
	return true
}

func (r *replay) mapping(n *xnode, t reflect.Type, keys *[]any) bool {
	switch t.Kind() {
	case reflect.Struct:
		return r.mappingStruct(n, t)
	case reflect.Slice:
		r.terror(n, tagMap, t, nil)
		return false
	case reflect.Map:
	case reflect.Interface:
		t = defaultMap
		keys = nil
	default:
		r.terror(n, tagMap, t, nil)
		return false
	}
	if keys == nil {
		fresh := []any{}
		keys = &fresh
	}
	for i := 0; i < len(n.children); i += 2 {
		if isMerge(n.children[i]) {
			r.merge(n.children[i+1], t, keys)
			continue
		}
		k := r.keyValue(n.children[i])
		r.path = append(r.path, keyText(n.children[i]))
		r.unmarshal(n.children[i+1], t.Elem(), nil)
		r.path = r.path[:len(r.path)-1]
		r.setMapIndex(n.children[i+1], n.children[i], keys, k)
	}
	return true
}

func keyText(n *xnode) string {
	for n.kind == aliasNode {
		n = n.alias
	}
	return n.value
}

func (r *replay) keyValue(n *xnode) any {
	r.unmarshal(n, ifaceType, nil)
	target := n
	for target.kind == aliasNode {
		target = target.alias
	}
	switch target.kind {
	case mappingNode, sequenceNode:
		got := map[int]string{mappingNode: "mapping", sequenceNode: "sequence"}[target.kind]
		panic(fatalErr{data: map[string]any{"class": "map-key", "got": got}, msg: "invalid map key: ", prefix: true})
	}
	return scalarValue(target)
}

func scalarValue(n *xnode) any {
	if n.tag == "" && !n.implicit {
		return n.value
	}
	tag, v := resolve(n.tag, n.value)
	if tag == tagBinary {
		data, _ := base64.StdEncoding.DecodeString(v.(string))
		return string(data)
	}
	if tag == tagTimestamp {
		return n.value
	}
	return v
}

func (r *replay) setMapIndex(vn, kn *xnode, keys *[]any, k any) {
	if r.strict {
		for _, existing := range *keys {
			if existing == k {
				msg := fmt.Sprintf("line %d: key %#v already set in map", vn.line+1, k)
				path := append(r.pathCopy(), keyText(kn))
				r.issues = append(r.issues, issue{data: map[string]any{"line": vn.line + 1, "kind": "duplicate", "path": path}, msg: msg})
				return
			}
		}
	}
	for i, existing := range *keys {
		if existing == k {
			(*keys)[i] = k
			return
		}
	}
	*keys = append(*keys, k)
}

type fieldInfo struct {
	index int
	typ   reflect.Type
}

func structFields(t reflect.Type) (map[string]fieldInfo, int) {
	m := map[string]fieldInfo{}
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		key := f.Tag.Get("yaml")
		if key == "" {
			key = strings.ToLower(f.Name)
		}
		m[key] = fieldInfo{index: i, typ: f.Type}
	}
	return m, t.NumField()
}

func (r *replay) mappingStruct(n *xnode, t reflect.Type) bool {
	fields, count := structFields(t)
	done := make([]bool, count)
	name := ""
	for i := 0; i < len(n.children); i += 2 {
		ni := n.children[i]
		if isMerge(ni) {
			r.merge(n.children[i+1], t, nil)
			continue
		}
		r.inKey = true
		ok := r.unmarshal(ni, stringType, nil)
		r.inKey = false
		if !ok {
			continue
		}
		name = keyString(ni, name)
		if info, found := fields[name]; found {
			if r.strict {
				if done[info.index] {
					msg := fmt.Sprintf("line %d: field %s already set in type %s", ni.line+1, name, t)
					path := append(r.pathCopy(), name)
					r.issues = append(r.issues, issue{data: map[string]any{"line": ni.line + 1, "kind": "duplicate", "path": path}, msg: msg})
					continue
				}
				done[info.index] = true
			}
			r.path = append(r.path, name)
			r.unmarshal(n.children[i+1], info.typ, nil)
			r.path = r.path[:len(r.path)-1]
		} else if r.strict {
			msg := fmt.Sprintf("line %d: field %s not found in type %s", ni.line+1, name, t)
			path := append(r.pathCopy(), name)
			r.issues = append(r.issues, issue{data: map[string]any{"line": ni.line + 1, "kind": "unknown", "path": path}, msg: msg})
		}
	}
	return true
}

func keyString(n *xnode, previous string) string {
	target := n
	for target.kind == aliasNode {
		target = target.alias
	}
	if target.kind != scalarNode {
		return previous
	}
	if target.tag == "" && !target.implicit {
		return target.value
	}
	tag, v := resolve(target.tag, target.value)
	if v == nil {
		return ""
	}
	if tag == tagBinary {
		data, _ := base64.StdEncoding.DecodeString(v.(string))
		return string(data)
	}
	return target.value
}

func failWantMap() {
	fail(map[string]any{"class": "merge"}, "map merge requires map or sequence of maps as the value")
}

func (r *replay) merge(n *xnode, t reflect.Type, keys *[]any) {
	switch n.kind {
	case mappingNode:
		r.unmarshal(n, t, keys)
	case aliasNode:
		if n.alias != nil && n.alias.kind != mappingNode {
			failWantMap()
		}
		r.unmarshal(n, t, keys)
	case sequenceNode:
		for i := len(n.children) - 1; i >= 0; i-- {
			ni := n.children[i]
			if ni.kind == aliasNode {
				if ni.alias != nil && ni.alias.kind != mappingNode {
					failWantMap()
				}
			} else if ni.kind != mappingNode {
				failWantMap()
			}
			r.unmarshal(ni, t, keys)
		}
	default:
		failWantMap()
	}
}

func isMerge(n *xnode) bool {
	return n.kind == scalarNode && n.value == "<<" && (n.implicit || n.tag == tagMerge)
}

var resolveTable = make([]byte, 256)

type resolveItem struct {
	value any
	tag   string
}

var resolveMap = map[string]resolveItem{}

func init() {
	t := resolveTable
	t['+'] = 'S'
	t['-'] = 'S'
	for _, c := range "0123456789" {
		t[c] = 'D'
	}
	for _, c := range "yYnNtTfFoO~" {
		t[c] = 'M'
	}
	t['.'] = '.'
	list := []struct {
		v   any
		tag string
		l   []string
	}{
		{true, tagBool, []string{"y", "Y", "yes", "Yes", "YES"}},
		{true, tagBool, []string{"true", "True", "TRUE"}},
		{true, tagBool, []string{"on", "On", "ON"}},
		{false, tagBool, []string{"n", "N", "no", "No", "NO"}},
		{false, tagBool, []string{"false", "False", "FALSE"}},
		{false, tagBool, []string{"off", "Off", "OFF"}},
		{nil, tagNull, []string{"", "~", "null", "Null", "NULL"}},
		{math.NaN(), tagFloat, []string{".nan", ".NaN", ".NAN"}},
		{math.Inf(+1), tagFloat, []string{".inf", ".Inf", ".INF"}},
		{math.Inf(+1), tagFloat, []string{"+.inf", "+.Inf", "+.INF"}},
		{math.Inf(-1), tagFloat, []string{"-.inf", "-.Inf", "-.INF"}},
		{"<<", tagMerge, []string{"<<"}},
	}
	for _, item := range list {
		for _, s := range item.l {
			resolveMap[s] = resolveItem{item.v, item.tag}
		}
	}
}

func shortTag(tag string) string {
	if strings.HasPrefix(tag, longTagPrefix) {
		return "!!" + tag[len(longTagPrefix):]
	}
	return tag
}

func resolvableTag(tag string) bool {
	switch tag {
	case "", tagStr, tagBool, tagInt, tagFloat, tagNull, tagTimestamp:
		return true
	}
	return false
}

var yamlStyleFloat = regexp.MustCompile(`^[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?$`)

func resolve(tag, in string) (string, any) {
	if !resolvableTag(tag) {
		return tag, in
	}
	rtag, out := resolveInner(tag, in)
	switch tag {
	case "", rtag, tagStr, tagBinary:
		return rtag, out
	case tagFloat:
		if rtag == tagInt {
			switch v := out.(type) {
			case int64:
				return tagFloat, float64(v)
			case int:
				return tagFloat, float64(v)
			}
		}
	}
	fail(map[string]any{"class": "tag", "resolved": shortTag(rtag), "tag": shortTag(tag), "text": in},
		fmt.Sprintf("cannot decode %s `%s` as a %s", shortTag(rtag), in, shortTag(tag)))
	return "", nil
}

func resolveInner(tag, in string) (string, any) {
	hint := byte('N')
	if in != "" {
		hint = resolveTable[in[0]]
	}
	if hint != 0 && tag != tagStr && tag != tagBinary {
		if item, ok := resolveMap[in]; ok {
			return item.tag, item.value
		}
		switch hint {
		case 'M':
		case '.':
			if f, err := strconv.ParseFloat(in, 64); err == nil {
				return tagFloat, f
			}
		case 'D', 'S':
			if tag == "" || tag == tagTimestamp {
				if t, ok := parseTimestamp(in); ok {
					return tagTimestamp, t
				}
			}
			plain := strings.Replace(in, "_", "", -1)
			if i, err := strconv.ParseInt(plain, 0, 64); err == nil {
				return tagInt, int(i)
			}
			if u, err := strconv.ParseUint(plain, 0, 64); err == nil {
				return tagInt, u
			}
			if yamlStyleFloat.MatchString(plain) {
				if f, err := strconv.ParseFloat(plain, 64); err == nil {
					return tagFloat, f
				}
			}
			if strings.HasPrefix(plain, "0b") {
				if i, err := strconv.ParseInt(plain[2:], 2, 64); err == nil {
					return tagInt, int(i)
				}
				if u, err := strconv.ParseUint(plain[2:], 2, 64); err == nil {
					return tagInt, u
				}
			} else if strings.HasPrefix(plain, "-0b") {
				if i, err := strconv.ParseInt("-"+plain[3:], 2, 64); err == nil {
					return tagInt, int(i)
				}
			}
		default:
			panic("unhandled hint")
		}
	}
	return tagStr, in
}

var timestampFormats = []string{
	"2006-1-2T15:4:5.999999999Z07:00",
	"2006-1-2t15:4:5.999999999Z07:00",
	"2006-1-2 15:4:5.999999999",
	"2006-1-2",
}

func parseTimestamp(s string) (time.Time, bool) {
	i := 0
	for ; i < len(s); i++ {
		if c := s[i]; c < '0' || c > '9' {
			break
		}
	}
	if i != 4 || i == len(s) || s[i] != '-' {
		return time.Time{}, false
	}
	for _, format := range timestampFormats {
		if t, err := time.Parse(format, s); err == nil {
			return t, true
		}
	}
	return time.Time{}, false
}
