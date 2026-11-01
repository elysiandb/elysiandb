package acl_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
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

func writeDoc(entity, id, owner string) {
	api_storage.WriteEntity(entity, map[string]any{
		"id":              id,
		acl.UsernameField: owner,
	})
}

func writeUser(username string, role security.Role) {
	api_storage.WriteEntity(security.UserEntity, map[string]any{
		"id":       username,
		"username": username,
		"role":     string(role),
	})
}

func TestCanCreateEntity_DefaultDeny(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	writeACL("u", "doc", perms)

	if acl.CanCreateEntity("u", "doc") {
		t.Fatalf("expected false")
	}
}

func TestCanCreateEntity_WithPermission(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	writeACL("u", "doc", perms)

	if !acl.CanCreateEntity("u", "doc") {
		t.Fatalf("expected true")
	}
}

func TestCanCreateEntity_NewEntityTypeUsesRoleDefaults(t *testing.T) {
	setup(t, true)
	writeUser("boss", security.RoleAdmin)
	writeUser("u", security.RoleUser)

	if !acl.CanCreateEntity("boss", "fresh") {
		t.Fatalf("expected admin to create a new entity type")
	}

	if acl.CanCreateEntity("u", "fresh") {
		t.Fatalf("expected regular user to be denied")
	}

	if acl.CanCreateEntity("ghost", "fresh") {
		t.Fatalf("expected unknown user to be denied")
	}
}

func TestCanCreateEntity_ExistingEntityTypeWithoutACL(t *testing.T) {
	setup(t, true)
	writeUser("boss", security.RoleAdmin)
	writeDoc("doc", "1", "x")

	if acl.CanCreateEntity("boss", "doc") {
		t.Fatalf("expected false")
	}
}

func TestCanCreateEntity_CoreEntityType(t *testing.T) {
	setup(t, true)
	writeUser("boss", security.RoleAdmin)

	if acl.CanCreateEntity("boss", acl.ACLEntity) {
		t.Fatalf("expected false for an existing core entity")
	}

	if acl.CanCreateEntity("boss", "_elysiandb_core_fresh") {
		t.Fatalf("expected false for a new core entity")
	}
}

func TestCanWriteEntity_RequiresCreate(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)

	if acl.CanWriteEntity("u", "doc", map[string]any{"a": 1}) {
		t.Fatalf("expected false")
	}
}

func TestCanWriteEntity_NewDocument(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	writeACL("u", "doc", perms)

	if !acl.CanWriteEntity("u", "doc", map[string]any{"a": 1}) {
		t.Fatalf("expected true without id")
	}

	if !acl.CanWriteEntity("u", "doc", map[string]any{"id": "missing"}) {
		t.Fatalf("expected true for an unused id")
	}
}

func TestCanWriteEntity_ExistingIdRequiresOwnership(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "mine", "u")
	writeDoc("doc", "other", "x")

	if !acl.CanWriteEntity("u", "doc", map[string]any{"id": "mine"}) {
		t.Fatalf("expected true for own document")
	}

	if acl.CanWriteEntity("u", "doc", map[string]any{"id": "other"}) {
		t.Fatalf("expected false for another user's document")
	}
}

func TestCanWriteEntity_ExistingIdWithGlobalUpdate(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	perms[acl.PermissionUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "other", "x")

	if !acl.CanWriteEntity("u", "doc", map[string]any{"id": "other"}) {
		t.Fatalf("expected true")
	}
}

func TestCanWriteListOfEntities_ExistingIds(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionCreate] = true
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "mine", "u")
	writeDoc("doc", "other", "x")

	ok := acl.CanWriteListOfEntities("u", "doc", []map[string]any{
		{"a": 1},
		{"id": "mine"},
	})
	if !ok {
		t.Fatalf("expected true")
	}

	ok = acl.CanWriteListOfEntities("u", "doc", []map[string]any{
		{"a": 1},
		{"id": "other"},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestCanReadEntity_Global(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionRead] = true
	writeACL("u", "doc", perms)

	if !acl.CanReadEntity("u", "doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected true")
	}
}

func TestCanReadEntity_Owning(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningRead] = true
	writeACL("u", "doc", perms)

	if !acl.CanReadEntity("u", "doc", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanReadEntity("u", "doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateEntity_Global(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "1", "x")

	if !acl.CanUpdateEntity("u", "doc", "1", map[string]any{"a": 1}) {
		t.Fatalf("expected true")
	}

	if !acl.CanUpdateEntity("u", "doc", "1", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}
}

func TestCanUpdateEntity_Owning(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "mine", "u")
	writeDoc("doc", "other", "x")

	if !acl.CanUpdateEntity("u", "doc", "mine", map[string]any{"a": 1}) {
		t.Fatalf("expected true")
	}

	if !acl.CanUpdateEntity("u", "doc", "mine", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanUpdateEntity("u", "doc", "other", map[string]any{"a": 1}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateEntity_OwningUsesStoredOwner(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "other", "x")

	if acl.CanUpdateEntity("u", "doc", "other", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateEntity_OwningCannotChangeOwner(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "mine", "u")

	if acl.CanUpdateEntity("u", "doc", "mine", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}

	if acl.CanUpdateEntity("u", "doc", "mine", map[string]any{acl.UsernameField: nil}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateEntity_OwningMissingEntity(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)

	if acl.CanUpdateEntity("u", "doc", "missing", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected false")
	}
}

func TestCanDeleteEntity_Global(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionDelete] = true
	writeACL("u", "doc", perms)

	if !acl.CanDeleteEntity("u", "doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected true")
	}
}

func TestCanDeleteEntity_Owning(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningDelete] = true
	writeACL("u", "doc", perms)

	if !acl.CanDeleteEntity("u", "doc", map[string]any{acl.UsernameField: "u"}) {
		t.Fatalf("expected true")
	}

	if acl.CanDeleteEntity("u", "doc", map[string]any{acl.UsernameField: "x"}) {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateListOfEntities_Global(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "1", "x")
	writeDoc("doc", "2", "y")

	ok := acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{"id": "1", "a": 1},
		{"id": "2", "a": 2},
	})
	if !ok {
		t.Fatalf("expected true")
	}
}

func TestCanUpdateListOfEntities_Owning(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "1", "u")
	writeDoc("doc", "2", "u")
	writeDoc("doc", "3", "x")

	ok := acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{"id": "1", "a": 1},
		{"id": "2", acl.UsernameField: "u"},
	})
	if !ok {
		t.Fatalf("expected true")
	}

	ok = acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{"id": "1", "a": 1},
		{"id": "3", "a": 1},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateListOfEntities_OwningUsesStoredOwner(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "1", "u")
	writeDoc("doc", "2", "x")

	ok := acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{"id": "1", acl.UsernameField: "u"},
		{"id": "2", acl.UsernameField: "u"},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateListOfEntities_OwningCannotChangeOwner(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)
	writeDoc("doc", "1", "u")

	ok := acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{"id": "1", acl.UsernameField: "x"},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestCanUpdateListOfEntities_OwningRequiresId(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningUpdate] = true
	writeACL("u", "doc", perms)

	ok := acl.CanUpdateListOfEntities("u", "doc", []map[string]any{
		{acl.UsernameField: "u"},
	})
	if ok {
		t.Fatalf("expected false")
	}
}

func TestFilterListOfEntities_GlobalRead(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionRead] = true
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "x"},
	}

	out := acl.FilterListOfEntities("u", "doc", data)
	if len(out) != 2 {
		t.Fatalf("expected 2")
	}
}

func TestFilterListOfEntities_Owning(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningRead] = true
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
		{acl.UsernameField: "x"},
	}

	out := acl.FilterListOfEntities("u", "doc", data)
	if len(out) != 1 {
		t.Fatalf("expected 1")
	}
}

func TestFilterListOfEntities_NoPermission(t *testing.T) {
	setup(t, true)

	perms := acl.NewPermissions()
	writeACL("u", "doc", perms)

	data := []map[string]any{
		{acl.UsernameField: "u"},
	}

	out := acl.FilterListOfEntities("u", "doc", data)
	if len(out) != 0 {
		t.Fatalf("expected empty")
	}
}

func TestAllAllowedWhenAuthDisabled(t *testing.T) {
	setup(t, false)

	if !acl.CanCreateEntity("u", "x") {
		t.Fatalf("expected true")
	}
	if !acl.CanReadEntity("u", "x", map[string]any{}) {
		t.Fatalf("expected true")
	}
	if !acl.CanUpdateEntity("u", "x", "1", map[string]any{}) {
		t.Fatalf("expected true")
	}
	if !acl.CanDeleteEntity("u", "x", map[string]any{}) {
		t.Fatalf("expected true")
	}
	if !acl.CanWriteEntity("u", "x", map[string]any{"id": "1"}) {
		t.Fatalf("expected true")
	}

	data := []map[string]any{{"a": 1}}
	out := acl.FilterListOfEntities("u", "x", data)
	if len(out) != 1 {
		t.Fatalf("expected passthrough")
	}
}

func TestInitACLReplacesAnInferredSchema(t *testing.T) {
	setup(t, true)

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true

	writeUser("admin", security.RoleAdmin)
	api_storage.WriteEntity(schema.SchemaEntity, map[string]any{
		"id": acl.ACLEntity,
		"fields": map[string]any{
			"permissions": map[string]any{"name": "permissions", "type": "map[string]bool", "required": true},
		},
	})

	if err := acl.InitACL(); err != nil {
		t.Fatalf("expected the ACLs to be created, got %v", err)
	}

	if !schema.IsManualSchema(acl.ACLEntity, nil) {
		t.Fatalf("expected the ACL entity to get its manual schema")
	}

	api_storage.WriteEntity("book", map[string]any{"id": "b1"})
	if err := acl.InitACL(); err != nil {
		t.Fatalf("expected the ACLs of a new entity type to be created, got %v", err)
	}

	if acl.GetACLEntityForUsername("book", "admin") == nil {
		t.Fatalf("expected an ACL for the new entity type")
	}

	if err := acl.UpdateACLEntityForUsername("book", "admin", acl.NewPermissions()); err != nil {
		t.Fatalf("expected the ACL update to succeed, got %v", err)
	}
}
