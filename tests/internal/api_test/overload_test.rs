use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use elysiandb::api::guard;
use elysiandb::api::{self, Guard, GuardError, ListQuery};
use elysiandb::configuration::{ApiConfig, ApiQueryConfig, Config, StoreConfig};
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::query::Filters;
use elysiandb::storage;
use elysiandb::sync::{Condvar, Mutex};

use super::setup;
use super::tempdir::TempDir;

pub const OVERLOAD_DOCS: usize = 5000;

pub struct OverloadStore {
    _dir: TempDir,
}

impl Drop for OverloadStore {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub fn init_overload_store(query: ApiQueryConfig) -> OverloadStore {
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: dir.path().to_string_lossy().into_owned(),
            shards: 8,
            ..StoreConfig::default()
        },
        api: ApiConfig {
            query,
            ..ApiConfig::default()
        },
        ..Config::default()
    })));
    storage::load_db();
    storage::load_json_db();

    let mut list: Vec<Map> = (0..OVERLOAD_DOCS)
        .map(|i| {
            let mut m = Map::new();
            m.insert("id".to_string(), Value::String(format!("d{i:05}")));
            m.insert("note".to_string(), Value::String("n".repeat(20)));
            m.insert("k".to_string(), Value::Int((i % 7) as i64));
            m
        })
        .collect();
    api::write_list_of_entities(b"big", &mut list);

    OverloadStore { _dir: dir }
}

static HEAVY_FILTERS: LazyLock<Filters> =
    LazyLock::new(|| super::filters(&[("note", &[("neq", "x")])]));

fn heavy_query(guard: Option<&Guard>) -> ListQuery<'_, '_> {
    ListQuery {
        entity: b"big",
        filters: Some(&HEAVY_FILTERS),
        search: b"nnn",
        count_only: true,
        guard,
        ..ListQuery::default()
    }
}

type Outcome = (usize, Option<GuardError>);

fn run_plan(guard: Arc<Guard>) -> mpsc::Receiver<Outcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let res = api::plan(heavy_query(Some(&guard)));
        let _ = tx.send((res.total, res.err));
    });
    rx
}

struct LaneHolder {
    released: Arc<(Mutex<bool>, Condvar)>,
    done: mpsc::Receiver<Outcome>,
}

impl LaneHolder {
    fn free(&self) {
        let (lock, cv) = &*self.released;
        *lock.lock() = true;
        cv.notify_all();
    }

    fn wait(&self) -> Outcome {
        self.done
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("holder never finished"))
    }
}

impl Drop for LaneHolder {
    fn drop(&mut self) {
        self.free();
    }
}

fn hold_heavy_lane() -> LaneHolder {
    let released = Arc::new((Mutex::new(false), Condvar::new()));
    let (inside_tx, inside_rx) = mpsc::channel::<()>();
    let inside_tx = Mutex::new(Some(inside_tx));
    let calls = AtomicI64::new(0);
    let gate = released.clone();
    let guard = Arc::new(Guard::new(
        None,
        Some(Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 < 3 {
                std::thread::sleep(Duration::from_millis(3));
                return false;
            }
            if calls.load(Ordering::SeqCst) == 3 {
                if let Some(tx) = inside_tx.lock().take() {
                    let _ = tx.send(());
                }
                let (lock, cv) = &*gate;
                let mut open = lock.lock();
                while !*open {
                    open = cv.wait(open);
                }
            }
            false
        })),
    ));
    let done = run_plan(guard);
    if inside_rx.recv_timeout(Duration::from_secs(5)).is_err() {
        panic!("the holder never entered the heavy lane");
    }

    LaneHolder { released, done }
}

fn slow_guard(gone: Option<Arc<AtomicBool>>) -> Guard {
    Guard::new(
        None,
        Some(Box::new(move || {
            std::thread::sleep(Duration::from_millis(1));
            gone.as_ref().is_some_and(|g| g.load(Ordering::SeqCst))
        })),
    )
}

fn wait_for_queue(n: i64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while api::heavy_waiting() != n {
        if Instant::now() > deadline {
            panic!(
                "expected {n} queued heavy queries, got {}",
                api::heavy_waiting()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn recv(name: &str, rx: &mpsc::Receiver<Outcome>) -> Outcome {
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("{name} never finished"))
}

#[test]
fn test_heavy_lane_rejects_requests_when_its_queue_is_full() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        workers: 1,
        max_queued: 1,
        queue_timeout_ms: -1,
        ..ApiQueryConfig::default()
    });
    let holder = hold_heavy_lane();
    let queued = run_plan(Arc::new(slow_guard(None)));
    wait_for_queue(1);
    let guard = slow_guard(None);
    let start = Instant::now();
    let res = api::plan(heavy_query(Some(&guard)));
    if res.err != Some(GuardError::Busy) || guard.err() != Some(GuardError::Busy) {
        panic!(
            "a request beyond the queue limit is rejected, got {:?} / {:?}",
            res.err,
            guard.err()
        );
    }
    if start.elapsed() > Duration::from_secs(1) {
        panic!(
            "the rejection comes after a few milliseconds of work, took {:?}",
            start.elapsed()
        );
    }
    let internal = run_plan(Arc::new(Guard::until(
        Some(Arc::new(slow_guard(None))),
        Some(Instant::now() + Duration::from_secs(60)),
    )));
    wait_for_queue(2);
    holder.free();
    for (name, rx) in [
        ("holder", &holder.done),
        ("internal", &internal),
        ("queued", &queued),
    ] {
        let (total, err) = recv(name, rx);
        if err.is_some() || total != OVERLOAD_DOCS {
            panic!("{name}: expected {OVERLOAD_DOCS} documents, got {total} ({err:?})");
        }
    }
}

#[test]
fn test_queued_heavy_query_leaves_when_its_client_is_gone() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        workers: 1,
        queue_timeout_ms: -1,
        ..ApiQueryConfig::default()
    });
    let holder = hold_heavy_lane();
    let gone = Arc::new(AtomicBool::new(false));
    let result = run_plan(Arc::new(slow_guard(Some(gone.clone()))));
    wait_for_queue(1);
    gone.store(true, Ordering::SeqCst);
    match result.recv_timeout(Duration::from_secs(2)) {
        Ok((_, err)) => {
            if err != Some(GuardError::Abandoned) {
                panic!("expected the abandoned error, got {err:?}");
            }
        }
        Err(_) => panic!("an abandoned request should leave the queue"),
    }
    if api::heavy_waiting() != 0 {
        panic!("the queue should be empty, got {}", api::heavy_waiting());
    }
    holder.free();
    holder.wait();
}

#[test]
fn test_queued_heavy_query_gives_up_after_the_queue_timeout() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        workers: 1,
        queue_timeout_ms: 100,
        ..ApiQueryConfig::default()
    });
    let holder = hold_heavy_lane();
    let start = Instant::now();
    let guard = slow_guard(None);
    let res = api::plan(heavy_query(Some(&guard)));
    let waited = start.elapsed();
    if res.err != Some(GuardError::Busy) {
        panic!("expected busy after the queue timeout, got {:?}", res.err);
    }
    if waited < Duration::from_millis(100) || waited > Duration::from_secs(2) {
        panic!("the request should wait about the queue timeout, waited {waited:?}");
    }
    let admitted = run_plan(Arc::new(Guard::until(
        Some(Arc::new(slow_guard(None))),
        Some(Instant::now() + Duration::from_secs(60)),
    )));
    std::thread::sleep(Duration::from_millis(300));
    if api::heavy_waiting() != 1 {
        panic!("an admitted query waits past the queue timeout");
    }
    holder.free();
    let (total, err) = recv("admitted", &admitted);
    if err.is_some() || total != OVERLOAD_DOCS {
        panic!("the admitted query should finish, got {total} {err:?}");
    }
    holder.wait();
}

#[test]
fn test_abandon_stops_a_scan_and_keeps_its_error() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        queue_timeout_ms: -1,
        ..ApiQueryConfig::default()
    });
    let checks = Arc::new(AtomicI64::new(0));
    let counter = checks.clone();
    let guard = Arc::new(Guard::new(
        None,
        Some(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst) + 1 > 3
        })),
    ));
    let res = api::plan(heavy_query(Some(&guard)));
    if res.err != Some(GuardError::Abandoned) || res.total != 0 || !res.docs.is_empty() {
        panic!(
            "an abandoned scan returns nothing, got {} {:?}",
            res.total, res.err
        );
    }
    let n = checks.load(Ordering::SeqCst);
    if n > 5 {
        panic!("the scan should stop at the first failed check, checked {n} times");
    }
    let again = api::plan(ListQuery {
        entity: b"big",
        limit: 1,
        guard: Some(&guard),
        ..ListQuery::default()
    });
    if again.err != Some(GuardError::Abandoned) {
        panic!(
            "a stopped guard stops every later plan of the request, got {:?}",
            again.err
        );
    }
    let child = Guard::until(
        Some(guard.clone()),
        Some(Instant::now() + Duration::from_secs(3600)),
    );
    if child.check() != Err(GuardError::Abandoned) {
        panic!("a child guard sees the error of its parent");
    }
    let child = Guard::until(
        Some(Arc::new(Guard::default())),
        Instant::now().checked_sub(Duration::from_millis(1)),
    );
    if child.check() != Err(GuardError::Deadline) {
        panic!("a child guard stops at its own deadline");
    }
    if guard::check(None).is_err() || guard::err(None).is_some() || guard::stopped(None) {
        panic!("a nil guard never stops");
    }
}

#[test]
fn test_a_cheap_plan_never_waits_for_the_heavy_lane() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        workers: 1,
        max_queued: 1,
        queue_timeout_ms: -1,
        ..ApiQueryConfig::default()
    });
    let holder = hold_heavy_lane();
    let start = Instant::now();
    let guard = Guard::default();
    let res = api::plan(ListQuery {
        entity: b"big",
        search: b"nnn",
        limit: 10,
        guard: Some(&guard),
        ..ListQuery::default()
    });
    if res.err.is_some() || res.docs.len() != 10 {
        panic!(
            "a search page is served while the lane is held, got {} {:?}",
            res.docs.len(),
            res.err
        );
    }
    if start.elapsed() > Duration::from_secs(1) || api::heavy_waiting() != 0 {
        panic!(
            "a search page does not queue, took {:?} with {} waiting",
            start.elapsed(),
            api::heavy_waiting()
        );
    }
    holder.free();
    holder.wait();
}

fn hook_config(max_queued: i64) {
    globals::set_config(Some(Arc::new(Config {
        api: ApiConfig {
            query: ApiQueryConfig {
                max_queued,
                queue_timeout_ms: -1,
                ..ApiQueryConfig::default()
            },
            ..ApiConfig::default()
        },
        ..Config::default()
    })));
}

fn spawn_hook_run(guard: Arc<Guard>, errs: mpsc::Sender<Option<GuardError>>) {
    std::thread::spawn(move || {
        let err = api::acquire_hook_lane(Some(&guard)).err();
        let _ = errs.send(err);
    });
}

#[test]
fn test_the_hook_lane_queue_is_longer_than_the_heavy_one() {
    let _s = setup();
    hook_config(0);
    let free = match api::acquire_hook_lane(Some(&Guard::default())) {
        Ok(slot) => slot,
        Err(err) => panic!("{err}"),
    };
    const WAITING: usize = 32;
    let (tx, rx) = mpsc::channel();
    for _ in 0..WAITING {
        spawn_hook_run(Arc::new(Guard::default()), tx.clone());
    }
    wait_for_queue(WAITING as i64);
    match api::acquire_hook_lane(Some(&Guard::default())) {
        Err(GuardError::Busy) => {}
        other => panic!("beyond its queue the lane refuses, got {:?}", other.err()),
    }
    drop(free);
    for _ in 0..WAITING {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => {}
            Ok(Some(err)) => panic!("a queued hook is served, got {err}"),
            Err(_) => panic!("the queued hooks never got a slot"),
        }
    }
    wait_for_queue(0);
}

#[test]
fn test_a_request_counts_once_in_the_hook_lane_queue() {
    let _s = setup();
    hook_config(2);
    let free = match api::acquire_hook_lane(Some(&Guard::default())) {
        Ok(slot) => slot,
        Err(err) => panic!("{err}"),
    };
    const HELPERS: usize = 8;
    let (tx, rx) = mpsc::channel();
    let page = Arc::new(Guard::default());
    for _ in 0..HELPERS {
        spawn_hook_run(page.clone(), tx.clone());
    }
    wait_for_queue(1);
    spawn_hook_run(Arc::new(Guard::default()), tx.clone());
    wait_for_queue(2);
    let beyond = Guard::default();
    match api::acquire_hook_lane(Some(&beyond)) {
        Err(GuardError::Busy) => {}
        other => panic!(
            "a third request is beyond the queue limit of 2, got {:?}",
            other.err()
        ),
    }
    drop(free);
    for _ in 0..=HELPERS {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => {}
            Ok(Some(err)) => panic!("a request within the queue limit is served, got {err}"),
            Err(_) => panic!("the waiting runs never got a slot"),
        }
    }
    wait_for_queue(0);
}
