package api

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"maps"
	"mime/multipart"
	"net"
	"net/http/httputil"
	"os"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

const (
	restorePath       = "/api/restore"
	backupPath        = "/api/backup"
	hiddenBodyHeader  = "X-Elysian-Internal-Body-Length"
	restoreReadIdle   = 5 * time.Minute
	backupWriteWindow = time.Hour
)

func isRoute(uri []byte, route string) bool {
	if !bytes.HasPrefix(uri, []byte(route)) {
		return false
	}

	return len(uri) == len(route) || uri[len(route)] == '?'
}

func StreamingRoutes(h *fasthttp.RequestHeader) fasthttp.RequestConfig {
	uri := h.RequestURI()
	switch {
	case h.IsGet() && isRoute(uri, backupPath):
		return fasthttp.RequestConfig{WriteTimeout: backupWriteWindow}
	case h.IsPost() && isRoute(uri, restorePath):
		h.Del(hiddenBodyHeader)
		length := h.ContentLength()
		if length > 0 || length == -1 {
			h.Set(hiddenBodyHeader, strconv.Itoa(length))
			h.SetContentLength(0)
		}
	}

	return fasthttp.RequestConfig{}
}

func CloseAfterUnreadBody(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		next(ctx)
		if len(ctx.Request.Header.Peek(hiddenBodyHeader)) > 0 && !ctx.Hijacked() {
			ctx.SetConnectionClose()
		}
	}
}

func RestoreController(ctx *fasthttp.RequestCtx) {
	if hidden := ctx.Request.Header.Peek(hiddenBodyHeader); len(hidden) > 0 {
		length, err := strconv.Atoi(string(hidden))
		if err == nil {
			restoreFromConnection(ctx, length)
			return
		}
	}

	ctx.Response.Header.Set("Content-Type", "application/json")

	src, closeSource, err := restoreSource(ctx)
	if err != nil {
		observe.Audit(ctx, "restore rejected", "reason", err.Error())
		sendRestoreError(ctx, fasthttp.StatusBadRequest, err.Error())
		return
	}

	defer closeSource()

	runRestore(ctx, src)
}

func runRestore(ctx *fasthttp.RequestCtx, src io.ReadSeeker) {
	start := time.Now()
	report, err := backup.Restore(src)
	if err != nil {
		var incomplete *backup.IncompleteRestoreError
		var invalid *backup.ArchiveError
		if !errors.As(err, &incomplete) && errors.As(err, &invalid) {
			observe.Audit(ctx, "restore rejected", "reason", err.Error())
			sendRestoreError(ctx, fasthttp.StatusBadRequest, err.Error())
			return
		}

		observe.SetError(ctx, err)
		sendRestoreError(ctx, fasthttp.StatusInternalServerError, err.Error())
		return
	}

	observe.Audit(ctx, "restore done",
		"entities", report.Entities,
		"schemas", report.Schemas,
		"kv_keys", report.KVKeys,
		"duration_ms", log.Since(start),
	)

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

type idleReader struct {
	c net.Conn
}

func (r idleReader) Read(p []byte) (int, error) {
	_ = r.c.SetReadDeadline(time.Now().Add(restoreReadIdle))

	return r.c.Read(p)
}

func restoreFromConnection(ctx *fasthttp.RequestCtx, length int) {
	contentType := string(ctx.Request.Header.ContentType())
	boundary := string(ctx.Request.Header.MultipartFormBoundary())

	observe.Defer(ctx)
	ctx.HijackSetNoResponse(true)
	ctx.Hijack(func(c net.Conn) {
		defer observe.FinishDeferred(ctx)

		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.Response.Header.SetConnectionClose()

		var body io.Reader = idleReader{c: c}
		if length < 0 {
			body = httputil.NewChunkedReader(bufio.NewReaderSize(body, 64<<10))
		} else {
			body = &exactReader{r: body, left: int64(length)}
		}

		file, err := receiveArchive(body, contentType, boundary)
		if file != nil {
			defer os.Remove(file.Name())
			defer file.Close()
		}

		switch {
		case err == nil:
			runRestore(ctx, file)
		case errors.Is(err, errBadArchiveUpload):
			observe.Audit(ctx, "restore rejected", "reason", err.Error())
			sendRestoreError(ctx, fasthttp.StatusBadRequest, errors.Unwrap(err).Error())
		default:
			observe.SetError(ctx, err)
			sendRestoreError(ctx, fasthttp.StatusInternalServerError, err.Error())
		}

		_ = c.SetWriteDeadline(time.Now().Add(restoreReadIdle))
		w := bufio.NewWriter(c)
		if err := ctx.Response.Write(w); err == nil {
			_ = w.Flush()
		}
	})
}

type exactReader struct {
	r    io.Reader
	left int64
}

func (e *exactReader) Read(p []byte) (int, error) {
	if e.left <= 0 {
		return 0, io.EOF
	}

	if int64(len(p)) > e.left {
		p = p[:e.left]
	}

	n, err := e.r.Read(p)
	e.left -= int64(n)
	if err == io.EOF && e.left > 0 {
		return n, io.ErrUnexpectedEOF
	}

	return n, err
}

var errBadArchiveUpload = errors.New("bad upload")

type uploadError struct {
	reason error
}

func (e *uploadError) Error() string { return e.reason.Error() }
func (e *uploadError) Unwrap() error { return e.reason }
func (e *uploadError) Is(target error) bool {
	return target == errBadArchiveUpload
}

func rejected(reason string) error {
	return &uploadError{reason: errors.New(reason)}
}

func receiveArchive(body io.Reader, contentType, boundary string) (*os.File, error) {
	if !strings.HasPrefix(contentType, "multipart/form-data") {
		file, err := backup.NewUpload()
		if err != nil {
			return nil, err
		}

		n, err := io.Copy(file, body)
		if err != nil {
			return file, err
		}

		if n == 0 {
			return file, rejected("the request body must be a backup archive")
		}

		_, err = file.Seek(0, io.SeekStart)

		return file, err
	}

	if boundary == "" {
		return nil, rejected("invalid multipart body")
	}

	reader := multipart.NewReader(body, boundary)
	files := map[string]*os.File{}
	defer func() {
		for _, f := range files {
			if f != nil {
				f.Close()
				os.Remove(f.Name())
			}
		}
	}()

	for {
		part, err := reader.NextPart()
		if err == io.EOF {
			break
		}

		if err != nil {
			return nil, rejected("invalid multipart body")
		}

		field := part.FormName()
		if part.FileName() == "" {
			continue
		}

		if _, seen := files[field]; seen {
			continue
		}

		file, err := backup.NewUpload()
		if err != nil {
			return nil, err
		}
		files[field] = file

		if _, err := io.Copy(file, part); err != nil {
			return nil, rejected("invalid multipart body")
		}
	}

	if len(files) == 0 {
		return nil, rejected("the multipart body holds no file")
	}

	first := slices.Sorted(maps.Keys(files))[0]
	file := files[first]
	files[first] = nil

	_, err := file.Seek(0, io.SeekStart)

	return file, err
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
