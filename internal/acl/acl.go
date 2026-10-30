package acl

import (
	"errors"
	"fmt"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/security"
)

var ErrSubEntityForbidden = errors.New("forbidden")

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

	return canUpdate(GetACLEntityForUsername(entity, username), username, data)
}

func canUpdate(acl *ACL, username string, data map[string]any) bool {
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

func CheckSubEntityWrites(data map[string]any) error {
	if !security.UserAuthenticationIsEnabled() {
		return api_storage.RejectCoreSubEntities(data)
	}

	username := security.GetCurrentUsername()

	return api_storage.VisitSubEntityWrites(data, func(entity, id string, sub map[string]any) error {
		if err := api_storage.CheckSubEntityType(entity); err != nil {
			return err
		}

		var stored map[string]any
		if id != "" {
			stored = engine.ReadEntityById(entity, id)
		}

		if stored != nil {
			if !canUpdate(GetACLEntityForUsername(entity, username), username, stored) {
				return fmt.Errorf("%w: cannot update %s", ErrSubEntityForbidden, entity)
			}

			if owner, ok := stored[UsernameField]; ok {
				sub[UsernameField] = owner
			} else {
				delete(sub, UsernameField)
			}

			return nil
		}

		if engine.EntityTypeExists(entity) && !canCreate(GetACLEntityForUsername(entity, username)) {
			return fmt.Errorf("%w: cannot create %s", ErrSubEntityForbidden, entity)
		}

		sub[UsernameField] = username

		return nil
	})
}

func canCreate(acl *ACL) bool {
	return acl != nil && (acl.Can(PermissionCreate) || acl.Can(PermissionOwningWrite))
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

		if !acl.Can(PermissionOwningUpdate) || dataUsername != username {
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
