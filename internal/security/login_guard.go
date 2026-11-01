package security

import (
	"sync"
	"time"
)

const (
	loginFailuresPerClientAndUser = 5
	loginFailuresPerClient        = 20
	loginLockoutMax               = 15 * time.Minute
	loginFailureMemory            = 15 * time.Minute
	loginGuardMaxEntries          = 100_000
)

type loginAttempts struct {
	failures    int
	lastFailure time.Time
	lockedUntil time.Time
}

type loginGuard struct {
	mu       sync.Mutex
	attempts map[string]*loginAttempts
	now      func() time.Time
}

var defaultLoginGuard = newLoginGuard()

func newLoginGuard() *loginGuard {
	return &loginGuard{attempts: map[string]*loginAttempts{}, now: time.Now}
}

func ResetLoginAttempts() {
	defaultLoginGuard.mu.Lock()
	defer defaultLoginGuard.mu.Unlock()

	defaultLoginGuard.attempts = map[string]*loginAttempts{}
}

func SetLoginClock(now func() time.Time) {
	defaultLoginGuard.mu.Lock()
	defer defaultLoginGuard.mu.Unlock()

	if now == nil {
		now = time.Now
	}

	defaultLoginGuard.now = now
}

func clientLoginKey(source LoginSource) string {
	return "client|" + source.clientKey()
}

func clientUserLoginKey(source LoginSource, username string) string {
	return "user|" + source.clientKey() + "|" + username
}

func (g *loginGuard) retryAfter(source LoginSource, username string) time.Duration {
	g.mu.Lock()
	defer g.mu.Unlock()

	now := g.now()
	wait := g.lockRemaining(clientLoginKey(source), now)
	if userWait := g.lockRemaining(clientUserLoginKey(source, username), now); userWait > wait {
		wait = userWait
	}

	return wait
}

func (g *loginGuard) lockRemaining(key string, now time.Time) time.Duration {
	entry, ok := g.attempts[key]
	if !ok {
		return 0
	}

	if g.forgotten(entry, now) {
		delete(g.attempts, key)
		return 0
	}

	if now.Before(entry.lockedUntil) {
		return entry.lockedUntil.Sub(now)
	}

	return 0
}

func (g *loginGuard) recordFailure(source LoginSource, username string) {
	g.mu.Lock()
	defer g.mu.Unlock()

	now := g.now()
	g.fail(clientLoginKey(source), loginFailuresPerClient, now)
	g.fail(clientUserLoginKey(source, username), loginFailuresPerClientAndUser, now)
}

func (g *loginGuard) recordSuccess(source LoginSource, username string) {
	g.mu.Lock()
	defer g.mu.Unlock()

	delete(g.attempts, clientUserLoginKey(source, username))
}

func (g *loginGuard) fail(key string, threshold int, now time.Time) {
	entry, ok := g.attempts[key]
	if ok && g.forgotten(entry, now) {
		ok = false
	}

	if !ok {
		g.makeRoom(now)
		entry = &loginAttempts{}
		g.attempts[key] = entry
	}

	entry.failures++
	entry.lastFailure = now

	if entry.failures >= threshold {
		entry.lockedUntil = now.Add(lockoutDuration(entry.failures - threshold))
	}
}

func (g *loginGuard) forgotten(entry *loginAttempts, now time.Time) bool {
	return !now.Before(entry.lockedUntil) && now.Sub(entry.lastFailure) > loginFailureMemory
}

func (g *loginGuard) makeRoom(now time.Time) {
	if len(g.attempts) < loginGuardMaxEntries {
		return
	}

	for key, entry := range g.attempts {
		if g.forgotten(entry, now) {
			delete(g.attempts, key)
		}
	}

	if len(g.attempts) < loginGuardMaxEntries {
		return
	}

	oldestKey := ""
	var oldest time.Time
	for key, entry := range g.attempts {
		if now.Before(entry.lockedUntil) {
			continue
		}

		if oldestKey == "" || entry.lastFailure.Before(oldest) {
			oldestKey = key
			oldest = entry.lastFailure
		}
	}

	if oldestKey != "" {
		delete(g.attempts, oldestKey)
	}
}

func lockoutDuration(excess int) time.Duration {
	if excess >= 10 {
		return loginLockoutMax
	}

	duration := time.Second << excess
	if duration > loginLockoutMax {
		return loginLockoutMax
	}

	return duration
}
