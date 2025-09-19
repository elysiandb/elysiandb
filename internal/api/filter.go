package api_storage

func FiltersMatchEntity(
	entityData map[string]interface{},
	filters map[string]string,
) bool {
	if len(filters) == 0 {
		return true
	}

	for field, value := range filters {
		if entityData[field] != value {
			return false
		}
	}

	return true
}
