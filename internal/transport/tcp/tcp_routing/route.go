package tcprouting

import (
	"log/slog"
	"net"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/tcp/handler"
	"github.com/taymour/elysiandb/internal/transport/tcp/parsing"
)

var (
	tcpLog          = log.For("tcp")
	securityLog     = log.For("security")
	auditLog        = log.For("audit")
	tcpAuthThrottle = log.NewThrottle(time.Minute)
)

const maxLoggedCommandBytes = 64

type Session struct {
	authenticated bool
	actor         string
}

func NewSession() *Session {
	return &Session{authenticated: !security.CredentialsAreRequired()}
}

func (s *Session) IsAuthenticated() bool {
	return s.authenticated
}

func RouteLine(line []byte, c net.Conn, session *Session) []byte {
	cmd, query := parsing.FirstWordBytes(line)

	switch {
	case parsing.EqASCII(cmd, []byte("PING")):
		return []byte("PONG")

	case parsing.EqASCII(cmd, []byte("EXIT")):
		_ = c.Close()
		return []byte("Goodbye!")

	case parsing.EqASCII(cmd, []byte("AUTH")):
		source := security.LoginSourceFromAddr(c.RemoteAddr())
		if !security.AuthenticateCredentials(string(query), source) {
			if security.TokenAuthenticationIsEnabled() {
				logTokenAuthFailure(source.String())
			}
			return []byte("ERR invalid credentials")
		}

		session.authenticated = true
		session.actor = actorFromCredentials(string(query))

		return []byte("OK")
	}

	if !session.authenticated {
		return []byte("ERR auth required")
	}

	switch {
	case parsing.EqASCII(cmd, []byte("GET")):
		return handler.HandleGet(query)

	case parsing.EqASCII(cmd, []byte("MGET")):
		return handler.HandleMultiGet(query)

	case parsing.EqASCII(cmd, []byte("SET")):
		ttl := extractTTLFromQuery(&query)
		return handler.HandleSet(query, ttl)

	case parsing.EqASCII(cmd, []byte("DEL")):
		return handler.HandleDelete(query)

	case parsing.EqASCII(cmd, []byte("RESET")):
		resp := handler.HandleReset()
		auditLog.Info("store reset", "actor", session.actorName(), "source", "tcp", "remote_addr", remoteAddr(c))
		return resp

	case parsing.EqASCII(cmd, []byte("SAVE")):
		return handler.HandleSave()
	}

	if log.Enabled(slog.LevelDebug) {
		command := cmd
		if len(command) > maxLoggedCommandBytes {
			command = command[:maxLoggedCommandBytes]
		}
		tcpLog.Debug("unknown command", "remote_addr", remoteAddr(c), "command", string(command))
	}

	return []byte("ERR")
}

func ResetThrottles() {
	tcpAuthThrottle = log.NewThrottle(time.Minute)
}

func logTokenAuthFailure(addr string) {
	metrics.FailedLogin(addr)
	if suppressed, ok := tcpAuthThrottle.Allow(addr); ok {
		securityLog.Warn("tcp auth failed", log.WithSuppressed([]any{"remote_addr", addr, "auth_mode", "token"}, suppressed)...)
	}
}

func (s *Session) actorName() string {
	if s.actor != "" {
		return s.actor
	}

	if !security.CredentialsAreRequired() {
		return "anonymous"
	}

	return "unknown"
}

func actorFromCredentials(credentials string) string {
	if security.TokenAuthenticationIsEnabled() {
		return "token"
	}

	username, _, ok := strings.Cut(strings.TrimSpace(credentials), " ")
	if !ok {
		return ""
	}

	return username
}

func remoteAddr(c net.Conn) string {
	if c == nil || c.RemoteAddr() == nil {
		return "unknown"
	}

	return security.LoginSourceFromAddr(c.RemoteAddr()).String()
}

func extractTTLFromQuery(query *[]byte) int {
	ttlParam, rest := parsing.FirstWordBytes(*query)

	if len(ttlParam) < 4 {
		return 0
	}

	if parsing.EqASCII(ttlParam[:4], []byte("TTL=")) {
		ttl, err := parsing.ParseDecimalBytes(ttlParam[4:])
		if err != nil || ttl < 0 {
			return 0
		}

		*query = rest
		return ttl
	}

	return 0
}

func CommandName(line []byte) string {
	cmd, _ := parsing.FirstWordBytes(line)

	switch {
	case parsing.EqASCII(cmd, []byte("GET")):
		return metrics.TCPGet
	case parsing.EqASCII(cmd, []byte("MGET")):
		return metrics.TCPMget
	case parsing.EqASCII(cmd, []byte("SET")):
		return metrics.TCPSet
	case parsing.EqASCII(cmd, []byte("DEL")):
		return metrics.TCPDel
	}

	return metrics.TCPOther
}
