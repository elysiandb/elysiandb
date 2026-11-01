package log

import (
	"log/slog"
	"slices"
	"strings"
	"sync"
	"time"
)

const (
	DefaultQueryLimit = 200
	MaxQueryLimit     = 1000
)

type Entry struct {
	Seq       uint64         `json:"seq"`
	Time      time.Time      `json:"time"`
	Level     string         `json:"level"`
	Component string         `json:"component,omitempty"`
	Message   string         `json:"msg"`
	Attrs     map[string]any `json:"attrs,omitempty"`

	level slog.Level
}

type Query struct {
	MinLevel  slog.Level
	Component string
	Text      string
	User      string
	RequestID string
	Since     time.Time
	Until     time.Time
	Before    uint64
	After     uint64
	Limit     int
}

type Page struct {
	Entries     []Entry    `json:"entries"`
	HasMore     bool       `json:"has_more"`
	OldestSeq   uint64     `json:"oldest_seq"`
	LatestSeq   uint64     `json:"latest_seq"`
	Size        int        `json:"size"`
	Capacity    int        `json:"capacity"`
	BufferSince *time.Time `json:"buffer_since"`
	Components  []string   `json:"components"`
}

type ring struct {
	mu      sync.RWMutex
	entries []Entry
	next    int
	size    int
	seq     uint64
}

var buffer = &ring{}

func resizeBuffer(capacity int) {
	if capacity < 0 {
		capacity = 0
	}

	buffer.mu.Lock()
	defer buffer.mu.Unlock()

	if capacity == len(buffer.entries) {
		return
	}

	kept := buffer.orderedLocked()
	if len(kept) > capacity {
		kept = kept[len(kept)-capacity:]
	}

	buffer.entries = make([]Entry, capacity)
	copy(buffer.entries, kept)
	buffer.size = len(kept)
	buffer.next = 0
	if capacity > 0 {
		buffer.next = len(kept) % capacity
	}
}

func BufferEnabled() bool {
	buffer.mu.RLock()
	defer buffer.mu.RUnlock()

	return len(buffer.entries) > 0
}

func record(t time.Time, level slog.Level, msg string, fields []field) {
	buffer.mu.Lock()
	defer buffer.mu.Unlock()

	if len(buffer.entries) == 0 {
		return
	}

	buffer.seq++
	entry := Entry{
		Seq:     buffer.seq,
		Time:    t,
		Level:   levelLabel(level),
		Message: msg,
		level:   level,
	}

	for _, f := range fields {
		if f.key == "component" {
			if s, ok := f.value.(string); ok {
				entry.Component = s
				continue
			}
		}

		if entry.Attrs == nil {
			entry.Attrs = make(map[string]any, len(fields))
		}
		entry.Attrs[f.key] = f.value
	}

	buffer.entries[buffer.next] = entry
	buffer.next = (buffer.next + 1) % len(buffer.entries)
	if buffer.size < len(buffer.entries) {
		buffer.size++
	}
}

func (r *ring) orderedLocked() []Entry {
	out := make([]Entry, 0, r.size)
	if r.size == 0 {
		return out
	}

	start := (r.next - r.size + len(r.entries)) % len(r.entries)
	for i := 0; i < r.size; i++ {
		out = append(out, r.entries[(start+i)%len(r.entries)])
	}

	return out
}

func Logs(q Query) Page {
	buffer.mu.RLock()
	ordered := buffer.orderedLocked()
	capacity := len(buffer.entries)
	buffer.mu.RUnlock()

	limit := q.Limit
	if limit <= 0 {
		limit = DefaultQueryLimit
	}
	if limit > MaxQueryLimit {
		limit = MaxQueryLimit
	}

	page := Page{
		Entries:    []Entry{},
		Size:       len(ordered),
		Capacity:   capacity,
		Components: []string{},
	}

	components := map[string]struct{}{}
	for _, e := range ordered {
		if e.Component != "" {
			components[e.Component] = struct{}{}
		}
	}
	for c := range components {
		page.Components = append(page.Components, c)
	}
	slices.Sort(page.Components)

	if len(ordered) > 0 {
		page.OldestSeq = ordered[0].Seq
		page.LatestSeq = ordered[len(ordered)-1].Seq
		since := ordered[0].Time
		page.BufferSince = &since
	}

	text := strings.ToLower(strings.TrimSpace(q.Text))

	for i := len(ordered) - 1; i >= 0; i-- {
		e := ordered[i]
		if !q.matches(e, text) {
			continue
		}

		if len(page.Entries) == limit {
			page.HasMore = true
			break
		}

		page.Entries = append(page.Entries, e)
	}

	return page
}

func (q Query) matches(e Entry, text string) bool {
	if e.level < q.MinLevel {
		return false
	}
	if q.After > 0 && e.Seq <= q.After {
		return false
	}
	if q.Before > 0 && e.Seq >= q.Before {
		return false
	}
	if !q.Since.IsZero() && e.Time.Before(q.Since) {
		return false
	}
	if !q.Until.IsZero() && e.Time.After(q.Until) {
		return false
	}
	if q.Component != "" && e.Component != q.Component {
		return false
	}
	if q.User != "" && !attrEquals(e, q.User, "user", "actor") {
		return false
	}
	if q.RequestID != "" && !attrEquals(e, q.RequestID, "request_id") {
		return false
	}
	if text != "" && !entryContains(e, text) {
		return false
	}

	return true
}

func attrEquals(e Entry, want string, keys ...string) bool {
	for _, k := range keys {
		if v, ok := e.Attrs[k]; ok && textValue(v) == want {
			return true
		}
	}

	return false
}

func entryContains(e Entry, text string) bool {
	if strings.Contains(strings.ToLower(e.Message), text) || strings.Contains(strings.ToLower(e.Component), text) {
		return true
	}

	for k, v := range e.Attrs {
		if strings.Contains(strings.ToLower(k), text) || strings.Contains(strings.ToLower(textValue(v)), text) {
			return true
		}
	}

	return false
}
