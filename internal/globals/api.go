package globals

import "fmt"

const (
	ApiEntityPattern                   = "api:entity:%s"
	ApiEntitiesPattern                 = "api:entity:%s:*"
	ApiSingleEntityPattern             = "api:entity:%s:id:%s"
	ApiEntityIndexIdPattern            = "api:entity:%s:internal:index:id"
	ApiEntityIndexPattern              = "api:entity:%s:internal:index:*"
	ApiEntityIndexFieldFilterPattern   = "api:entity:%s:internal:index:field:%s:filter"
	ApiEntityIndexAllFieldsPattern     = "api:entity:%s:internal:index:fields:all"
	ApiEntityIndexFieldAllPattern      = "api:entity:%s:internal:index:field:%s:*"
	ApiEntityIndexFieldSortAscPattern  = "api:entity:%s:internal:index:field:%s:sort:asc"
	ApiEntityIndexFieldSortDescPattern = "api:entity:%s:internal:index:field:%s:sort:desc"
)

func ApiEntityKey(entity string) string {
	return fmt.Sprintf(ApiEntityPattern, entity)
}

func ApiEntitiesAllKey(entity string) string {
	return fmt.Sprintf(ApiEntitiesPattern, entity)
}

func ApiSingleEntityKey(entity string, id string) string {
	return fmt.Sprintf(ApiSingleEntityPattern, entity, id)
}

func ApiEntityIndexIdKey(entity string) string {
	return fmt.Sprintf(ApiEntityIndexIdPattern, entity)
}

func ApiEntityIndexFieldKey(entity string, field string) string {
	return fmt.Sprintf(ApiEntityIndexFieldFilterPattern, entity, field)
}

func ApiEntityIndexFieldSortAscKey(entity string, field string) string {
	return fmt.Sprintf(ApiEntityIndexFieldSortAscPattern, entity, field)
}

func ApiEntityIndexFieldSortDescKey(entity string, field string) string {
	return fmt.Sprintf(ApiEntityIndexFieldSortDescPattern, entity, field)
}

func ApiEntityIndexAllFieldsKey(entity string) string {
	return fmt.Sprintf(ApiEntityIndexAllFieldsPattern, entity)
}

func ApiEntityIndexPatternKey(entity string) string {
	return fmt.Sprintf(ApiEntityIndexPattern, entity)
}

func ApiEntityIndexFieldAllKey(entity string, field string) string {
	return fmt.Sprintf(ApiEntityIndexFieldAllPattern, entity, field)
}
