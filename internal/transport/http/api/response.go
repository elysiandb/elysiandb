package api

import (
	"bufio"
	"bytes"
	"fmt"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

const streamThreshold = 1 << 20

func sendDeadlineExceeded(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusServiceUnavailable)
	ctx.SetBodyString(`{"error":"request deadline exceeded"}`)
}

func countPlanned(res *api_storage.ListResult, paged bool, mode acl.ReadMode, username string) int {
	switch mode {
	case acl.ReadNone:
		return 0
	case acl.ReadOwned:
		n := 0
		for _, d := range res.Docs {
			if doc, err := d.Decode(); err == nil && acl.OwnedBy(doc, username) {
				n++
			}
		}
		return n
	}

	if paged {
		return len(res.Docs)
	}

	return res.Total
}

func cacheDeps(entity, includes string) []string {
	return api_storage.CacheDependencies(entity, includes, security.UserAuthenticationIsEnabled())
}

func writeCount(ctx *fasthttp.RequestCtx, count int, cacheEntity string, hash []byte, deps []string) {
	response := []byte(`{"count":` + fmt.Sprintf("%d", count) + `}`)

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)

	if globals.GetConfig().Api.Cache.Enabled && hash != nil {
		cache.CacheStore.SetWithDeps(cacheEntity, hash, response, deps)
	}
}

func documentBytes(docs []datastore.Doc) ([][]byte, int, error) {
	parts := make([][]byte, 0, len(docs))
	size := 2
	for _, d := range docs {
		b, err := d.Bytes()
		if err != nil {
			return nil, 0, err
		}

		parts = append(parts, b)
		size += len(b) + 1
	}

	return parts, size, nil
}

func writeDocuments(ctx *fasthttp.RequestCtx, docs []datastore.Doc, cacheEntity string, hash []byte, cacheHeader bool, deps []string) error {
	parts, size, err := documentBytes(docs)
	if err != nil {
		return err
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	if cacheHeader {
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
	}
	ctx.SetStatusCode(fasthttp.StatusOK)

	cacheable := globals.GetConfig().Api.Cache.Enabled && hash != nil && cache.CacheStore.Accepts(size)
	if !cacheable && size > streamThreshold {
		ctx.SetBodyStreamWriter(func(w *bufio.Writer) {
			_ = w.WriteByte('[')
			for i, p := range parts {
				if i > 0 {
					_ = w.WriteByte(',')
				}
				_, _ = w.Write(p)
				if i%64 == 63 {
					_ = w.Flush()
				}
			}
			_ = w.WriteByte(']')
		})

		return nil
	}

	var body bytes.Buffer
	body.Grow(size)
	body.WriteByte('[')
	for i, p := range parts {
		if i > 0 {
			body.WriteByte(',')
		}
		body.Write(p)
	}
	body.WriteByte(']')

	ctx.SetBody(body.Bytes())
	if cacheable {
		cache.CacheStore.SetWithDeps(cacheEntity, hash, body.Bytes(), deps)
	}

	return nil
}
