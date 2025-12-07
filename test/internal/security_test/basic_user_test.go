package security_test

import (
	"crypto/sha256"
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
	"golang.org/x/crypto/bcrypt"
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
		Role:     security.RoleUser,
	}

	if err := security.AddUserToFile(user); err != nil {
		t.Fatal(err)
	}
}

func TestAddUserToFileAppend(t *testing.T) {
	setup(t)

	u1 := &security.BasicHashedcUser{Username: "a", Password: "x", Role: security.RoleAdmin}
	u2 := &security.BasicHashedcUser{Username: "b", Password: "y", Role: security.RoleUser}

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
		Role:     security.RoleAdmin,
	}

	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}
}

func TestCreateBasicUserMultiple(t *testing.T) {
	setup(t)

	users := []*security.BasicUser{
		{Username: "u1", Password: "p1", Role: security.RoleUser},
		{Username: "u2", Password: "p2", Role: security.RoleAdmin},
		{Username: "u3", Password: "p3", Role: security.RoleUser},
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
		Role:     security.RoleAdmin,
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

	if hu.Role != u.Role {
		t.Fatal("role mismatch")
	}
}

func TestCheckBasicAuthenticationSuccess(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Store.Folder = t.TempDir()
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "basic"

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatalf("key error: %v", err)
	}

	sum := sha256.Sum256([]byte("secret" + key))
	pass := sum[:]

	hashed, err := bcrypt.GenerateFromPassword(pass, bcrypt.DefaultCost)
	if err != nil {
		t.Fatalf("bcrypt error: %v", err)
	}

	user := &security.BasicHashedcUser{
		Username: "john",
		Password: string(hashed),
		Role:     security.RoleUser,
	}

	if err := security.AddUserToFile(user); err != nil {
		t.Fatalf("add user error: %v", err)
	}

	header := "Basic " + base64.StdEncoding.EncodeToString([]byte("john:secret"))

	req := fasthttp.AcquireRequest()
	req.Header.Set("Authorization", header)

	ctx := fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)

	if !security.CheckBasicAuthentication(&ctx) {
		t.Fatalf("authentication failed")
	}
}

func TestCheckBasicAuthenticationWrongPassword(t *testing.T) {
	setup(t)

	user := &security.BasicUser{Username: "admin", Password: "secret", Role: security.RoleAdmin}
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

func TestDeleteBasicUser_Success(t *testing.T) {
	setup(t)

	user := &security.BasicUser{Username: "john", Password: "secret", Role: security.RoleUser}
	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}

	err := security.DeleteBasicUser("john")
	if err != nil {
		t.Fatal(err)
	}

	users, err := security.LoadUsersFromFile()
	if err != nil {
		t.Fatal(err)
	}

	if len(users.Users) != 0 {
		t.Fatalf("expected user to be deleted")
	}
}

func TestDeleteBasicUser_NotFound(t *testing.T) {
	setup(t)

	err := security.DeleteBasicUser("missing")
	if err == nil {
		t.Fatalf("expected error for missing user")
	}
}

func TestDeleteBasicUser_NoUsersFile(t *testing.T) {
	dir := setup(t)

	path := filepath.Join(dir, security.UsersFilename)
	os.Remove(path)

	err := security.DeleteBasicUser("john")
	if err == nil {
		t.Fatalf("expected error when users.json does not exist")
	}
}

func TestLoadUsersFromFile(t *testing.T) {
	setup(t)

	u := &security.BasicHashedcUser{Username: "x", Password: "y", Role: security.RoleUser}
	if err := security.AddUserToFile(u); err != nil {
		t.Fatal(err)
	}

	users, err := security.LoadUsersFromFile()
	if err != nil {
		t.Fatal(err)
	}

	if len(users.Users) != 1 {
		t.Fatalf("expected 1 user, got %d", len(users.Users))
	}
}

func TestLoadUsersFromFile_NotExists(t *testing.T) {
	dir := setup(t)
	path := filepath.Join(dir, security.UsersFilename)
	os.Remove(path)

	_, err := security.LoadUsersFromFile()
	if err == nil {
		t.Fatal("expected error")
	}
}

func TestAuthenticateUserSuccess(t *testing.T) {
	setup(t)

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	sum := sha256.Sum256([]byte("pwd" + key))
	hashed, err := bcrypt.GenerateFromPassword(sum[:], bcrypt.DefaultCost)
	if err != nil {
		t.Fatal(err)
	}

	u := &security.BasicHashedcUser{
		Username: "bob",
		Password: string(hashed),
		Role:     security.RoleUser,
	}

	if err := security.AddUserToFile(u); err != nil {
		t.Fatal(err)
	}

	res, ok := security.AuthenticateUser("bob", "pwd")
	if !ok || res == nil {
		t.Fatal("expected success")
	}
}

func TestAuthenticateUserWrongPassword(t *testing.T) {
	setup(t)

	user := &security.BasicUser{Username: "john", Password: "abc", Role: security.RoleUser}
	if err := security.CreateBasicUser(user); err != nil {
		t.Fatal(err)
	}

	_, ok := security.AuthenticateUser("john", "wrong")
	if ok {
		t.Fatal("expected authentication failure")
	}
}

func TestAuthenticateUserNotExisting(t *testing.T) {
	setup(t)

	_, ok := security.AuthenticateUser("nouser", "pwd")
	if ok {
		t.Fatal("expected failure")
	}
}

func TestAuthenticateUserKeyError(t *testing.T) {
	dir := setup(t)

	keyPath := filepath.Join(dir, security.KeyFilename)
	os.Remove(keyPath)

	os.WriteFile(keyPath, []byte{}, 0000)

	_, ok := security.AuthenticateUser("u", "p")
	if ok {
		t.Fatal("expected failure due to key error")
	}
}

func TestToHashedUser_DefaultRole(t *testing.T) {
	setup(t)

	u := &security.BasicUser{
		Username: "noRole",
		Password: "x",
		Role:     "",
	}

	hu, err := u.ToHasedUser()
	if err != nil {
		t.Fatal(err)
	}

	if hu.Role != security.RoleUser {
		t.Fatalf("expected default role user")
	}
}

func TestAddUserToFileInvalidJSON(t *testing.T) {
	dir := setup(t)

	path := filepath.Join(dir, security.UsersFilename)
	os.WriteFile(path, []byte("{invalid json"), 0644)

	u := &security.BasicHashedcUser{Username: "x", Password: "x", Role: security.RoleUser}

	err := security.AddUserToFile(u)
	if err == nil {
		t.Fatal("expected failure on invalid JSON file")
	}
}

func TestCreateKeyFileOrGetKey_Reopen(t *testing.T) {
	setup(t)

	k1, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	k2, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}

	if k1 != k2 {
		t.Fatal("keys should match")
	}
}

func TestDeleteBasicUser_OnlyOne(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "solo", Password: "pwd", Role: security.RoleAdmin}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	if err := security.DeleteBasicUser("solo"); err != nil {
		t.Fatal(err)
	}

	users, err := security.LoadUsersFromFile()
	if err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}

	if len(users.Users) != 0 {
		t.Fatal("expected no users")
	}
}
