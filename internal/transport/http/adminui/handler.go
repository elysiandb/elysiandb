package adminui

import (
	"path"

	"github.com/taymour/elysiandb/adminui"
	"github.com/valyala/fasthttp"
)

var AdminUIHandler func(ctx *fasthttp.RequestCtx) = func(ctx *fasthttp.RequestCtx) {
	p := string(ctx.Path())

	if p == "/admin" || p == "/admin/" {
		p = "/admin/index.html"
	}

	rel := p[len("/admin"):]
	rel = path.Clean(rel)

	fsPath := "dist" + rel

	data, err := adminui.UI.ReadFile(fsPath)
	if err != nil {
		index, err := adminui.UI.ReadFile("dist/index.html")
		if err != nil {
			ctx.SetStatusCode(404)
			return
		}
		ctx.SetContentType("text/html")
		ctx.SetBody(index)
		return
	}

	ext := path.Ext(fsPath)
	switch ext {
	case ".js":
		ctx.SetContentType("text/javascript")
	case ".css":
		ctx.SetContentType("text/css")
	case ".html":
		ctx.SetContentType("text/html")
	case ".svg":
		ctx.SetContentType("image/svg+xml")
	case ".png":
		ctx.SetContentType("image/png")
	case ".ico":
		ctx.SetContentType("image/x-icon")
	}

	ctx.SetBody(data)
}
