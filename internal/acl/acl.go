package acl

import (
	"github.com/taymour/elysiandb/internal/security"
)

func CanCreateEntity(entity string) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	return acl.Can(PermissionCreate)
}

func CanDeleteEntity(entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
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

func CanUpdateEntity(entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionUpdate) {
		return true
	}

	dataUsername, ok := data[UsernameField].(string)
	if !ok || dataUsername == "" {
		return false
	}

	return acl.Can(PermissionOwningUpdate) && dataUsername == username
}

func CanUpdateListOfEntities(entity string, data []map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
	acl := GetACLEntityForUsername(entity, username)
	if acl == nil {
		return false
	}

	if acl.Can(PermissionUpdate) {
		return true
	}

	for _, item := range data {
		dataUsername, ok := item[UsernameField].(string)
		if !ok || dataUsername == "" {
			return false
		}

		if !(acl.Can(PermissionOwningUpdate) && dataUsername == username) {
			return false
		}
	}

	return true
}

func CanReadEntity(entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
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

func FilterListOfEntities(entity string, data []map[string]any) []map[string]any {
	if !security.UserAuthenticationIsEnabled() {
		return data
	}

	username := security.GetCurrentUsername()
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
