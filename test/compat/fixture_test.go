package compat_test

import (
	"fmt"
	"strings"
)

type fixtureRequest struct {
	Method    string `json:"method"`
	Path      string `json:"path"`
	Body      string `json:"body,omitempty"`
	As        string `json:"as,omitempty"`
	Unordered bool   `json:"unordered,omitempty"`
}

var authorNames = []string{"Ursula Le Guin", "frank herbert", "Octavia Butler", "Isaac Asimov", "N. K. Jemisin", "Ted Chiang", "Mary Shelley", "Liu Cixin"}
var countries = []string{"US", "us", "UK", "CN", "FR"}
var genres = []string{"scifi", "fantasy", "horror", "classic", "short", "drama"}
var statuses = []string{"published", "draft", "Published", "archived", "out_of_print"}
var titleWords = []string{"Dragon", "Dune", "Kindred", "Foundation", "Fifth", "Story", "Frankenstein", "Body", "Left", "Dispossessed", "Lathe", "Dawn", "Robots", "Season", "Exhalation", "Problem"}

func authorFixtures() []string {
	docs := []string{}
	for i, name := range authorNames {
		fields := []string{
			fmt.Sprintf(`"id":"a%d"`, i+1),
			fmt.Sprintf(`"name":%q`, name),
			fmt.Sprintf(`"born":"19%02d-%02d-%02d"`, 10+i*7, 1+i%12, 1+i*3),
			fmt.Sprintf(`"country":%q`, countries[i%len(countries)]),
			fmt.Sprintf(`"rating":%d.%d`, 3+i%3, i),
			fmt.Sprintf(`"active":%t`, i%3 != 0),
			fmt.Sprintf(`"tags":["%s","%s"]`, genres[i%len(genres)], genres[(i+2)%len(genres)]),
			fmt.Sprintf(`"meta":{"score":%d,"level":"L%d"}`, 100-i*9, i%3),
		}
		if i%4 == 3 {
			fields = append(fields, `"nickname":"the great"`)
		}
		docs = append(docs, "{"+strings.Join(fields, ",")+"}")
	}

	return docs
}

func bookFixtures() []string {
	docs := []string{}
	for i := 0; i < 30; i++ {
		title := fmt.Sprintf("%s %s %02d", titleWords[i%len(titleWords)], titleWords[(i*5+3)%len(titleWords)], i)
		if i%7 == 0 {
			title = strings.ToLower(title)
		}
		fields := []string{
			fmt.Sprintf(`"id":"b%02d"`, i+1),
			fmt.Sprintf(`"title":%q`, title),
			fmt.Sprintf(`"year":%d`, 1950+i*2),
			fmt.Sprintf(`"published":"%d-%02d-%02dT%02d:30:00%s"`, 1950+i*2, 1+i%12, 1+i%27, i%24, []string{"Z", "+02:00", "-05:00"}[i%3]),
			fmt.Sprintf(`"price":%d.%02d`, 5+i, (i*37)%100),
			fmt.Sprintf(`"author":{"@entity":"author","id":"a%d"}`, 1+i%len(authorNames)),
			fmt.Sprintf(`"genres":["%s","%s"]`, genres[i%len(genres)], genres[(i+1)%len(genres)]),
			fmt.Sprintf(`"stock":{"warehouse":%d,"shop":%d}`, (i*13)%40, 100+i),
			fmt.Sprintf(`"isbn":"978%07d"`, 1000000+i*7919),
			fmt.Sprintf(`"status":%q`, statuses[i%len(statuses)]),
			fmt.Sprintf(`"summary":"A %s tale about %s and the %s of everything"`, genres[i%len(genres)], strings.ToLower(titleWords[(i+4)%len(titleWords)]), []string{"end", "beginning", "middle"}[i%3]),
			fmt.Sprintf(`"inStock":%t`, i%2 == 0),
		}
		if i%3 == 0 {
			fields = append(fields, fmt.Sprintf(`"coauthors":[{"@entity":"author","id":"a%d"},{"@entity":"author","id":"a%d"}]`, 1+(i+1)%len(authorNames), 1+(i+3)%len(authorNames)))
		}
		if i%5 == 0 {
			fields = append(fields, `"edition":"1"`)
		} else if i%5 == 1 {
			fields = append(fields, `"edition":"2"`)
		}
		if i%6 == 0 {
			fields = append(fields, `"awards":[]`)
		} else if i%6 == 1 {
			fields = append(fields, `"awards":["hugo","nebula"]`)
		}
		if i == 4 {
			fields = append(fields, `"notes":null`)
		}
		docs = append(docs, "{"+strings.Join(fields, ",")+"}")
	}

	return docs
}

func reviewFixtures() []string {
	docs := []string{}
	for i := 0; i < 40; i++ {
		docs = append(docs, fmt.Sprintf(`{"id":"r%02d","book":{"@entity":"book","id":"b%02d"},"stars":%d,"text":"review number %d is %s","created":"2024-%02d-%02d"}`,
			i+1, 1+(i*7)%30, 1+i%5, i, []string{"great", "bad", "fine", "Great"}[i%4], 1+i%12, 1+i%28))
	}

	return docs
}

func fixtureLoadRequests() []fixtureRequest {
	reqs := []fixtureRequest{}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/author", Body: "[" + strings.Join(authorFixtures(), ",") + "]"})
	books := bookFixtures()
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/book", Body: "[" + strings.Join(books[:20], ",") + "]"})
	for _, b := range books[20:] {
		reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/book", Body: b})
	}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/review", Body: "[" + strings.Join(reviewFixtures(), ",") + "]"})

	purchases := []string{}
	for i := 0; i < 36; i++ {
		purchases = append(purchases, fmt.Sprintf(`{"id":"p%02d","month":"2024-%02d","amount":%d}`, i+1, 1+i%12, 10+i*3))
	}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/purchase", Body: "[" + strings.Join(purchases, ",") + "]"})

	months := []string{}
	for i := 0; i < 12; i++ {
		months = append(months, fmt.Sprintf(`{"id":"m%02d","month":"2024-%02d","limit":%d}`, i+1, i+1, 50+i*10))
	}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/budget_month", Body: "[" + strings.Join(months, ",") + "]"})

	programs := []string{`{"id":"pg1","name":"Alpha","status":"open"}`, `{"id":"pg2","name":"Beta","status":"closed"}`, `{"id":"pg3","name":"Gamma","status":"open"}`}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/program", Body: "[" + strings.Join(programs, ",") + "]"})
	steps := []string{}
	for i := 0; i < 15; i++ {
		steps = append(steps, fmt.Sprintf(`{"id":"st%02d","program_id":"pg%d","done":%t,"order":%d}`, i+1, 1+i%3, i%2 == 0, i))
	}
	reqs = append(reqs, fixtureRequest{Method: "POST", Path: "/api/step", Body: "[" + strings.Join(steps, ",") + "]"})

	return reqs
}

const budgetHookScript = `function postRead(ctx) {
  var rows = ctx.query("purchase", { month: { eq: ctx.entity.month } });
  var total = 0;
  for (var i = 0; i < rows.length; i++) { total += rows[i].amount; }
  ctx.entity.spent = total;
  ctx.entity.remaining = ctx.entity.limit - total;
}`

const progressHookScript = `function postRead(ctx) {
  var all = ctx.query("step", { program_id: { eq: ctx.entity.id } });
  var done = ctx.query("step", { program_id: { eq: ctx.entity.id }, done: { eq: "true" } });
  ctx.entity.progress = all.length === 0 ? 0 : Math.round(done.length * 100 / all.length);
}`

const preReadHookScript = `function preRead(ctx) {
  ctx.entity.label = ctx.entity.name + " (" + ctx.entity.status + ")";
}`

func hookRequests() []fixtureRequest {
	hook := func(name, entity, event, script string, priority int) string {
		return fmt.Sprintf(`{"name":%q,"entity":%q,"event":%q,"language":"javascript","script":%q,"priority":%d,"enabled":true}`, name, entity, event, script, priority)
	}

	return []fixtureRequest{
		{Method: "POST", Path: "/api/hook/budget_month", Body: hook("budget", "budget_month", "post_read", budgetHookScript, 1)},
		{Method: "POST", Path: "/api/hook/program", Body: hook("progress", "program", "post_read", progressHookScript, 2)},
		{Method: "POST", Path: "/api/hook/program", Body: hook("label", "program", "pre_read", preReadHookScript, 1)},
	}
}

func readRequests() []fixtureRequest {
	g := func(path string) fixtureRequest { return fixtureRequest{Method: "GET", Path: path} }
	q := func(body string) fixtureRequest {
		return fixtureRequest{Method: "POST", Path: "/api/query", Body: body}
	}

	reqs := []fixtureRequest{
		g("/api/author"),
		g("/api/book"),
		g("/api/review"),
		g("/api/book?limit=5"),
		g("/api/book?limit=5&offset=5"),
		g("/api/book?offset=28"),
		g("/api/book?offset=40"),
		g("/api/book?limit=0"),
		g("/api/book/count"),
		g("/api/author/count"),
		g("/api/nothing"),
		g("/api/nothing/count"),
		g("/api/book?countOnly=true"),
		g("/api/book?countOnly=true&limit=3"),
		g("/api/book/b03"),
		g("/api/book/b03?fields=title,year"),
		g("/api/book/b03?fields=title,stock.shop"),
		g("/api/book/b03?includes=author"),
		g("/api/book/b01?includes=author,coauthors"),
		g("/api/book/b01?includes=all"),
		g("/api/book/b01?includes=author&fields=title,author.name"),
		g("/api/book/zz"),
		g("/api/book/b03/exists"),
		g("/api/book/zz/exists"),
		g("/api/book?fields=title"),
		g("/api/book?fields=title,author&limit=4"),
		g("/api/book?includes=author&limit=3"),
		g("/api/book?includes=author,coauthors&limit=4&offset=2"),
		g("/api/review?includes=book.author&limit=3"),
		g("/api/review?includes=all&limit=2"),
		g("/api/entity/types"),
		g("/api/entity/types/name"),
		g("/api/book/schema"),
		g("/api/author/schema"),
		g("/api/export"),
	}

	sorts := []string{"title", "year", "price", "published", "stock.shop", "isbn"}
	for _, s := range sorts {
		reqs = append(reqs, g("/api/book?sort["+s+"]=asc"), g("/api/book?sort["+s+"]=desc&limit=7"), g("/api/book?sort["+s+"]=asc&limit=4&offset=6"))
	}
	reqs = append(reqs,
		g("/api/author?sort[name]=asc"),
		g("/api/author?sort[born]=desc"),
		g("/api/author?sort[meta.score]=asc"),
		g("/api/book?sort[title]=sideways"),
		g("/api/book?sort[year]=desc&filter[year][gt]=1990&limit=5"),
		g("/api/book?sort[price]=asc&filter[status]=published&offset=1&limit=3"),
		g("/api/book?sort[title]=asc&search=dragon"),
	)

	filters := []string{
		"filter[status]=published",
		"filter[status]=PUBLISHED",
		"filter[status][eq]=pub*",
		"filter[status][eq]=*print",
		"filter[status][eq]=*a*",
		"filter[status][neq]=published",
		"filter[status][contains]=ub",
		"filter[status][not_contains]=ub",
		"filter[title][eq]=dune*",
		"filter[title][contains]=FIFTH",
		"filter[title][lt]=F",
		"filter[title][gte]=Story",
		"filter[year][gt]=1990",
		"filter[year][gte]=1990&filter[year][lte]=2000",
		"filter[year][eq]=1972",
		"filter[year][neq]=1972",
		"filter[year][lt]=abc",
		"filter[year]=1972",
		"filter[price][lte]=10.5",
		"filter[price][gt]=20",
		"filter[published][gt]=1990-01-01",
		"filter[published][lt]=1970-06-01T00:00:00Z",
		"filter[published][eq]=1960-11-11",
		"filter[published][gte]=1980-01-01&filter[published][lt]=1990-01-01",
		"filter[published][eq]=1960*",
		"filter[inStock]=true",
		"filter[inStock]=false",
		"filter[inStock]=1",
		"filter[inStock][neq]=true",
		"filter[inStock]=maybe",
		"filter[genres][contains]=scifi",
		"filter[genres][not_contains]=scifi",
		"filter[genres][all]=scifi,fantasy",
		"filter[genres][any]=horror,drama",
		"filter[genres][none]=horror,drama",
		"filter[genres][eq]=fantasy,horror",
		"filter[genres]=scifi",
		"filter[awards][contains]=hugo",
		"filter[awards][none]=hugo",
		"filter[edition]=1",
		"filter[edition][gt]=1",
		"filter[edition][eq]=2",
		"filter[stock.shop][gt]=120",
		"filter[stock.warehouse][lt]=10",
		"filter[author.id]=a2",
		"filter[author.id][eq]=a1",
		"filter[author.name][eq]=*asimov*",
		"filter[author.country]=us",
		"filter[author.meta.score][gt]=60",
		"filter[coauthors.id]=a4",
		"filter[coauthors.name][contains]=chiang",
		"filter[missing]=x",
		"filter[missing][neq]=x",
		"filter[notes]=null",
		"filter[isbn][eq]=9781*",
		"filter[summary][contains]=beginning",
		"filter[status]=published&filter[year][gt]=1980",
		"filter[status]=draft&filter[genres][contains]=fantasy",
		"filter[title][eq]=\\*",
	}
	for _, f := range filters {
		reqs = append(reqs, g("/api/book?"+f))
		reqs = append(reqs, g("/api/book?countOnly=true&"+f))
	}
	reqs = append(reqs,
		g("/api/book?filter[status]=published&limit=2"),
		g("/api/book?filter[status]=published&limit=2&offset=2"),
		g("/api/book?filter[year][gt]=1960&offset=100"),
		g("/api/book?filter[author.name]=Ted Chiang&includes=author"),
		g("/api/book?filter[author.id]=a3&fields=title,author"),
		g("/api/author?filter[active]=true"),
		g("/api/author?filter[tags][contains]=horror"),
		g("/api/author?filter[meta.level]=L1"),
		g("/api/author?filter[born][lt]=1950-01-01"),
		g("/api/author?filter[nickname]=the great"),
		g("/api/review?filter[stars][gte]=4"),
		g("/api/review?filter[book.id]=b08"),
		g("/api/review?filter[book.title][contains]=dune"),
		g("/api/review?filter[created][gte]=2024-06-01&filter[created][lt]=2024-09-01"),
		g("/api/review?filter[text]=*GREAT"),
	)

	searches := []string{"dragon", "DUNE", "the", "ab", "*ing", "beg*ing", "zzz", "a", "2024", "great", "tale about"}
	for _, s := range searches {
		reqs = append(reqs, g("/api/book?search="+s))
	}
	reqs = append(reqs,
		g("/api/book?search=dragon&limit=2"),
		g("/api/book?search=the&countOnly=true"),
		g("/api/book?search=asimov&includes=author"),
		g("/api/book?search=chiang&includes=coauthors"),
		g("/api/review?search=great&filter[stars][gt]=2"),
		g("/api/author?search=us"),
	)

	reqs = append(reqs,
		q(`{"entity":"book"}`),
		q(`{"entity":"book","limit":5}`),
		q(`{"entity":"book","offset":3,"limit":4}`),
		q(`{"entity":"book","filters":{"status":{"eq":"published"}}}`),
		q(`{"entity":"book","filters":{"year":{"gt":1990}},"countOnly":true}`),
		q(`{"entity":"book","filters":{"and":[{"year":{"gte":1960}},{"year":{"lt":1980}}]}}`),
		q(`{"entity":"book","filters":{"or":[{"status":{"eq":"draft"}},{"genres":{"contains":"horror"}}]}}`),
		q(`{"entity":"book","filters":{"or":[{"and":[{"status":{"eq":"published"}},{"inStock":{"eq":true}}]},{"price":{"lt":8}}]}}`),
		q(`{"entity":"book","filters":{"and":[]}}`),
		q(`{"entity":"book","filters":{"or":[]}}`),
		q(`{"entity":"book","filters":{"inStock":{"eq":false}},"fields":"title,inStock"}`),
		q(`{"entity":"book","sorts":{"year":"desc"},"limit":5}`),
		q(`{"entity":"book","sorts":{"title":"asc"}}`),
		q(`{"entity":"book","sorts":{"status":"asc","year":"desc"}}`),
		q(`{"entity":"book","sorts":{"year":"desc","status":"asc"}}`),
		q(`{"entity":"book","sorts":{"inStock":"desc","price":"asc"},"offset":2,"limit":6}`),
		q(`{"entity":"book","sorts":{"status":"asc","stock.shop":"desc"},"filters":{"year":{"gt":1970}}}`),
		q(`{"entity":"book","filters":{"author.id":{"eq":"a2"}}}`),
		q(`{"entity":"book","filters":{"author.name":{"eq":"Ted Chiang"}}}`),
		q(`{"entity":"book","filters":{"published":{"gte":"1990-01-01"}},"sorts":{"published":"asc"}}`),
		q(`{"entity":"book","filters":{"edition":{"eq":"1"}}}`),
		q(`{"entity":"author","sorts":{"country":"asc","name":"asc"}}`),
		q(`{"entity":"author","filters":{"tags":{"any":"scifi,drama"}},"countOnly":true}`),
		q(`{"entity":"nothing"}`),
		q(`{"entity":"_elysiandb_core_user"}`),
		q(`{"entity":"book","filters":{"or":{"x":1}}}`),
		q(`{"entity":"book","filters":{"status":"published"}}`),
		q(`{"entity":"book","filters":{"status":{"eq":["a"]}}}`),
		q(`not json`),
		q(`{"entity":"budget_month","limit":3}`),
		q(`{"entity":"program","filters":{"status":{"eq":"open"}}}`),
	)

	reqs = append(reqs,
		g("/api/budget_month"),
		g("/api/budget_month?sort[month]=desc&limit=4"),
		g("/api/budget_month/m03"),
		g("/api/budget_month?countOnly=true"),
		g("/api/program"),
		g("/api/program?filter[status]=open"),
		g("/api/program?filter[label]=*open*"),
		g("/api/program?countOnly=true&filter[status]=open"),
		g("/api/program?filter[label]=Beta*&limit=1"),
		g("/api/program/pg2"),
		g("/api/hook/budget_month"),
		g("/api/hook/program"),
		g("/api/_elysiandb_core_user"),
		g("/api/_elysiandb_core_acl/x"),
	)

	return reqs
}

func writeRequests() []fixtureRequest {
	return []fixtureRequest{
		{Method: "POST", Path: "/api/book", Body: `{"id":"b99","title":"Inline Author","year":2025,"author":{"@entity":"author","id":"a99","name":"New Person","country":"FR"}}`},
		{Method: "GET", Path: "/api/author/a99"},
		{Method: "GET", Path: "/api/book/b99?includes=author"},
		{Method: "POST", Path: "/api/book", Body: `{"id":"b98","title":"Embedded list","coauthors":[{"@entity":"author","id":"a1","nickname":"updated"},{"@entity":"author","id":"a98","name":"Other"}]}`},
		{Method: "GET", Path: "/api/author/a1"},
		{Method: "GET", Path: "/api/author/a98"},
		{Method: "PUT", Path: "/api/book/b02", Body: `{"title":"Renamed","extra":{"k":"v"}}`},
		{Method: "GET", Path: "/api/book/b02"},
		{Method: "PUT", Path: "/api/book/zz", Body: `{"title":"x"}`},
		{Method: "PUT", Path: "/api/book", Body: `[{"id":"b03","status":"draft"},{"id":"b04","status":"draft"},{"id":"nope","status":"x"}]`},
		{Method: "GET", Path: "/api/book?filter[status]=draft"},
		{Method: "GET", Path: "/api/book?sort[title]=asc&limit=5"},
		{Method: "DELETE", Path: "/api/book/b05"},
		{Method: "DELETE", Path: "/api/book/b05"},
		{Method: "GET", Path: "/api/book/b05"},
		{Method: "GET", Path: "/api/book/count"},
		{Method: "GET", Path: "/api/book?sort[year]=asc&limit=6"},
		{Method: "GET", Path: "/api/book?limit=6"},
		{Method: "POST", Path: "/api/book", Body: `not json`},
		{Method: "POST", Path: "/api/book", Body: `{}`},
		{Method: "POST", Path: "/api/book", Body: `[]`},
		{Method: "POST", Path: "/api/book", Body: `{"id":"b97","title":"With nested core","author":{"@entity":"_elysiandb_core_user","id":"admin"}}`},
		{Method: "POST", Path: "/api/gadget", Body: `{"id":"g1","name":"first","weight":3}`},
		{Method: "POST", Path: "/api/gadget", Body: `{"id":"g2","name":"second","weight":"heavy"}`},
		{Method: "GET", Path: "/api/gadget/schema"},
		{Method: "GET", Path: "/api/gadget"},
		{Method: "PUT", Path: "/api/gadget/schema", Body: `{"fields":{"name":{"type":"string","required":true},"weight":{"type":"number","required":false}}}`},
		{Method: "POST", Path: "/api/gadget", Body: `{"id":"g3","weight":4}`},
		{Method: "POST", Path: "/api/gadget", Body: `{"id":"g4","name":"fourth","weight":"x"}`},
		{Method: "POST", Path: "/api/gadget", Body: `[{"id":"g5","name":"ok"},{"id":"g6","weight":1}]`},
		{Method: "GET", Path: "/api/gadget"},
		{Method: "POST", Path: "/api/widget/create", Body: `{"fields":{"label":{"type":"string","required":true}}}`},
		{Method: "POST", Path: "/api/widget/create", Body: `{"fields":{"label":{"type":"string","required":true}}}`},
		{Method: "GET", Path: "/api/widget/schema"},
		{Method: "GET", Path: "/api/entity/types/name"},
		{Method: "POST", Path: "/api/book/migrate", Body: `[{"set":[{"status":"migrated"}]}]`},
		{Method: "GET", Path: "/api/book?filter[status]=migrated&countOnly=true"},
		{Method: "GET", Path: "/api/book/b06"},
		{Method: "POST", Path: "/api/book/migrate", Body: `[{"drop":{"x":1}}]`},
		{Method: "DELETE", Path: "/api/review"},
		{Method: "GET", Path: "/api/review"},
		{Method: "GET", Path: "/api/review/count"},
		{Method: "POST", Path: "/api/review", Body: `{"id":"r100","stars":5,"text":"after reset"}`},
		{Method: "GET", Path: "/api/review"},
		{Method: "GET", Path: "/api/entity/types/name"},
		{Method: "GET", Path: "/api/book?includes=author&filter[author.name]=New*"},
	}
}

func transactionRequests() []fixtureRequest {
	return []fixtureRequest{
		{Method: "POST", Path: "/api/tx/begin"},
		{Method: "POST", Path: "/api/tx/{tx}/entity/gadget", Body: `{"id":"g10","name":"tx one"}`},
		{Method: "PUT", Path: "/api/tx/{tx}/entity/gadget/g1", Body: `{"name":"tx updated"}`},
		{Method: "DELETE", Path: "/api/tx/{tx}/entity/gadget/g2"},
		{Method: "POST", Path: "/api/tx/{tx}/commit"},
		{Method: "GET", Path: "/api/gadget"},
		{Method: "POST", Path: "/api/tx/begin"},
		{Method: "POST", Path: "/api/tx/{tx}/entity/gadget", Body: `{"id":"g11","weight":2}`},
		{Method: "POST", Path: "/api/tx/{tx}/commit"},
		{Method: "GET", Path: "/api/gadget/g11"},
		{Method: "POST", Path: "/api/tx/begin"},
		{Method: "POST", Path: "/api/tx/{tx}/entity/gadget", Body: `{"id":"g12","name":"rolled back"}`},
		{Method: "POST", Path: "/api/tx/{tx}/rollback"},
		{Method: "GET", Path: "/api/gadget/g12"},
		{Method: "POST", Path: "/api/tx/unknown/commit"},
	}
}

func kvRequests() []fixtureRequest {
	return []fixtureRequest{
		{Method: "PUT", Path: "/kv/session:1", Body: "alpha"},
		{Method: "PUT", Path: "/kv/session:2", Body: "beta"},
		{Method: "PUT", Path: "/kv/derived:total", Body: "42"},
		{Method: "PUT", Path: "/kv/temp?ttl=3600", Body: "short lived"},
		{Method: "GET", Path: "/kv/session:1"},
		{Method: "GET", Path: "/kv/temp"},
		{Method: "GET", Path: "/kv/missing"},
		{Method: "GET", Path: "/kv/session:*", Unordered: true},
		{Method: "GET", Path: "/kv/nothing:*"},
		{Method: "GET", Path: "/kv/mget?keys=session:1,missing,derived:total,session:1"},
		{Method: "GET", Path: "/kv/mget?keys=session:*", Unordered: true},
		{Method: "GET", Path: "/kv/mget?keys="},
		{Method: "DELETE", Path: "/kv/session:2"},
		{Method: "GET", Path: "/kv/session:2"},
		{Method: "PUT", Path: "/kv/session:1", Body: "alpha2"},
		{Method: "GET", Path: "/kv/session:1"},
		{Method: "DELETE", Path: "/kv/session:*"},
		{Method: "GET", Path: "/kv/session:1"},
		{Method: "TCP", Path: "PING"},
		{Method: "TCP", Path: "SET tcp:a hello"},
		{Method: "TCP", Path: "SET tcp:b world"},
		{Method: "TCP", Path: "SET TTL=100 tcp:c ttl value"},
		{Method: "TCP", Path: "GET tcp:a"},
		{Method: "TCP", Path: "GET tcp:c"},
		{Method: "TCP", Path: "GET tcp:none"},
		{Method: "TCP", Path: "MGET tcp:a tcp:b tcp:none"},
		{Method: "TCP", Path: "GET tcp:*", Unordered: true},
		{Method: "TCP", Path: "DEL tcp:b"},
		{Method: "TCP", Path: "GET tcp:b"},
		{Method: "TCP", Path: "DEL tcp:*"},
		{Method: "TCP", Path: "GET tcp:a"},
		{Method: "TCP", Path: "NOPE"},
		{Method: "GET", Path: "/kv/api:entity:types:list"},
	}
}

func ownershipRequests() []fixtureRequest {
	return []fixtureRequest{
		{Method: "POST", Path: "/api/security/user", Body: `{"username":"reader","password":"reader-password-1","role":"user"}`},
		{Method: "LOGIN", Path: "reader", Body: "reader-password-1", As: "reader"},
		{Method: "POST", Path: "/api/note", Body: `{"id":"n1","text":"admin note"}`},
		{Method: "PUT", Path: "/api/acl/reader/note", Body: `{"permissions":{"create":true,"read":false,"update":false,"delete":false,"owning_read":true,"owning_write":true,"owning_update":true,"owning_delete":true}}`},
		{Method: "GET", Path: "/api/acl/reader/note"},
		{Method: "POST", Path: "/api/note", Body: `{"id":"n2","text":"reader note"}`, As: "reader"},
		{Method: "POST", Path: "/api/note", Body: `{"id":"n3","text":"second reader note"}`, As: "reader"},
		{Method: "GET", Path: "/api/note", As: "reader"},
		{Method: "GET", Path: "/api/note?limit=2", As: "reader"},
		{Method: "GET", Path: "/api/note/count", As: "reader"},
		{Method: "GET", Path: "/api/note?countOnly=true", As: "reader"},
		{Method: "GET", Path: "/api/note/n1", As: "reader"},
		{Method: "GET", Path: "/api/note/n2", As: "reader"},
		{Method: "PUT", Path: "/api/note/n1", Body: `{"text":"hacked"}`, As: "reader"},
		{Method: "PUT", Path: "/api/note/n2", Body: `{"text":"edited"}`, As: "reader"},
		{Method: "DELETE", Path: "/api/note/n1", As: "reader"},
		{Method: "POST", Path: "/api/query", Body: `{"entity":"note"}`, As: "reader"},
		{Method: "GET", Path: "/api/book?limit=2", As: "reader"},
		{Method: "GET", Path: "/api/note"},
		{Method: "GET", Path: "/api/note/count"},
	}
}
