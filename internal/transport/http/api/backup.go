package api

import (
	"bufio"
	"fmt"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

const backupWriteIdle = 10 * time.Minute

func BackupController(ctx *fasthttp.RequestCtx) {
	now := time.Now().UTC()
	start := time.Now()

	src, err := backup.Prepare(now)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"unable to create the backup"}`)
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/gzip")
	ctx.Response.Header.Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s"`, backup.FileName(now)))
	ctx.SetStatusCode(fasthttp.StatusOK)

	audit := observe.AuditContext(ctx)
	conn := ctx.Conn()
	beat := func() { _ = conn.SetWriteDeadline(time.Now().Add(backupWriteIdle)) }

	ctx.SetBodyStreamWriter(func(w *bufio.Writer) {
		defer src.Close()

		counted := &writtenBytes{w: w}
		var err error
		durable.RunGently(func() {
			err = src.WriteArchive(counted, backup.Options{Beat: beat, Flush: w.Flush, Pace: true})
		})
		if err == nil {
			err = w.Flush()
		}

		if err != nil {
			audit("backup failed", "output", "download", "err", err.Error(), "duration_ms", log.Since(start))
			_ = conn.Close()
			return
		}

		audit("backup created", "output", "download", "bytes", counted.n, "duration_ms", log.Since(start))
	})
}

type writtenBytes struct {
	w *bufio.Writer
	n int64
}

func (c *writtenBytes) Write(p []byte) (int, error) {
	n, err := c.w.Write(p)
	c.n += int64(n)

	return n, err
}
