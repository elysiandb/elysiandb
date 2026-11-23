package main

import (
	"archive/zip"
	"fmt"
	"io"
	"math"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var zoneNames = []string{
	"UTC", "Europe/Paris", "America/New_York", "America/Sao_Paulo", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Chatham",
	"Africa/Casablanca", "Asia/Tokyo", "Pacific/Kiritimati", "America/St_Johns", "Europe/Dublin", "Antarctica/Troll", "Asia/Kathmandu",
}

var timeLayouts = []string{
	time.RFC3339, time.RFC3339Nano, "2006-01-02", "20060102-150405", "20060102150405.000000000", "20060102T150405", "2006-01-02T15:04:05.000Z07:00",
}

func loadZones() (map[string][]byte, error) {
	root := runtime.GOROOT()
	zr, err := zip.OpenReader(filepath.Join(root, "lib", "time", "zoneinfo.zip"))
	if err != nil {
		return nil, err
	}
	defer zr.Close()
	want := map[string]bool{}
	for _, z := range zoneNames {
		want[z] = true
	}
	out := map[string][]byte{}
	for _, f := range zr.File {
		if !want[f.Name] {
			continue
		}
		rc, err := f.Open()
		if err != nil {
			return nil, err
		}
		data, err := io.ReadAll(rc)
		rc.Close()
		if err != nil {
			return nil, err
		}
		out[f.Name] = data
	}
	if len(out) != len(zoneNames) {
		return nil, fmt.Errorf("zoneinfo.zip holds %d of %d zones", len(out), len(zoneNames))
	}
	return out, nil
}

func zoneFile(name string) string {
	return "time/zoneinfo/" + strings.ReplaceAll(name, "/", "_") + ".tzif"
}

func instantRow(t time.Time) map[string]any {
	name, off := t.Zone()
	start, end := t.ZoneBounds()
	y, mo, d := t.Date()
	h, mi, s := t.Clock()
	row := map[string]any{
		"unix": t.Unix(), "nanos": t.Nanosecond(), "abbr": name, "offset": off, "dst": t.IsDST(),
		"date": []int{y, int(mo), d, h, mi, s}, "weekday": int(t.Weekday()), "yday": t.YearDay(),
		"rfc3339nano": t.Format(time.RFC3339Nano),
	}
	if !start.IsZero() {
		row["zone_start"] = start.Unix()
	}
	if !end.IsZero() {
		row["zone_end"] = end.Unix()
	}
	return row
}

func genTime(out string) error {
	zones, err := loadZones()
	if err != nil {
		return err
	}
	locs := map[string]*time.Location{}
	for _, name := range zoneNames {
		if err := goldenio.WriteFile(out, zoneFile(name), zones[name]); err != nil {
			return err
		}
		loc, err := time.LoadLocationFromTZData(name, zones[name])
		if err != nil {
			return err
		}
		locs[name] = loc
	}
	var zl goldenio.Lines
	fixed := []int64{math.MinInt32, -62135596800, -2208988800, -1, 0, 1, 951782400, 1700000000, 1800000000, 2147483647, 2147483648, 4102444800, 7258118400, 13569465600, 253402300799}
	for _, name := range zoneNames {
		loc := locs[name]
		seen := map[int64]bool{}
		var instants []int64
		add := func(u int64) {
			if !seen[u] {
				seen[u] = true
				instants = append(instants, u)
			}
		}
		for _, u := range fixed {
			add(u)
		}
		t := time.Date(1890, 1, 1, 0, 0, 0, 0, time.UTC).In(loc)
		limit := time.Date(2440, 1, 1, 0, 0, 0, 0, time.UTC)
		for steps := 0; t.Before(limit) && steps < 5000; steps++ {
			_, end := t.ZoneBounds()
			if end.IsZero() {
				break
			}
			u := end.Unix()
			add(u - 1)
			add(u)
			add(u + 1)
			if !end.After(t) {
				end = t.Add(time.Second)
			}
			t = end.In(loc)
		}
		for y := 2030; y <= 2420; y += 13 {
			for _, m := range []time.Month{1, 4, 7, 10} {
				add(time.Date(y, m, 15, 12, 0, 0, 0, time.UTC).Unix())
			}
		}
		for _, u := range instants {
			row := instantRow(time.Unix(u, 0).In(loc))
			row["zone"] = name
			zl.Add(row)
		}
	}
	if err := zl.Write(out, "time/zones.jsonl"); err != nil {
		return err
	}
	var fl goldenio.Lines
	fixedZones := []*time.Location{time.UTC, time.FixedZone("", 3600), time.FixedZone("", -34200), time.FixedZone("", 19800), time.FixedZone("X", 45296), time.FixedZone("", -1), locs["Europe/Paris"]}
	years := []int{-1, 0, 1, 999, 1969, 1970, 2000, 2026, 9999, 10000, 12345}
	nanos := []int{0, 1, 10, 100000, 120000000, 123456789, 999999999}
	for zi, loc := range fixedZones {
		for _, y := range years {
			for _, ns := range nanos {
				t := time.Date(y, time.Month(1+(y&7)), 1+(y%27+27)%27, 13, 4, 5, ns, loc)
				row := map[string]any{"zone_index": zi, "unix": t.Unix(), "nanos": t.Nanosecond(), "formats": formatAll(t)}
				fl.Add(row)
			}
		}
	}
	if err := fl.Write(out, "time/format.jsonl"); err != nil {
		return err
	}
	if err := goldenio.WriteJSON(out, "time/format_zones.json", []map[string]any{
		{"index": 0, "kind": "utc"}, {"index": 1, "kind": "fixed", "name": "", "offset": 3600}, {"index": 2, "kind": "fixed", "name": "", "offset": -34200},
		{"index": 3, "kind": "fixed", "name": "", "offset": 19800}, {"index": 4, "kind": "fixed", "name": "X", "offset": 45296}, {"index": 5, "kind": "fixed", "name": "", "offset": -1},
		{"index": 6, "kind": "tzif", "name": "Europe/Paris"},
	}); err != nil {
		return err
	}
	var pl goldenio.Lines
	for _, s := range parseTimeInputs() {
		row := map[string]any{"in": hs(s)}
		for _, layout := range []string{time.RFC3339, time.RFC3339Nano, "2006-01-02"} {
			res := map[string]any{}
			for _, zn := range []string{"UTC", "Europe/Paris"} {
				t, err := time.ParseInLocation(layout, s, locs[zn])
				if err != nil {
					res[zn] = map[string]any{"ok": false}
					continue
				}
				name, off := t.Zone()
				res[zn] = map[string]any{"ok": true, "unix": t.Unix(), "nanos": t.Nanosecond(), "abbr": name, "offset": off, "utc": t.Location() == time.UTC}
			}
			row[layout] = res
		}
		pl.Add(row)
	}
	if err := pl.Write(out, "time/parse.jsonl"); err != nil {
		return err
	}
	var dl goldenio.Lines
	durs := []int64{0, 1, 999, 1000, 1001, 1500, 999999, 1000000, 1500000, 1000000000, 1500000000, 61000000000, 3600000000000, 3661001001001, 86400000000000,
		-1, -1500000, -61000000000, math.MaxInt64, math.MinInt64, math.MinInt64 + 1, 123456789, 59999999999, 359999999999999}
	for _, d := range durs {
		dd := time.Duration(d)
		row := map[string]any{"ns": i64s(d), "string": dd.String(), "seconds": fbits(dd.Seconds()), "ms": i64s(dd.Milliseconds())}
		for _, m := range []time.Duration{time.Millisecond, 10 * time.Millisecond, time.Second, time.Minute, 0, -1} {
			row["round_"+i64s(int64(m))] = i64s(int64(dd.Round(m)))
			row["trunc_"+i64s(int64(m))] = i64s(int64(dd.Truncate(m)))
		}
		dl.Add(row)
	}
	if err := dl.Write(out, "time/duration.jsonl"); err != nil {
		return err
	}
	var cl goldenio.Lines
	civil := [][7]int{
		{2024, 2, 29, 0, 0, 0, 0}, {2023, 2, 29, 0, 0, 0, 0}, {1900, 2, 29, 0, 0, 0, 0}, {2000, 2, 29, 0, 0, 0, 0}, {0, 2, 29, 0, 0, 0, 0},
		{2026, 13, 1, 0, 0, 0, 0}, {2026, 0, 0, 0, 0, 0, 0}, {2026, 1, 1, 24, 60, 60, 1000000000}, {2026, 1, 1, -1, -1, -1, -1}, {-4713, 11, 24, 12, 0, 0, 0},
		{292277026596, 12, 4, 15, 30, 7, 0}, {1, 1, 1, 0, 0, 0, 0}, {9999, 12, 31, 23, 59, 59, 999999999}, {1969, 12, 31, 23, 59, 59, 999999999},
	}
	for _, c := range civil {
		t := time.Date(c[0], time.Month(c[1]), c[2], c[3], c[4], c[5], c[6], time.UTC)
		row := instantRow(t)
		row["in"] = c
		cl.Add(row)
	}
	for i := int64(-80000); i <= 80000; i += 997 {
		cl.Add(instantRow(time.Unix(i*86400+i*37, 0).UTC()))
	}
	return cl.Write(out, "time/civil.jsonl")
}

func formatAll(t time.Time) map[string]string {
	out := map[string]string{}
	for _, l := range timeLayouts {
		out[l] = t.Format(l)
	}
	return out
}

func parseTimeInputs() []string {
	base := []string{
		"2026-11-23T18:39:01Z", "2026-11-23T18:39:01z", "2026-11-23t18:39:01Z", "2026-11-23 18:39:01Z", "2026-11-23T18:39:01+01:00", "2026-11-23T18:39:01-00:00",
		"2026-11-23T18:39:01.5Z", "2026-11-23T18:39:01,5Z", "2026-11-23T18:39:01.123456789Z", "2026-11-23T18:39:01.1234567891Z", "2026-11-23T18:39:01.000000000000Z",
		"2026-11-23T18:39:01.Z", "2026-11-23T8:39:01Z", "2026-11-23T18:9:01Z", "2026-11-23T18:39:1Z", "2026-1-23T18:39:01Z", "2026-11-3T18:39:01Z",
		"2026-11-23T24:00:00Z", "2026-11-23T23:60:00Z", "2026-11-23T23:59:60Z", "2026-11-23T18:39:01+24:00", "2026-11-23T18:39:01+23:59", "2026-11-23T18:39:01+25:61",
		"2026-11-23T18:39:01+0100", "2026-11-23T18:39:01+01", "2026-11-23T18:39:01 +01:00", "2026-11-23T18:39:01", "2026-11-23T18:39Z", "2026-11-23",
		"2026-02-29", "2024-02-29", "1900-02-29", "2000-02-29", "0000-01-01", "9999-12-31", "10000-01-01", "-001-01-01", "2026-00-10", "2026-13-01", "2026-11-31",
		"2026-11-23x", "2026-11-23 ", " 2026-11-23", "2026/11/23", "20261123", "2026-11-2", "", "2026-11-23T18:39:01Zjunk", "2026-11-23T18:39:01+01:00junk",
		"0000-01-01T00:00:00Z", "9999-12-31T23:59:59.999999999Z", "1969-12-31T23:59:59.999999999Z", "2026-03-29T02:30:00+01:00", "2026-03-29T02:30:00+02:00",
		"2026-10-25T02:30:00+01:00", "2026-10-25T02:30:00+02:00", "2026-11-23T18:39:01+01:00:00", "2026-11-23T18:39:01.-5Z", "2026-11-23T18:39:01.5+01:00",
	}
	for _, sign := range []string{"+", "-"} {
		for _, hh := range []string{"00", "01", "12", "23", "24", "25"} {
			for _, mm := range []string{"00", "30", "59", "60", "61"} {
				base = append(base, "2026-06-15T12:00:00"+sign+hh+":"+mm)
			}
		}
	}
	for frac := 0; frac <= 12; frac++ {
		digits := strings.Repeat("7", frac)
		base = append(base, "2026-06-15T12:00:00."+digits+"Z", "2026-06-15T12:00:00,"+digits+"Z")
	}
	return base
}
