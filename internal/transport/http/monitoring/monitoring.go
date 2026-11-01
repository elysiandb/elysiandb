package monitoring

import (
	"bytes"
	"crypto/subtle"
	"encoding/json"
	"fmt"
	"log/slog"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

const maxLevelMinutes = 24 * 60

func writeJSON(ctx *fasthttp.RequestCtx, status int, body any) {
	data, err := json.Marshal(body)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	ctx.SetContentType("application/json")
	ctx.SetStatusCode(status)
	ctx.SetBody(data)
}

func writeError(ctx *fasthttp.RequestCtx, status int, message string) {
	writeJSON(ctx, status, map[string]string{"error": message})
}

func OverviewController(ctx *fasthttp.RequestCtx) {
	writeJSON(ctx, fasthttp.StatusOK, metrics.CurrentOverview())
}

func DetailsController(ctx *fasthttp.RequestCtx) {
	writeJSON(ctx, fasthttp.StatusOK, metrics.CurrentDetails())
}

func SeriesController(ctx *fasthttp.RequestCtx) {
	rangeName := string(ctx.QueryArgs().Peek("range"))
	if rangeName == "" {
		rangeName = "15m"
	}

	since, err := optionalInt(ctx, "since")
	if err != nil {
		writeError(ctx, fasthttp.StatusBadRequest, "since must be a unix time in milliseconds")
		return
	}

	series, ok := metrics.Series(rangeName, since)
	if !ok {
		writeError(ctx, fasthttp.StatusBadRequest, "range must be 15m, 1h or 24h")
		return
	}

	if cfg := globals.GetConfig(); cfg != nil {
		series.SlowRequestMs = cfg.Log.SlowRequestMs
	}

	writeJSON(ctx, fasthttp.StatusOK, series)
}

func LogsController(ctx *fasthttp.RequestCtx) {
	if !log.BufferEnabled() {
		writeError(ctx, fasthttp.StatusNotFound, "the log buffer is disabled: set log.bufferLines above 0")
		return
	}

	query, err := logQuery(ctx)
	if err != nil {
		writeError(ctx, fasthttp.StatusBadRequest, err.Error())
		return
	}

	writeJSON(ctx, fasthttp.StatusOK, log.Logs(query))
}

func LogsDownloadController(ctx *fasthttp.RequestCtx) {
	if !log.BufferEnabled() {
		writeError(ctx, fasthttp.StatusNotFound, "the log buffer is disabled: set log.bufferLines above 0")
		return
	}

	query, err := logQuery(ctx)
	if err != nil {
		writeError(ctx, fasthttp.StatusBadRequest, err.Error())
		return
	}

	query.Limit = log.MaxQueryLimit
	var b bytes.Buffer
	before := query.Before

	for {
		query.Before = before
		page := log.Logs(query)
		for _, entry := range page.Entries {
			line, _ := json.Marshal(entry)
			b.Write(line)
			b.WriteByte('\n')
		}

		if !page.HasMore || len(page.Entries) == 0 {
			break
		}

		before = page.Entries[len(page.Entries)-1].Seq
	}

	name := fmt.Sprintf("elysiandb-logs-%s.ndjson", time.Now().UTC().Format("20060102-150405"))
	ctx.Response.Header.Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s"`, name))
	ctx.SetContentType("application/x-ndjson")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(b.Bytes())
}

func logQuery(ctx *fasthttp.RequestCtx) (log.Query, error) {
	args := ctx.QueryArgs()

	level, err := log.ParseLevel(string(args.Peek("level")))
	if err != nil {
		return log.Query{}, err
	}

	if len(args.Peek("level")) == 0 {
		level = slog.LevelDebug
	}

	query := log.Query{
		MinLevel:  level,
		Component: strings.TrimSpace(string(args.Peek("component"))),
		Text:      string(args.Peek("q")),
		User:      strings.TrimSpace(string(args.Peek("user"))),
		RequestID: strings.TrimSpace(string(args.Peek("request_id"))),
	}

	for _, field := range []struct {
		name   string
		target *time.Time
	}{{"since", &query.Since}, {"until", &query.Until}} {
		v, err := optionalInt(ctx, field.name)
		if err != nil {
			return log.Query{}, fmt.Errorf("%s must be a unix time in milliseconds", field.name)
		}
		if v > 0 {
			*field.target = time.UnixMilli(v)
		}
	}

	for _, field := range []struct {
		name   string
		target *uint64
	}{{"before", &query.Before}, {"after", &query.After}} {
		v, err := optionalInt(ctx, field.name)
		if err != nil || v < 0 {
			return log.Query{}, fmt.Errorf("%s must be a sequence number", field.name)
		}
		*field.target = uint64(v)
	}

	limit, err := optionalInt(ctx, "limit")
	if err != nil || limit < 0 {
		return log.Query{}, fmt.Errorf("limit must be between 1 and %d", log.MaxQueryLimit)
	}
	query.Limit = int(limit)

	return query, nil
}

func optionalInt(ctx *fasthttp.RequestCtx, name string) (int64, error) {
	raw := string(ctx.QueryArgs().Peek(name))
	if raw == "" {
		return 0, nil
	}

	return strconv.ParseInt(raw, 10, 64)
}

func GetLogLevelController(ctx *fasthttp.RequestCtx) {
	writeJSON(ctx, fasthttp.StatusOK, log.CurrentLevel())
}

type levelRequest struct {
	Level   string `json:"level"`
	Minutes int    `json:"minutes"`
}

func PutLogLevelController(ctx *fasthttp.RequestCtx) {
	var req levelRequest
	if err := json.Unmarshal(ctx.PostBody(), &req); err != nil {
		writeError(ctx, fasthttp.StatusBadRequest, "invalid request body")
		return
	}

	level, err := log.ParseLevel(req.Level)
	if err != nil || strings.TrimSpace(req.Level) == "" {
		writeError(ctx, fasthttp.StatusBadRequest, "level must be debug, info, warn or error")
		return
	}

	if level != log.ConfiguredLevel() && (req.Minutes < 1 || req.Minutes > maxLevelMinutes) {
		writeError(ctx, fasthttp.StatusBadRequest, fmt.Sprintf("minutes must be between 1 and %d", maxLevelMinutes))
		return
	}

	info := log.SetLevelFor(level, time.Duration(req.Minutes)*time.Minute)
	observe.Audit(ctx, "log level changed", "level", info.Level, "configured", info.Configured, "minutes", req.Minutes)

	writeJSON(ctx, fasthttp.StatusOK, info)
}

func PrometheusController(ctx *fasthttp.RequestCtx) {
	ctx.SetContentType("text/plain; version=0.0.4; charset=utf-8")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(metrics.Prometheus())
}

func PrometheusAuth(adminAuth func(fasthttp.RequestHandler) fasthttp.RequestHandler, next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		token := globals.GetConfig().Monitoring.Prometheus.Token
		if token == "" {
			adminAuth(next)(ctx)
			return
		}

		header := string(ctx.Request.Header.Peek(fasthttp.HeaderAuthorization))
		presented, ok := strings.CutPrefix(header, "Bearer ")
		if !ok || subtle.ConstantTimeCompare([]byte(presented), []byte(token)) != 1 {
			ctx.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		next(ctx)
	}
}
