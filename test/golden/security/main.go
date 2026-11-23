package main

import (
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"regexp"
	"sort"
	"strings"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"github.com/valyala/fasthttp"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	out := ""
	for i, a := range os.Args {
		if a == "-out" && i+1 < len(os.Args) {
			out = os.Args[i+1]
		}
	}
	if out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	log.SetOutput(io.Discard, false)
	globals.VERSION = "golden"
	for _, step := range []func(string) error{genACL, genDTO, genCLI, genHTTP} {
		if err := step(out); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
}

var tmpBase string

func openStore(mode string) string {
	if tmpBase == "" {
		tmpBase, _ = os.MkdirTemp("", "elysiandb-golden-security-")
	}
	dir, _ := os.MkdirTemp(tmpBase, "store-")
	cfg := &configuration.Config{}
	cfg.Store.Folder = dir
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = mode != ""
	cfg.Security.Authentication.Mode = mode
	if mode == "token" {
		cfg.Security.Authentication.Token = "golden-token"
	}
	cfg.Server.HTTP.CORS.AllowedOrigins = []string{"http://allowed.test"}
	cfg.Server.HTTP.TrustedProxies = []string{"10.0.0.0/8"}
	cfg.Api.Schema.Enabled = true
	cfg.Api.Hooks.Enabled = true
	cfg.Stats.Enabled = true
	cfg.Log.Level = "error"
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
	security.ResetLoginAttempts()
	_ = security.InitAdminUserIfNotExists()
	_ = acl.InitACL()
	hook.InitHooks()
	return dir
}

func closeStore() {
	_ = storage.Close()
}

func genACL(out string) error {
	var l goldenio.Lines
	for _, role := range []security.Role{security.RoleAdmin, security.RoleUser, security.Role("auditor")} {
		perms := acl.DefaultPermissionsForRole(role)
		a := acl.ACL{Username: "u", Entity: "book", Permissions: perms}
		b, err := json.Marshal(a)
		if err != nil {
			return err
		}
		l.Add(map[string]any{"fn": "default", "role": string(role), "acl_json": string(b)})
	}
	openStore("user")
	list := acl.GenerateACLFoAllrEntities()
	b, _ := json.Marshal(list)
	l.Add(map[string]any{"fn": "GenerateACLFoAllrEntities", "zero_types": string(b)})
	_ = security.CreateBasicUser(&security.BasicUser{Username: "ann", Password: "secret-1", Role: security.RoleUser})
	entity := acl.GetACLEntityForUsername("book", "ann")
	eb, _ := json.Marshal(entity)
	l.Add(map[string]any{"fn": "GetACLEntityForUsername", "entity": "book", "username": "ann", "acl_json": string(eb)})
	closeStore()
	return l.Write(out, "acl.jsonl")
}

type loginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

type userDto struct {
	Username string        `json:"username"`
	Password string        `json:"password"`
	Role     security.Role `json:"role"`
}

type passwordDto struct {
	Password *string `json:"password"`
}

type userRoleDto struct {
	Role string `json:"role"`
}

type aclPayload struct {
	Permissions map[string]bool `json:"permissions"`
}

func genDTO(out string) error {
	var l goldenio.Lines
	bodies := []string{`{"username":"a","password":"b"}`, `{"USERNAME":"a","Password":"b","role":"admin"}`, `{"username":5}`, `{"password":null}`, `{}`, `null`, `[]`, `{"username":"a"} x`, `{"username":`, ``, `{"role":"admin","extra":1}`, `{"permissions":{"read":true,"create":"yes"}}`, `{"permissions":{"read":true}}`, `{"password":""}`, `{"username":"\u00e9","password":"\ud800"}`}
	targets := map[string]func() any{"loginRequest": func() any { return &loginRequest{} }, "UserDto": func() any { return &userDto{} }, "passwordDto": func() any { return &passwordDto{} }, "userRoleDto": func() any { return &userRoleDto{} }, "aclPayload": func() any { return &aclPayload{} }}
	names := make([]string, 0, len(targets))
	for k := range targets {
		names = append(names, k)
	}
	sort.Strings(names)
	for _, name := range names {
		for _, body := range bodies {
			v := targets[name]()
			row := map[string]any{"type": name, "body_hex": hex.EncodeToString([]byte(body))}
			if err := json.Unmarshal([]byte(body), v); err != nil {
				row["error"] = true
			} else {
				b, _ := json.Marshal(v)
				row["decoded"] = string(b)
				if p, ok := v.(*passwordDto); ok {
					row["password_set"] = p.Password != nil
				}
			}
			l.Add(row)
		}
	}
	return l.Write(out, "dto_decode.jsonl")
}

func withStdin(input string, fn func()) string {
	r, w, _ := os.Pipe()
	or, ow, _ := os.Pipe()
	saved := os.Stdin
	savedOut := os.Stdout
	os.Stdin = r
	os.Stdout = ow
	go func() {
		_, _ = w.Write([]byte(input))
		_ = w.Close()
	}()
	done := make(chan string)
	go func() {
		b, _ := io.ReadAll(or)
		done <- string(b)
	}()
	fn()
	os.Stdin = saved
	os.Stdout = savedOut
	_ = ow.Close()
	printed := <-done
	_ = r.Close()
	return printed
}

func genCLI(out string) error {
	var l goldenio.Lines
	var stdout strings.Builder
	cmd.Printf = func(format string, a ...any) (int, error) {
		return fmt.Fprintf(&stdout, format, a...)
	}
	cases := []struct {
		mode      string
		command   string
		stdin     string
		passwords []string
	}{
		{"", "create-user", "ann\nuser\n", []string{"secret-1", "secret-1"}},
		{"token", "create-user", "ann\nuser\n", []string{"secret-1", "secret-1"}},
		{"basic", "create-user", "ann\nuser\n", []string{"secret-1", "secret-1"}},
		{"user", "create-user", "ann\nadmin\n", []string{"secret-1", "secret-1"}},
		{"user", "create-user", "bob\nuser\n", []string{"secret-1", "different"}},
		{"user", "create-user", "carl\nuser\n", []string{"short", "short"}},
		{"user", "create-user", "a b\nuser\n", []string{"secret-1", "secret-1"}},
		{"user", "change-password", "admin\n", []string{"new-secret", "new-secret"}},
		{"user", "change-password", "ghost\n", []string{"new-secret", "new-secret"}},
		{"user", "delete-user", "ghost\n", nil},
		{"user", "delete-user", "admin\n", nil},
		{"basic", "delete-user", "admin\n", nil},
	}
	for _, c := range cases {
		openStore(c.mode)
		stdout.Reset()
		pw := append([]string(nil), c.passwords...)
		cmd.ReadPassword = func(int) ([]byte, error) {
			if len(pw) == 0 {
				return nil, io.EOF
			}
			p := pw[0]
			pw = pw[1:]
			return []byte(p), nil
		}
		printed := withStdin(c.stdin, func() {
			switch c.command {
			case "create-user":
				cmd.CreateUser()
			case "change-password":
				cmd.ChangePassword()
			case "delete-user":
				cmd.DeleteUser()
			}
		})
		users, _ := security.ListBasicUsers()
		var names []string
		for _, u := range users {
			if n, ok := u["username"].(string); ok {
				names = append(names, n)
			}
		}
		sort.Strings(names)
		l.Add(map[string]any{"mode": c.mode, "command": c.command, "stdin_hex": hex.EncodeToString([]byte(c.stdin)), "passwords": c.passwords, "stdout_hex": hex.EncodeToString([]byte(printed)), "printf_hex": hex.EncodeToString([]byte(stdout.String())), "users": names})
		closeStore()
	}
	return l.Write(out, "cli.jsonl")
}

var (
	uuidRe = regexp.MustCompile(`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`)
	ridRe  = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-z]+$`)
	sidRe  = regexp.MustCompile(`edb_session=[0-9a-f]+`)
	timeRe = regexp.MustCompile(`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})`)
)

func mask(s string) string {
	if cfg := globals.GetConfig(); cfg != nil && cfg.Store.Folder != "" {
		s = strings.ReplaceAll(s, cfg.Store.Folder, "<DATA>")
	}
	s = uuidRe.ReplaceAllString(s, "<uuid>")
	s = sidRe.ReplaceAllString(s, "edb_session=<SID>")
	s = timeRe.ReplaceAllString(s, "<TIME>")
	return s
}

type httpCase struct {
	name    string
	method  string
	path    string
	headers []string
	body    string
	peer    string
}

func genHTTP(out string) error {
	var l goldenio.Lines
	basic := func(u, p string) string {
		return "Authorization: Basic " + base64.StdEncoding.EncodeToString([]byte(u+":"+p))
	}
	common := []httpCase{
		{name: "health", method: "GET", path: "/health"},
		{name: "list-no-credentials", method: "GET", path: "/api/book"},
		{name: "kv-no-credentials", method: "GET", path: "/kv/x"},
		{name: "options-allowed", method: "OPTIONS", path: "/api/book", headers: []string{"Origin: http://allowed.test", "Access-Control-Request-Method: POST"}},
		{name: "options-other", method: "OPTIONS", path: "/api/book", headers: []string{"Origin: http://evil.test", "Access-Control-Request-Method: POST"}},
		{name: "cross-site-write", method: "POST", path: "/api/book", headers: []string{"Origin: http://evil.test", "Content-Type: application/json"}, body: `{"id":"x"}`},
		{name: "same-origin-write", method: "POST", path: "/api/book", headers: []string{"Origin: http://h", "Sec-Fetch-Site: same-origin", "Content-Type: application/json"}, body: `{"id":"y"}`},
		{name: "cross-site-fetch-meta", method: "DELETE", path: "/api/book/x", headers: []string{"Sec-Fetch-Site: cross-site"}},
	}
	modes := map[string][]httpCase{
		"": append(append([]httpCase{}, common...), httpCase{name: "config", method: "GET", path: "/config"}),
		"basic": append(append([]httpCase{}, common...),
			httpCase{name: "basic-ok", method: "GET", path: "/api/book", headers: []string{basic("admin", "admin")}},
			httpCase{name: "basic-remote-default-admin", method: "GET", path: "/api/book", headers: []string{basic("admin", "admin")}, peer: "203.0.113.5"},
			httpCase{name: "basic-wrong", method: "GET", path: "/api/book", headers: []string{basic("admin", "nope")}},
			httpCase{name: "basic-admin-route", method: "GET", path: "/config", headers: []string{basic("admin", "admin")}},
		),
		"token": append(append([]httpCase{}, common...),
			httpCase{name: "token-ok", method: "GET", path: "/api/book", headers: []string{"Authorization: Bearer golden-token"}},
			httpCase{name: "token-wrong", method: "GET", path: "/api/book", headers: []string{"Authorization: Bearer nope"}},
			httpCase{name: "token-padded", method: "GET", path: "/api/book", headers: []string{"Authorization: Bearer  golden-token "}},
		),
		"user": append(append([]httpCase{}, common...),
			httpCase{name: "login", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"admin","password":"admin"}`},
			httpCase{name: "login-remote-default", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"admin","password":"admin"}`, peer: "203.0.113.5"},
			httpCase{name: "login-forwarded-remote", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json", "X-Forwarded-For: 203.0.113.9"}, body: `{"username":"admin","password":"admin"}`, peer: "10.1.1.1"},
			httpCase{name: "login-wrong-1", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "login-wrong-2", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "login-wrong-3", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "login-wrong-4", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "login-wrong-5", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "login-locked", method: "POST", path: "/api/security/login", headers: []string{"Content-Type: application/json"}, body: `{"username":"ann","password":"x"}`},
			httpCase{name: "me-no-session", method: "GET", path: "/api/security/me"},
			httpCase{name: "me-bad-session", method: "GET", path: "/api/security/me", headers: []string{"Cookie: edb_session=00"}},
		),
	}
	for _, mode := range []string{"", "basic", "token", "user"} {
		openStore(mode)
		handler := boot.NewHTTPHandler(boot.NewRouter())
		for _, c := range modes[mode] {
			req := fasthttp.AcquireRequest()
			req.SetRequestURI(c.path)
			req.Header.SetMethod(c.method)
			req.Header.SetHost("h")
			for _, h := range c.headers {
				k, v, _ := strings.Cut(h, ": ")
				req.Header.Set(k, v)
			}
			if c.body != "" {
				req.SetBodyString(c.body)
			}
			peer := c.peer
			if peer == "" {
				peer = "127.0.0.1"
			}
			ctx := &fasthttp.RequestCtx{}
			ctx.Init(req, &net.TCPAddr{IP: net.ParseIP(peer), Port: 40000}, nil)
			handler(ctx)
			headers := map[string]string{}
			for _, k := range []string{"Content-Type", "Set-Cookie", "Retry-After", "Access-Control-Allow-Origin", "Access-Control-Allow-Methods", "Access-Control-Allow-Headers", "Access-Control-Allow-Credentials", "Vary", "Allow", "X-Content-Type-Options", "X-Frame-Options", "Referrer-Policy", "Content-Security-Policy", "Cache-Control", "X-Elysian-Version"} {
				if v := ctx.Response.Header.Peek(k); len(v) > 0 {
					headers[k] = mask(string(v))
				}
			}
			rid := string(ctx.Response.Header.Peek("X-Request-Id"))
			if ridRe.MatchString(rid) {
				rid = "<RID>"
			}
			headers["X-Request-Id"] = rid
			l.Add(map[string]any{"mode": mode, "name": c.name, "method": c.method, "path": c.path, "request_headers": c.headers, "body": c.body, "peer": peer, "status": ctx.Response.StatusCode(), "headers": headers, "response_body": mask(string(ctx.Response.Body()))})
		}
		closeStore()
	}
	if err := l.Write(out, "http.jsonl"); err != nil {
		return err
	}
	if tmpBase != "" {
		_ = os.RemoveAll(tmpBase)
	}
	return nil
}
