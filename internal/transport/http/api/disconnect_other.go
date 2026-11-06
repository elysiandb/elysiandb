//go:build !unix

package api

import "net"

func disconnectProbe(conn net.Conn) func() bool {
	return nil
}
