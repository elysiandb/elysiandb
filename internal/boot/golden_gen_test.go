package boot

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
	var lines goldenio.Lines
	for _, b := range []int64{0, 1 << 20, 900 << 20, 901 << 20, 1 << 30, 4 << 30} {
		for _, c := range []int64{0, -1, 1 << 30, 2 << 30} {
			lines.Add(map[string]any{"fn": "BudgetWithinContainer", "in": []string{strconv.FormatInt(b, 10), strconv.FormatInt(c, 10)}, "out": strconv.FormatInt(BudgetWithinContainer(b, c), 10)})
		}
	}
	saved := cgroupLimitFiles
	defer func() { cgroupLimitFiles = saved }()
	contents := []string{"max", "max\n", "123456789", " 456\n", "0", "-1", "abc", "1152921504606846976", "1152921504606846975", "9223372036854775807", "9223372036854775808", "", "12 34"}
	tmp := t.TempDir()
	for i, c := range contents {
		p := filepath.Join(tmp, strconv.Itoa(i))
		if err := os.WriteFile(p, []byte(c), 0o644); err != nil {
			t.Fatal(err)
		}
		cgroupLimitFiles = []string{p}
		lines.Add(map[string]any{"fn": "containerMemoryLimit", "first_file": c, "out": strconv.FormatInt(containerMemoryLimit(), 10)})
		fallback := filepath.Join(tmp, "fallback")
		if err := os.WriteFile(fallback, []byte("2147483648"), 0o644); err != nil {
			t.Fatal(err)
		}
		cgroupLimitFiles = []string{p, fallback}
		lines.Add(map[string]any{"fn": "containerMemoryLimit", "first_file": c, "second_file": "2147483648", "out": strconv.FormatInt(containerMemoryLimit(), 10)})
	}
	cgroupLimitFiles = []string{filepath.Join(tmp, "absent")}
	lines.Add(map[string]any{"fn": "containerMemoryLimit", "first_file": nil, "out": strconv.FormatInt(containerMemoryLimit(), 10)})
	if err := lines.Write(dir, "memory_container.jsonl"); err != nil {
		t.Fatal(err)
	}
}
