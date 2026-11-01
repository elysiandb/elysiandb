package api

import (
	"fmt"
	"io"
	"os"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

type temporaryFile struct {
	*os.File
}

func (f temporaryFile) Close() error {
	err := f.File.Close()
	os.Remove(f.Name())

	return err
}

func BackupController(ctx *fasthttp.RequestCtx) {
	now := time.Now().UTC()

	start := time.Now()
	file, size, err := createTemporaryBackup(now)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"unable to create the backup"}`)
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/gzip")
	ctx.Response.Header.Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s"`, backup.FileName(now)))
	observe.Audit(ctx, "backup created", "output", "download", "bytes", size, "duration_ms", log.Since(start))
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyStream(file, int(size))
}

func createTemporaryBackup(now time.Time) (*temporaryFile, int64, error) {
	file, err := os.CreateTemp("", "elysiandb-backup-*"+backup.FileNameExtension)
	if err != nil {
		return nil, 0, err
	}

	tmp := &temporaryFile{File: file}
	if _, err := backup.Create(file, now); err != nil {
		tmp.Close()
		return nil, 0, err
	}

	size, err := file.Seek(0, io.SeekCurrent)
	if err == nil {
		_, err = file.Seek(0, io.SeekStart)
	}

	if err != nil {
		tmp.Close()
		return nil, 0, err
	}

	return tmp, size, nil
}
