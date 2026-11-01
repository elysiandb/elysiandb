package storage

func DeepCopyDocument(document map[string]any) map[string]any {
	if document == nil {
		return nil
	}

	copied := make(map[string]any, len(document))
	for key, value := range document {
		copied[key] = deepCopyValue(value)
	}

	return copied
}

func deepCopyValue(value any) any {
	switch typed := value.(type) {
	case map[string]any:
		return DeepCopyDocument(typed)
	case []any:
		copied := make([]any, len(typed))
		for i, item := range typed {
			copied[i] = deepCopyValue(item)
		}
		return copied
	case []map[string]any:
		copied := make([]map[string]any, len(typed))
		for i, item := range typed {
			copied[i] = DeepCopyDocument(item)
		}
		return copied
	case []string:
		return append([]string(nil), typed...)
	case map[string]string:
		copied := make(map[string]string, len(typed))
		for key, item := range typed {
			copied[key] = item
		}
		return copied
	default:
		return value
	}
}
