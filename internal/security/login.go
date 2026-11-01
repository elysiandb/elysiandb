package security

import (
	"crypto/subtle"
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

var (
	securityLog         = log.For("security")
	failedLoginThrottle = log.NewThrottle(time.Minute)
)

const (
	LoginFailureBadCredentials          = "bad_credentials"
	LoginFailureDefaultPasswordRemotely = "default_password_remote"
	LoginFailureNotAdmin                = "not_admin"
	LoginFailureBadRequest              = "bad_request"
)

func LogLogin(username string, source LoginSource) {
	securityLog.Info("login succeeded", "user", username, "remote_addr", source.String())
}

func LogLogout(username string) {
	securityLog.Debug("logout", "user", username)
}

func LogFailedLogin(username string, source LoginSource, reason string) {
	addr := source.String()
	metrics.FailedLogin(addr)

	if suppressed, ok := failedLoginThrottle.Allow(addr + "|" + username); ok {
		securityLog.Warn("login failed", log.WithSuppressed([]any{"user", username, "remote_addr", addr, "reason", reason}, suppressed)...)
	}
}

func AuthenticateUserFrom(username, password string, source LoginSource) (*BasicHashedUser, time.Duration, bool) {
	if wait := defaultLoginGuard.retryAfter(source, username); wait > 0 {
		return nil, wait, false
	}

	user, ok := AuthenticateUser(username, password)
	reason := LoginFailureBadCredentials
	if ok && !source.Local && IsDefaultAdminCredentials(username, password) {
		reason = LoginFailureDefaultPasswordRemotely
		ok = false
	}

	if !ok {
		defaultLoginGuard.recordFailure(source, username)
		LogFailedLogin(username, source, reason)

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
		securityLog.Warn("default admin password still valid",
			"user", DefaultAdminUsername,
			"hint", "it is only accepted from this machine: change it from the Admin UI or with `elysiandb change-password`, or set "+AdminPasswordEnvVar+" before the first start",
		)
	}
}
