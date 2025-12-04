package tcprouting

import (
	"net"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/tcp/handler"
	"github.com/taymour/elysiandb/internal/transport/tcp/parsing"
)

func RouteLine(line []byte, c net.Conn) []byte {
	cmd, query := parsing.FirstWordBytes(line)

	switch {
	case parsing.EqASCII(cmd, []byte("PING")):
		return []byte("PONG")

	case parsing.EqASCII(cmd, []byte("EXIT")):
		_ = c.Close()
		return []byte("Goodbye!")

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
		return handler.HandleReset()

	case parsing.EqASCII(cmd, []byte("SAVE")):
		return handler.HandleSave()
	}

	log.Error("Unknown command:", string(cmd))

	return []byte("ERR")
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
