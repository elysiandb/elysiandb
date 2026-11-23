package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

const pinnedCommit = "cb4a6c8074671c00df8cbc17a620c0f9462b312a"

func main() {
	out := flag.String("out", "", "tests/golden/hooks")
	checkout := flag.String("test262", "", "test262 checkout at the commit pinned by goja")
	flag.Parse()
	if *out == "" || *checkout == "" {
		fmt.Fprintln(os.Stderr, "missing -out or -test262")
		os.Exit(2)
	}
	if err := run(*out, *checkout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func command(dir string, name string, args ...string) *exec.Cmd {
	c := exec.Command(name, args...)
	c.Dir = dir
	c.Env = append(os.Environ(), "GOWORK=off", "GOFLAGS=-mod=mod", "GOPROXY=off", "GOSUMDB=off")
	return c
}

func run(out, checkout string) error {
	head, err := command(checkout, "git", "rev-parse", "HEAD").Output()
	if err != nil {
		return err
	}
	if strings.TrimSpace(string(head)) != pinnedCommit {
		return fmt.Errorf("test262 checkout is at %s, want %s", strings.TrimSpace(string(head)), pinnedCommit)
	}
	modDir, err := exec.Command("go", "list", "-m", "-f", "{{.Dir}}", "github.com/dop251/goja").Output()
	if err != nil {
		return err
	}
	version, err := exec.Command("go", "list", "-m", "-f", "{{.Version}}", "github.com/dop251/goja").Output()
	if err != nil {
		return err
	}
	src := strings.TrimSpace(string(modDir))
	work, err := os.MkdirTemp("", "elysiandb-golden-test262-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	if err := copyTree(src, work); err != nil {
		return err
	}
	sums, err := os.ReadFile("go.sum")
	if err != nil {
		return err
	}
	own, err := os.ReadFile(filepath.Join(work, "go.sum"))
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(work, "go.sum"), append(own, sums...), 0o644); err != nil {
		return err
	}
	pinned := map[string]string{}
	for _, m := range []string{"golang.org/x/text", "github.com/dlclark/regexp2", "github.com/go-sourcemap/sourcemap", "github.com/google/pprof", "gopkg.in/yaml.v2"} {
		v, err := exec.Command("go", "list", "-m", "-f", "{{.Version}}", m).Output()
		if err != nil {
			return err
		}
		pinned[m] = strings.TrimSpace(string(v))
		if err := command(work, "go", "mod", "edit", "-require="+m+"@"+pinned[m]).Run(); err != nil {
			return err
		}
	}
	for _, m := range []string{"github.com/dop251/goja_nodejs", "github.com/kr/pretty"} {
		if err := command(work, "go", "mod", "edit", "-droprequire="+m).Run(); err != nil {
			return err
		}
	}
	abs, err := filepath.Abs(checkout)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(work, "testdata"), 0o755); err != nil {
		return err
	}
	if err := os.Symlink(abs, filepath.Join(work, "testdata", "test262")); err != nil {
		return err
	}
	cmd := command(work, "go", "test", "-vet=off", "-run", "^TestTC39$", "-count=1", "-v", ".")
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		return err
	}
	var passed, skipped, failed []string
	sc := bufio.NewScanner(stdout)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		for prefix, list := range map[string]*[]string{"--- PASS: TestTC39/tc39/": &passed, "--- SKIP: TestTC39/tc39/": &skipped, "--- FAIL: TestTC39/tc39/": &failed} {
			if rest, ok := strings.CutPrefix(line, prefix); ok {
				name, _, _ := strings.Cut(rest, " ")
				if strings.HasSuffix(name, ".js") {
					*list = append(*list, name)
				}
			}
		}
	}
	waitErr := cmd.Wait()
	sort.Strings(passed)
	if len(passed) == 0 {
		return fmt.Errorf("no passing test262 file found (go test: %v)", waitErr)
	}
	var b bytes.Buffer
	for _, p := range passed {
		b.WriteString(p + "\n")
	}
	if err := goldenio.WriteFile(out, "test262-pass.tsv", b.Bytes()); err != nil {
		return err
	}
	return goldenio.WriteJSON(out, "test262.json", map[string]any{"test262_commit": pinnedCommit, "goja_version": strings.TrimSpace(string(version)), "dependencies": pinned, "passed": len(passed), "skipped": len(skipped), "failed": len(failed), "go_test_failed": waitErr != nil})
}

func copyTree(src, dst string) error {
	return filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)
		if d.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		in, err := os.Open(path)
		if err != nil {
			return err
		}
		defer in.Close()
		outFile, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
		if err != nil {
			return err
		}
		if _, err := io.Copy(outFile, in); err != nil {
			outFile.Close()
			return err
		}
		return outFile.Close()
	})
}
