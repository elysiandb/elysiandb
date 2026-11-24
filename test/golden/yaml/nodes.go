package main

import (
	"bytes"
	"fmt"
	"io"
	"reflect"
	"sort"
	"strconv"

	"gopkg.in/yaml.v2"
)

const (
	documentNode = 1
	mappingNode  = 2
	sequenceNode = 4
	scalarNode   = 8
	aliasNode    = 16
)

const (
	readerError  = 2
	scannerError = 3
	parserError  = 4
	aliasEvent   = 5
)

type xnode struct {
	id       int
	kind     int
	line     int
	column   int
	tag      string
	value    string
	implicit bool
	children []*xnode
	alias    *xnode
}

type parsed struct {
	failed bool
	msg    string
	doc    *xnode
	data   map[string]any
}

var kindNames = map[int]string{documentNode: "doc", mappingNode: "map", sequenceNode: "seq", scalarNode: "scalar", aliasNode: "alias"}

func mark(v reflect.Value) []int64 {
	return []int64{v.FieldByName("index").Int(), v.FieldByName("line").Int(), v.FieldByName("column").Int()}
}

func parseCase(input []byte, shallow bool) (parsed, error) {
	dec := yaml.NewDecoder(bytes.NewReader(nonEmpty(input)))
	var sink interface{}
	err := dec.Decode(&sink)
	p := reflect.ValueOf(dec).Elem().FieldByName("parser").Elem()
	lp := p.FieldByName("parser")
	ue := yaml.Unmarshal(input, &sink)
	if errType := lp.FieldByName("error").Int(); errType != 0 {
		kinds := map[int64]string{readerError: "reader", scannerError: "scanner", parserError: "parser"}
		kind, ok := kinds[errType]
		if !ok {
			return parsed{}, fmt.Errorf("unexpected parser error type %d", errType)
		}
		problem := lp.FieldByName("problem").String()
		pm := mark(lp.FieldByName("problem_mark"))
		cm := mark(lp.FieldByName("context_mark"))
		line := int64(0)
		if pm[1] != 0 {
			line = pm[1]
			if errType == scannerError {
				line++
			}
		} else if cm[1] != 0 {
			line = cm[1]
		}
		msg := "yaml: "
		if line != 0 {
			msg += "line " + strconv.FormatInt(line, 10) + ": "
		}
		msg += problem
		if err == nil || err.Error() != msg || ue == nil || ue.Error() != msg {
			return parsed{}, fmt.Errorf("syntax replay %q, decoder %v, unmarshal %v", msg, err, ue)
		}
		data := map[string]any{"error": map[string]any{
			"class":          "syntax",
			"kind":           kind,
			"problem":        problem,
			"line":           line,
			"problem_mark":   pm,
			"context_mark":   cm,
			"problem_offset": lp.FieldByName("problem_offset").Int(),
			"problem_value":  lp.FieldByName("problem_value").Int(),
		}}
		return parsed{failed: true, msg: msg, data: data}, nil
	}
	ev := p.FieldByName("event")
	if err != nil && ev.FieldByName("typ").Int() == aliasEvent {
		anchor := string(ev.FieldByName("anchor").Bytes())
		msg := "yaml: unknown anchor '" + anchor + "' referenced"
		if err.Error() != msg || ue == nil || ue.Error() != msg {
			return parsed{}, fmt.Errorf("anchor replay %q, decoder %v, unmarshal %v", msg, err, ue)
		}
		data := map[string]any{"error": map[string]any{"class": "unknown-anchor", "anchor": anchor}}
		return parsed{failed: true, msg: msg, data: data}, nil
	}
	if err == io.EOF {
		if ue != nil {
			return parsed{}, fmt.Errorf("empty stream but unmarshal gave %v", ue)
		}
		return parsed{data: map[string]any{"empty": true}}, nil
	}
	docv := p.FieldByName("doc")
	if docv.IsNil() {
		return parsed{}, fmt.Errorf("no document, decoder %v", err)
	}
	doc, list, ptrs := extract(docv)
	data := map[string]any{"count": len(list), "depth": depth(doc)}
	if !shallow {
		rows := make([]any, 0, len(list))
		for _, n := range list {
			r := []any{n.id, kindNames[n.kind], n.line, n.column}
			switch n.kind {
			case scalarNode:
				r = append(r, n.tag, n.value, n.implicit)
			case mappingNode, sequenceNode, documentNode:
				ids := make([]int, len(n.children))
				for i, c := range n.children {
					ids[i] = c.id
				}
				r = append(r, n.tag, ids)
			case aliasNode:
				r = append(r, n.value, n.alias.id)
			}
			rows = append(rows, r)
		}
		data["nodes"] = rows
		anchors := docv.Elem().FieldByName("anchors")
		names := make([]string, 0, anchors.Len())
		for _, k := range anchors.MapKeys() {
			names = append(names, k.String())
		}
		sort.Strings(names)
		am := make([]any, 0, len(names))
		for _, name := range names {
			target := anchors.MapIndex(reflect.ValueOf(name)).Pointer()
			am = append(am, []any{name, ptrs[target]})
		}
		data["anchors"] = am
	}
	return parsed{doc: doc, data: data}, nil
}

func extract(docv reflect.Value) (*xnode, []*xnode, map[uintptr]int) {
	ids := map[uintptr]*xnode{}
	ptrs := map[uintptr]int{}
	var list []*xnode
	var walk func(p reflect.Value) *xnode
	walk = func(p reflect.Value) *xnode {
		if x, ok := ids[p.Pointer()]; ok {
			return x
		}
		n := p.Elem()
		x := &xnode{
			id:       len(list),
			kind:     int(n.FieldByName("kind").Int()),
			line:     int(n.FieldByName("line").Int()),
			column:   int(n.FieldByName("column").Int()),
			tag:      n.FieldByName("tag").String(),
			value:    n.FieldByName("value").String(),
			implicit: n.FieldByName("implicit").Bool(),
		}
		ids[p.Pointer()] = x
		ptrs[p.Pointer()] = x.id
		list = append(list, x)
		ch := n.FieldByName("children")
		for i := 0; i < ch.Len(); i++ {
			x.children = append(x.children, walk(ch.Index(i)))
		}
		if a := n.FieldByName("alias"); !a.IsNil() {
			x.alias = walk(a)
		}
		return x
	}
	doc := walk(docv)
	return doc, list, ptrs
}

func depth(n *xnode) int {
	best := 0
	type item struct {
		n *xnode
		d int
	}
	stack := []item{{n, 1}}
	for len(stack) > 0 {
		it := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if it.d > best {
			best = it.d
		}
		for _, c := range it.n.children {
			stack = append(stack, item{c, it.d + 1})
		}
	}
	return best
}
