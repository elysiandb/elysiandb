package docpath

import (
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func dateInputs() []string {
	in := []string{
		"2026-11-23", "2026-11-23T18:39:01Z", "2026-11-23T18:39:01+01:00", "2026-11-23T18:39:01-05:30", "2026-11-23T18:39:01.123Z", "2026-11-23 18:39:01Z",
		"2026-11-23t18:39:01z", "2026-11-23T18:39:01", "2026-1-23", "2026-11-3", "26-11-23", "2026/11/23", "2026-11-23x", "2026-11-23T", "", "short", "abcd-11-23",
		"2026-13-01", "2026-00-01", "2026-11-31", "2026-11-00", "0000-01-01", "9999-12-31", "9999-12-31T23:59:59.999999999Z", "0000-01-01T00:00:00Z",
	}
	for _, y := range []string{"0000", "1900", "2000", "2023", "2024"} {
		in = append(in, y+"-02-29", y+"-02-29T12:00:00Z", y+"-02-28", y+"-03-01")
	}
	for _, sep := range []string{"T", "t", " "} {
		for _, tm := range []string{"1:2:3", "01:02:03", "1:02:03", "01:2:03", "01:02:3", "23:59:59", "24:00:00", "23:60:00", "23:59:60"} {
			in = append(in, "2026-06-15"+sep+tm+"Z")
		}
	}
	for frac := 0; frac <= 12; frac++ {
		d := strings.Repeat("5", frac)
		in = append(in, "2026-06-15T12:00:00."+d+"Z", "2026-06-15T12:00:00,"+d+"Z")
	}
	for _, sign := range []string{"+", "-"} {
		for _, hh := range []string{"00", "01", "12", "23", "24", "25"} {
			for _, mm := range []string{"00", "30", "59", "60", "61"} {
				in = append(in, "2026-06-15T12:00:00"+sign+hh+":"+mm)
			}
		}
	}
	for _, tail := range []string{"Z ", "Zx", "+01:00x", " ", "\x00", "+0100", "+01"} {
		in = append(in, "2026-06-15T12:00:00"+tail)
	}
	return in
}

func TestGenerateEngineGolden(t *testing.T) {
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	dir := filepath.Join(root, "engine")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	var lines goldenio.Lines
	for _, s := range dateInputs() {
		row := map[string]any{"in": hex.EncodeToString([]byte(s))}
		tm, ok, dateOnly := ParseDate(s)
		row["ok"] = ok
		row["date_only"] = dateOnly
		if ok {
			row["unix"] = tm.Unix()
			row["nanos"] = tm.Nanosecond()
		}
		st, sok := ParseSortDate(s)
		row["sort_ok"] = sok
		if sok {
			row["sort_unix"] = st.Unix()
			row["sort_nanos"] = st.Nanosecond()
			row["sort_day_unix"] = st.Truncate(24 * 3600 * 1e9).Unix()
		}
		lines.Add(row)
	}
	if err := lines.Write(dir, "dates.jsonl"); err != nil {
		t.Fatal(err)
	}
}
