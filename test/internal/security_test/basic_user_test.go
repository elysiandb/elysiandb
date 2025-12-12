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
			t.Fatalf("type mismatch")
		}
		if m["required"] != true {
			t.Fatalf("required mismatch")
		}
	}
}

func TestInitBasicUsersStorage_CreatesEntityTypeAndSchema(t *testing.T) {
	setup(t)

	if api_storage.EntityTypeExists(security.UserEntity) {
		t.Fatalf("should not exist before init")
	}

	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}

	if !api_storage.EntityTypeExists(security.UserEntity) {
		t.Fatal("entity not created")
	}

	if api_storage.GetEntitySchema(security.UserEntity) == nil {
		t.Fatal("schema not created")
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
		t.Fatal("id mismatch")
	}
	if m["username"] != "bob" {
		t.Fatal("username mismatch")
	}
	if m["password"] != "hash" {
		t.Fatal("password mismatch")
	}
	if m["role"] != string(security.RoleAdmin) {
		t.Fatal("role mismatch")
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
		t.Fatal(err)
	}

	out := api_storage.ReadEntityById(security.UserEntity, "saveuser")
	if out == nil {
		t.Fatal("not saved")
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
		t.Fatal("mismatch")
	}
	if hu.Password == "" {
		t.Fatal("empty hash")
	}
	if hu.Role != u.Role {
		t.Fatal("role mismatch")
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
		t.Fatal("expected default role user")
	}
}

func TestCreateBasicUser_Single(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "alice", Password: "secret", Role: security.RoleAdmin}

	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	if api_storage.ReadEntityById(security.UserEntity, "alice") == nil {
		t.Fatal("not written")
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
			t.Fatal(err)
		}
	}

	for _, u := range users {
		if api_storage.ReadEntityById(security.UserEntity, u.Username) == nil {
			t.Fatal("missing")
		}
	}
}

func TestGetBasicUserByUsername(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: "bob", Password: "pwd", Role: security.RoleUser}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	out, err := security.GetBasicUserByUsername("bob")
	if err != nil {
		t.Fatal(err)
	}

	if out["username"] != "bob" {
		t.Fatal("username mismatch")
	}
	if _, ok := out["password"]; ok {
		t.Fatal("password should not be exposed")
	}
}

func TestGetBasicUserByUsername_NotFound(t *testing.T) {
	setup(t)

	if _, err := security.GetBasicUserByUsername("missing"); err == nil {
		t.Fatal("expected error")
	}
}

func TestListBasicUsers(t *testing.T) {
	setup(t)

	users := []*security.BasicUser{
		{Username: "a", Password: "1", Role: security.RoleUser},
		{Username: "b", Password: "2", Role: security.RoleAdmin},
	}

	for _, u := range users {
		if err := security.CreateBasicUser(u); err != nil {
			t.Fatal(err)
		}
	}

	list, err := security.ListBasicUsers()
	if err != nil {
		t.Fatal(err)
	}

	if len(list) != 2 {
		t.Fatalf("expected 2 users, got %d", len(list))
	}

	for _, u := range list {
		if _, ok := u["password"]; ok {
			t.Fatal("password should not be exposed")
		}
	}
}

func TestInitAdminUserIfNotExists_CreatesDefaultAdmin(t *testing.T) {
	setup(t)

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}

	out := api_storage.ReadEntityById(security.UserEntity, security.DefaultAdminUsername)
	if out == nil {
		t.Fatal("admin not created")
	}
}

func TestInitAdminUserIfNotExists_DoesNotOverwriteExisting(t *testing.T) {
	setup(t)

	u := &security.BasicUser{Username: security.DefaultAdminUsername, Password: "custom", Role: security.RoleAdmin}
	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}

	out := api_storage.ReadEntityById(security.UserEntity, security.DefaultAdminUsername)
	if out["username"] != security.DefaultAdminUsername {
		t.Fatal("username mismatch")
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

	_, ok := security.AuthenticateUser("john", "newpwd")
	if !ok {
		t.Fatal("expected success")
	}
}

func TestDeleteBasicUser_DoesNotDeleteAdmin(t *testing.T) {
	setup(t)

	u := &security.BasicUser{
		Username: security.DefaultAdminUsername,
		Password: "x",
		Role:     security.RoleAdmin,
	}

	if err := security.CreateBasicUser(u); err != nil {
		t.Fatal(err)
	}

	security.DeleteBasicUser(security.DefaultAdminUsername)

	out := api_storage.ReadEntityById(security.UserEntity, security.DefaultAdminUsername)
	if out == nil {
		t.Fatal("admin should not be deleted")
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
	req.Header.Set("Authorization", header)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if !security.CheckBasicAuthentication(&ctx) {
		t.Fatal("expected success")
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
		t.Fatal("missing schema")
	}

	if !reflect.DeepEqual(expected, stored) {
		t.Fatalf("schema mismatch")
	}
}
