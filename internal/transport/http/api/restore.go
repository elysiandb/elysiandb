package api

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"maps"
	"slices"
	"strings"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/valyala/fasthttp"
)

func RestoreController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	src, closeSource, err := restoreSource(ctx)
	if err != nil {
		sendRestoreError(ctx, fasthttp.StatusBadRequest, err.Error())
		return
	}

	defer closeSource()

	report, err := backup.Restore(src)
	if err != nil {
		var incomplete *backup.IncompleteRestoreError
		var invalid *backup.ArchiveError
		if !errors.As(err, &incomplete) && errors.As(err, &invalid) {
			sendRestoreError(ctx, fasthttp.StatusBadRequest, err.Error())
			return
		}

		log.Error("Unable to restore a backup: ", err)
		sendRestoreError(ctx, fasthttp.StatusInternalServerError, err.Error())
		return
	}

	body, _ := json.Marshal(map[string]any{
		"status":          "restore completed",
		"entities":        report.Entities,
		"schemas":         report.Schemas,
		"kv_keys":         report.KVKeys,
		"kv_keys_expired": report.ExpiredKVKeys,
	})

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(body)
}

func restoreSource(ctx *fasthttp.RequestCtx) (io.ReadSeeker, func(), error) {
	if !strings.HasPrefix(string(ctx.Request.Header.ContentType()), "multipart/form-data") {
		body := ctx.PostBody()
		if len(body) == 0 {
			return nil, nil, errors.New("the request body must be a backup archive")
		}

		return bytes.NewReader(body), func() {}, nil
	}

	form, err := ctx.MultipartForm()
	if err != nil {
		return nil, nil, errors.New("invalid multipart body")
	}

	for _, field := range slices.Sorted(maps.Keys(form.File)) {
		for _, header := range form.File[field] {
			file, err := header.Open()
			if err != nil {
				return nil, nil, errors.New("the uploaded archive cannot be read")
			}

			return file, func() { file.Close() }, nil
		}
	}

	return nil, nil, errors.New("the multipart body holds no file")
}

func sendRestoreError(ctx *fasthttp.RequestCtx, status int, message string) {
	body, _ := json.Marshal(map[string]string{"error": message})
	ctx.SetStatusCode(status)
	ctx.SetBody(body)
}
