package acl

import (
	"errors"
	"fmt"
	"strings"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
)

const UsernameField = globals.CoreFieldsPrefix + "username"

const ACLEntity = "_elysiandb_core_acl"

type Permission int

const (
	PermissionCreate Permission = iota
	PermissionRead
	PermissionUpdate
	PermissionDelete
	PermissionOwningRead
	PermissionOwningWrite
	PermissionOwningDelete
	PermissionOwningUpdate
)

var permissionStrings = map[Permission]string{
	PermissionCreate:       "create",
	PermissionRead:         "read",
	PermissionUpdate:       "update",
	PermissionDelete:       "delete",
	PermissionOwningRead:   "owning_read",
	PermissionOwningWrite:  "owning_write",
	PermissionOwningDelete: "owning_delete",
	PermissionOwningUpdate: "owning_update",
}

var stringToPermission = map[string]Permission{
	"create":        PermissionCreate,
	"read":          PermissionRead,
	"update":        PermissionUpdate,
	"delete":        PermissionDelete,
	"owning_read":   PermissionOwningRead,
	"owning_write":  PermissionOwningWrite,
	"owning_delete": PermissionOwningDelete,
	"owning_update": PermissionOwningUpdate,
}

type ACL struct {
	Username    string              `json:"username"`
	Entity      string              `json:"entity"`
	Permissions map[Permission]bool `json:"permissions"`
}

func NewPermissions() map[Permission]bool {
	m := make(map[Permission]bool, len(permissionStrings))
	for p := range permissionStrings {
		m[p] = false
	}

	return m
}

func DefaultPermissionsForRole(role security.Role) map[Permission]bool {
	p := NewPermissions()

	if role == security.RoleAdmin {
		for perm := range p {
			p[perm] = true
		}

		return p
	}

	p[PermissionOwningRead] = true
	p[PermissionOwningWrite] = true
	p[PermissionOwningDelete] = true
	p[PermissionOwningUpdate] = true

	return p
}

func (a *ACL) ToDataMap() map[string]any {
	perms := make(map[string]any, len(a.Permissions))
	for p, v := range a.Permissions {
		perms[permissionStrings[p]] = v
	}

	return map[string]any{
		"id":          GetACLEntityId(a.Username, a.Entity),
		"username":    a.Username,
		"entity":      a.Entity,
		"permissions": perms,
	}
}

func (a *ACL) Can(p Permission) bool {
	return a.Permissions[p]
}

func (a *ACL) Grant(p Permission) {
	a.Permissions[p] = true
}

func (a *ACL) Revoke(p Permission) {
	a.Permissions[p] = false
}

func ACLEntitySchema() map[string]any {
	permissions := make(map[string]any, len(permissionStrings))
	for _, name := range permissionStrings {
		permissions[name] = map[string]any{
			"type":     "boolean",
			"required": true,
		}
	}

	return map[string]any{
		"username": map[string]any{
			"type":     "string",
			"required": true,
		},
		"entity": map[string]any{
			"type":     "string",
			"required": true,
		},
		"permissions": map[string]any{
			"type":     "object",
			"required": true,
			"fields":   permissions,
		},
	}
}

func InitACLStorage() {
	if !engine.EntityTypeExists(ACLEntity) {
		engine.AddEntityType(ACLEntity)
	}

	if !schema.IsManualSchema(ACLEntity, engine.GetEntitySchema(ACLEntity)) {
		engine.UpdateEntitySchema(ACLEntity, ACLEntitySchema())
	}
}

func InitACL() error {
	if !security.UserAuthenticationIsEnabled() {
		return nil
	}

	InitACLStorage()

	if err := saveACLList(GenerateACLFoAllrEntities()); err != nil {
		log.Error("Unable to create the default ACLs: ", err)
		return err
	}

	return nil
}

func saveACLList(acls []ACL) error {
	var errs []error
	for _, acl := range acls {
		id := GetACLEntityId(acl.Username, acl.Entity)
		if engine.EntityExists(ACLEntity, id) {
			continue
		}

		if err := writeACL(&acl); err != nil {
			errs = append(errs, err)
		}
	}

	return errors.Join(errs...)
}

func writeACL(a *ACL) error {
	validationErrors := engine.WriteEntity(ACLEntity, a.ToDataMap())
	if len(validationErrors) == 0 {
		return nil
	}

	messages := make([]string, 0, len(validationErrors))
	for _, e := range validationErrors {
		messages = append(messages, e.ToError().Error())
	}

	return fmt.Errorf("unable to write the ACL of %s on %s: %s", a.Username, a.Entity, strings.Join(messages, "; "))
}

func GenerateACLFoAllrEntities() []ACL {
	var acls []ACL

	users, err := security.ListBasicUsers()
	if err != nil {
		return acls
	}

	entities := engine.ListPublicEntityTypes()

	for _, user := range users {
		username, ok1 := user["username"].(string)
		roleStr, ok2 := user["role"].(string)
		if !ok1 || !ok2 {
			continue
		}

		role := security.Role(roleStr)

		for _, entity := range entities {
			acls = append(acls, ACL{
				Username:    username,
				Entity:      entity,
				Permissions: DefaultPermissionsForRole(role),
			})
		}
	}

	return acls
}

func GetACLEntityId(username, entity string) string {
	return username + "::" + entity
}

func GetACLEntityForUsername(entity, username string) *ACL {
	data := engine.ReadEntityById(ACLEntity, GetACLEntityId(username, entity))
	if data == nil {
		return nil
	}

	acl := &ACL{
		Permissions: NewPermissions(),
	}

	acl.Username, _ = data["username"].(string)
	acl.Entity, _ = data["entity"].(string)

	raw := data["permissions"]

	switch perms := raw.(type) {

	case map[string]bool:
		for k, v := range perms {
			if p, ok := stringToPermission[k]; ok {
				acl.Permissions[p] = v
			}
		}

	case map[string]any:
		for k, v := range perms {
			if p, ok := stringToPermission[k]; ok {
				if allowed, ok := v.(bool); ok {
					acl.Permissions[p] = allowed
				}
			}
		}
	}

	return acl
}

func UpdateACLEntityForUsername(entity, username string, permissions map[Permission]bool) error {
	existing := GetACLEntityForUsername(entity, username)
	if existing == nil {
		return fmt.Errorf("ACL does not exist for username %s and entity %s", username, entity)
	}

	existing.Permissions = permissions

	return writeACL(existing)
}

func StringToPermission(s string) (Permission, bool) {
	p, ok := stringToPermission[s]
	return p, ok
}

func DeleteUserACls(username string) error {
	entities := engine.ListPublicEntityTypes()

	for _, entity := range entities {
		id := GetACLEntityId(username, entity)
		if engine.EntityExists(ACLEntity, id) {
			engine.DeleteEntityById(ACLEntity, id)
		}
	}

	return nil
}

func DeleteACLForEntityType(entity string) error {
	users, err := security.ListBasicUsers()
	if err != nil {
		return err
	}

	for _, user := range users {
		username, ok := user["username"].(string)
		if !ok {
			continue
		}

		id := GetACLEntityId(username, entity)
		if engine.EntityExists(ACLEntity, id) {
			engine.DeleteEntityById(ACLEntity, id)
		}
	}

	return nil
}

func ResetACLEntityToDefault(entity, username string) error {
	users, err := security.ListBasicUsers()
	if err != nil {
		return err
	}

	var role security.Role
	found := false

	for _, user := range users {
		u, ok1 := user["username"].(string)
		r, ok2 := user["role"].(string)
		if ok1 && ok2 && u == username {
			role = security.Role(r)
			found = true
			break
		}
	}

	if !found {
		return fmt.Errorf("user %s not found", username)
	}

	permissions := DefaultPermissionsForRole(role)

	existing := GetACLEntityForUsername(entity, username)
	if existing == nil {
		existing = &ACL{
			Username:    username,
			Entity:      entity,
			Permissions: permissions,
		}
	} else {
		existing.Permissions = permissions
	}

	return writeACL(existing)
}
