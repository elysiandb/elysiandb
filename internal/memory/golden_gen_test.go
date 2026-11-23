package memory

import (
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func TestGenerateEngineGolden(t *testing.T) {
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	dir := filepath.Join(root, "engine")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	defer Configure(0)
	var lines goldenio.Lines
	budgets := []int64{0, -1, 1, 1 << 20, 100 << 20, 512 << 20, 1 << 30, 3 << 30, 16 << 30, 1 << 40}
	for _, b := range budgets {
		Configure(b)
		limits := map[string]string{}
		for _, p := range Pools() {
			limits[p.Name()] = strconv.FormatInt(p.Limit(), 10)
		}
		effective := map[string]map[string]string{}
		for _, lv := range []Level{Normal, High, Critical} {
			level.Store(int32(lv))
			e := map[string]string{}
			for _, p := range Pools() {
				e[p.Name()] = strconv.FormatInt(p.Effective(), 10)
			}
			effective[lv.String()] = e
		}
		level.Store(int32(Normal))
		lines.Add(map[string]any{"fn": "Configure", "in": strconv.FormatInt(b, 10), "budget": strconv.FormatInt(Budget(), 10), "limits": limits, "effective": effective, "request_cap": strconv.FormatInt(RequestCap(), 10)})
	}
	for _, b := range []int64{0, 1000, 1 << 30} {
		for _, lf := range []float64{0, 0.5, 0.64, 0.65, 0.66, 0.79, 0.80, 0.81, 1.2} {
			for _, tf := range []float64{0, 0.9, 0.94, 0.95, 0.96, 1.04, 1.05, 1.06} {
				l := int64(float64(b) * lf)
				tt := int64(float64(b) * tf)
				lines.Add(map[string]any{"fn": "levelFor", "in": []string{strconv.FormatInt(b, 10), strconv.FormatInt(l, 10), strconv.FormatInt(tt, 10)}, "out": levelFor(b, l, tt).String()})
			}
		}
	}
	type step struct {
		Live  float64 `json:"live"`
		Level Level   `json:"level"`
	}
	sequences := map[string][]step{
		"grow": {{0.1, Normal}, {0.1, Normal}, {0.2, Normal}, {0.3, Normal}, {0.47, Normal}, {0.47, Normal}},
		"grow-to-cap": func() []step {
			var s []step
			for i := 0; i < 30; i++ {
				s = append(s, step{0.1, Normal})
			}
			return s
		}(),
		"shrink":       {{0.1, Normal}, {0.1, Normal}, {0.1, Normal}, {0.6, Normal}, {0.6, Normal}, {0.5, Normal}},
		"level-resets": {{0.1, Normal}, {0.1, Normal}, {0.1, High}, {0.1, Normal}, {0.1, Critical}},
	}
	for _, name := range []string{"grow", "grow-to-cap", "shrink", "level-resets"} {
		for _, b := range []int64{0, 1 << 30, 1000} {
			elastic.Store(0)
			var outs []string
			for _, s := range sequences[name] {
				followSpare(b, int64(float64(b)*s.Live), s.Level)
				outs = append(outs, strconv.FormatInt(Elastic(), 10))
			}
			lines.Add(map[string]any{"fn": "followSpare", "sequence": name, "budget": strconv.FormatInt(b, 10), "steps": sequences[name], "elastic": outs})
		}
	}
	elastic.Store(0)
	if err := lines.Write(dir, "memory.jsonl"); err != nil {
		t.Fatal(err)
	}
}
