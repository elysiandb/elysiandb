package security_test

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) string {
	dir := t.TempDir()

	cfg := &configuration.Config{}
	cfg.Store.Folder = dir
	globals.SetConfig(cfg)

	return dir
}

func TestGenerateKey(t *testing.T) {
	key, err := security.GenerateKey()
	if err != nil {
		t.Fatal(err)
	}
	if len(key) == 0 {
		t.Fatal("empty key")
	}
}

func TestCreateKeyFileOrGetKeyCreate(t *testing.T) {
	dir := setup(t)

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	path := filepath.Join(dir, security.KeyFilename)
	if _, err := os.Stat(path); err != nil {
		t.Fatal(err)
	}

	if key == "" {
		t.Fatal("empty key")
	}
}

func TestCreateKeyFileOrGetKeyRead(t *testing.T) {
	setup(t)

	key1, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	key2, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	if key1 != key2 {
		t.Fatal("key mismatch")
	}
}

func TestAddUserToFileEmpty(t *testing.T) {
	setup(t)

	user := &security.BasicHashedcUser{
		Username: "john",
		Password: "hash",
	}

	if err := security.AddUserToFile(user); err != nil {
		t.Fatal(err)
	}
}

func TestAddUserToFileAppend(t *testing.T) {
	setup(t)

	u1 := &security.BasicHashedcUser{Username: "a", Password: "x"}
	u2 := &security.BasicHashedcUser{Username: "b", Password: "y"}

	if err := security.AddUserToFile(u1); err != nil {
		t.Fatal(err)
	}

	if err := security.AddUserToFile(u2); err != nil {
		t.Fatal(err)
	}
}

func TestCreateBasicUser(t *testing.T) {
	setup(t)

	user := &security.BasicUser{
		Username: "alice",
		Password: "secret",
	}

	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}
}

func TestCreateBasicUserMultiple(t *testing.T) {
	setup(t)

	users := []*security.BasicUser{
		{Username: "u1", Password: "p1"},
		{Username: "u2", Password: "p2"},
		{Username: "u3", Password: "p3"},
	}

	for _, u := range users {
		if err := security.CreateBasicUser(u); err != nil {
			t.Fatal(err)
		}
	}
}

func TestToHasedUser(t *testing.T) {
	setup(t)

	u := &security.BasicUser{
		Username: "test",
		Password: "pass",
	}

	hu, err := u.ToHasedUser()
	if err != nil {
		t.Fatal(err)
	}

	if hu.Username != u.Username {
		t.Fatal("username mismatch")
	}

	if hu.Password == "" {
		t.Fatal("empty hash")
	}
}

func TestCheckBasicAuthenticationSuccess(t *testing.T) {
	setup(t)

	user := &security.BasicUser{Username: "admin", Password: "secret"}
	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)

	token := base64.StdEncoding.EncodeToString([]byte("admin:secret"))
	req.Header.Set("Authorization", "Basic "+token)

	ctx := fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)

	if !security.CheckBasicAuthentication(&ctx) {
		t.Fatal("authentication failed")
	}
}

func TestCheckBasicAuthenticationWrongPassword(t *testing.T) {
	setup(t)

	user := &security.BasicUser{Username: "admin", Password: "secret"}
	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)

	token := base64.StdEncoding.EncodeToString([]byte("admin:wrong"))
	req.Header.Set("Authorization", "Basic "+token)

	ctx := fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)

	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("authentication should fail")
	}
}

func TestCheckBasicAuthenticationNoHeader(t *testing.T) {
	setup(t)

	ctx := fasthttp.RequestCtx{}
	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("authentication should fail")
	}
}

func TestCheckBasicAuthenticationMalformedHeader(t *testing.T) {
	setup(t)

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)

	req.Header.Set("Authorization", "Basic invalidbase64")

	ctx := fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)

	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("authentication should fail")
	}
}
