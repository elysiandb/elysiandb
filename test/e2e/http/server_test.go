package e2e

import (
	"net"
	"testing"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

func startTestServer(t *testing.T) (*fasthttp.Client, func()) {
	t.Helper()

	return startTestServerWithConfig(t, &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: t.TempDir(),
			Shards: 8,
		},
		Stats: configuration.StatsConfig{
			Enabled: true,
		},
	})
}

func startTestServerWithConfig(t *testing.T, cfg *configuration.Config) (*fasthttp.Client, func()) {
	t.Helper()

	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()

	r := router.New()
	routing.RegisterRoutes(r)
	srv := &fasthttp.Server{Handler: r.Handler}

	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()

	client := &fasthttp.Client{
		Dial: func(addr string) (net.Conn, error) { return ln.Dial() },
	}

	teardown := func() {
		_ = ln.Close()
		_ = srv.Shutdown()
	}
	return client, teardown
}
