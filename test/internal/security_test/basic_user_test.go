package security_test

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder: dir,
			Shards: 8,
		},
		Stats: configuration.StatsConfig{
			Enabled: false,
		},
	})
	storage.LoadDB()
	storage.LoadJsonDB()
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

func TestCreateKeyFileOrGetKey_Create(t *testing.T) {
	dir := setup(t)

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		t.Fatal(err)
	}
	if key == "" {
		t.Fatal("empty key")
	}

	path := filepath.Join(dir, security.KeyFilename)
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected key file to exist, got %v", err)
	}
}

func TestCreateKeyFileOrGetKey_ReadSameKey(t *testing.T) {
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
		t.Fatalf("expected same key, got %s and %s", k1, k2)
	}
}

func TestUserEntitySchema(t *testing.T) {
	s := security.UserEntitySchema()

	for _, field := range []string{"id", "username", "password", "role"} {
		v, ok := s[field]
		if !ok {
			t.Fatalf("missing field %s in schema", field)
		}
		m, ok := v.(map[string]interface{})
		if !ok {
			t.Fatalf("field %s is not a map", field)
		}
		if m["type"] != "string" {
			t.Fatalf("field %s type mismatch, got %v", field, m["type"])
		}
		if m["required"] != true {
			t.Fatalf("field %s required mismatch, got %v", field, m["required"])
		}
	}
}

func TestInitBasicUsersStorage_CreatesEntityTypeAndSchema(t *testing.T) {
	setup(t)

	if api_storage.EntityTypeExists(security.UserEntity) {
		t.Fatalf("user entity should not exist before init")
	}

	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}

	if !api_storage.EntityTypeExists(security.UserEntity) {
		t.Fatal("user entity type not created")
	}

	schema := api_storage.GetEntitySchema(security.UserEntity)
	if schema == nil {
		t.Fatal("user entity schema not created")
	}
}

func TestInitBasicUsersStorage_Idempotent(t *testing.T) {
	setup(t)

	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}
	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}
}

func TestBasicHashedUser_ToDataMap(t *testing.T) {
	setup(t)

	u := &security.BasicHashedUser{
		Username: "bob",
		Password: "hash",
		Role:     security.RoleAdmin,
	}

	m := u.ToDataMap()
	if m["id"] != "bob" {
		t.Fatalf("id mismatch, got %v", m["id"])
	}
	if m["username"] != "bob" {
		t.Fatalf("username mismatch, got %v", m["username"])
	}
	if m["password"] != "hash" {
		t.Fatalf("password mismatch, got %v", m["password"])
	}
	if m["role"] != string(security.RoleAdmin) {
		t.Fatalf("role mismatch, got %v", m["role"])
	}
}

func TestBasicHashedUser_Save(t *testing.T) {
	setup(t)

	u := &security.BasicHashedUser{
		Username: "saveuser",
		Password: "hash",
		Role:     security.RoleUser,
	}

	if err := u.Save(); err != nil {
		t.Fatalf("save error: %v", err)
	}

	out := api_storage.ReadEntityById(security.UserEntity, "saveuser")
	if out == nil {
		t.Fatal("saved user not found")
	}

	if out["username"] != "saveuser" {
		t.Fatalf("username mismatch, got %v", out["username"])
	}
}

func TestBasicUser_ToHasedUser(t *testing.T) {
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
		t.Fatalf("username mismatch, got %s", hu.Username)
	}
	if hu.Password == "" {
		t.Fatal("empty hashed password")
	}
	if hu.Role != u.Role {
		t.Fatalf("role mismatch, got %v", hu.Role)
	}
}

func TestBasicUser_ToHasedUser_DefaultRole(t *testing.T) {
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
		t.Fatalf("expected default role user, got %v", hu.Role)
	}
}

func TestCreateBasicUser_Single(t *testing.T) {
	setup(t)

	u := &security.BasicUser{
		Username: "alice",
		Password: "secret",
		Role:     security.RoleAdmin,
	}

	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	out := api_storage.ReadEntityById(security.UserEntity, "alice")
	if out == nil {
		t.Fatal("user not written")
	}
	if out["username"] != "alice" {
		t.Fatalf("username mismatch, got %v", out["username"])
	}
}

func TestCreateBasicUser_Multiple(t *testing.T) {
	setup(t)

	users := []*security.BasicUser{
		{Username: "u1", Password: "p1", Role: security.RoleUser},
		{Username: "u2", Password: "p2", Role: security.RoleAdmin},
		{Username: "u3", Password: "p3", Role: security.RoleUser},
	}

	for _, u := range users {
		if err := security.CreateBasicUser(u); err != nil {
			t.Fatalf("CreateBasicUser error for %s: %v", u.Username, err)
		}
	}

	for _, u := range users {
		out := api_storage.ReadEntityById(security.UserEntity, u.Username)
		if out == nil {
			t.Fatalf("user %s missing", u.Username)
		}
	}
}

func TestChangeUserPassword_Success(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "john", Password: "oldpwd", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	if err := security.ChangeUserPassword("john", "newpwd"); err != nil {
		t.Fatal(err)
	}

	res, ok := security.AuthenticateUser("john", "newpwd")
	if !ok || res == nil {
		t.Fatal("expected authentication success with new password")
	}
}

func TestChangeUserPassword_UserNotFound(t *testing.T) {
	setup(t)

	err := security.ChangeUserPassword("ghost", "pwd")
	if err == nil {
		t.Fatal("expected error for missing user")
	}
}

func TestChangeUserPassword_KeyError(t *testing.T) {
	dir := setup(t)

	u := &security.BasicUser{Username: "a", Password: "b", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	keyPath := filepath.Join(dir, security.KeyFilename)
	os.Remove(keyPath)
	os.WriteFile(keyPath, []byte{}, 0000)

	err := security.ChangeUserPassword("a", "x")
	if err == nil {
		t.Fatal("expected key read error")
	}
}

func TestDeleteBasicUser_Success(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "john", Password: "pwd", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	security.DeleteBasicUser("john")

	if api_storage.ReadEntityById(security.UserEntity, "john") != nil {
		t.Fatal("expected user to be deleted")
	}
}

func TestDeleteBasicUser_NotFound(t *testing.T) {
	setup(t)
	security.DeleteBasicUser("missing")
}

func TestAuthenticateUser_Success(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "bob", Password: "pwd", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	res, ok := security.AuthenticateUser("bob", "pwd")
	if !ok || res == nil {
		t.Fatal("expected authentication success")
	}
	if res.Username != "bob" {
		t.Fatalf("username mismatch, got %s", res.Username)
	}
}

func TestAuthenticateUser_WrongPassword(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "john", Password: "abc", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	_, ok := security.AuthenticateUser("john", "wrong")
	if ok {
		t.Fatal("expected authentication failure with wrong password")
	}
}

func TestAuthenticateUser_NotExisting(t *testing.T) {
	setup(t)

	_, ok := security.AuthenticateUser("nouser", "pwd")
	if ok {
		t.Fatal("expected authentication failure for missing user")
	}
}

func TestAuthenticateUser_KeyError(t *testing.T) {
	dir := setup(t)

	u := &security.BasicUser{Username: "u", Password: "p", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	keyPath := filepath.Join(dir, security.KeyFilename)
	os.Remove(keyPath)
	os.WriteFile(keyPath, []byte{}, 0000)

	_, ok := security.AuthenticateUser("u", "p")
	if ok {
		t.Fatal("expected failure due to key error")
	}
}

func TestCheckBasicAuthentication_Success(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "john", Password: "secret", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	header := "Basic " + base64.StdEncoding.EncodeToString([]byte("john:secret"))

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)
	req.Header.Set("Authorization", header)
	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if !security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected authentication success")
	}
}

func TestCheckBasicAuthentication_WrongPassword(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "admin", Password: "secret", Role: security.RoleAdmin}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	token := base64.StdEncoding.EncodeToString([]byte("admin:wrong"))

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)
	req.Header.Set("Authorization", "Basic "+token)
	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected authentication failure")
	}
}

func TestCheckBasicAuthentication_NoHeader(t *testing.T) {
	setup(t)

	var ctx fasthttp.RequestCtx
	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected authentication failure without header")
	}
}

func TestCheckBasicAuthentication_MalformedBase64(t *testing.T) {
	setup(t)

	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)
	req.Header.Set("Authorization", "Basic !!!notbase64")

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected authentication failure with malformed base64")
	}
}

func TestCheckBasicAuthentication_MalformedPayload(t *testing.T) {
	setup(t)

	payload := base64.StdEncoding.EncodeToString([]byte("nocolon"))
	req := fasthttp.AcquireRequest()
	defer fasthttp.ReleaseRequest(req)
	req.Header.Set("Authorization", "Basic "+payload)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected authentication failure with malformed payload")
	}
}

func TestInitBasicUsersStorage_SchemaMatchesUserEntitySchema(t *testing.T) {
	setup(t)

	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}

	expected := api_storage.UpdateEntitySchema(security.UserEntity, security.UserEntitySchema())
	stored := api_storage.GetEntitySchema(security.UserEntity)
	if stored == nil {
		t.Fatal("expected stored schema for user entity")
	}

	if !reflect.DeepEqual(expected, stored) {
		t.Fatalf("stored schema mismatch, expected %v got %v", expected, stored)
	}
}
