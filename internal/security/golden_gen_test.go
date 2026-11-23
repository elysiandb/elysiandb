package security

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"github.com/valyala/fasthttp"
	"golang.org/x/crypto/bcrypt"
)

type countingReader struct {
	n uint64
}

func (r *countingReader) Read(p []byte) (int, error) {
	for i := 0; i < len(p); i += 32 {
		var seed [8]byte
		binary.LittleEndian.PutUint64(seed[:], r.n)
		r.n++
		sum := sha256.Sum256(seed[:])
		copy(p[i:], sum[:])
	}
	return len(p), nil
}

func securityOut(t *testing.T) string {
	t.Helper()
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	dir := filepath.Join(root, "security")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	return dir
}

func hx(b []byte) string { return hex.EncodeToString(b) }

func write(t *testing.T, dir, name string, l *goldenio.Lines) {
	t.Helper()
	if err := l.Write(dir, name); err != nil {
		t.Fatal(err)
	}
}

func securityStore(t *testing.T, mode string) string {
	t.Helper()
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = mode != ""
	cfg.Security.Authentication.Mode = mode
	cfg.Server.HTTP.CORS.AllowedOrigins = []string{" http://allowed.test/ ", "HTTPS://Upper.Test", "http://\u212aelvin.test"}
	cfg.Server.HTTP.TrustedProxies = []string{"10.0.0.0/8", "192.168.1.1", "::1/128"}
	cfg.Log.Level = "error"
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
	ResetLoginAttempts()
	return cfg.Store.Folder
}

func TestGenerateSecurityGolden(t *testing.T) {
	dir := securityOut(t)
	saved := rand.Reader
	rand.Reader = &countingReader{}
	defer func() { rand.Reader = saved }()

	key := strings.Repeat("ab", 512)
	var bl goldenio.Lines
	passwords := []string{"", "admin", "secret-1", "\u00e9t\u00e9\u20ac\U0001f600\u4e2d\u6587\u0391\u03a9", "\xff\xfe\x00abc", strings.Repeat("p", 100), "a b", "\x00"}
	for i := 0; len(passwords) < 72; i++ {
		passwords = append(passwords, fmt.Sprintf("gen-%d", i))
	}
	var zeroFirst, zeroLater int
	for i := 0; zeroFirst < 8 || zeroLater < 8; i++ {
		p := fmt.Sprintf("zero-%d", i)
		sum := sha256.Sum256([]byte(p + key))
		switch {
		case sum[0] == 0 && zeroFirst < 8:
			zeroFirst++
			passwords = append(passwords, p)
		case sum[0] != 0 && bytes.IndexByte(sum[:], 0) > 0 && zeroLater < 8:
			zeroLater++
			passwords = append(passwords, p)
		}
	}
	for _, p := range passwords {
		digest := sha256.Sum256([]byte(p + key))
		hash, err := bcrypt.GenerateFromPassword(digest[:], bcrypt.DefaultCost)
		if err != nil {
			t.Fatal(err)
		}
		negative := sha256.Sum256([]byte(p + key + "x"))
		bl.Add(map[string]any{
			"key": "ab*512", "password_hex": hx([]byte(p)), "digest_hex": hx(digest[:]), "hash": string(hash),
			"verifies":            bcrypt.CompareHashAndPassword(hash, digest[:]) == nil,
			"negative_digest_hex": hx(negative[:]), "verifies_negative": bcrypt.CompareHashAndPassword(hash, negative[:]) == nil,
		})
	}
	write(t, dir, "bcrypt.jsonl", &bl)

	var bp goldenio.Lines
	digest := sha256.Sum256([]byte("admin" + key))
	base, _ := bcrypt.GenerateFromPassword(digest[:], bcrypt.DefaultCost)
	b := string(base)
	variants := map[string]string{
		"orig": b, "2": "$2$" + b[4:], "2b": "$2b$" + b[4:], "2x": "$2x$" + b[4:], "2y": "$2y$" + b[4:], "3a": "$3a$" + b[4:],
		"cost03": b[:4] + "03" + b[6:], "cost04": b[:4] + "04" + b[6:], "cost31": b[:4] + "31" + b[6:], "cost32": b[:4] + "32" + b[6:],
		"trunc58": b[:58], "trunc59": b[:59], "alter-salt": b[:10] + "A" + b[11:], "alter-hash": b[:len(b)-1] + "e", "empty": "", "garbage": "not a hash",
	}
	for _, name := range []string{"orig", "2", "2b", "2x", "2y", "3a", "cost03", "cost04", "cost31", "cost32", "trunc58", "trunc59", "alter-salt", "alter-hash", "empty", "garbage"} {
		if name == "cost31" {
			cost, err := bcrypt.Cost([]byte(variants[name]))
			bp.Add(map[string]any{"name": name, "hash": variants[name], "digest_hex": hx(digest[:]), "result": "not computed", "parsed_cost": cost, "parse_error": err != nil})
			continue
		}
		result := "no match"
		if bcrypt.CompareHashAndPassword([]byte(variants[name]), digest[:]) == nil {
			result = "match"
		}
		bp.Add(map[string]any{"name": name, "hash": variants[name], "digest_hex": hx(digest[:]), "result": result})
	}
	write(t, dir, "bcrypt_parse.jsonl", &bp)

	var sh goldenio.Lines
	pattern := func(n int) []byte {
		out := make([]byte, n)
		for i := range out {
			out[i] = byte((i*31 + 7) & 0xff)
		}
		return out
	}
	for _, n := range []int{0, 3, 55, 56, 63, 64, 65, 1000} {
		in := pattern(n)
		if n == 3 {
			in = []byte("abc")
		}
		sum := sha256.Sum256(in)
		sh.Add(map[string]any{"input_hex": hx(in), "sum_hex": hx(sum[:])})
	}
	mib := pattern(1 << 20)
	msum := sha256.Sum256(mib)
	sh.Add(map[string]any{"repeat": "1 MiB, byte i = (i*31+7)&0xff", "sum_hex": hx(msum[:])})
	write(t, dir, "sha256.jsonl", &sh)

	var se goldenio.Lines
	for _, id := range []string{"", "abc", strings.Repeat("0", 64), "session-id-1"} {
		se.Add(map[string]any{"fn": "hashSessionID", "id": id, "token_hash": hashSessionID(id)})
	}
	for _, h := range []string{"", string(base), "$2a$10$xyz"} {
		se.Add(map[string]any{"fn": "credentialsFingerprint", "bcrypt": h, "credentials": credentialsFingerprint(h)})
	}
	folder := securityStore(t, "user")
	if err := InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}
	maskFile := func(step string) {
		data, err := os.ReadFile(filepath.Join(folder, SessionsFilename))
		if err != nil {
			se.Add(map[string]any{"fn": "sessions_file", "step": step, "missing": true})
			return
		}
		var sf SessionsFile
		_ = json.Unmarshal(data, &sf)
		masked := string(data)
		for _, s := range sf.Sessions {
			masked = strings.ReplaceAll(masked, s.TokenHash, "<id>")
			masked = strings.ReplaceAll(masked, fmt.Sprint(s.ExpiresAt), "<now+86400>")
		}
		se.Add(map[string]any{"fn": "sessions_file", "step": step, "sessions_file_hex": hx([]byte(masked))})
	}
	s1, err := CreateSession("admin", RoleAdmin, 24*time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	maskFile("create")
	got, err := GetSession(s1.ID)
	se.Add(map[string]any{"fn": "GetSession", "found": err == nil && got != nil})
	if err := DeleteSession(s1.ID); err != nil {
		t.Fatal(err)
	}
	maskFile("delete-last")
	if _, err := CreateSession("admin", RoleAdmin, 24*time.Hour); err != nil {
		t.Fatal(err)
	}
	if err := DeleteUserSessions("admin"); err != nil {
		t.Fatal(err)
	}
	maskFile("delete-user-sessions")
	write(t, dir, "session.jsonl", &se)

	var sd goldenio.Lines
	files := []string{
		`{"sessions":[{"token_hash":"aa","username":"u","role":"admin","credentials":"cc","expires_at":5}]}`,
		`{"SESSIONS":[{"TOKEN_HASH":"aa","Username":"u"}]}`, `{"sessions":[{"token_hash":"aa","legacy":1,"id":"x"}],"other":true}`, `{"sessions":[{}]}`,
		`null`, `{"sessions":null}`, `{"sessions":[]} trailing`, `{"sessions":[]}{"sessions":[{"username":"second"}]}`, `{"sessions":`, `[]`, `{"sessions":{}}`,
		`{"sessions":[{"expires_at":"5"}]}`, `{"sessions":[{"expires_at":1.5}]}`, ``, `   `,
	}
	for _, f := range files {
		p := filepath.Join(t.TempDir(), "sessions.json")
		_ = os.WriteFile(p, []byte(f), 0o600)
		row := map[string]any{"file_hex": hx([]byte(f))}
		sf, err := readSessionsFile(p)
		if err != nil {
			row["error"] = true
		} else {
			out, _ := json.Marshal(sf)
			row["decoded"] = string(out)
		}
		sd.Add(row)
	}
	write(t, dir, "sessions_decode.jsonl", &sd)

	var ck goldenio.Lines
	for _, variant := range []struct {
		name  string
		tls   bool
		proto string
	}{{"plain", false, ""}, {"peer-loopback", true, ""}, {"proto-https", false, "https"}, {"proto-list", false, "HTTPS, http"}, {"proto-spaces", false, " https "}, {"proto-http-first", false, "http, https"}} {
		for _, clear := range []bool{false, true} {
			ctx := &fasthttp.RequestCtx{}
			if variant.proto != "" {
				ctx.Request.Header.Set("X-Forwarded-Proto", variant.proto)
			}
			if variant.tls {
				ctx.Init(&ctx.Request, &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 1}, nil)
			}
			if clear {
				ClearSessionCookie(ctx)
			} else {
				SetSessionCookie(ctx, &Session{ID: "0123456789abcdef"})
			}
			ck.Add(map[string]any{"request": variant.name, "clear": clear, "set_cookie": string(ctx.Response.Header.Peek("Set-Cookie"))})
		}
	}
	write(t, dir, "cookies.jsonl", &ck)

	var bh goldenio.Lines
	headers := []string{"Basic " + base64.StdEncoding.EncodeToString([]byte("admin:admin")), "Basic " + base64.StdEncoding.EncodeToString([]byte("user:pa:ss")), "Basic " + base64.StdEncoding.EncodeToString([]byte("nocolon")),
		"Basic " + base64.StdEncoding.EncodeToString([]byte(":empty")), "Basic " + base64.StdEncoding.EncodeToString([]byte("\xff:\xfe")), "Basic !!!", "Basic YWRtaW46YWRtaW4", "basic YWRtaW46YWRtaW4=", "Basic  YWRtaW46YWRtaW4=", "Bearer x", "", "Basic "}
	for _, h := range headers {
		row := map[string]any{"header_hex": hx([]byte(h))}
		if h == "" || !strings.HasPrefix(h, "Basic ") {
			row["accepted_shape"] = false
			bh.Add(row)
			continue
		}
		payload, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(h, "Basic "))
		if err != nil {
			row["error"] = true
			row["accepted_shape"] = false
			bh.Add(row)
			continue
		}
		row["decoded_hex"] = hx(payload)
		parts := strings.SplitN(string(payload), ":", 2)
		row["accepted_shape"] = len(parts) == 2
		if len(parts) == 2 {
			row["username_hex"] = hx([]byte(parts[0]))
			row["password_hex"] = hx([]byte(parts[1]))
		}
		bh.Add(row)
	}
	write(t, dir, "basic_header.jsonl", &bh)

	var og goldenio.Lines
	for _, o := range []string{"http://allowed.test", "HTTP://ALLOWED.TEST", "http://allowed.test/", "https://upper.test", "http://kelvin.test", "http://\u212aelvin.test", "http://other.test", "", "null", "*"} {
		og.Add(map[string]any{"fn": "OriginIsAllowed", "origin": o, "allowed": OriginIsAllowed(o)})
	}
	for _, method := range []string{"GET", "POST", "PUT", "DELETE", "OPTIONS", "HEAD", "PATCH"} {
		for _, origin := range []string{"", "http://allowed.test", "http://h", "http://evil.test", "not a url", "http://H"} {
			for _, site := range []string{"", "same-origin", "none", "cross-site", "same-site"} {
				ctx := &fasthttp.RequestCtx{}
				ctx.Request.Header.SetMethod(method)
				ctx.Request.Header.SetHost("h")
				if origin != "" {
					ctx.Request.Header.Set("Origin", origin)
				}
				if site != "" {
					ctx.Request.Header.Set("Sec-Fetch-Site", site)
				}
				og.Add(map[string]any{"fn": "CrossOriginWriteIsRejected", "method": method, "origin": origin, "sec_fetch_site": site, "host": "h", "rejected": CrossOriginWriteIsRejected(ctx)})
			}
		}
	}
	write(t, dir, "origin.jsonl", &og)

	var ipl goldenio.Lines
	for _, s := range []string{"127.0.0.1", "127.1.2.3", "::1", "::ffff:127.0.0.1", "::127.0.0.1", "fe80::1%eth0", "010.000.000.001", "10.0.0.1", "2001:db8::1", "2001:0db8:0000:0000:0000:0000:0000:0001", "0.0.0.0", "::", "1.2.3.4.5", "", "192.168.1.1", "::ffff:10.1.2.3"} {
		ip := net.ParseIP(s)
		row := map[string]any{"fn": "ParseIP", "in": s, "valid": ip != nil}
		if ip != nil {
			row["string"] = ip.String()
			row["loopback"] = ip.IsLoopback()
			row["is4"] = ip.To4() != nil
		}
		ipl.Add(row)
	}
	for _, pair := range [][2]string{{"10.0.0.0/8", "10.1.2.3"}, {"10.0.0.0/8", "11.0.0.1"}, {"192.168.1.1", "192.168.1.1"}, {"192.168.1.1", "192.168.1.2"}, {"::1/128", "::1"}, {"10.0.0.0/8", "::ffff:10.0.0.1"}, {"bad", "1.2.3.4"}, {"2001:db8::/32", "2001:db8::5"}} {
		n := parseNetwork(pair[0])
		row := map[string]any{"fn": "parseNetwork", "network": pair[0], "ip": pair[1], "parsed": n != nil}
		if n != nil {
			row["contains"] = n.Contains(net.ParseIP(pair[1]))
		}
		ipl.Add(row)
	}
	for _, c := range []struct {
		peer    string
		headers map[string]string
	}{
		{"127.0.0.1", nil}, {"203.0.113.5", nil}, {"10.1.1.1", map[string]string{"X-Forwarded-For": "203.0.113.9"}}, {"10.1.1.1", map[string]string{"X-Forwarded-For": "198.51.100.1, 10.2.2.2"}},
		{"203.0.113.5", map[string]string{"X-Forwarded-For": "127.0.0.1"}}, {"127.0.0.1", map[string]string{"X-Forwarded-For": "203.0.113.9"}}, {"10.1.1.1", map[string]string{"X-Real-Ip": "198.51.100.7"}},
		{"192.168.1.1", map[string]string{"Forwarded": "for=1.2.3.4"}}, {"10.1.1.1", map[string]string{"X-Forwarded-For": "garbage"}}, {"::1", map[string]string{"X-Forwarded-For": "::1"}},
	} {
		ctx := &fasthttp.RequestCtx{}
		var req fasthttp.Request
		for k, v := range c.headers {
			req.Header.Set(k, v)
		}
		ctx.Init(&req, &net.TCPAddr{IP: net.ParseIP(c.peer), Port: 4000}, nil)
		src := LoginSourceFromRequest(ctx)
		row := map[string]any{"fn": "LoginSourceFromRequest", "peer": c.peer, "headers": c.headers, "local": src.Local, "client_key": src.clientKey()}
		if src.IP != nil {
			row["ip"] = src.IP.String()
		}
		ipl.Add(row)
	}
	write(t, dir, "ip.jsonl", &ipl)

	var gl goldenio.Lines
	securityStore(t, "user")
	if err := InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}
	if err := CreateBasicUser(&BasicUser{Username: "ann", Password: "right-pass", Role: RoleUser}); err != nil {
		t.Fatal(err)
	}
	clock := time.Unix(1800000000, 0)
	SetLoginClock(func() time.Time { return clock })
	defer SetLoginClock(nil)
	local := LoginSource{IP: net.ParseIP("127.0.0.1"), Local: true}
	remote := LoginSource{IP: net.ParseIP("203.0.113.5")}
	other := LoginSource{IP: net.ParseIP("203.0.113.6")}
	stepRun := func(scenario string, advance time.Duration, src LoginSource, user, pass string) time.Duration {
		clock = clock.Add(advance)
		_, wait, ok := AuthenticateUserFrom(user, pass, src)
		gl.Add(map[string]any{"scenario": scenario, "advance_ns": int64(advance), "client": src.clientKey(), "username": user, "right_password": pass != "wrong", "ok": ok, "wait_ns": int64(wait), "retry_after": RetryAfterSeconds(wait)})
		return wait
	}
	ResetLoginAttempts()
	for i := 0; i < 6; i++ {
		stepRun("per-user-lock", 0, remote, "ann", "wrong")
	}
	stepRun("per-user-lock", 0, remote, "ann", "right-pass")
	stepRun("per-user-lock", 500*time.Millisecond, remote, "ann", "right-pass")
	stepRun("per-user-lock", 500*time.Millisecond, remote, "ann", "right-pass")
	stepRun("per-user-lock", 0, remote, "bob", "wrong")
	ResetLoginAttempts()
	var wait time.Duration
	for i := 0; i < 18; i++ {
		stepRun("growth-to-cap", wait, remote, "ann", "wrong")
		wait = stepRun("growth-to-cap", 0, remote, "ann", "wrong")
	}
	ResetLoginAttempts()
	for i := 0; i < 23; i++ {
		stepRun("per-client-lock", 0, other, fmt.Sprintf("user%02d", i), "wrong")
	}
	stepRun("per-client-lock", 0, other, "ann", "right-pass")
	stepRun("per-client-lock", 0, remote, "ann", "right-pass")
	ResetLoginAttempts()
	for i := 0; i < 4; i++ {
		stepRun("forgetting", 0, remote, "ann", "wrong")
	}
	stepRun("forgetting", 16*time.Minute, remote, "ann", "wrong")
	for i := 0; i < 4; i++ {
		stepRun("forgetting", 0, remote, "ann", "wrong")
	}
	stepRun("forgetting", 0, remote, "ann", "wrong")
	ResetLoginAttempts()
	for i := 0; i < 3; i++ {
		stepRun("success-clears-user-key", 0, remote, "ann", "wrong")
	}
	stepRun("success-clears-user-key", 0, remote, "ann", "right-pass")
	for i := 0; i < 4; i++ {
		stepRun("success-clears-user-key", 0, remote, "ann", "wrong")
	}
	stepRun("success-clears-user-key", 0, remote, "ann", "wrong")
	ResetLoginAttempts()
	stepRun("default-admin-remote", 0, remote, "admin", "admin")
	stepRun("default-admin-local", 0, local, "admin", "admin")
	write(t, dir, "guard.jsonl", &gl)

	var vl goldenio.Lines
	values := []string{"", "a", "admin", "a b", "a/b", "a\tb", "a\u00a0b", "a\u2028b", "a\u0085b", "a\u200bb", "a\x7fb", "a\x00b", "\xff", strings.Repeat("u", 128), strings.Repeat("u", 129), "\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9", "\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9\u00e9", "1234567", "12345678", "\u3000x", "x\ufeff"}
	for _, v := range values {
		row := map[string]any{"value_hex": hx([]byte(v))}
		if err := ValidateUsername(v); err != nil {
			row["username_err"] = err.Error()
		} else {
			row["username_err"] = nil
		}
		if err := ValidatePassword(v); err != nil {
			row["password_err"] = err.Error()
		} else {
			row["password_err"] = nil
		}
		vl.Add(row)
	}
	write(t, dir, "validation.jsonl", &vl)

	var kl goldenio.Lines
	for _, content := range []string{"", "abc", "abc def", "  key-with-spaces-before", "\n\nkey", "key\n", "\x00\x01", " \n "} {
		f := securityStore(t, "")
		if content != "" {
			_ = os.WriteFile(filepath.Join(f, KeyFilename), []byte(content), 0o600)
		}
		row := map[string]any{"content_hex": hx([]byte(content))}
		k, err := CreateKeyFileOrGetKey()
		switch {
		case err != nil:
			row["error"] = true
		case content == "":
			row["generated"] = true
			row["length"] = len(k)
			row["hex_alphabet"] = strings.Trim(k, "0123456789abcdef") == ""
		default:
			row["key"] = k
		}
		kl.Add(row)
	}
	write(t, dir, "keyfile.jsonl", &kl)

	var sc goldenio.Lines
	for _, in := range []string{"alice\n", "  bob  \n", "carol dave\n", "\n", "", "eve", "frank\r\n", "2\n", "x\ny\n", "\xff\xfe\n"} {
		r := &oneByteReader{r: strings.NewReader(in)}
		var token string
		_, err := fmt.Fscanln(r, &token)
		rest, _ := io.ReadAll(r)
		row := map[string]any{"stdin_hex": hx([]byte(in)), "token_hex": hx([]byte(token)), "scan_error": err != nil, "remaining_hex": hx(rest)}
		r2 := &oneByteReader{r: strings.NewReader(in)}
		var choice int
		_, err2 := fmt.Fscanln(r2, &choice)
		row["int_choice"] = choice
		row["int_error"] = err2 != nil
		sc.Add(row)
	}
	write(t, dir, "scan.jsonl", &sc)
}

type oneByteReader struct {
	r io.Reader
}

func (o *oneByteReader) Read(p []byte) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	return o.r.Read(p[:1])
}
