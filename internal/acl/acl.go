package acl

import (
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func CanCreateEntity(username, entity string) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return canCreateNewEntityType(username, entity)
	}

	return acl.Can(PermissionCreate)
}

func canCreateNewEntityType(username, entity string) bool {
	if strings.HasPrefix(entity, globals.CoreFieldsPrefix) || api_storage.EntityTypeExists(entity) {
		return false
	}

	user, err := security.GetBasicUserByUsername(username)
	if err != nil {
		return false
	}

	role, _ := user["role"].(string)

	return DefaultPermissionsForRole(security.Role(role))[PermissionCreate]
}

func CanWriteEntity(username, entity string, data map[string]any) bool {
	return CanWriteListOfEntities(username, entity, []map[string]any{data})
}

func CanWriteListOfEntities(username, entity string, list []map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	if !CanCreateEntity(username, entity) {
		return false
	}

	for _, item := range list {
		id, ok := item["id"].(string)
		if !ok || id == "" || !api_storage.EntityExists(entity, id) {
			continue
		}

		if !CanUpdateEntity(username, entity, id, item) {
			return false
		}
	}

	return true
}

func CanDeleteEntity(username, entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionDelete) {
		return true
	}

	dataUsername, ok := data[UsernameField].(string)
	if !ok || dataUsername == "" {
		return false
	}

	return acl.Can(PermissionOwningDelete) && dataUsername == username
}

func CanUpdateEntity(username, entity, id string, updates map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionUpdate) {
		return true
	}

	return canOwningUpdate(acl, entity, id, username, updates)
}

func CanUpdateListOfEntities(username, entity string, updates []map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionUpdate) {
		return true
	}

	for _, item := range updates {
		id, ok := item["id"].(string)
		if !ok || id == "" {
			return false
		}

		if !canOwningUpdate(acl, entity, id, username, item) {
			return false
		}
	}

	return true
}

func canOwningUpdate(acl *ACL, entity, id, username string, updates map[string]any) bool {
	if !acl.Can(PermissionOwningUpdate) {
		return false
	}

	existing := api_storage.ReadEntityById(entity, id)
	owner, ok := existing[UsernameField].(string)
	if !ok || owner == "" || owner != username {
		return false
	}

	newOwner, changesOwner := updates[UsernameField]

	return !changesOwner || newOwner == username
}

func CanReadEntity(username, entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionRead) {
		return true
	}

	dataUsername, ok := data[UsernameField].(string)
	if !ok || dataUsername == "" {
		return false
	}

	return acl.Can(PermissionOwningRead) && dataUsername == username
}

func FilterListOfEntities(username, entity string, data []map[string]any) []map[string]any {
	if !security.UserAuthenticationIsEnabled() {
		return data
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return []map[string]any{}
	}

	if acl.Can(PermissionRead) {
		return data
	}

	filteredData := make([]map[string]any, 0)
	for _, item := range data {
		dataUsername, ok := item[UsernameField].(string)
		if !ok || dataUsername == "" {
			continue
		}

		if acl.Can(PermissionOwningRead) && dataUsername == username {
			filteredData = append(filteredData, item)
		}
	}

	return filteredData
}

type ReadMode int

const (
	ReadAll ReadMode = iota
	ReadNone
	ReadOwned
)

func ReadModeFor(username, entity string) ReadMode {
	if !security.UserAuthenticationIsEnabled() {
		return ReadAll
	}

	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return ReadNone
	}

	if acl.Can(PermissionRead) {
		return ReadAll
	}

	if acl.Can(PermissionOwningRead) {
		return ReadOwned
	}

	return ReadNone
}

func OwnedBy(data map[string]any, username string) bool {
	owner, ok := data[UsernameField].(string)
	return ok && owner != "" && owner == username
}
