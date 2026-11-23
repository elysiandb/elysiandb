package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"log/slog"
	"math"
	"regexp"
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var (
	textTimeRe = regexp.MustCompile(`(?m)^time=[^ ]+`)
	jsonTimeRe = regexp.MustCompile(`(?m)^\{"time":"[^"]*"`)
	pageTimeRe = regexp.MustCompile(`"(time|buffer_since)":"[^"]*"`)
)

func maskLogTimes(s string) string {
	s = textTimeRe.ReplaceAllString(s, "time=<T>")
	return jsonTimeRe.ReplaceAllString(s, `{"time":"<T>"`)
}

type logCall struct {
	name string
	call func()
}

func logValues() []logCall {
	type named struct {
		name string
		v    any
	}
	values := []named{
		{"string", "plain"}, {"empty", ""}, {"spaces", "a b"}, {"equals", "a=b"}, {"quote", "say \"hi\""}, {"backslash", "a\\b"},
		{"newline", "a\nb"}, {"tab", "a\tb"}, {"control", "a\x01b"}, {"nbsp", "a\u00a0b"}, {"zero-width", "a\u200bb"}, {"invalid-utf8", "a\xffb"},
		{"emoji", "\U0001f600"}, {"int", 42}, {"int64", int64(math.MinInt64)}, {"uint64", uint64(math.MaxUint64)}, {"float", 1.5}, {"float-big", 1e21},
		{"float-small", 1e-7}, {"float-int", 3.0}, {"nan", math.NaN()}, {"inf", math.Inf(1)}, {"bool", true}, {"nil", nil},
		{"duration", 1500 * time.Millisecond}, {"time", time.Date(2026, 11, 23, 18, 39, 1, 120000000, time.UTC)}, {"error", errors.New("boom \"x\"")},
		{"strings", []string{"a", "b c"}}, {"map", map[string]any{"b": 1, "a": "x"}}, {"bytes", []byte("raw")}, {"struct", struct{ A int }{1}},
	}
	var calls []logCall
	for _, v := range values {
		v := v
		calls = append(calls, logCall{name: "value " + v.name, call: func() { log.Info("event", "component", "golden", "value", v.v) }})
	}
	calls = append(calls,
		logCall{"duplicates", func() { log.Info("dup", "component", "golden", "k", 1, "k", 2) }},
		logCall{"odd args", func() { log.Info("odd", "component", "golden", "lonely") }},
		logCall{"no component", func() { log.Warn("bare message") }},
		logCall{"message quoting", func() { log.Info("msg with \"quotes\" and = sign", "component", "golden") }},
		logCall{"for component", func() { log.For("storage").Info("from logger", "n", 1) }},
		logCall{"error level", func() { log.Error("failed", "component", "golden", "err", errors.New("disk full")) }},
		logCall{"debug hidden", func() { log.Debug("hidden", "component", "golden") }},
		logCall{"group", func() { log.Info("grouped", "component", "golden", slog.Group("g", "a", 1, "b", "x")) }},
	)
	return calls
}

func genLogs(out string) error {
	var l goldenio.Lines
	for _, c := range logValues() {
		row := map[string]any{"call": c.name}
		for _, mode := range []struct {
			key    string
			format string
			color  bool
		}{{"text", "text", false}, {"text_color", "text", true}, {"json", "json", false}} {
			if err := log.Configure(log.Options{Level: "info", Format: mode.format, BufferLines: 100}); err != nil {
				return err
			}
			var buf bytes.Buffer
			log.SetOutput(&buf, mode.color)
			c.call()
			row[mode.key] = maskLogTimes(buf.String())
		}
		l.Add(row)
	}
	if err := log.Configure(log.Options{Level: "debug", Format: "text", BufferLines: 8}); err != nil {
		return err
	}
	var sink bytes.Buffer
	log.SetOutput(&sink, false)
	for i := 0; i < 12; i++ {
		switch i % 4 {
		case 0:
			log.Debug("line", "component", "a", "i", i)
		case 1:
			log.Info("line", "component", "b", "i", i, "user", "ann")
		case 2:
			log.Warn("line", "component", "a", "i", i, "request_id", "r-1")
		default:
			log.Error("line", "component", "c", "i", i)
		}
	}
	queries := []struct {
		name string
		q    log.Query
	}{
		{"all", log.Query{Limit: 100}}, {"limit 3", log.Query{Limit: 3}}, {"warn+", log.Query{MinLevel: slog.LevelWarn, Limit: 100}},
		{"component a", log.Query{Component: "a", Limit: 100}}, {"text", log.Query{Text: "line", Limit: 2}}, {"user", log.Query{User: "ann", Limit: 100}},
		{"request id", log.Query{RequestID: "r-1", Limit: 100}}, {"before", log.Query{Before: 10, Limit: 100}}, {"after", log.Query{After: 8, Limit: 100}},
	}
	for _, q := range queries {
		page := log.Logs(q.q)
		b, err := json.Marshal(page)
		if err != nil {
			return err
		}
		l.Add(map[string]any{"call": "buffer " + q.name, "page": pageTimeRe.ReplaceAllString(string(b), `"$1":"<T>"`)})
	}
	return l.Write(out, "logs/cases.jsonl")
}
