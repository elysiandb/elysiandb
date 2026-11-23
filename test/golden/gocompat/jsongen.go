package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"math"
	"strings"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func bsl(s string) string {
	return strings.ReplaceAll(s, "~", "\\")
}

func desc(v any) any {
	switch x := v.(type) {
	case nil:
		return []any{"null"}
	case bool:
		return []any{"bool", x}
	case float64:
		return []any{"num", fbits(x)}
	case float32:
		return []any{"f32", fbits(float64(x))}
	case int:
		return []any{"int", i64s(int64(x))}
	case int64:
		return []any{"i64", i64s(x)}
	case uint64:
		return []any{"u64", u64s(x)}
	case string:
		return []any{"str", hs(x)}
	case json.RawMessage:
		if x == nil {
			return []any{"raw", nil}
		}
		return []any{"raw", hx(x)}
	case []byte:
		if x == nil {
			return []any{"bytes", nil}
		}
		return []any{"bytes", hx(x)}
	case []string:
		if x == nil {
			return []any{"strs", nil}
		}
		return []any{"strs", hexList(x)}
	case []any:
		if x == nil {
			return []any{"arr", nil}
		}
		arr := make([]any, len(x))
		for i, e := range x {
			arr[i] = desc(e)
		}
		return []any{"arr", arr}
	case map[string]any:
		if x == nil {
			return []any{"obj", nil}
		}
		pairs := make([]any, 0, len(x))
		for _, k := range sortedKeys(x) {
			pairs = append(pairs, []any{hs(k), desc(x[k])})
		}
		return []any{"obj", pairs}
	case json.Delim:
		return []any{"delim", string(x)}
	}
	panic("unsupported value")
}

func jsonErr(err error) map[string]any {
	var se *json.SyntaxError
	var te *json.UnmarshalTypeError
	switch {
	case errors.As(err, &se):
		return map[string]any{"kind": "syntax", "offset": se.Offset}
	case errors.As(err, &te):
		return map[string]any{"kind": "type", "field": te.Field, "offset": te.Offset, "value": te.Value, "type_kind": te.Type.Kind().String()}
	case errors.Is(err, io.EOF):
		return map[string]any{"kind": "eof"}
	case errors.Is(err, io.ErrUnexpectedEOF):
		return map[string]any{"kind": "unexpected_eof"}
	}
	var ue *json.UnsupportedValueError
	if errors.As(err, &ue) {
		return map[string]any{"kind": "unsupported_value"}
	}
	var me *json.MarshalerError
	if errors.As(err, &me) {
		return map[string]any{"kind": "marshaler"}
	}
	return map[string]any{"kind": "other"}
}

type sample struct {
	Name   string          `json:"name"`
	Count  int             `json:"count"`
	Big    int64           `json:"big"`
	Small  uint32          `json:"small"`
	Ratio  float64         `json:"ratio"`
	Flag   bool            `json:"flag"`
	Opt    *bool           `json:"opt,omitempty"`
	Tags   []string        `json:"tags"`
	Meta   map[string]any  `json:"meta"`
	Inner  sampleInner     `json:"inner"`
	Raw    json.RawMessage `json:"raw"`
	Any    any             `json:"any"`
	Skip   string          `json:"-"`
	Empty  string          `json:"empty,omitempty"`
	NoTag  string
	Nested *sampleInner `json:"nested,omitempty"`
}

type sampleInner struct {
	A string `json:"a"`
	B []int  `json:"b"`
}

func encodeValues() []any {
	var vals []any
	for _, f := range floatCorpus() {
		vals = append(vals, f)
	}
	vals = append(vals, math.NaN(), float32(0.1), float32(1e21), float32(1e-7), float32(3.4028235e38))
	for _, s := range quoteInputs() {
		vals = append(vals, s)
	}
	t := true
	vals = append(vals,
		nil, true, false, 0, -1, math.MaxInt64, int64(math.MinInt64), uint64(math.MaxUint64), int64(1<<53+1),
		[]any{}, []any(nil), map[string]any{}, map[string]any(nil), []string(nil), []string{}, []string{"a", "<b>"},
		[]byte(nil), []byte{}, []byte("hello"), []byte{0, 0xff, 0x10}, []byte("abcd"),
		json.RawMessage(nil), json.RawMessage(`{ "a" : [1, 2 ,3] , "b":"<&>" }`), json.RawMessage(`not json`), json.RawMessage(`"\xff"`), json.RawMessage(` 12 `),
		map[string]any{"b": 1.0, "a": 2.0, "A": 3.0, "\xc3\xa9": 4.0, "\xff": 5.0, "": 6.0, "<": 7.0, "a\xe2\x80\xa8": 8.0, "10": 9.0, "9": 10.0},
		map[string]any{"x": []any{1.0, "two", nil, true, map[string]any{"y": []any{}}}, "z": map[string]any{"q": math.Copysign(0, -1)}},
		[]any{[]any{[]any{[]any{}}}, map[string]any{"": map[string]any{"": nil}}},
		map[string]any{"bad": math.Inf(1)},
		sample{Name: "n<>&", Count: -3, Big: math.MaxInt64, Small: 7, Ratio: 0.5, Flag: true, Opt: &t, Tags: []string{"x"}, Meta: map[string]any{"k": 1.0}, Inner: sampleInner{A: "a", B: []int{1, 2}}, Raw: json.RawMessage(`[1]`), Any: "any", Skip: "skip", NoTag: "notag"},
		sample{},
		&sampleInner{A: "p"},
	)
	return vals
}

func marshalEscapeHTMLOff(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	err := enc.Encode(v)
	return buf.Bytes(), err
}

func describeEncodeInput(v any) any {
	switch x := v.(type) {
	case sample, *sampleInner:
		b, _ := json.Marshal(x)
		return []any{"struct", string(b)}
	}
	return desc(v)
}

func decodeInputs() []string {
	in := []string{
		``, ` `, `null`, `true`, `false`, `0`, `-0`, `1`, `-1`, `1.5`, `1e3`, `1E+3`, `1e-3`, `0.1`, `1e400`, `-1e400`, `1e-400`, `01`, `-`, `+1`, `.5`, `5.`, `1.e5`, `1e`, `0x10`, `NaN`, `Infinity`, `-01`,
		`123456789012345678901234567890`, `9007199254740993`, `4.9406564584124654e-324`, `2.4703282292062327e-324`, `1.7976931348623159e308`,
		`""`, `"a"`, `"~""`, `"~~"`, `"~/"`, `"~b~f~n~r~t"`, `"~u0041"`, `"~u00e9"`, `"~u00E9"`, `"~ud83d~ude00"`, `"~ud83d"`, `"~ude00"`, `"~ud83dx"`, `"~ud83d~u0041"`,
		`"~u0000"`, `"~uD800~uDBFF"`, `"~x41"`, `"~a"`, `"~u12"`, `"~u12g4"`, "\"\x01\"", "\"\x1f\"", "\"\x7f\"", "\"a\tb\"", "\"\xff\"", "\"\xc3\x28\"", "\"\xed\xa0\x80\"", "\"\xf4\x90\x80\x80\"",
		"\"\xe2\x80\xa8\"", `"unterminated`, `"a"x`, `[]`, `[ ]`, `[1,2,3]`, `[1,]`, `[,1]`, `[1 2]`, `[`, `]`, `[[[]]]`, `{}`, `{ }`, `{"a":1}`, `{"a":1,"a":2}`, `{"a":1,}`,
		`{,}`, `{"a"}`, `{"a":}`, `{"a" 1}`, `{a:1}`, `{'a':1}`, `{"a":1}}`, `{"a":1} x`, `{"a":1}{"b":2}`, `{"":0}`, `{"~u0041":1,"A":2}`, `{"b":1,"a":{"c":[1,{"d":null}]}}`,
		"\ufeff{}", " \t\r\n{} \t\r\n", "\v{}", `{"a":1}` + "\x00", `[1,2,3` + strings.Repeat(" ", 3), `nul`, `nulll`, `tru`, `trUe`, `fals`, `[true,false,null]`, `-x`, `1.5.5`, `--1`,
		strings.Repeat("[", 10000) + strings.Repeat("]", 10000), strings.Repeat("[", 10001) + strings.Repeat("]", 10001),
		strings.Repeat(`{"a":`, 10001) + `1` + strings.Repeat("}", 10001), `{"entity":"book",}`, `{"entity":"b","limit":5,}`, `not json`, `x`, `{"entity":"b"} x`,
	}
	out := make([]string, len(in))
	for i, s := range in {
		out[i] = bsl(s)
	}
	return out
}

func genJSON(out string) error {
	var enc goldenio.Lines
	for _, v := range encodeValues() {
		row := map[string]any{"value": describeEncodeInput(v)}
		if b, err := json.Marshal(v); err != nil {
			row["error"] = jsonErr(err)
		} else {
			textOrHex(row, "marshal", b)
		}
		if b, err := json.MarshalIndent(v, "", "  "); err == nil {
			textOrHex(row, "indent", b)
		}
		if b, err := json.MarshalIndent(v, ">", "\t"); err == nil {
			textOrHex(row, "indent_prefix", b)
		}
		if b, err := marshalEscapeHTMLOff(v); err == nil {
			textOrHex(row, "encode_no_html", b)
		}
		enc.Add(row)
	}
	if err := enc.Write(out, "json/encode.jsonl"); err != nil {
		return err
	}
	var dec goldenio.Lines
	seen := map[string]bool{}
	addDecode := func(s string, withStruct bool) {
		if seen[s] {
			return
		}
		seen[s] = true
		var v any
		err := json.Unmarshal([]byte(s), &v)
		row := map[string]any{"in": hs(s), "valid": json.Valid([]byte(s))}
		if err != nil {
			row["error"] = jsonErr(err)
		} else if d := depth(v); d > 100 {
			row["value_depth"] = d
		} else {
			row["value"] = desc(v)
		}
		if withStruct {
			var st sample
			if err := json.Unmarshal([]byte(s), &st); err != nil {
				row["struct_error"] = jsonErr(err)
			}
			b, _ := json.Marshal(st)
			row["struct"] = string(b)
		}
		var raw json.RawMessage
		if err := json.Unmarshal([]byte(s), &raw); err != nil {
			row["raw_error"] = jsonErr(err)
		} else {
			row["raw"] = hx(raw)
		}
		dec.Add(row)
	}
	for _, s := range decodeInputs() {
		addDecode(s, true)
	}
	for _, s := range structInputs() {
		addDecode(s, true)
	}
	for _, v := range encodeValues() {
		if b, err := json.Marshal(v); err == nil {
			addDecode(string(b), false)
		}
	}
	if err := dec.Write(out, "json/decode.jsonl"); err != nil {
		return err
	}
	var streams goldenio.Lines
	for _, s := range streamInputs() {
		streams.Add(map[string]any{"in": hs(s), "values": decodeStream(s), "tokens": tokenStream(s), "object_walk": objectWalk(s)})
	}
	return streams.Write(out, "json/stream.jsonl")
}

func structInputs() []string {
	in := []string{
		`{"name":"x","count":3,"big":9223372036854775807,"small":4294967295,"ratio":1.5,"flag":true,"opt":false,"tags":["a"],"meta":{"k":[1]},"inner":{"a":"y","b":[1,2]},"raw":{"z": 1},"any":[1,"a"],"Skip":"s","NoTag":"t"}`,
		`{"NAME":"upper","Count":1,"cOuNt":2}`, `{"name":1}`, `{"count":"1"}`, `{"count":1.5}`, `{"count":1e2}`, `{"count":9223372036854775808}`, `{"big":-9223372036854775809}`,
		`{"small":-1}`, `{"small":4294967296}`, `{"flag":"true"}`, `{"flag":1}`, `{"tags":"a"}`, `{"tags":[1]}`, `{"tags":null}`, `{"meta":[]}`, `{"inner":{"b":["x"]}}`,
		`{"inner":{"b":[1.5]}}`, `{"inner":[]}`, `{"unknown":1}`, `{"name":null,"count":null}`, `{"opt":null}`, `{"raw":null}`, `{"nested":{"a":"n"}}`, `{"nested":null}`,
		`[1]`, `"s"`, `{"name":"a","count":"x","flag":"y"}`, `{"empty":"e"}`, `{"-":"dash"}`, `{"notag":"lower"}`, `{"name":"~ud800"}`, `{"name":"\xff"}`,
	}
	out := make([]string, len(in))
	for i, s := range in {
		out[i] = bsl(s)
	}
	return out
}

func streamInputs() []string {
	in := []string{
		``, ` `, `1 2 3`, `{"a":1} {"b":2}`, `{"a":1}` + "\n" + `{"a":`, `[1,2] x`, `"a""b"`, `{}garbage`, `nulltrue`, `1 2 x 3`, `{"a":1}]`,
		`{"entity":"b","limit":5}`, `[1,"a",true,null,{"x":[]}]`, `{"a" 1}`, `{,}`, `{"a":1,}`, `x`, `{"z":1,"a":{"q":2},"m":[3]}`, `{"b":{},"a":[],"b":1}`,
		`{"a":1 "b":2}`, `{"a":[1,2,}`, `{"sorts":{"title":"asc","year":"desc"}}`, `{"title":"asc","year":"desc","title":"desc"}`, `{"a":1}` + "\x00", `{"~u0061":1}`,
	}
	out := make([]string, len(in))
	for i, s := range in {
		out[i] = bsl(s)
	}
	return out
}

func decodeStream(s string) []any {
	d := json.NewDecoder(strings.NewReader(s))
	var steps []any
	for i := 0; i < 20; i++ {
		var v any
		err := d.Decode(&v)
		step := map[string]any{"offset": d.InputOffset()}
		if err != nil {
			step["error"] = jsonErr(err)
			steps = append(steps, step)
			break
		}
		step["value"] = desc(v)
		steps = append(steps, step)
	}
	return steps
}

func tokenStream(s string) []any {
	d := json.NewDecoder(strings.NewReader(s))
	var steps []any
	for i := 0; i < 64; i++ {
		more := d.More()
		tok, err := d.Token()
		step := map[string]any{"more": more, "offset": d.InputOffset()}
		if err != nil {
			step["error"] = jsonErr(err)
			steps = append(steps, step)
			break
		}
		step["token"] = desc(tok)
		steps = append(steps, step)
	}
	return steps
}

func objectWalk(s string) map[string]any {
	d := json.NewDecoder(strings.NewReader(s))
	tok, err := d.Token()
	if err != nil || tok != json.Delim('{') {
		return map[string]any{"object": false}
	}
	var keys []any
	var values []any
	for d.More() {
		t, terr := d.Token()
		key, isString := t.(string)
		keys = append(keys, map[string]any{"key": hs(key), "string": isString, "token_error": terr != nil})
		var v json.RawMessage
		verr := d.Decode(&v)
		values = append(values, map[string]any{"raw": hx(v), "error": verr != nil})
		if len(keys) > 64 {
			break
		}
	}
	return map[string]any{"object": true, "keys": keys, "values": values}
}

func depth(v any) int {
	switch x := v.(type) {
	case []any:
		m := 0
		for _, e := range x {
			m = max(m, depth(e))
		}
		return m + 1
	case map[string]any:
		m := 0
		for _, e := range x {
			m = max(m, depth(e))
		}
		return m + 1
	}
	return 0
}
