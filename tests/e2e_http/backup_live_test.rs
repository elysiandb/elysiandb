use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elysiandb::acl;
use elysiandb::archive::gzip;
use elysiandb::archive::tar;
use elysiandb::backup::{self, KvEntry};
use elysiandb::boot;
use elysiandb::cache;
use elysiandb::cmd;
use elysiandb::configuration::{Config, CrashRecoveryConfig};
use elysiandb::datastore::Tx;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Decoder, Map, Value};
use elysiandb::gocompat::time::{Duration as GoDuration, Time};
use elysiandb::hook;
use elysiandb::httpd::{Server, bind_tcp4};
use elysiandb::security;
use elysiandb::storage;

use crate::client::{self, Body, Head, Jar, Multipart};
use crate::harness::{
    Configure, EnvGuard, TempDir, default_api_config, map, register_packages, vn, vs,
};

struct LiveServer {
    url: String,
    addr: String,
    host: String,
    jar: Mutex<Jar>,
    server: Server,
    thread: Option<JoinHandle<io::Result<()>>>,
    _dir: TempDir,
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        let _ = self.server.shutdown();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = storage::close();
    }
}

fn start_live_server(configure: Configure<'_>) -> LiveServer {
    register_packages();
    let dir = TempDir::new();
    let mut cfg = default_api_config(&dir.folder());
    cfg.store.crash_recovery = CrashRecoveryConfig::default();
    cfg.store.crash_recovery.enabled = true;
    cfg.store.crash_recovery.max_log_mb = 100;
    cfg.api.schema = Default::default();
    if let Some(f) = configure {
        f(&mut cfg);
    }
    globals::set_config(Some(Arc::new(cfg)));
    security::reset_login_attempts();
    if let Err(err) = storage::open(storage::MODE_SERVER) {
        panic!("{err}");
    }
    cache::init_cache(GoDuration(10 * GoDuration::SECOND.0));
    hook::init_hooks();
    if let Err(err) = security::init_admin_user_if_not_exists() {
        panic!("{err}");
    }
    let _ = acl::init_acl();

    let ln = bind_tcp4("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    let addr = ln
        .local_addr()
        .unwrap_or_else(|e| panic!("{e}"))
        .to_string();

    let srv = boot::new_http_server();
    let serving = srv.clone();
    let thread = std::thread::spawn(move || serving.serve(ln));

    LiveServer {
        url: format!("http://{addr}"),
        host: addr.clone(),
        addr,
        jar: Mutex::new(Jar::default()),
        server: srv,
        thread: Some(thread),
        _dir: dir,
    }
}

impl LiveServer {
    fn login(&self) {
        let (status, _) = self.do_(
            "POST",
            "/api/security/login",
            Body::Sized(br#"{"username":"admin","password":"admin"}"#),
            "application/json",
        );
        if status != 200 {
            panic!("login: {status}");
        }
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Body<'_>,
        content_type: &str,
    ) -> io::Result<client::Response> {
        let mut head = Head::new();
        if !content_type.is_empty() {
            head.set("Content-Type", content_type);
        }
        let cookies = self.jar.lock().unwrap_or_else(|e| e.into_inner()).header();
        if !cookies.is_empty() {
            head.set("Cookie", cookies);
        }

        let request = client::go_request(method, &self.host, path, &body, &head);
        let stream = TcpStream::connect(&self.addr)?;
        let resp = client::round_trip(stream, &request, method)?;
        self.jar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store(&resp);
        Ok(resp)
    }

    fn do_(&self, method: &str, path: &str, body: Body<'_>, content_type: &str) -> (u16, Vec<u8>) {
        match self.send(method, path, body, content_type) {
            Ok(resp) => (resp.status, resp.body),
            Err(err) => panic!("{method} {path}: {err}"),
        }
    }

    fn cookie_header(&self) -> String {
        self.jar.lock().unwrap_or_else(|e| e.into_inner()).header()
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn archive_files(archive: &[u8]) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    let gz = gzip::new_reader(archive).unwrap_or_else(|e| panic!("{e}"));

    let mut files = BTreeMap::new();
    let mut order = Vec::new();
    let mut tr = tar::Reader::new(gz);
    loop {
        let h = match tr.next() {
            Ok(Some(h)) => h,
            Ok(None) => return (files, order),
            Err(err) => panic!("{err}"),
        };

        let mut data = Vec::new();
        if let Err(err) = tr.read_to_end(&mut data) {
            panic!("{err}");
        }
        let name = text(&h.name);
        files.insert(name.clone(), data);
        order.push(name);
    }
}

fn document_ids(data: &[u8]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut dec = Decoder::new(data);
    loop {
        let mut doc = Map::new();
        match dec.decode(&mut doc) {
            Ok(()) => {}
            Err(err) if err.is_eof() => return ids,
            Err(err) => panic!("{err}"),
        }
        let id = match doc.get("id") {
            Some(Value::String(id)) => id.clone(),
            other => panic!("the document id is not a string: {other:?}"),
        };
        ids.insert(id);
    }
}

fn entity_file<'a>(files: &'a BTreeMap<String, Vec<u8>>, entity: &str) -> &'a [u8] {
    files
        .get(&backup::entity_path(entity.as_bytes()))
        .map_or(&[][..], Vec::as_slice)
}

fn doc(id: &str, n: f64, pad: &str) -> Map {
    map(vec![("id", vs(id)), ("n", vn(n)), ("pad", vs(pad))])
}

#[test]
fn live_backup_holds_one_instant_under_writes() {
    let s = start_live_server(None);
    s.login();

    let e = storage::engine().unwrap_or_else(|| panic!("no engine"));
    let pad = "x".repeat(400);
    let put = |n: i64| {
        let id = format!("n{n:07}");
        e.update(|tx: &mut Tx<'_>| {
            tx.ensure_type(b"left");
            tx.ensure_type(b"right");
            tx.put(b"left", id.as_bytes(), &doc(&id, n as f64, &pad))?;
            tx.put(b"right", id.as_bytes(), &doc(&id, n as f64, &pad))
        })
    };

    let mut n = 1;
    while n <= 30000 {
        if let Err(err) = e.update(|tx: &mut Tx<'_>| {
            tx.ensure_type(b"left");
            tx.ensure_type(b"right");
            for i in n..n + 1000 {
                let id = format!("n{i:07}");
                tx.put(b"left", id.as_bytes(), &doc(&id, f64::from(i), &pad))?;
                tx.put(b"right", id.as_bytes(), &doc(&id, f64::from(i), &pad))?;
            }
            Ok(())
        }) {
            panic!("{err}");
        }
        n += 1000;
    }

    let acked = AtomicI64::new(30000);
    let stop = AtomicBool::new(false);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());

    let (status, archive, before, after) = std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut n: i64 = 30001;
            while !stop.load(Ordering::SeqCst) {
                if let Err(err) = put(n) {
                    errors
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(format!("write {n}: {err}"));
                    return;
                }
                if let Err(err) = e.kv_put(b"counter", n.to_string().as_bytes(), 0) {
                    errors
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(format!("key {n}: {err}"));
                    return;
                }
                acked.store(n, Ordering::SeqCst);
                n += 1;
            }
        });

        std::thread::sleep(Duration::from_millis(20));
        let before = acked.load(Ordering::SeqCst);
        let (status, archive) = s.do_("GET", "/api/backup", Body::None, "");
        let after = acked.load(Ordering::SeqCst);
        stop.store(true, Ordering::SeqCst);
        (status, archive, before, after)
    });

    let errors = errors.into_inner().unwrap_or_else(|e| e.into_inner());

    if status != 200 {
        panic!("backup: {status} {errors:?}");
    }

    let (files, _) = archive_files(&archive);
    let left = document_ids(entity_file(&files, "left"));
    let right = document_ids(entity_file(&files, "right"));
    if left.len() != right.len() {
        panic!(
            "a transaction is torn in the archive: {} left, {} right",
            left.len(),
            right.len()
        );
    }

    for id in &left {
        if !right.contains(id) {
            panic!("{id} is only on one side");
        }
    }

    for n in 1..=left.len() {
        if !left.contains(&format!("n{n:07}")) {
            panic!(
                "hole at {n}: the archive is not one instant of a sequence of {} writes",
                left.len()
            );
        }
    }

    if (left.len() as i64) < before {
        panic!(
            "a write confirmed before the backup is missing: {} documents, {before} confirmed",
            left.len()
        );
    }

    if left.len() as i64 > after + 1 {
        panic!(
            "the archive holds writes made after it: {} documents, {after} confirmed at the end",
            left.len()
        );
    }

    let mut counter = String::new();
    for line in files
        .get(backup::KV_PATH)
        .map_or(&[][..], Vec::as_slice)
        .split(|&c| c == b'\n')
    {
        let mut kv = KvEntry::default();
        if json::unmarshal(line, &mut kv).is_ok() && kv.key.as_bytes() == b"counter" {
            counter = text(kv.value.as_bytes());
        }
    }

    let got: i64 = counter.parse().unwrap_or(0);
    let docs = left.len() as i64;
    if got < before || got > docs || got < docs - 1 {
        panic!(
            "the key is not of the same instant: {counter:?} for {} documents",
            left.len()
        );
    }

    if docs == after && after - before == 0 {
        eprintln!("no write landed during this backup");
    }

    let mut snapshot = BTreeMap::new();
    let live_snap = e.snapshot(b"left");
    for id in &left {
        let Some(d) = live_snap.get(id.as_bytes()) else {
            panic!("{id} is gone");
        };
        let raw = d.bytes().unwrap_or_else(|err| panic!("{err}"));
        snapshot.insert(id.clone(), raw.to_vec());
    }

    let (status, report) = s.do_(
        "POST",
        "/api/restore",
        Body::Sized(&archive),
        "application/gzip",
    );
    if status != 200 {
        panic!("restore: {status} {}", text(&report));
    }

    let engine = storage::engine().unwrap_or_else(|| panic!("no engine"));
    let restored = engine.snapshot(b"left");
    if restored.len() != left.len() || engine.snapshot(b"right").len() != right.len() {
        panic!("restored {}, archive {}", restored.len(), left.len());
    }

    for (id, want) in &snapshot {
        let Some(d) = restored.get(id.as_bytes()) else {
            panic!("{id} is missing after the restore");
        };
        match d.bytes() {
            Ok(got) if &got[..] == want.as_slice() => {}
            Ok(got) => panic!("{id} differs after the restore: {}", text(&got)),
            Err(err) => panic!("{id} differs after the restore: {err}"),
        }
    }

    match engine.kv_get(b"counter") {
        Some((Some(value), _)) if text(&value) == counter => {}
        other => panic!(
            "key {:?} after the restore, {counter:?} in the archive",
            other.and_then(|(v, _)| v).map(|v| text(&v))
        ),
    }

    let (again, _) = archive_files(&must_backup());
    if entity_file(&again, "left") != entity_file(&files, "left") {
        panic!("a backup of the restored data holds other bytes");
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}

fn must_backup() -> Vec<u8> {
    let mut out = Vec::new();
    if let Err(err) = backup::create(&mut out, Time::now().utc()) {
        panic!("{err}");
    }

    out
}

fn random_payload(n: usize) -> String {
    let mut b = vec![0u8; n / 2];
    let _ = elysiandb::crypto::rand::os_random(&mut b);

    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn fill_random(docs: usize, size: usize) {
    let e = storage::engine().unwrap_or_else(|| panic!("no engine"));
    let mut n = 0;
    while n < docs {
        if let Err(err) = e.update(|tx: &mut Tx<'_>| {
            tx.ensure_type(b"big");
            let mut i = n;
            while i < n + 100 && i < docs {
                let id = format!("b{i:06}");
                tx.put(
                    b"big",
                    id.as_bytes(),
                    &map(vec![("id", vs(&id)), ("data", vs(&random_payload(size)))]),
                )?;
                i += 1;
            }
            Ok(())
        }) {
            panic!("{err}");
        }
        n += 100;
    }
}

fn big_count() -> usize {
    storage::engine()
        .map(|e| e.snapshot(b"big").len())
        .unwrap_or(0)
}

#[test]
fn large_archives_travel_without_the_limits_of_the_other_routes() {
    let s = start_live_server(None);
    s.login();
    fill_random(2000, 8000);

    let (status, archive) = s.do_("GET", "/api/backup", Body::None, "");
    if status != 200 || archive.len() < 6 << 20 {
        panic!("backup: {status}, {} bytes", archive.len());
    }

    let check = |label: &str| {
        let n = big_count();
        if n != 2000 {
            panic!("{label}: {n} documents");
        }
    };

    let (status, body) = s.do_(
        "POST",
        "/api/restore",
        Body::Sized(&archive),
        "application/gzip",
    );
    if status != 200 {
        panic!("raw restore: {status} {}", text(&body));
    }
    s.login();
    check("raw");

    let (status, body) = s.do_(
        "POST",
        "/api/restore",
        Body::Chunked(&archive),
        "application/gzip",
    );
    if status != 200 {
        panic!("chunked restore: {status} {}", text(&body));
    }
    s.login();
    check("chunked");

    let mut form = Multipart::new();
    form.file("archive", "backup.tar.gz", &archive);
    let content_type = form.content_type();
    let form = form.close();
    let (status, body) = s.do_("POST", "/api/restore", Body::Sized(&form), &content_type);
    if status != 200 {
        panic!("multipart restore: {status} {}", text(&body));
    }
    s.login();
    check("multipart");

    let mut limited = TcpStream::connect(&s.addr).unwrap_or_else(|e| panic!("{e}"));
    let _ = write!(
        limited,
        "POST /api/big HTTP/1.1\r\nHost: x\r\nCookie: {}\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n",
        s.cookie_header(),
        5 << 20
    );
    let answer = client::read_response(
        &mut BufReader::new(limited.try_clone().unwrap_or_else(|e| panic!("{e}"))),
        "POST",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let limit_body = text(&answer.body);
    if answer.status != 400 || !limit_body.contains("request body too large") {
        panic!(
            "the other routes keep their limit: {} {limit_body}",
            answer.status
        );
    }
    drop(limited);

    let garbage = "not an archive".repeat(1 << 18);
    let (status, body) = s.do_(
        "POST",
        "/api/restore",
        Body::Sized(garbage.as_bytes()),
        "application/gzip",
    );
    if status != 400 {
        panic!("garbage: {status} {}", text(&body));
    }
    check("garbage leaves the data untouched");

    let (status, body) = s.do_("POST", "/api/restore", Body::Sized(b""), "application/gzip");
    if status != 400 || !text(&body).contains("must be a backup archive") {
        panic!("empty body: {status} {}", text(&body));
    }

    let mut no_file = Multipart::new();
    no_file.field("note", "no file");
    let content_type = no_file.content_type();
    let no_file = no_file.close();
    let (status, body) = s.do_("POST", "/api/restore", Body::Sized(&no_file), &content_type);
    if status != 400 || !text(&body).contains("holds no file") {
        panic!("multipart without a file: {status} {}", text(&body));
    }

    let mut conn = TcpStream::connect(&s.addr).unwrap_or_else(|e| panic!("{e}"));
    let _ = write!(
        conn,
        "POST /api/restore HTTP/1.1\r\nHost: x\r\nCookie: {}\r\nContent-Length: {}\r\nContent-Type: application/gzip\r\n\r\n",
        s.cookie_header(),
        archive.len()
    );
    let _ = conn.write_all(&archive[..archive.len() / 2]);
    drop(conn);
    std::thread::sleep(Duration::from_millis(300));
    check("an interrupted upload leaves the data untouched");

    let folder = globals::config()
        .map(|c| c.store.folder.clone())
        .unwrap_or_default();
    let left: Vec<String> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(".restore-upload-"))
                .collect()
        })
        .unwrap_or_default();
    if !left.is_empty() {
        panic!("uploads are removed: {left:?}");
    }

    let mut head = Head::new();
    head.set("Content-Type", "application/gzip");
    let request = client::go_request(
        "POST",
        &s.host,
        "/api/restore",
        &Body::Sized(&archive),
        &head,
    );
    let anonymous =
        TcpStream::connect(&s.addr).and_then(|stream| client::round_trip(stream, &request, "POST"));
    match anonymous {
        Err(err) => {
            if !matches!(
                err.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset
            ) {
                panic!("{err}");
            }
        }
        Ok(resp) => {
            if resp.status != 401 {
                panic!("anonymous restore: {}", resp.status);
            }
        }
    }
    check("a refused restore leaves the data untouched");
}

struct SlowReader {
    r: TcpStream,
    chunk: usize,
    pause: Duration,
}

impl Read for SlowReader {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        let n = p.len().min(self.chunk);
        std::thread::sleep(self.pause);

        self.r.read(&mut p[..n])
    }
}

fn set_read_buffer(conn: &TcpStream, bytes: libc::c_int) {
    let value = bytes;
    unsafe {
        libc::setsockopt(
            conn.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&value as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

#[test]
fn a_backup_can_take_longer_than_the_write_timeout() {
    let configure = |cfg: &mut Config| cfg.server.http.write_timeout_seconds = 1;
    let s = start_live_server(Some(&configure));
    s.login();
    fill_random(3000, 8000);

    let mut conn = TcpStream::connect(&s.addr).unwrap_or_else(|e| panic!("{e}"));
    set_read_buffer(&conn, 16 << 10);

    let _ = write!(
        conn,
        "GET /api/backup HTTP/1.1\r\nHost: x\r\nCookie: {}\r\nConnection: close\r\n\r\n",
        s.cookie_header()
    );

    let start = Instant::now();
    let slow = SlowReader {
        r: conn,
        chunk: 64 << 10,
        pause: Duration::from_millis(25),
    };
    let resp = client::read_response(&mut BufReader::with_capacity(64 << 10, slow), "GET")
        .unwrap_or_else(|err| panic!("after {:?}: {err}", start.elapsed()));

    let took = start.elapsed();
    if took < Duration::from_millis(1500) {
        panic!("the test should outlast the write timeout, took {took:?}");
    }

    if let Err(err) = backup::verify(&resp.body[..]) {
        panic!("{err}");
    }
}

#[test]
fn the_command_backs_up_a_running_server() {
    let s = start_live_server(None);
    fill_random(50, 100);

    let dir = TempDir::new();
    let folder = dir.folder();
    let _password = EnvGuard::set(cmd::REMOTE_PASSWORD_ENV, "admin");
    let path = cmd::run_backup(&[
        "--url",
        s.url.as_str(),
        "--user",
        "admin",
        "--output",
        folder.as_str(),
    ])
    .unwrap_or_else(|err| panic!("{err}"));

    let f = std::fs::File::open(&path).unwrap_or_else(|err| panic!("{err}"));

    let manifest = backup::verify(f).unwrap_or_else(|err| panic!("{err}"));

    let mut found = false;
    for entity in &manifest.entities {
        found = found || (entity.name.as_bytes() == b"big" && entity.documents == 50);
    }
    if !found {
        panic!("manifest {:?}", manifest.entities);
    }

    let left: Vec<String> = std::fs::read_dir(dir.path())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    if !left.is_empty() {
        panic!("temporary files are removed: {left:?}");
    }

    let missing = dir.path().join("missing").to_string_lossy().into_owned();
    if cmd::run_backup(&[
        "--url",
        s.url.as_str(),
        "--user",
        "admin",
        "--output",
        missing.as_str(),
    ])
    .is_ok()
    {
        panic!("a missing folder is refused");
    }

    let _wrong = EnvGuard::set(cmd::REMOTE_PASSWORD_ENV, "wrong");
    if cmd::run_backup(&[
        "--url",
        s.url.as_str(),
        "--user",
        "admin",
        "--output",
        folder.as_str(),
    ])
    .is_ok()
    {
        panic!("a wrong password is refused");
    }
}

#[test]
fn periodic_backups_keep_the_last_ones() {
    let _s = start_live_server(None);
    fill_random(10, 100);

    let dir = TempDir::new();
    for name in [
        "elysiandb-backup-20200101T000000Z.tar.gz",
        "elysiandb-backup-20200102T000000Z.tar.gz",
        "other.txt",
    ] {
        if let Err(err) = std::fs::write(dir.path().join(name), b"x") {
            panic!("{err}");
        }
    }

    backup::run_periodic(&dir.folder(), 2);

    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap_or_else(|err| panic!("{err}"))
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();

    if names.len() != 3
        || names[0] != "elysiandb-backup-20200102T000000Z.tar.gz"
        || names[2] != "other.txt"
    {
        panic!("kept {names:?}");
    }

    let st = backup::current_status();
    if !st.last_ok || st.documents < 10 {
        panic!("status {st:?}");
    }
}

struct FreeSpaceReset;

impl Drop for FreeSpaceReset {
    fn drop(&mut self) {
        let mut hooks = storage::hooks();
        hooks.free_space_probe = None;
        storage::set_hooks(hooks);
    }
}

#[test]
fn writes_are_refused_without_disk_and_come_back() {
    let free = Arc::new(AtomicI64::new(1 << 40));
    let probe = free.clone();
    let mut hooks = storage::hooks();
    hooks.free_space_probe = Some(Arc::new(move || Ok(probe.load(Ordering::SeqCst))));
    storage::set_hooks(hooks);
    let _reset = FreeSpaceReset;

    let s = start_live_server(None);
    s.login();

    let (status, body) = s.do_(
        "POST",
        "/api/full",
        Body::Sized(br#"{"id":"before"}"#),
        "application/json",
    );
    if status != 200 {
        panic!("write: {status} {}", text(&body));
    }

    free.store(1, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(5);
    while storage::write_refusal().is_none() {
        if Instant::now() > deadline {
            panic!("the low disk is not noticed");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let (status, body) = s.do_(
        "POST",
        "/api/full",
        Body::Sized(br#"{"id":"refused"}"#),
        "application/json",
    );
    if status != 507 || !text(&body).contains("disk full") {
        panic!("expected a refusal, got {status} {}", text(&body));
    }

    let (status, _) = s.do_("PUT", "/kv/refused", Body::Sized(b"v"), "");
    if status != 507 {
        panic!("key write: {status}");
    }

    let (status, body) = s.do_("GET", "/api/full/before", Body::None, "");
    if status != 200 || !text(&body).contains("before") {
        panic!("reads are served: {status} {}", text(&body));
    }

    let (status, body) = s.do_(
        "POST",
        "/api/query",
        Body::Sized(br#"{"entity":"full","filters":{}}"#),
        "application/json",
    );
    if status == 507 {
        panic!("queries are served: {status} {}", text(&body));
    }

    let (status, _) = s.do_("GET", "/api/full/refused", Body::None, "");
    if status != 404 {
        panic!("a refused write leaves nothing: {status}");
    }

    if storage::refused_writes() < 2 {
        panic!("refusals are counted: {}", storage::refused_writes());
    }

    let (status, _) = s.do_("DELETE", "/api/full/before", Body::None, "");
    if status >= 500 {
        panic!("deleting is how space comes back: {status}");
    }

    free.store(1 << 40, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(5);
    while storage::write_refusal().is_some() {
        if Instant::now() > deadline {
            panic!("writes never resume");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let (status, body) = s.do_(
        "POST",
        "/api/full",
        Body::Sized(br#"{"id":"after"}"#),
        "application/json",
    );
    if status != 200 {
        panic!("writes resume: {status} {}", text(&body));
    }
}

struct StartingReset;

impl Drop for StartingReset {
    fn drop(&mut self) {
        globals::set_starting(false);
    }
}

#[test]
fn the_health_route_answers_once_the_server_can_serve() {
    let s = start_live_server(None);

    globals::set_starting(true);
    let _reset = StartingReset;

    let (status, _) = s.do_("GET", "/health", Body::None, "");
    if status != 503 {
        panic!("while starting: {status}");
    }

    globals::set_starting(false);
    let (status, _) = s.do_("GET", "/health", Body::None, "");
    if status != 200 {
        panic!("once ready: {status}");
    }
}
