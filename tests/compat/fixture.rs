#![allow(dead_code)]

use elysiandb::gocompat::strconv::quote;
use elysiandb::json_struct;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct FixtureRequest {
        pub method: String => "method",
        pub path: String => "path",
        pub body: String => "body" [omitempty],
        pub as_user: String => "as" [omitempty],
        pub unordered: bool => "unordered" [omitempty],
    }
}

impl FixtureRequest {
    pub fn new(method: &str, path: &str, body: &str) -> FixtureRequest {
        FixtureRequest {
            method: method.to_string(),
            path: path.to_string(),
            body: body.to_string(),
            ..FixtureRequest::default()
        }
    }

    pub fn by(mut self, user: &str) -> FixtureRequest {
        self.as_user = user.to_string();
        self
    }

    pub fn unordered(mut self) -> FixtureRequest {
        self.unordered = true;
        self
    }
}

fn q(s: &str) -> String {
    quote(s.as_bytes())
}

const AUTHOR_NAMES: [&str; 8] = [
    "Ursula Le Guin",
    "frank herbert",
    "Octavia Butler",
    "Isaac Asimov",
    "N. K. Jemisin",
    "Ted Chiang",
    "Mary Shelley",
    "Liu Cixin",
];
const COUNTRIES: [&str; 5] = ["US", "us", "UK", "CN", "FR"];
const GENRES: [&str; 6] = ["scifi", "fantasy", "horror", "classic", "short", "drama"];
const STATUSES: [&str; 5] = [
    "published",
    "draft",
    "Published",
    "archived",
    "out_of_print",
];
const TITLE_WORDS: [&str; 16] = [
    "Dragon",
    "Dune",
    "Kindred",
    "Foundation",
    "Fifth",
    "Story",
    "Frankenstein",
    "Body",
    "Left",
    "Dispossessed",
    "Lathe",
    "Dawn",
    "Robots",
    "Season",
    "Exhalation",
    "Problem",
];

fn author_fixtures() -> Vec<String> {
    let mut docs = Vec::new();
    for (i, name) in AUTHOR_NAMES.iter().enumerate() {
        let mut fields = vec![
            format!(r#""id":"a{}""#, i + 1),
            format!(r#""name":{}"#, q(name)),
            format!(
                r#""born":"19{:02}-{:02}-{:02}""#,
                10 + i * 7,
                1 + i % 12,
                1 + i * 3
            ),
            format!(r#""country":{}"#, q(COUNTRIES[i % COUNTRIES.len()])),
            format!(r#""rating":{}.{}"#, 3 + i % 3, i),
            format!(r#""active":{}"#, i % 3 != 0),
            format!(
                r#""tags":["{}","{}"]"#,
                GENRES[i % GENRES.len()],
                GENRES[(i + 2) % GENRES.len()]
            ),
            format!(
                r#""meta":{{"score":{},"level":"L{}"}}"#,
                100 - i as i64 * 9,
                i % 3
            ),
        ];
        if i % 4 == 3 {
            fields.push(r#""nickname":"the great""#.to_string());
        }
        docs.push(format!("{{{}}}", fields.join(",")));
    }

    docs
}

fn book_fixtures() -> Vec<String> {
    let mut docs = Vec::new();
    for i in 0..30usize {
        let mut title = format!(
            "{} {} {:02}",
            TITLE_WORDS[i % TITLE_WORDS.len()],
            TITLE_WORDS[(i * 5 + 3) % TITLE_WORDS.len()],
            i
        );
        if i % 7 == 0 {
            title = title.to_lowercase();
        }
        let mut fields = vec![
            format!(r#""id":"b{:02}""#, i + 1),
            format!(r#""title":{}"#, q(&title)),
            format!(r#""year":{}"#, 1950 + i * 2),
            format!(
                r#""published":"{}-{:02}-{:02}T{:02}:30:00{}""#,
                1950 + i * 2,
                1 + i % 12,
                1 + i % 27,
                i % 24,
                ["Z", "+02:00", "-05:00"][i % 3]
            ),
            format!(r#""price":{}.{:02}"#, 5 + i, (i * 37) % 100),
            format!(
                r#""author":{{"@entity":"author","id":"a{}"}}"#,
                1 + i % AUTHOR_NAMES.len()
            ),
            format!(
                r#""genres":["{}","{}"]"#,
                GENRES[i % GENRES.len()],
                GENRES[(i + 1) % GENRES.len()]
            ),
            format!(
                r#""stock":{{"warehouse":{},"shop":{}}}"#,
                (i * 13) % 40,
                100 + i
            ),
            format!(r#""isbn":"978{:07}""#, 1000000 + i * 7919),
            format!(r#""status":{}"#, q(STATUSES[i % STATUSES.len()])),
            format!(
                r#""summary":"A {} tale about {} and the {} of everything""#,
                GENRES[i % GENRES.len()],
                TITLE_WORDS[(i + 4) % TITLE_WORDS.len()].to_lowercase(),
                ["end", "beginning", "middle"][i % 3]
            ),
            format!(r#""inStock":{}"#, i % 2 == 0),
        ];
        if i % 3 == 0 {
            fields.push(format!(
                r#""coauthors":[{{"@entity":"author","id":"a{}"}},{{"@entity":"author","id":"a{}"}}]"#,
                1 + (i + 1) % AUTHOR_NAMES.len(),
                1 + (i + 3) % AUTHOR_NAMES.len()
            ));
        }
        if i % 5 == 0 {
            fields.push(r#""edition":"1""#.to_string());
        } else if i % 5 == 1 {
            fields.push(r#""edition":"2""#.to_string());
        }
        if i % 6 == 0 {
            fields.push(r#""awards":[]"#.to_string());
        } else if i % 6 == 1 {
            fields.push(r#""awards":["hugo","nebula"]"#.to_string());
        }
        if i == 4 {
            fields.push(r#""notes":null"#.to_string());
        }
        docs.push(format!("{{{}}}", fields.join(",")));
    }

    docs
}

fn review_fixtures() -> Vec<String> {
    let mut docs = Vec::new();
    for i in 0..40usize {
        docs.push(format!(
            r#"{{"id":"r{:02}","book":{{"@entity":"book","id":"b{:02}"}},"stars":{},"text":"review number {} is {}","created":"2024-{:02}-{:02}"}}"#,
            i + 1,
            1 + (i * 7) % 30,
            1 + i % 5,
            i,
            ["great", "bad", "fine", "Great"][i % 4],
            1 + i % 12,
            1 + i % 28
        ));
    }

    docs
}

fn list(docs: &[String]) -> String {
    format!("[{}]", docs.join(","))
}

pub fn fixture_load_requests() -> Vec<FixtureRequest> {
    let mut reqs = Vec::new();
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/author",
        &list(&author_fixtures()),
    ));
    let books = book_fixtures();
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/book",
        &list(&books[..20]),
    ));
    for b in &books[20..] {
        reqs.push(FixtureRequest::new("POST", "/api/book", b));
    }
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/review",
        &list(&review_fixtures()),
    ));

    let purchases: Vec<String> = (0..36)
        .map(|i| {
            format!(
                r#"{{"id":"p{:02}","month":"2024-{:02}","amount":{}}}"#,
                i + 1,
                1 + i % 12,
                10 + i * 3
            )
        })
        .collect();
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/purchase",
        &list(&purchases),
    ));

    let months: Vec<String> = (0..12)
        .map(|i| {
            format!(
                r#"{{"id":"m{:02}","month":"2024-{:02}","limit":{}}}"#,
                i + 1,
                i + 1,
                50 + i * 10
            )
        })
        .collect();
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/budget_month",
        &list(&months),
    ));

    let programs = [
        r#"{"id":"pg1","name":"Alpha","status":"open"}"#.to_string(),
        r#"{"id":"pg2","name":"Beta","status":"closed"}"#.to_string(),
        r#"{"id":"pg3","name":"Gamma","status":"open"}"#.to_string(),
    ];
    reqs.push(FixtureRequest::new(
        "POST",
        "/api/program",
        &list(&programs),
    ));
    let steps: Vec<String> = (0..15)
        .map(|i| {
            format!(
                r#"{{"id":"st{:02}","program_id":"pg{}","done":{},"order":{}}}"#,
                i + 1,
                1 + i % 3,
                i % 2 == 0,
                i
            )
        })
        .collect();
    reqs.push(FixtureRequest::new("POST", "/api/step", &list(&steps)));

    reqs
}

const BUDGET_HOOK_SCRIPT: &str = "function postRead(ctx) {
  var rows = ctx.query(\"purchase\", { month: { eq: ctx.entity.month } });
  var total = 0;
  for (var i = 0; i < rows.length; i++) { total += rows[i].amount; }
  ctx.entity.spent = total;
  ctx.entity.remaining = ctx.entity.limit - total;
}";

const PROGRESS_HOOK_SCRIPT: &str = "function postRead(ctx) {
  var all = ctx.query(\"step\", { program_id: { eq: ctx.entity.id } });
  var done = ctx.query(\"step\", { program_id: { eq: ctx.entity.id }, done: { eq: \"true\" } });
  ctx.entity.progress = all.length === 0 ? 0 : Math.round(done.length * 100 / all.length);
}";

const PRE_READ_HOOK_SCRIPT: &str = "function preRead(ctx) {
  ctx.entity.label = ctx.entity.name + \" (\" + ctx.entity.status + \")\";
}";

pub const HOOK_QUERY_FIELDS: [(&str, &str); 3] = [
    ("purchase", "month"),
    ("step", "program_id"),
    ("step", "done"),
];

pub fn hook_requests() -> Vec<FixtureRequest> {
    let hook = |name: &str, entity: &str, event: &str, script: &str, priority: i64| {
        format!(
            r#"{{"name":{},"entity":{},"event":{},"language":"javascript","script":{},"priority":{},"enabled":true}}"#,
            q(name),
            q(entity),
            q(event),
            q(script),
            priority
        )
    };

    vec![
        FixtureRequest::new(
            "POST",
            "/api/hook/budget_month",
            &hook("budget", "budget_month", "post_read", BUDGET_HOOK_SCRIPT, 1),
        ),
        FixtureRequest::new(
            "POST",
            "/api/hook/program",
            &hook("progress", "program", "post_read", PROGRESS_HOOK_SCRIPT, 2),
        ),
        FixtureRequest::new(
            "POST",
            "/api/hook/program",
            &hook("label", "program", "pre_read", PRE_READ_HOOK_SCRIPT, 1),
        ),
    ]
}

fn g(path: &str) -> FixtureRequest {
    FixtureRequest::new("GET", path, "")
}

fn query(body: &str) -> FixtureRequest {
    FixtureRequest::new("POST", "/api/query", body)
}

pub fn read_requests() -> Vec<FixtureRequest> {
    let mut reqs: Vec<FixtureRequest> = [
        "/api/author",
        "/api/book",
        "/api/review",
        "/api/book?limit=5",
        "/api/book?limit=5&offset=5",
        "/api/book?offset=28",
        "/api/book?offset=40",
        "/api/book?limit=0",
        "/api/book/count",
        "/api/author/count",
        "/api/nothing",
        "/api/nothing/count",
        "/api/book?countOnly=true",
        "/api/book?countOnly=true&limit=3",
        "/api/book/b03",
        "/api/book/b03?fields=title,year",
        "/api/book/b03?fields=title,stock.shop",
        "/api/book/b03?includes=author",
        "/api/book/b01?includes=author,coauthors",
        "/api/book/b01?includes=all",
        "/api/book/b01?includes=author&fields=title,author.name",
        "/api/book/zz",
        "/api/book/b03/exists",
        "/api/book/zz/exists",
        "/api/book?fields=title",
        "/api/book?fields=title,author&limit=4",
        "/api/book?includes=author&limit=3",
        "/api/book?includes=author,coauthors&limit=4&offset=2",
        "/api/review?includes=book.author&limit=3",
        "/api/review?includes=all&limit=2",
        "/api/entity/types",
        "/api/entity/types/name",
        "/api/book/schema",
        "/api/author/schema",
        "/api/export",
    ]
    .iter()
    .map(|p| g(p))
    .collect();

    let sorts = ["title", "year", "price", "published", "stock.shop", "isbn"];
    for s in sorts {
        reqs.push(g(&format!("/api/book?sort[{s}]=asc")));
        reqs.push(g(&format!("/api/book?sort[{s}]=desc&limit=7")));
        reqs.push(g(&format!("/api/book?sort[{s}]=asc&limit=4&offset=6")));
    }
    for p in [
        "/api/author?sort[name]=asc",
        "/api/author?sort[born]=desc",
        "/api/author?sort[meta.score]=asc",
        "/api/book?sort[title]=sideways",
        "/api/book?sort[year]=desc&filter[year][gt]=1990&limit=5",
        "/api/book?sort[price]=asc&filter[status]=published&offset=1&limit=3",
        "/api/book?sort[title]=asc&search=dragon",
    ] {
        reqs.push(g(p));
    }

    let filters = [
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
    ];
    for f in filters {
        reqs.push(g(&format!("/api/book?{f}")));
        reqs.push(g(&format!("/api/book?countOnly=true&{f}")));
    }
    for p in [
        "/api/book?filter[status]=published&limit=2",
        "/api/book?filter[status]=published&limit=2&offset=2",
        "/api/book?filter[year][gt]=1960&offset=100",
        "/api/book?filter[author.name]=Ted Chiang&includes=author",
        "/api/book?filter[author.id]=a3&fields=title,author",
        "/api/author?filter[active]=true",
        "/api/author?filter[tags][contains]=horror",
        "/api/author?filter[meta.level]=L1",
        "/api/author?filter[born][lt]=1950-01-01",
        "/api/author?filter[nickname]=the great",
        "/api/review?filter[stars][gte]=4",
        "/api/review?filter[book.id]=b08",
        "/api/review?filter[book.title][contains]=dune",
        "/api/review?filter[created][gte]=2024-06-01&filter[created][lt]=2024-09-01",
        "/api/review?filter[text]=*GREAT",
    ] {
        reqs.push(g(p));
    }

    let searches = [
        "dragon",
        "DUNE",
        "the",
        "ab",
        "*ing",
        "beg*ing",
        "zzz",
        "a",
        "2024",
        "great",
        "tale about",
    ];
    for s in searches {
        reqs.push(g(&format!("/api/book?search={s}")));
    }
    for p in [
        "/api/book?search=dragon&limit=2",
        "/api/book?search=the&countOnly=true",
        "/api/book?search=asimov&includes=author",
        "/api/book?search=chiang&includes=coauthors",
        "/api/review?search=great&filter[stars][gt]=2",
        "/api/author?search=us",
    ] {
        reqs.push(g(p));
    }

    for body in [
        r#"{"entity":"book"}"#,
        r#"{"entity":"book","limit":5}"#,
        r#"{"entity":"book","offset":3,"limit":4}"#,
        r#"{"entity":"book","filters":{"status":{"eq":"published"}}}"#,
        r#"{"entity":"book","filters":{"year":{"gt":1990}},"countOnly":true}"#,
        r#"{"entity":"book","filters":{"and":[{"year":{"gte":1960}},{"year":{"lt":1980}}]}}"#,
        r#"{"entity":"book","filters":{"or":[{"status":{"eq":"draft"}},{"genres":{"contains":"horror"}}]}}"#,
        r#"{"entity":"book","filters":{"or":[{"and":[{"status":{"eq":"published"}},{"inStock":{"eq":true}}]},{"price":{"lt":8}}]}}"#,
        r#"{"entity":"book","filters":{"and":[]}}"#,
        r#"{"entity":"book","filters":{"or":[]}}"#,
        r#"{"entity":"book","filters":{"inStock":{"eq":false}},"fields":"title,inStock"}"#,
        r#"{"entity":"book","sorts":{"year":"desc"},"limit":5}"#,
        r#"{"entity":"book","sorts":{"title":"asc"}}"#,
        r#"{"entity":"book","sorts":{"status":"asc","year":"desc"}}"#,
        r#"{"entity":"book","sorts":{"year":"desc","status":"asc"}}"#,
        r#"{"entity":"book","sorts":{"inStock":"desc","price":"asc"},"offset":2,"limit":6}"#,
        r#"{"entity":"book","sorts":{"status":"asc","stock.shop":"desc"},"filters":{"year":{"gt":1970}}}"#,
        r#"{"entity":"book","filters":{"author.id":{"eq":"a2"}}}"#,
        r#"{"entity":"book","filters":{"author.name":{"eq":"Ted Chiang"}}}"#,
        r#"{"entity":"book","filters":{"published":{"gte":"1990-01-01"}},"sorts":{"published":"asc"}}"#,
        r#"{"entity":"book","filters":{"edition":{"eq":"1"}}}"#,
        r#"{"entity":"author","sorts":{"country":"asc","name":"asc"}}"#,
        r#"{"entity":"author","filters":{"tags":{"any":"scifi,drama"}},"countOnly":true}"#,
        r#"{"entity":"nothing"}"#,
        r#"{"entity":"_elysiandb_core_user"}"#,
        r#"{"entity":"book","filters":{"or":{"x":1}}}"#,
        r#"{"entity":"book","filters":{"status":"published"}}"#,
        r#"{"entity":"book","filters":{"status":{"eq":["a"]}}}"#,
        "not json",
        r#"{"entity":"budget_month","limit":3}"#,
        r#"{"entity":"program","filters":{"status":{"eq":"open"}}}"#,
    ] {
        reqs.push(query(body));
    }

    for p in [
        "/api/budget_month",
        "/api/budget_month?sort[month]=desc&limit=4",
        "/api/budget_month/m03",
        "/api/budget_month?countOnly=true",
        "/api/program",
        "/api/program?filter[status]=open",
        "/api/program?filter[label]=*open*",
        "/api/program?countOnly=true&filter[status]=open",
        "/api/program?filter[label]=Beta*&limit=1",
        "/api/program/pg2",
        "/api/hook/budget_month",
        "/api/hook/program",
        "/api/_elysiandb_core_user",
        "/api/_elysiandb_core_acl/x",
    ] {
        reqs.push(g(p));
    }

    reqs
}

pub fn write_requests() -> Vec<FixtureRequest> {
    [
        ("POST", "/api/book", r#"{"id":"b99","title":"Inline Author","year":2025,"author":{"@entity":"author","id":"a99","name":"New Person","country":"FR"}}"#),
        ("GET", "/api/author/a99", ""),
        ("GET", "/api/book/b99?includes=author", ""),
        ("POST", "/api/book", r#"{"id":"b98","title":"Embedded list","coauthors":[{"@entity":"author","id":"a1","nickname":"updated"},{"@entity":"author","id":"a98","name":"Other"}]}"#),
        ("GET", "/api/author/a1", ""),
        ("GET", "/api/author/a98", ""),
        ("PUT", "/api/book/b02", r#"{"title":"Renamed","extra":{"k":"v"}}"#),
        ("GET", "/api/book/b02", ""),
        ("PUT", "/api/book/zz", r#"{"title":"x"}"#),
        ("PUT", "/api/book", r#"[{"id":"b03","status":"draft"},{"id":"b04","status":"draft"},{"id":"nope","status":"x"}]"#),
        ("GET", "/api/book?filter[status]=draft", ""),
        ("GET", "/api/book?sort[title]=asc&limit=5", ""),
        ("DELETE", "/api/book/b05", ""),
        ("DELETE", "/api/book/b05", ""),
        ("GET", "/api/book/b05", ""),
        ("GET", "/api/book/count", ""),
        ("GET", "/api/book?sort[year]=asc&limit=6", ""),
        ("GET", "/api/book?limit=6", ""),
        ("POST", "/api/book", "not json"),
        ("POST", "/api/book", "{}"),
        ("POST", "/api/book", "[]"),
        ("POST", "/api/book", r#"{"id":"b97","title":"With nested core","author":{"@entity":"_elysiandb_core_user","id":"admin"}}"#),
        ("POST", "/api/gadget", r#"{"id":"g1","name":"first","weight":3}"#),
        ("POST", "/api/gadget", r#"{"id":"g2","name":"second","weight":"heavy"}"#),
        ("GET", "/api/gadget/schema", ""),
        ("GET", "/api/gadget", ""),
        ("PUT", "/api/gadget/schema", r#"{"fields":{"name":{"type":"string","required":true},"weight":{"type":"number","required":false}}}"#),
        ("POST", "/api/gadget", r#"{"id":"g3","weight":4}"#),
        ("POST", "/api/gadget", r#"{"id":"g4","name":"fourth","weight":"x"}"#),
        ("POST", "/api/gadget", r#"[{"id":"g5","name":"ok"},{"id":"g6","weight":1}]"#),
        ("GET", "/api/gadget", ""),
        ("POST", "/api/widget/create", r#"{"fields":{"label":{"type":"string","required":true}}}"#),
        ("POST", "/api/widget/create", r#"{"fields":{"label":{"type":"string","required":true}}}"#),
        ("GET", "/api/widget/schema", ""),
        ("GET", "/api/entity/types/name", ""),
        ("POST", "/api/book/migrate", r#"[{"set":[{"status":"migrated"}]}]"#),
        ("GET", "/api/book?filter[status]=migrated&countOnly=true", ""),
        ("GET", "/api/book/b06", ""),
        ("POST", "/api/book/migrate", r#"[{"drop":{"x":1}}]"#),
        ("DELETE", "/api/review", ""),
        ("GET", "/api/review", ""),
        ("GET", "/api/review/count", ""),
        ("POST", "/api/review", r#"{"id":"r100","stars":5,"text":"after reset"}"#),
        ("GET", "/api/review", ""),
        ("GET", "/api/entity/types/name", ""),
        ("GET", "/api/book?includes=author&filter[author.name]=New*", ""),
    ]
    .iter()
    .map(|(method, path, body)| FixtureRequest::new(method, path, body))
    .collect()
}

pub fn transaction_requests() -> Vec<FixtureRequest> {
    [
        ("POST", "/api/tx/begin", ""),
        (
            "POST",
            "/api/tx/{tx}/entity/gadget",
            r#"{"id":"g10","name":"tx one"}"#,
        ),
        (
            "PUT",
            "/api/tx/{tx}/entity/gadget/g1",
            r#"{"name":"tx updated"}"#,
        ),
        ("DELETE", "/api/tx/{tx}/entity/gadget/g2", ""),
        ("POST", "/api/tx/{tx}/commit", ""),
        ("GET", "/api/gadget", ""),
        ("POST", "/api/tx/begin", ""),
        (
            "POST",
            "/api/tx/{tx}/entity/gadget",
            r#"{"id":"g11","weight":2}"#,
        ),
        ("POST", "/api/tx/{tx}/commit", ""),
        ("GET", "/api/gadget/g11", ""),
        ("POST", "/api/tx/begin", ""),
        (
            "POST",
            "/api/tx/{tx}/entity/gadget",
            r#"{"id":"g12","name":"rolled back"}"#,
        ),
        ("POST", "/api/tx/{tx}/rollback", ""),
        ("GET", "/api/gadget/g12", ""),
        ("POST", "/api/tx/unknown/commit", ""),
    ]
    .iter()
    .map(|(method, path, body)| FixtureRequest::new(method, path, body))
    .collect()
}

pub fn kv_requests() -> Vec<FixtureRequest> {
    let r = FixtureRequest::new;

    vec![
        r("PUT", "/kv/session:1", "alpha"),
        r("PUT", "/kv/session:2", "beta"),
        r("PUT", "/kv/derived:total", "42"),
        r("PUT", "/kv/temp?ttl=3600", "short lived"),
        r("GET", "/kv/session:1", ""),
        r("GET", "/kv/temp", ""),
        r("GET", "/kv/missing", ""),
        r("GET", "/kv/session:*", "").unordered(),
        r("GET", "/kv/nothing:*", ""),
        r(
            "GET",
            "/kv/mget?keys=session:1,missing,derived:total,session:1",
            "",
        ),
        r("GET", "/kv/mget?keys=session:*", "").unordered(),
        r("GET", "/kv/mget?keys=", ""),
        r("DELETE", "/kv/session:2", ""),
        r("GET", "/kv/session:2", ""),
        r("PUT", "/kv/session:1", "alpha2"),
        r("GET", "/kv/session:1", ""),
        r("DELETE", "/kv/session:*", ""),
        r("GET", "/kv/session:1", ""),
        r("TCP", "PING", ""),
        r("TCP", "SET tcp:a hello", ""),
        r("TCP", "SET tcp:b world", ""),
        r("TCP", "SET TTL=100 tcp:c ttl value", ""),
        r("TCP", "GET tcp:a", ""),
        r("TCP", "GET tcp:c", ""),
        r("TCP", "GET tcp:none", ""),
        r("TCP", "MGET tcp:a tcp:b tcp:none", ""),
        r("TCP", "GET tcp:*", "").unordered(),
        r("TCP", "DEL tcp:b", ""),
        r("TCP", "GET tcp:b", ""),
        r("TCP", "DEL tcp:*", ""),
        r("TCP", "GET tcp:a", ""),
        r("TCP", "NOPE", ""),
        r("GET", "/kv/api:entity:types:list", ""),
    ]
}

pub const READER_USER: &str =
    r#"{"username":"reader","password":"reader-password-1","role":"user"}"#;
pub const READER_ACL: &str = r#"{"permissions":{"create":true,"read":false,"update":false,"delete":false,"owning_read":true,"owning_write":true,"owning_update":true,"owning_delete":true}}"#;

pub fn ownership_requests() -> Vec<FixtureRequest> {
    let r = FixtureRequest::new;

    vec![
        r("POST", "/api/security/user", READER_USER),
        r("LOGIN", "reader", "reader-password-1").by("reader"),
        r("POST", "/api/note", r#"{"id":"n1","text":"admin note"}"#),
        r("PUT", "/api/acl/reader/note", READER_ACL),
        r("GET", "/api/acl/reader/note", ""),
        r("POST", "/api/note", r#"{"id":"n2","text":"reader note"}"#).by("reader"),
        r(
            "POST",
            "/api/note",
            r#"{"id":"n3","text":"second reader note"}"#,
        )
        .by("reader"),
        r("GET", "/api/note", "").by("reader"),
        r("GET", "/api/note?limit=2", "").by("reader"),
        r("GET", "/api/note/count", "").by("reader"),
        r("GET", "/api/note?countOnly=true", "").by("reader"),
        r("GET", "/api/note/n1", "").by("reader"),
        r("GET", "/api/note/n2", "").by("reader"),
        r("PUT", "/api/note/n1", r#"{"text":"hacked"}"#).by("reader"),
        r("PUT", "/api/note/n2", r#"{"text":"edited"}"#).by("reader"),
        r("DELETE", "/api/note/n1", "").by("reader"),
        r("POST", "/api/query", r#"{"entity":"note"}"#).by("reader"),
        r("GET", "/api/book?limit=2", "").by("reader"),
        r("GET", "/api/note", ""),
        r("GET", "/api/note/count", ""),
    ]
}
