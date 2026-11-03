package bench_test

import (
	"encoding/json"
	"fmt"
	"math/rand"
	"net"
	"os"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/fasthttp/router"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

var (
	setupOnce sync.Once
	client    *fasthttp.Client
	works     = 10000
	statuses  = []string{"published", "draft", "archived", "review", "out_of_print"}
	words     = []string{"dragon", "dune", "kindred", "foundation", "fifth", "story", "body", "left", "lathe", "dawn", "robots", "season", "exhalation", "problem"}
)

func config(folder string) *configuration.Config {
	return &configuration.Config{
		Store: configuration.StoreConfig{Folder: folder, Shards: 64, CrashRecovery: configuration.CrashRecoveryConfig{Enabled: os.Getenv("ELYSIAN_BENCH_RECOVERY") != "0", MaxLogMB: 100}},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{Enabled: true, Strict: true},
			Hooks:  configuration.HooksConfig{Enabled: true},
			Index:  configuration.ApiIndexConfig{Workers: 4},
		},
		Log: configuration.LogConfig{Level: "error"},
	}
}

func work(i int, r *rand.Rand) map[string]any {
	return map[string]any{
		"id":        fmt.Sprintf("w%06d", i),
		"title":     fmt.Sprintf("%s %s %d", words[r.Intn(len(words))], words[r.Intn(len(words))], i),
		"year":      float64(1900 + r.Intn(125)),
		"published": fmt.Sprintf("%d-%02d-%02dT10:00:00Z", 1900+r.Intn(125), 1+r.Intn(12), 1+r.Intn(28)),
		"status":    statuses[r.Intn(len(statuses))],
		"rating":    float64(r.Intn(50)) / 10,
		"author":    map[string]any{"@entity": "person", "id": fmt.Sprintf("p%04d", r.Intn(2000))},
		"summary":   fmt.Sprintf("a %s tale about %s and %s", words[r.Intn(len(words))], words[r.Intn(len(words))], words[r.Intn(len(words))]),
		"tags":      []any{words[r.Intn(len(words))], words[r.Intn(len(words))]},
	}
}

func insert(entity string, docs []map[string]any) {
	for start := 0; start < len(docs); start += 500 {
		end := min(start+500, len(docs))
		api_storage.WriteListOfEntities(entity, docs[start:end])
	}
}

func setup(b *testing.B) {
	setupOnce.Do(func() {
		if n, err := strconv.Atoi(os.Getenv("ELYSIAN_BENCH_WORKS")); err == nil && n > 0 {
			works = n
		}

		folder, err := os.MkdirTemp("", "elysiandb-bench-")
		if err != nil {
			b.Fatal(err)
		}

		globals.SetConfig(config(folder))
		storage.LoadDB()
		storage.LoadJsonDB()
		hook.InitHooks()

		r := rand.New(rand.NewSource(1))
		people := make([]map[string]any, 0, 2000)
		for i := 0; i < 2000; i++ {
			people = append(people, map[string]any{"id": fmt.Sprintf("p%04d", i), "name": fmt.Sprintf("Person %d", i), "country": statuses[i%5]})
		}
		insert("person", people)

		docs := make([]map[string]any, 0, works)
		for i := 0; i < works; i++ {
			docs = append(docs, work(i, r))
		}
		insert("work", docs)

		purchases := []map[string]any{}
		for i := 0; i < 1680; i++ {
			purchases = append(purchases, map[string]any{"id": fmt.Sprintf("pu%05d", i), "month": fmt.Sprintf("m%03d", i%120), "amount": float64(i % 97)})
		}
		insert("purchase", purchases)

		months := []map[string]any{}
		for i := 0; i < 120; i++ {
			months = append(months, map[string]any{"id": fmt.Sprintf("m%03d", i), "month": fmt.Sprintf("m%03d", i), "limit": float64(100)})
		}
		insert("budget_month", months)

		script := `function postRead(ctx){var rows=ctx.query("purchase",{month:{eq:ctx.entity.month}});var t=0;for(var i=0;i<rows.length;i++){t+=rows[i].amount}ctx.entity.spent=t}`
		if err := hook.CreateHook(&hook.Hook{Name: "budget", Entity: "budget_month", Event: hook.HookEventPostRead, Language: "javascript", Script: script, Enabled: true}); err != nil {
			b.Fatal(err)
		}

		r2 := router.New()
		routing.RegisterRoutes(r2)
		srv := &fasthttp.Server{Handler: r2.Handler, MaxRequestBodySize: 64 << 20}
		ln := fasthttputil.NewInmemoryListener()
		go func() { _ = srv.Serve(ln) }()
		client = &fasthttp.Client{Dial: func(string) (net.Conn, error) { return ln.Dial() }, MaxResponseBodySize: 256 << 20, ReadTimeout: time.Minute}

		for _, f := range []string{"year", "title"} {
			api_storage.ListEntities("work", 1, 0, f, true, nil, "", "")
		}
		api_storage.ListEntities("work", 1, 0, "", true, map[string]map[string]string{"status": {"eq": "draft"}}, "", "")
		api_storage.ListEntities("work", 1, 0, "", true, nil, "dragon", "")
		time.Sleep(2 * time.Second)
	})
}

func get(b *testing.B, method, path, body string) []byte {
	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(method)
	req.SetRequestURI("http://bench" + path)
	if body != "" {
		req.SetBodyString(body)
	}

	if err := client.Do(req, resp); err != nil {
		b.Fatal(err)
	}

	if resp.StatusCode() >= 300 {
		b.Fatalf("%s %s: %d %s", method, path, resp.StatusCode(), resp.Body())
	}

	return append([]byte(nil), resp.Body()...)
}

func BenchmarkReadByID(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", fmt.Sprintf("/api/work/w%06d", i%works), "")
	}
}

func BenchmarkUnfilteredPage(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?limit=20&offset=5000", "")
	}
}

func BenchmarkCount(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work/count", "")
	}
}

func BenchmarkFilteredCountOnly(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?countOnly=true&filter%5Bstatus%5D=draft", "")
	}
}

func BenchmarkFilterSortOffset(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?filter%5Bstatus%5D=published&sort%5Byear%5D=desc&offset=500&limit=20", "")
	}
}

func BenchmarkSortedPage(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?sort%5Btitle%5D=asc&offset=4000&limit=20", "")
	}
}

func BenchmarkSearch(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?search=exhalation&limit=20", "")
	}
}

func BenchmarkQueryAPIMultiSort(b *testing.B) {
	setup(b)
	body := `{"entity":"work","filters":{"and":[{"status":{"eq":"published"}},{"year":{"gte":1950}}]},"sorts":{"rating":"desc","year":"asc"},"limit":20}`
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "POST", "/api/query", body)
	}
}

func BenchmarkFilterThroughReferenceWithIncludes(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work?includes=author&filter%5Bauthor.id%5D=p0042&limit=20", "")
	}
}

func BenchmarkHookBudgetMonths(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/budget_month?sort%5Bmonth%5D=asc", "")
	}
}

func BenchmarkFullRead(b *testing.B) {
	setup(b)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		get(b, "GET", "/api/work", "")
	}
}

func BenchmarkInsertBatch500(b *testing.B) {
	setup(b)
	r := rand.New(rand.NewSource(int64(time.Now().UnixNano())))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		batch := make([]map[string]any, 0, 500)
		for j := 0; j < 500; j++ {
			doc := work(0, r)
			doc["id"] = fmt.Sprintf("ins-%d-%d-%d", time.Now().UnixNano(), i, j)
			batch = append(batch, doc)
		}
		body, _ := json.Marshal(batch)
		get(b, "POST", "/api/insert", string(body))
	}
}

func BenchmarkDeleteOne(b *testing.B) {
	setup(b)
	r := rand.New(rand.NewSource(2))
	ids := make([]string, b.N)
	for i := 0; i < b.N; i++ {
		doc := work(0, r)
		ids[i] = fmt.Sprintf("del-%d", i)
		doc["id"] = ids[i]
		api_storage.WriteEntity("work", doc)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		api_storage.DeleteEntityById("work", ids[i])
	}
}

func BenchmarkUpdateHotDocument(b *testing.B) {
	setup(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		api_storage.UpdateEntityById("work", "w000042", map[string]any{"rating": float64(i % 50)})
	}
}

func BenchmarkKVSetGet(b *testing.B) {
	setup(b)
	value := []byte(strings.Repeat("v", 64))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		key := fmt.Sprintf("session:%d", i%5000)
		if err := storage.PutKeyValue(key, value); err != nil {
			b.Fatal(err)
		}
		if _, err := storage.GetByKey(key); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkQueryAPIInternal(b *testing.B) {
	setup(b)
	q := query.Query{Entity: "work", Filter: query.FilterNode{Leaf: map[string]map[string]string{"status": {"eq": "review"}}}, Sorts: map[string]string{"year": "asc"}, Limit: 20}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := api_storage.ExecuteQuery(q); err != nil {
			b.Fatal(err)
		}
	}
}
