package main

import (
	"bytes"
	"compress/gzip"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"golang.org/x/term"
	"gopkg.in/yaml.v2"
)

type cliCase struct {
	name  string
	args  []string
	stdin string
	env   []string
	setup func(dir string)
	extra map[string]string
}

var (
	cliTimeRe        = regexp.MustCompile(`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})`)
	cliStampRe       = regexp.MustCompile(`\d{8}-\d{6}|\d{8}T\d{6}Z?`)
	cliPortRe        = regexp.MustCompile(`127\.0\.0\.1:\d+`)
	cliBackupBytesRe = regexp.MustCompile(`(msg="backup written"[^\n]*? bytes=)\d+`)
	cliDurRe         = regexp.MustCompile(`(duration_ms|startup_ms|took_ms|estimated_seconds)=[0-9.]+`)
)

func goTexts(bin string, args []string, dir string) map[string]string {
	texts := map[string]string{}
	flags := flag.NewFlagSet(bin, flag.ContinueOnError)
	var usage bytes.Buffer
	flags.SetOutput(&usage)
	flags.String("config", "elysian.yaml", "Path to configuration file")
	if err := flags.Parse(args); err != nil && !errors.Is(err, flag.ErrHelp) {
		texts[err.Error()] = "\u27e8go:flag\u27e9"
	}
	usage.Reset()
	flags.Usage()
	if u := usage.String(); u != "" {
		texts[u] = "\u27e8go:usage\u27e9"
	}
	r, w, err := os.Pipe()
	if err == nil {
		if _, perr := term.ReadPassword(int(r.Fd())); perr != nil {
			texts[perr.Error()] = "\u27e8go:os\u27e9"
		}
		r.Close()
		w.Close()
	}
	cfgPath := ""
	for i, a := range args {
		if (a == "-config" || a == "--config") && i+1 < len(args) {
			cfgPath = args[i+1]
		}
		if v, ok := strings.CutPrefix(a, "-config="); ok {
			cfgPath = v
		}
		if v, ok := strings.CutPrefix(a, "--config="); ok {
			cfgPath = v
		}
	}
	if cfgPath != "" {
		if _, err := configuration.LoadConfig(filepath.Join(dir, cfgPath)); err != nil {
			var inner error = err
			for errors.Unwrap(inner) != nil {
				inner = errors.Unwrap(inner)
			}
			if inner != err {
				class := "\u27e8go:yaml-syntax\u27e9"
				var pe *fs.PathError
				var te *yaml.TypeError
				switch {
				case errors.As(err, &pe):
					class = "\u27e8go:os\u27e9"
				case errors.As(err, &te):
					class = "\u27e8go:yaml-type\u27e9"
				}
				texts[inner.Error()] = class
			}
		}
	}
	if data, err := os.ReadFile(filepath.Join(dir, "garbage.tar.gz")); err == nil {
		if _, gerr := gzip.NewReader(bytes.NewReader(data)); gerr != nil {
			texts[gerr.Error()] = "\u27e8go:gzip\u27e9"
		}
	}
	for _, p := range []string{"missing-folder", "no-such.tar.gz", "archive.tar.gz", "not-a-folder"} {
		if _, err := os.Stat(filepath.Join(dir, p)); err != nil {
			texts[err.Error()] = "\u27e8go:os\u27e9"
			texts[strings.ReplaceAll(err.Error(), dir+"/", "")] = "\u27e8go:os\u27e9"
		}
	}
	return texts
}

func maskCLI(s, bin, dir string, texts map[string]string) string {
	keys := make([]string, 0, len(texts))
	for k := range texts {
		keys = append(keys, k)
	}
	sort.Slice(keys, func(i, j int) bool { return len(keys[i]) > len(keys[j]) })
	for _, k := range keys {
		if k != "" {
			s = strings.ReplaceAll(s, k, texts[k])
		}
	}
	s = strings.ReplaceAll(s, bin, "<BIN>")
	s = strings.ReplaceAll(s, dir, "<DIR>")
	s = cliTimeRe.ReplaceAllString(s, "<T>")
	s = cliStampRe.ReplaceAllString(s, "<STAMP>")
	s = cliPortRe.ReplaceAllString(s, "127.0.0.1:<PORT>")
	s = cliDurRe.ReplaceAllString(s, "$1=<N>")
	s = cliBackupBytesRe.ReplaceAllString(s, "${1}<N>")
	return s
}

func freePort() int {
	ln, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return 18089
	}
	defer ln.Close()
	return ln.Addr().(*net.TCPAddr).Port
}

func cliConfig(mode string, port int) string {
	auth := "    enabled: false\n"
	switch mode {
	case "token":
		auth = "    enabled: true\n    mode: token\n    token: cli-token\n"
	case "user":
		auth = "    enabled: true\n    mode: user\n"
	case "basic":
		auth = "    enabled: true\n    mode: basic\n"
	}
	return fmt.Sprintf("store:\n  folder: data\n  shards: 4\nserver:\n  http: { enabled: true, host: 127.0.0.1, port: %d }\n  tcp: { enabled: false, host: 127.0.0.1, port: 1 }\nlog:\n  level: info\n  format: text\nsecurity:\n  authentication:\n%s", port, auth)
}

func genCLI(out string) error {
	base, err := os.MkdirTemp("", "elysiandb-golden-cli-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(base)
	bin := filepath.Join(base, "elysiandb")
	build := exec.Command("go", "build", "-ldflags", "-X github.com/taymour/elysiandb/internal/globals.VERSION=0.3.0", "-o", bin, ".")
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		return err
	}
	archive, err := os.ReadFile(filepath.Join("test", "compat", "testdata", "archives", "v0.2.4.tar.gz"))
	if err != nil {
		return err
	}
	cases := []cliCase{
		{name: "help -h", args: []string{"-h"}},
		{name: "help -help", args: []string{"-help"}},
		{name: "help --help", args: []string{"--help"}},
		{name: "unknown flag", args: []string{"-x"}},
		{name: "missing value", args: []string{"-config"}},
		{name: "unknown command", args: []string{"-config", "c.yaml", "frobnicate"}},
		{name: "help command", args: []string{"-config", "c.yaml", "help"}},
		{name: "missing config", args: []string{"-config", "absent.yaml", "help"}},
		{name: "missing config server", args: []string{"-config", "absent.yaml"}},
		{name: "invalid yaml", args: []string{"-config", "bad.yaml", "reset"}, setup: func(d string) { os.WriteFile(filepath.Join(d, "bad.yaml"), []byte("store: [x\n"), 0o644) }},
		{name: "validation error", args: []string{"-config", "v.yaml", "reset"}, setup: func(d string) {
			os.WriteFile(filepath.Join(d, "v.yaml"), []byte("log:\n  level: loud\n"), 0o644)
		}},
		{name: "config equals form", args: []string{"--config=c.yaml", "help"}},
		{name: "reset no", args: []string{"-config", "c.yaml", "reset"}, stdin: "no\n"},
		{name: "reset yes", args: []string{"-config", "c.yaml", "reset"}, stdin: "yes\n"},
		{name: "reset force", args: []string{"-config", "c.yaml", "reset", "--force"}},
		{name: "create-user off", args: []string{"-config", "c.yaml", "create-user"}, stdin: "ann\n2\n"},
		{name: "create-user user mode", args: []string{"-config", "u.yaml", "create-user"}, stdin: "ann\n2\nsecret-1\nsecret-1\n"},
		{name: "change-password user mode", args: []string{"-config", "u.yaml", "change-password"}, stdin: "admin\nsecret-2\nsecret-2\n"},
		{name: "delete-user user mode", args: []string{"-config", "u.yaml", "delete-user"}, stdin: "ghost\n"},
		{name: "backup local", args: []string{"-config", "c.yaml", "backup", "--output", "out"}},
		{name: "backup bad arg", args: []string{"-config", "c.yaml", "backup", "--nope"}},
		{name: "backup output missing value", args: []string{"-config", "c.yaml", "backup", "--output"}},
		{name: "backup missing folder", args: []string{"-config", "c.yaml", "backup", "--output", "missing-folder"}},
		{name: "backup not a folder", args: []string{"-config", "c.yaml", "backup", "--output", "c.yaml"}},
		{name: "restore abort", args: []string{"-config", "c.yaml", "restore", "archive.tar.gz"}, stdin: "no\n"},
		{name: "restore force", args: []string{"-config", "c.yaml", "restore", "archive.tar.gz", "--force"}},
		{name: "restore missing file", args: []string{"-config", "c.yaml", "restore", "no-such.tar.gz", "--force"}},
		{name: "restore not gzip", args: []string{"-config", "c.yaml", "restore", "garbage.tar.gz", "--force"}},
		{name: "restore no args", args: []string{"-config", "c.yaml", "restore"}},
		{name: "export-legacy bad args", args: []string{"-config", "c.yaml", "export-legacy"}},
		{name: "export-legacy non-empty", args: []string{"-config", "c.yaml", "export-legacy", "--output", "out"}},
		{name: "export-legacy ok", args: []string{"-config", "c.yaml", "export-legacy", "--output", "legacy-out"}},
	}
	var l goldenio.Lines
	for i, c := range cases {
		dir := filepath.Join(base, fmt.Sprintf("case%02d", i))
		if err := os.MkdirAll(filepath.Join(dir, "out"), 0o755); err != nil {
			return err
		}
		os.WriteFile(filepath.Join(dir, "out", "keep"), []byte("x"), 0o644)
		os.WriteFile(filepath.Join(dir, "c.yaml"), []byte(cliConfig("", 1)), 0o644)
		os.WriteFile(filepath.Join(dir, "u.yaml"), []byte(cliConfig("user", 1)), 0o644)
		os.WriteFile(filepath.Join(dir, "archive.tar.gz"), archive, 0o644)
		os.WriteFile(filepath.Join(dir, "garbage.tar.gz"), []byte("not a gzip stream"), 0o644)
		if c.setup != nil {
			c.setup(dir)
		}
		row, err := runCLI(bin, dir, c)
		if err != nil {
			return err
		}
		l.Add(row)
	}
	if err := invalidRestoreCases(bin, base, out, &l); err != nil {
		return err
	}
	if err := remoteBackupCases(bin, base, &l); err != nil {
		return err
	}
	return l.Write(out, "cli/cases.jsonl")
}

func invalidRestoreCases(bin, base, out string, l *goldenio.Lines) error {
	folder := filepath.Join(out, "archives", "invalid")
	archives, err := filepath.Glob(filepath.Join(folder, "*.tar.gz"))
	if err != nil {
		return err
	}
	if len(archives) == 0 {
		return fmt.Errorf("no invalid archive in %s: run the archives step first", folder)
	}
	sort.Strings(archives)
	ordered := map[string]string{}
	expected, err := os.ReadFile(filepath.Join(out, "archives", "expected.jsonl"))
	if err != nil {
		return err
	}
	for _, line := range strings.Split(strings.TrimSpace(string(expected)), "\n") {
		var row struct {
			File   string  `json:"file"`
			Reason *string `json:"reason"`
			Order  string  `json:"order"`
		}
		if err := json.Unmarshal([]byte(line), &row); err != nil {
			return err
		}
		if row.Order != "" && row.Reason != nil {
			ordered["archives/"+row.File] = *row.Reason
		}
	}
	for i, p := range archives {
		data, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		dir := filepath.Join(base, fmt.Sprintf("invalid%02d", i))
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
		os.WriteFile(filepath.Join(dir, "c.yaml"), []byte(cliConfig("", 1)), 0o644)
		os.WriteFile(filepath.Join(dir, "archive.tar.gz"), data, 0o644)
		texts, _ := archReplay(data)
		extra := map[string]string{}
		for k, v := range texts {
			extra[k] = archMarker(v)
		}
		slug := strings.TrimSuffix(filepath.Base(p), ".tar.gz")
		archive := "archives/invalid/" + slug + ".tar.gz"
		c := cliCase{name: "restore invalid " + slug, args: []string{"-config", "c.yaml", "restore", "archive.tar.gz", "--force"}, extra: extra}
		row, err := runCLI(bin, dir, c)
		if err != nil {
			return err
		}
		if want, ok := ordered[archive]; ok {
			for try := 0; !strings.Contains(row["stdout"].(string), want) && try < 500; try++ {
				if row, err = runCLI(bin, dir, c); err != nil {
					return err
				}
			}
			if !strings.Contains(row["stdout"].(string), want) {
				return fmt.Errorf("%s: no sorted pick", archive)
			}
			row["order"] = "first unknown path in sorted order (D20)"
		}
		row["archive"] = archive
		l.Add(row)
	}
	return nil
}

func runCLI(bin, dir string, c cliCase) (map[string]any, error) {
	cmd := exec.Command(bin, c.args...)
	cmd.Dir = dir
	cmd.Env = append([]string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "TZ=UTC", "NO_COLOR=1"}, c.env...)
	cmd.Stdin = strings.NewReader(c.stdin)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err := cmd.Run()
	exit := 0
	if err != nil {
		var ee *exec.ExitError
		if !errors.As(err, &ee) {
			return nil, err
		}
		exit = ee.ExitCode()
	}
	texts := goTexts(bin, c.args, dir)
	for k, v := range c.extra {
		texts[k] = v
	}
	maskedArgs := make([]string, len(c.args))
	for i, a := range c.args {
		maskedArgs[i] = cliPortRe.ReplaceAllString(a, "127.0.0.1:<PORT>")
	}
	return map[string]any{"name": c.name, "args": maskedArgs, "stdin": c.stdin, "env": c.env, "stdout": maskCLI(stdout.String(), bin, dir, texts), "stderr": maskCLI(stderr.String(), bin, dir, texts), "exit": exit}, nil
}

func remoteBackupCases(bin, base string, l *goldenio.Lines) error {
	for _, mode := range []string{"off", "token", "user", "basic"} {
		dir := filepath.Join(base, "remote-"+mode)
		if err := os.MkdirAll(filepath.Join(dir, "out"), 0o755); err != nil {
			return err
		}
		port := freePort()
		os.WriteFile(filepath.Join(dir, "s.yaml"), []byte(cliConfig(mode, port)), 0o644)
		server := exec.Command(bin, "-config", "s.yaml")
		server.Dir = dir
		server.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "TZ=UTC"}
		if err := server.Start(); err != nil {
			return err
		}
		url := "http://127.0.0.1:" + strconv.Itoa(port)
		for i := 0; i < 100; i++ {
			if conn, err := net.Dial("tcp4", "127.0.0.1:"+strconv.Itoa(port)); err == nil {
				conn.Close()
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		time.Sleep(300 * time.Millisecond)
		var cases []cliCase
		switch mode {
		case "off":
			cases = append(cases, cliCase{name: "remote off", args: []string{"-config", "s.yaml", "backup", "--url", url, "--output", "out"}})
		case "token":
			cases = append(cases, cliCase{name: "remote token", args: []string{"-config", "s.yaml", "backup", "--url", url, "--token", "cli-token", "--output", "out"}},
				cliCase{name: "remote token wrong", args: []string{"-config", "s.yaml", "backup", "--url", url, "--token", "nope", "--output", "out"}})
		case "user":
			cases = append(cases, cliCase{name: "remote user env password", args: []string{"-config", "s.yaml", "backup", "--url", url, "--user", "admin", "--output", "out"}, env: []string{"ELYSIANDB_PASSWORD=admin"}},
				cliCase{name: "remote user wrong password", args: []string{"-config", "s.yaml", "backup", "--url", url, "--user", "admin", "--output", "out"}, env: []string{"ELYSIANDB_PASSWORD=nope"}})
		case "basic":
			cases = append(cases, cliCase{name: "remote basic", args: []string{"-config", "s.yaml", "backup", "--url", url, "--user", "admin", "--output", "out"}, env: []string{"ELYSIANDB_PASSWORD=admin"}})
		}
		for _, c := range cases {
			row, err := runCLI(bin, dir, c)
			if err != nil {
				server.Process.Kill()
				return err
			}
			l.Add(row)
		}
		server.Process.Signal(os.Interrupt)
		done := make(chan error, 1)
		go func() { done <- server.Wait() }()
		select {
		case <-done:
		case <-time.After(10 * time.Second):
			server.Process.Kill()
			<-done
		}
	}
	dir := filepath.Join(base, "remote-refused")
	os.MkdirAll(filepath.Join(dir, "out"), 0o755)
	os.WriteFile(filepath.Join(dir, "s.yaml"), []byte(cliConfig("off", 1)), 0o644)
	refused := "http://127.0.0.1:" + strconv.Itoa(freePort())
	extra := map[string]string{}
	if _, herr := http.Get(refused + "/api/backup"); herr != nil {
		extra[herr.Error()] = "\u27e8go:http-client\u27e9"
	}
	row, err := runCLI(bin, dir, cliCase{name: "remote refused", args: []string{"-config", "s.yaml", "backup", "--url", refused, "--output", "out"}, extra: extra})
	if err != nil {
		return err
	}
	l.Add(row)
	return nil
}
