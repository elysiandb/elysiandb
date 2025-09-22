package storage

func isBareStar(p string) bool {
	if len(p) != 1 {
		return false
	}
	return p[0] == '*'
}

func MatchGlob(pattern string, s string) bool {
	p := pattern
	i, j := 0, 0
	star := -1
	match := 0

	for j < len(s) {
		if i < len(p) && p[i] == '\\' {
			if i+1 >= len(p) {
				return false
			}

			if p[i+1] != s[j] {
				if star != -1 {
					i = star + 1
					match++
					j = match

					continue
				}

				return false
			}

			i += 2
			j++

			continue
		}

		if i < len(p) && p[i] == '*' {
			star = i
			i++
			match = j

			continue
		}

		if i < len(p) && p[i] == s[j] {
			i++
			j++

			continue
		}

		if star != -1 {
			i = star + 1
			match++
			j = match

			continue
		}

		return false
	}

	for i < len(p) {
		if p[i] == '\\' {
			return false
		}

		if p[i] != '*' {
			return false
		}

		i++
	}

	return true
}
