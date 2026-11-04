package api

import (
	"bufio"
	"encoding/json"
	"slices"
	"sort"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

const accountsNotImportedMessage = "accounts and ACLs are not imported, use a backup and restore to move them"

var accountEntities = []string{acl.ACLEntity, security.UserEntity}

func ExportController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)

	if true {
		streamExport(ctx)
		return
	}

	dump := api_storage.DumpAll()
	for _, entity := range accountEntities {
		delete(dump, entity)
	}

	response, err := json.Marshal(dump)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"failed to marshal response"}`))
		return
	}

	ctx.SetBody(response)
}

func streamExport(ctx *fasthttp.RequestCtx) {
	e := storage.Engine()
	if e == nil {
		ctx.SetBodyString("{}")
		return
	}

	snaps := e.SnapshotAll()
	skipped := map[string]bool{schema.SchemaEntity: true}
	for _, entity := range accountEntities {
		skipped[entity] = true
	}

	names := []string{}
	for _, t := range e.Types() {
		if !skipped[t] {
			names = append(names, t)
		}
	}
	sort.Strings(names)
	names = slices.Compact(names)

	ctx.SetBodyStreamWriter(func(w *bufio.Writer) {
		_ = w.WriteByte('{')
		for i, name := range names {
			if i > 0 {
				_ = w.WriteByte(',')
			}

			key, _ := json.Marshal(name)
			_, _ = w.Write(key)
			_, _ = w.WriteString(":[")

			if snap, ok := snaps[name]; ok {
				first := true
				snap.Ascend(0, func(d datastore.Doc) bool {
					b, err := d.Bytes()
					if err != nil {
						return true
					}

					if !first {
						_ = w.WriteByte(',')
					}
					first = false
					_, _ = w.Write(b)

					return true
				})
			}

			_ = w.WriteByte(']')
			_ = w.Flush()
		}
		_ = w.WriteByte('}')
	})
}
