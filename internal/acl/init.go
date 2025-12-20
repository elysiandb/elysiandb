package acl

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
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
	perms := make(map[string]bool, len(a.Permissions))
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

func InitACL() {
	if !security.UserAuthenticationIsEnabled() {
		return
	}

	saveACLList(GenerateACLFoAllrEntities())
}

func saveACLList(acls []ACL) {
	for _, acl := range acls {
		id := GetACLEntityId(acl.Username, acl.Entity)
		if !engine.EntityExists(ACLEntity, id) {
			engine.WriteEntity(ACLEntity, acl.ToDataMap())
		}
	}
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

	engine.WriteEntity(ACLEntity, existing.ToDataMap())

	return nil
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

	engine.WriteEntity(ACLEntity, existing.ToDataMap())

	return nil
}
