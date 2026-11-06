package storage

import "github.com/taymour/elysiandb/internal/docpath"

func MatchGlob(pattern, s string) bool {
	return docpath.MatchGlob(pattern, s)
}
