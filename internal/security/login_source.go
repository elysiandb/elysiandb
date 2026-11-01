package security

import (
	"net"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

type LoginSource struct {
	IP    net.IP
	Local bool
}

func (s LoginSource) clientKey() string {
	if s.IP == nil {
		return "unknown"
	}

	if v4 := s.IP.To4(); v4 != nil {
		return v4.String()
	}

	return s.IP.Mask(net.CIDRMask(64, 128)).String() + "/64"
}

func (s LoginSource) String() string {
	if s.IP == nil {
		return "unknown"
	}

	return s.IP.String()
}

var forwardingHeaders = []string{"X-Forwarded-For", "X-Real-Ip", "Forwarded"}

func LoginSourceFromRequest(ctx *fasthttp.RequestCtx) LoginSource {
	peer := ctx.RemoteIP()
	proxies := trustedProxies()

	if ipIsTrusted(peer, proxies) {
		if client := forwardedClientIP(ctx, proxies); client != nil {
			return LoginSource{IP: client, Local: client.IsLoopback()}
		}
	}

	return LoginSource{IP: peer, Local: peer.IsLoopback() && !requestIsForwarded(ctx)}
}

func LoginSourceFromAddr(addr net.Addr) LoginSource {
	tcpAddr, ok := addr.(*net.TCPAddr)
	if !ok || tcpAddr == nil {
		return LoginSource{}
	}

	return LoginSource{IP: tcpAddr.IP, Local: tcpAddr.IP.IsLoopback()}
}

func requestIsForwarded(ctx *fasthttp.RequestCtx) bool {
	for _, header := range forwardingHeaders {
		if len(ctx.Request.Header.Peek(header)) > 0 {
			return true
		}
	}

	return false
}

func forwardedClientIP(ctx *fasthttp.RequestCtx, proxies []*net.IPNet) net.IP {
	var hops []string
	for _, value := range ctx.Request.Header.PeekAll("X-Forwarded-For") {
		hops = append(hops, strings.Split(string(value), ",")...)
	}

	var leftmost net.IP
	for i := len(hops) - 1; i >= 0; i-- {
		ip := net.ParseIP(strings.TrimSpace(hops[i]))
		if ip == nil {
			return nil
		}

		if !ipIsTrusted(ip, proxies) {
			return ip
		}

		leftmost = ip
	}

	return leftmost
}

func trustedProxies() []*net.IPNet {
	cfg := globals.GetConfig()
	if cfg == nil {
		return nil
	}

	var networks []*net.IPNet
	for _, entry := range cfg.Server.HTTP.TrustedProxies {
		if network := parseNetwork(strings.TrimSpace(entry)); network != nil {
			networks = append(networks, network)
		}
	}

	return networks
}

func parseNetwork(entry string) *net.IPNet {
	if _, network, err := net.ParseCIDR(entry); err == nil {
		return network
	}

	ip := net.ParseIP(entry)
	if ip == nil {
		return nil
	}

	if v4 := ip.To4(); v4 != nil {
		return &net.IPNet{IP: v4, Mask: net.CIDRMask(32, 32)}
	}

	return &net.IPNet{IP: ip, Mask: net.CIDRMask(128, 128)}
}

func ipIsTrusted(ip net.IP, proxies []*net.IPNet) bool {
	if ip == nil {
		return false
	}

	for _, network := range proxies {
		if network.Contains(ip) {
			return true
		}
	}

	return false
}
