package acl_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

func setup(t *testing.T, auth bool) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 2
	cfg.Security.Authentication.Enabled = auth
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()
	api_storage.DeleteAll()
}

func writeACL(username, entity string, perms map[acl.Permission]bool) {
	a := acl.ACL{
		Username:    username,
		Entity:      entity,
		Permissions: perms,
	}
	api_storage.WriteEntity(acl.ACLEntity, a.ToDataMap())
}

func TestCanCreateEntity_DefaultDeny(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	writeACL("u", "doc", perms)

	if acl.CanCreateEntity("doc") {
		t.Fatalf("expected false")
	}
}

func TestCanCreateEntity_WithPermission(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	writeACL("u", "doc", perms)

	if !acl.CanCreateEntity("doc") {
		t.Fatalf("expected true")
	}
}

func TestCanReadEntity_Global(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionRead] = true
	writeACL("u", "doc", perms)

	if !acl.CanReadEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected true")
	}
}

func TestCanReadEntity_Owning(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningRead] = true
	writeACL("u", "doc", perms)

	if !acl.CanReadEntity("doc", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanReadEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateEntity_Global(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionUpdate] = true
	writeACL("u", "doc", perms)

	if !acl.CanUpdateEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected true")
	}
}

func TestCanUpdateEntity_Owning(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)

	if !acl.CanUpdateEntity("doc", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanUpdateEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}
}

func TestCanDeleteEntity_Global(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionDelete] = true
	writeACL("u", "doc", perms)

	if !acl.CanDeleteEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected true")
	}
}

func TestCanDeleteEntity_Owning(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningDelete] = true
	writeACL("u", "doc", perms)

	if !acl.CanDeleteEntity("doc", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanDeleteEntity("doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateListOfEntities_Global(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionUpdate] = true
	writeACL("u", "doc", perms)

	ok := acl.CanUpdateListOfEntities("doc", []map[string]any{
		{acl.UsernameField: "x"},
		{acl.UsernameField: "y"},
	})
	if !ok {
		t.Fatalf("expected true")
	}
}

func TestCanUpdateListOfEntities_Owning(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)

	ok := acl.CanUpdateListOfEntities("doc", []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "u"},
	})
	if !ok {
		t.Fatalf("expected true")
	}

	ok = acl.CanUpdateListOfEntities("doc", []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "x"},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestFilterListOfEntities_GlobalRead(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionRead] = true
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "x"},
	}

	out := acl.FilterListOfEntities("doc", data)
	if len(out) != 2 {
		t.Fatalf("expected 2")
	}
}

func TestFilterListOfEntities_Owning(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningRead] = true
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "x"},
	}

	out := acl.FilterListOfEntities("doc", data)
	if len(out) != 1 {
		t.Fatalf("expected 1")
	}
}

func TestFilterListOfEntities_NoPermission(t *testing.T) {
	setup(t, true)
	security.SetCurrentUsername("u")

	perms := acl.NewPermissions()
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
	}

	out := acl.FilterListOfEntities("doc", data)
	if len(out) != 0 {
		t.Fatalf("expected empty")
	}
}

func TestAllAllowedWhenAuthDisabled(t *testing.T) {
	setup(t, false)
	security.SetCurrentUsername("any")

	if !acl.CanCreateEntity("x") {
		t.Fatalf("expected true")
	}
	if !acl.CanReadEntity("x", map[string]any{}) {
		t.Fatalf("expected true")
	}
	if !acl.CanUpdateEntity("x", map[string]any{}) {
		t.Fatalf("expected true")
	}
	if !acl.CanDeleteEntity("x", map[string]any{}) {
		t.Fatalf("expected true")
	}

	data := []map[string]any{{"a": 1}}
	out := acl.FilterListOfEntities("x", data)
	if len(out) != 1 {
		t.Fatalf("expected passthrough")
	}
}
