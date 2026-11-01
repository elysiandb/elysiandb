package security

import (
	"crypto/subtle"
	"fmt"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

func AuthenticateUserFrom(username, password string, source LoginSource) (*BasicHashedUser, time.Duration, bool) {
	if wait := defaultLoginGuard.retryAfter(source, username); wait > 0 {
		return nil, wait, false
	}

	user, ok := AuthenticateUser(username, password)
	if ok && !source.Local && IsDefaultAdminCredentials(username, password) {
		log.Warn(fmt.Sprintf("Login refused for %q from %s: the default admin password is only accepted from the local machine, change it to log in remotely", username, source))
		ok = false
	}

	if !ok {
		defaultLoginGuard.recordFailure(source, username)
		log.Warn(fmt.Sprintf("Failed login for %q from %s", username, source))

		return nil, 0, false
	}

	defaultLoginGuard.recordSuccess(source, username)

	return user, 0, true
}

func IsDefaultAdminCredentials(username, password string) bool {
	return subtle.ConstantTimeCompare([]byte(username), []byte(DefaultAdminUsername)) == 1 &&
		subtle.ConstantTimeCompare([]byte(password), []byte(DefaultAdminPassword)) == 1
}

func DefaultAdminPasswordIsInUse() bool {
	_, ok := AuthenticateUser(DefaultAdminUsername, DefaultAdminPassword)

	return ok
}

func RetryAfterSeconds(wait time.Duration) int {
	seconds := int((wait + time.Second - 1) / time.Second)
	if seconds < 1 {
		return 1
	}

	return seconds
}

func WarnIfDefaultAdminPassword() {
	if !BasicAuthenticationIsEnabled() && !UserAuthenticationIsEnabled() {
		return
	}

	if DefaultAdminPasswordIsInUse() {
		log.DirectWarn(fmt.Sprintf("The %q account still uses the default password: it is only accepted from this machine. Change it from the Admin UI or with `elysiandb change-password`, or set %s before the first start.", DefaultAdminUsername, AdminPasswordEnvVar))
	}
}
