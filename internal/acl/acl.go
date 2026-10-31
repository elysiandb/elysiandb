package acl

import (
	"errors"
	"fmt"
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/security"
)

var ErrForbidden = errors.New("forbidden")

func CanCreateEntity(entity string) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	return canCreate(writableACL(entity, security.GetCurrentUsername()))
}

func CanDeleteEntity(entity string, data map[string]any) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	username := security.GetCurrentUsername()
	acl := writableACL(entity, username)
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

	return canUpdate(writableACL(entity, username), username, data)
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

func CheckEntityWrite(entity string, data map[string]any) error {
	if !security.UserAuthenticationIsEnabled() {
		return nil
	}

	id, _ := data["id"].(string)

	return authorizeWrite(entity, id, security.GetCurrentUsername(), data)
}

func CheckEntityUpdate(entity, id string, data map[string]any) error {
	if !security.UserAuthenticationIsEnabled() {
		return nil
	}

	username := security.GetCurrentUsername()
	stored := engine.ReadEntityById(entity, id)
	if !canUpdate(writableACL(entity, username), username, stored) {
		return fmt.Errorf("%w: cannot update %s", ErrForbidden, entity)
	}

	keepStoredOwner(data, stored)

	return nil
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

		return authorizeWrite(entity, id, username, sub)
	})
}

func authorizeWrite(entity, id, username string, data map[string]any) error {
	var stored map[string]any
	if id != "" {
		stored = engine.ReadEntityById(entity, id)
	}

	if stored != nil {
		if !canUpdate(writableACL(entity, username), username, stored) {
			return fmt.Errorf("%w: cannot update %s", ErrForbidden, entity)
		}

		keepStoredOwner(data, stored)

		return nil
	}

	if isCoreEntityType(entity) || (engine.EntityTypeExists(entity) && !canCreate(writableACL(entity, username))) {
		return fmt.Errorf("%w: cannot create %s", ErrForbidden, entity)
	}

	data[UsernameField] = username

	return nil
}

func keepStoredOwner(data, stored map[string]any) {
	if owner, ok := stored[UsernameField]; ok {
		data[UsernameField] = owner
	} else {
		delete(data, UsernameField)
	}
}

func canCreate(acl *ACL) bool {
	return acl != nil && (acl.Can(PermissionCreate) || acl.Can(PermissionOwningWrite))
}

func writableACL(entity, username string) *ACL {
	if isCoreEntityType(entity) {
		return nil
	}

	return GetACLEntityForUsername(entity, username)
}

func isCoreEntityType(entity string) bool {
	return strings.HasPrefix(entity, api_storage.CoreEntityTypePrefix)
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
