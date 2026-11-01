package log

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"math"
	"strconv"
	"time"
	"unicode"
	"unicode/utf8"
)

const (
	reset   = "\033[0m"
	red     = "\033[31m"
	yellow  = "\033[33m"
	magenta = "\033[35m"
	cyan    = "\033[36m"
)

const textTimeFormat = "2006-01-02T15:04:05.000Z07:00"

type handler struct {
	attrs []slog.Attr
}

func (h *handler) Enabled(_ context.Context, level slog.Level) bool {
	return level >= levelVar.Level()
}

func (h *handler) WithAttrs(attrs []slog.Attr) slog.Handler {
	merged := make([]slog.Attr, 0, len(h.attrs)+len(attrs))
	merged = append(merged, h.attrs...)
	merged = append(merged, attrs...)

	return &handler{attrs: merged}
}

func (h *handler) WithGroup(_ string) slog.Handler {
	return h
}

func (h *handler) Handle(_ context.Context, r slog.Record) error {
	fields := make([]field, 0, len(h.attrs)+r.NumAttrs())
	for _, a := range h.attrs {
		fields = appendField(fields, "", a)
	}

	r.Attrs(func(a slog.Attr) bool {
		fields = appendField(fields, "", a)
		return true
	})

	switch {
	case r.Level >= slog.LevelError:
		errorCount.Add(1)
	case r.Level >= slog.LevelWarn:
		warningCount.Add(1)
	}

	t := r.Time
	if t.IsZero() {
		t = time.Now()
	}

	record(t, r.Level, r.Message, fields)

	current.mu.Lock()
	defer current.mu.Unlock()

	var line []byte
	if current.format == FormatJSON {
		line = jsonLine(t, r.Level, r.Message, fields)
	} else {
		line = textLine(t, r.Level, r.Message, fields, current.color)
	}

	_, err := current.out.Write(line)

	return err
}

type field struct {
	key   string
	value any
}

func appendField(fields []field, prefix string, a slog.Attr) []field {
	v := a.Value.Resolve()
	if a.Key == "" && v.Kind() != slog.KindGroup {
		return fields
	}

	key := a.Key
	if prefix != "" {
		key = prefix + "." + key
	}

	if v.Kind() == slog.KindGroup {
		for _, ga := range v.Group() {
			fields = appendField(fields, key, ga)
		}
		return fields
	}

	return append(fields, field{key: key, value: plain(v)})
}

func plain(v slog.Value) any {
	switch v.Kind() {
	case slog.KindString:
		return v.String()
	case slog.KindInt64:
		return v.Int64()
	case slog.KindUint64:
		return v.Uint64()
	case slog.KindFloat64:
		f := v.Float64()
		if math.IsNaN(f) || math.IsInf(f, 0) {
			return strconv.FormatFloat(f, 'g', -1, 64)
		}
		return f
	case slog.KindBool:
		return v.Bool()
	case slog.KindDuration:
		return v.Duration().String()
	case slog.KindTime:
		return v.Time().Format(time.RFC3339Nano)
	}

	switch x := v.Any().(type) {
	case nil:
		return nil
	case error:
		return x.Error()
	case fmt.Stringer:
		return x.String()
	case []byte:
		return string(x)
	case []string:
		return x
	}

	return fmt.Sprint(v.Any())
}

func levelLabel(level slog.Level) string {
	switch {
	case level < slog.LevelInfo:
		return "DEBUG"
	case level < slog.LevelWarn:
		return "INFO"
	case level < slog.LevelError:
		return "WARN"
	}

	return "ERROR"
}

func levelColor(level slog.Level) string {
	switch {
	case level < slog.LevelInfo:
		return magenta
	case level < slog.LevelWarn:
		return cyan
	case level < slog.LevelError:
		return yellow
	}

	return red
}

func textLine(t time.Time, level slog.Level, msg string, fields []field, color bool) []byte {
	var b bytes.Buffer
	b.WriteString("time=")
	b.WriteString(t.Format(textTimeFormat))
	b.WriteString(" level=")
	if color {
		b.WriteString(levelColor(level))
		b.WriteString(levelLabel(level))
		b.WriteString(reset)
	} else {
		b.WriteString(levelLabel(level))
	}
	b.WriteString(" msg=")
	b.WriteString(quoteIfNeeded(msg))

	for _, f := range fields {
		b.WriteByte(' ')
		b.WriteString(quoteIfNeeded(f.key))
		b.WriteByte('=')
		b.WriteString(quoteIfNeeded(textValue(f.value)))
	}

	b.WriteByte('\n')

	return b.Bytes()
}

func textValue(v any) string {
	switch x := v.(type) {
	case nil:
		return "<nil>"
	case string:
		return x
	case int64:
		return strconv.FormatInt(x, 10)
	case uint64:
		return strconv.FormatUint(x, 10)
	case float64:
		return strconv.FormatFloat(x, 'f', -1, 64)
	case bool:
		return strconv.FormatBool(x)
	}

	return fmt.Sprint(v)
}

func quoteIfNeeded(s string) string {
	if s == "" {
		return `""`
	}

	for i := 0; i < len(s); {
		r, size := utf8.DecodeRuneInString(s[i:])
		if r == utf8.RuneError || r <= ' ' || r == '=' || r == '"' || r == '\\' || !unicode.IsPrint(r) {
			return strconv.Quote(s)
		}
		i += size
	}

	return s
}

func jsonLine(t time.Time, level slog.Level, msg string, fields []field) []byte {
	var b bytes.Buffer
	b.WriteString(`{"time":`)
	writeJSON(&b, t.Format(time.RFC3339Nano))
	b.WriteString(`,"level":`)
	writeJSON(&b, levelLabel(level))
	b.WriteString(`,"msg":`)
	writeJSON(&b, msg)

	for _, f := range fields {
		b.WriteByte(',')
		writeJSON(&b, f.key)
		b.WriteByte(':')
		writeJSON(&b, f.value)
	}

	b.WriteString("}\n")

	return b.Bytes()
}

func writeJSON(b *bytes.Buffer, v any) {
	data, err := json.Marshal(v)
	if err != nil {
		data, _ = json.Marshal(fmt.Sprint(v))
	}

	b.Write(data)
}
