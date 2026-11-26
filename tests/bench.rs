#[path = "internal/tempdir.rs"]
mod tempdir;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use elysiandb::api;
use elysiandb::configuration::{
    ApiConfig, ApiIndexConfig, ApiSchemaConfig, Config, CrashRecoveryConfig, HooksConfig,
    LogConfig, StoreConfig,
};
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::memory;
use elysiandb::query::Filters;
use elysiandb::storage::{self, Hooks};

use tempdir::TempDir;

const STATUSES: [&str; 5] = ["published", "draft", "archived", "review", "out_of_print"];
const WORDS: [&str; 14] = [
    "dragon",
    "dune",
    "kindred",
    "foundation",
    "fifth",
    "story",
    "body",
    "left",
    "lathe",
    "dawn",
    "robots",
    "season",
    "exhalation",
    "problem",
];
const FILL_WORDS: &str = "nuit ombre retour dernier royaume silence étoile mémoire voyage rivière jardin fantôme empire cendre lumière secret hiver orage miroir frontière légende promesse désert océan tempête dragon horizon labyrinthe chasseur prophétie révolte forteresse planète galaxie machine spectre colline rêve mirage sanctuaire vengeance sentinelle odyssée crépuscule aurore abîme écho sirène volcan";

static REGISTER: Once = Once::new();

fn config(folder: &str) -> Config {
    Config {
        store: StoreConfig {
            folder: folder.to_string(),
            shards: 64,
            crash_recovery: CrashRecoveryConfig {
                enabled: std::env::var("ELYSIAN_BENCH_RECOVERY").as_deref() != Ok("0"),
                max_log_mb: 100,
            },
            ..StoreConfig::default()
        },
        api: ApiConfig {
            schema: ApiSchemaConfig {
                enabled: true,
                strict: true,
            },
            hooks: HooksConfig {
                enabled: true,
                ..HooksConfig::default()
            },
            index: ApiIndexConfig { workers: 4 },
            ..ApiConfig::default()
        },
        log: LogConfig {
            level: "error".to_string(),
            ..LogConfig::default()
        },
        ..Config::default()
    }
}

fn set_config(cfg: Config) {
    REGISTER.call_once(api::index::register);
    globals::set_config(Some(Arc::new(cfg)));
}

fn rng(seed: i64) -> Rand {
    Rand::new(rand::new_source(seed))
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn n(v: i64) -> Value {
    Value::Number(v as f64)
}

fn obj(items: Vec<(&str, Value)>) -> Map {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn word(r: &mut Rand) -> &'static str {
    WORDS[r.intn(WORDS.len() as i64) as usize]
}

fn work(i: i64, r: &mut Rand) -> Map {
    let title = format!("{} {} {i}", word(r), word(r));
    let year = n(1900 + r.intn(125));
    let published = format!(
        "{}-{:02}-{:02}T10:00:00Z",
        1900 + r.intn(125),
        1 + r.intn(12),
        1 + r.intn(28)
    );
    let status = STATUSES[r.intn(STATUSES.len() as i64) as usize];
    let rating = Value::Number(r.intn(50) as f64 / 10.0);
    let author = format!("p{:04}", r.intn(2000));
    let summary = format!("a {} tale about {} and {}", word(r), word(r), word(r));
    let tags = vec![s(word(r)), s(word(r))];
    obj(vec![
        ("id", s(&format!("w{i:06}"))),
        ("title", s(&title)),
        ("year", year),
        ("published", s(&published)),
        ("status", s(status)),
        ("rating", rating),
        (
            "author",
            Value::Object(obj(vec![("@entity", s("person")), ("id", s(&author))])),
        ),
        ("summary", s(&summary)),
        ("tags", Value::Array(tags)),
    ])
}

fn filters(field: &str, op: &str, value: &str) -> Filters {
    let mut ops = std::collections::BTreeMap::new();
    ops.insert(op.as_bytes().to_vec(), value.as_bytes().to_vec());
    let mut f = Filters::new();
    f.insert(field.as_bytes().to_vec(), ops);
    f
}

fn go_duration(d: Duration) -> GoDuration {
    GoDuration(i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

fn fill_words() -> Vec<&'static str> {
    FILL_WORDS.split_whitespace().collect()
}

fn fill_phrase(words: &[&'static str], r: &mut Rand, count: usize) -> String {
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(words[r.intn(words.len() as i64) as usize]);
    }
    out.join(" ")
}

fn fill_work(words: &[&'static str], i: i64, r: &mut Rand) -> Map {
    let mut directors = Vec::new();
    let mut k = 0;
    while k < 1 + r.intn(2) {
        directors.push(Value::Object(obj(vec![
            ("@entity", s("person")),
            ("id", s(&format!("p{:06}", r.intn(50000)))),
        ])));
        k += 1;
    }
    let title = format!("thanos {}", fill_phrase(words, r, 3));
    let title_original = fill_phrase(words, r, 3);
    let kind = ["film", "serie", "documentaire"][r.intn(3) as usize];
    let is_twist_work = r.intn(10) == 0;
    let year = n(1950 + r.intn(76));
    let runtime_min = n(80 + r.intn(110));
    let franchise = words[r.intn(words.len() as i64) as usize];
    let synopsis_safe = fill_phrase(words, r, 40);
    obj(vec![
        ("id", s(&format!("w{i:08}"))),
        ("title", s(&title)),
        ("title_original", s(&title_original)),
        ("title_uncertain", Value::Bool(false)),
        ("kind", s(kind)),
        ("is_twist_work", Value::Bool(is_twist_work)),
        ("year", year),
        ("runtime_min", runtime_min),
        ("franchise", s(franchise)),
        ("synopsis_safe", s(&synopsis_safe)),
        ("directors", Value::Array(directors)),
        ("warnings", Value::Array(Vec::new())),
        ("streaming", Value::Array(vec![s("netflix")])),
    ])
}

fn fill_edition(words: &[&'static str], i: i64, r: &mut Rand) -> Map {
    let label = format!("thanos {}", fill_phrase(words, r, 3));
    let verified_on_arrival = r.intn(2) == 0;
    let work_id = format!("w{:08}", r.intn(1_000_000));
    let location = ["pile_prioritaire", "etagere", "cave"][r.intn(3) as usize];
    let disc_count = n(1 + r.intn(3));
    let ean = format!("{:013}", r.int63n(9_999_999_999_999));
    let bonus = fill_phrase(words, r, 15);
    obj(vec![
        ("id", s(&format!("e{i:08}"))),
        ("label", s(&label)),
        ("supports", Value::Array(vec![s("bluray")])),
        ("support_uncertain", Value::Bool(false)),
        ("state", s("ouvert")),
        ("vostfr_ok", s("oui")),
        ("verified_on_arrival", Value::Bool(verified_on_arrival)),
        (
            "works",
            Value::Array(vec![Value::Object(obj(vec![
                ("@entity", s("work")),
                ("id", s(&work_id)),
            ]))]),
        ),
        ("location", s(location)),
        ("region_label", s("edition_francaise")),
        ("publisher", s("studio")),
        ("audio", Value::Array(vec![s("fr"), s("en")])),
        ("subtitles", Value::Array(vec![s("fr")])),
        ("disc_count", disc_count),
        ("ean", s(&ean)),
        ("bonus", s(&bonus)),
    ])
}

struct RestoreHooks(Hooks);

impl Drop for RestoreHooks {
    fn drop(&mut self) {
        let mut hooks = storage::hooks();
        hooks.value_cache_bytes = self.0.value_cache_bytes.take();
        hooks.page_cache_bytes = self.0.page_cache_bytes.take();
        storage::set_hooks(hooks);
    }
}

struct CloseStorage;

impl Drop for CloseStorage {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

#[test]
fn fill_with_a_memory_budget() {
    let total: i64 = std::env::var("ELYSIAN_FILL_DOCS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if total <= 0 {
        println!(
            "set ELYSIAN_FILL_DOCS (for example 3200000) to fill two entities under a 4 GB budget"
        );
        return;
    }

    let budget: i64 = 4096 << 20;
    let previous = storage::hooks();
    let mut hooks = previous.clone();
    hooks.value_cache_bytes = Some(Arc::new(move |_: &Config| budget / 4));
    hooks.page_cache_bytes = Some(Arc::new(move |_: &Config| budget / 4));
    storage::set_hooks(hooks);
    let _restore = RestoreHooks(previous);

    let dir = TempDir::new();
    let mut cfg = config(&dir.path().to_string_lossy());
    cfg.store.flush_interval_seconds = 5;
    cfg.store.memory.limit_mb = 4096;
    cfg.store.shards = 512;
    set_config(cfg);
    if let Err(err) = storage::open(storage::MODE_SERVER) {
        panic!("{err}");
    }
    let _close = CloseStorage;

    let words = fill_words();
    let mut seed = rng(1);
    api::write_list_of_entities(b"work", &mut [fill_work(&words, -1, &mut seed)]);
    api::write_list_of_entities(b"edition", &mut [fill_edition(&words, -1, &mut seed)]);
    for f in ["title", "year", "runtime_min"] {
        api::list_entities(b"work", 1, 0, f.as_bytes(), true, None, b"", b"");
    }
    for (field, value) in [
        ("kind", "film"),
        ("directors.id", "p000001"),
        ("is_twist_work", "true"),
    ] {
        let f = filters(field, "eq", value);
        api::list_entities(b"work", 1, 0, b"", true, Some(&f), b"", b"");
    }
    for (field, value) in [("location", "cave"), ("works.id", "w00000001")] {
        let f = filters(field, "eq", value);
        api::list_entities(b"edition", 1, 0, b"", true, Some(&f), b"", b"");
    }
    api::list_entities(b"work", 1, 0, b"", true, None, "tempête".as_bytes(), b"");
    api::list_entities(b"edition", 1, 0, b"", true, None, "tempête".as_bytes(), b"");
    std::thread::sleep(Duration::from_secs(1));

    let done = Arc::new(AtomicI64::new(0));
    let start = Instant::now();
    let mut workers = Vec::new();
    for w in 0..4i64 {
        let done = done.clone();
        let words = words.clone();
        workers.push(std::thread::spawn(move || {
            let mut r = rng(w + 10);
            let entity = ["work", "edition"][(w % 2) as usize];
            loop {
                let base = done.fetch_add(500, Ordering::SeqCst);
                if base >= total {
                    return;
                }
                let mut batch = Vec::with_capacity(500);
                for i in base..base + 500 {
                    if entity == "work" {
                        batch.push(fill_work(&words, i, &mut r));
                    } else {
                        batch.push(fill_edition(&words, i, &mut r));
                    }
                }
                api::write_list_of_entities(entity.as_bytes(), &mut batch);
            }
        }));
    }

    let step = 100_000;
    let mut next = step;
    let mut last_t = start;
    let mut peak: i64 = 0;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let live = memory::allocated();
        peak = peak.max(live);

        let n = done.load(Ordering::SeqCst);
        if n >= next || n >= total {
            let now = Instant::now();
            let st = storage::engine().map(|e| e.stats()).unwrap_or_default();
            println!(
                "{n:9} docs {:7.0} docs/s  memory {:5} MB  last checkpoint {:6.0} ms  pages {:5} MB",
                step as f64 / now.duration_since(last_t).as_secs_f64(),
                live >> 20,
                st.last_checkpoint_ms,
                st.resident_struct_bytes >> 20
            );
            last_t = now;
            next += step;
        }
        if n >= total {
            break;
        }
    }
    for w in workers {
        if let Err(p) = w.join() {
            std::panic::resume_unwind(p);
        }
    }
    println!(
        "fill: {total} documents in {}, peak memory {} MB for a budget of {} MB",
        go_duration(start.elapsed()).round(GoDuration::SECOND),
        peak >> 20,
        budget >> 20
    );

    let t0 = Instant::now();
    if let Err(err) = storage::checkpoint() {
        panic!("{err}");
    }
    println!(
        "checkpoint after the fill: {}",
        go_duration(t0.elapsed()).round(GoDuration::MILLISECOND)
    );

    api::delete_all_entities(b"work");
    api::delete_all_entities(b"edition");
    std::thread::sleep(Duration::from_secs(5));
    memory::free_os_memory();
    println!(
        "after deleting everything: allocated {} MB, resident {} MB",
        memory::allocated() >> 20,
        memory::resident() >> 20
    );
}

#[test]
fn scale_report() {
    if std::env::var_os("ELYSIAN_SCALE").is_none_or(|v| v.is_empty()) {
        println!("set ELYSIAN_SCALE=1 to measure the engine on 60,000 documents");
        return;
    }

    let dir = TempDir::new();
    set_config(config(&dir.path().to_string_lossy()));
    storage::load_db();

    let base = memory::allocated();
    let mut r = rng(7);
    let total: usize = 60_000;
    let mut docs: Vec<Map> = (0..total as i64).map(|i| work(i, &mut r)).collect();

    let start = Instant::now();
    for chunk in docs.chunks_mut(500) {
        api::write_list_of_entities(b"work", chunk);
    }
    let insert = start.elapsed();
    drop(docs);

    api::list_entities(b"work", 1, 0, b"year", true, None, b"", b"");
    let draft = filters("status", "eq", "draft");
    api::list_entities(b"work", 1, 0, b"", true, Some(&draft), b"", b"");
    let heap = memory::allocated() - base;

    let t0 = Instant::now();
    api::delete_entity_by_id(b"work", b"w030000");
    let delete_at_60k = t0.elapsed();

    if let Err(err) = storage::checkpoint() {
        panic!("{err}");
    }

    let t0 = Instant::now();
    if let Err(err) = storage::close() {
        panic!("{err}");
    }
    let close_time = t0.elapsed();

    let t0 = Instant::now();
    storage::load_db();
    let restart = t0.elapsed();

    for i in 0..2000 {
        api::write_entity(
            b"work",
            &mut obj(vec![
                ("id", s(&format!("tail-{i}"))),
                ("status", s("draft")),
                ("year", n(i)),
            ]),
        );
    }
    match storage::engine() {
        Some(e) => e.abandon(),
        None => panic!("the storage is not open"),
    }

    let t0 = Instant::now();
    storage::load_db();
    let crash_restart = t0.elapsed();

    let got = api::count_entities(b"work");
    assert_eq!(
        got,
        total - 1 + 2000,
        "expected {} documents after a crash, got {got}",
        total - 1 + 2000
    );

    let t0 = Instant::now();
    api::delete_all_entities(b"work");
    let reset = t0.elapsed();
    let _ = storage::close();

    println!(
        "insert: {total} docs in {} ({:.0} docs/s)",
        go_duration(insert).round(GoDuration::MILLISECOND),
        total as f64 / insert.as_secs_f64()
    );
    println!(
        "allocated after the fill with 2 indexes: {:.1} MB ({:.0} bytes per document)",
        heap as f64 / f64::from(1 << 20),
        heap as f64 / total as f64
    );
    println!(
        "one delete at {total} documents: {}",
        go_duration(delete_at_60k)
    );
    println!(
        "close with checkpoint: {}, restart after a clean stop: {}, restart after a crash with 2000 writes to replay: {}",
        go_duration(close_time).round(GoDuration::MILLISECOND),
        go_duration(restart).round(GoDuration::MILLISECOND),
        go_duration(crash_restart).round(GoDuration::MILLISECOND)
    );
    println!("entity reset: {}", go_duration(reset));
}
