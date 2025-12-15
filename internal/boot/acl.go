package boot

import "github.com/taymour/elysiandb/internal/acl"

func BootACL() {
	acl.InitACL()
}
