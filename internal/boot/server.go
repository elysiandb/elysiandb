package boot

import (
	"fmt"
	"time"

	"github.com/fasthttp/router"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func StartHTTP() {
	cfg := globals.GetConfig()

	addr := fmt.Sprintf("%s:%d", cfg.Server.HTTP.Host, cfg.Server.HTTP.Port)

	r := router.New()
	routing.RegisterRoutes(r)

	srv := &fasthttp.Server{
		Handler:               security.CORS(r.Handler),
		Name:                  "ElysianDB",
		NoDefaultServerHeader: true,
		ReduceMemoryUsage:     true,

		Concurrency: 100_000,

		ReadBufferSize:  64 << 10,
		WriteBufferSize: 64 << 10,

		ReadTimeout:  3 * time.Second,
		WriteTimeout: 3 * time.Second,
		IdleTimeout:  30 * time.Second,
	}

	log.DirectInfo("ElysianDB HTTP listening on http://", addr)
	if err := srv.ListenAndServe(addr); err != nil {
		log.Fatal("server error: ", err)
	}

	log.WriteLogs()
}
