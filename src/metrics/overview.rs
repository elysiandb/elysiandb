use std::collections::BTreeMap;

use crate::globals;
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::sort;
use crate::gocompat::time::{Duration, Time};
use crate::log;

use super::counters::{
    C_HTTP, C_HTTP5XX, C_SLOW, C_TCP, HookSummary, PersistenceStatus, RouteSummary,
    encode_optional_time, encode_time, hook_summaries, persistence, snapshot_routes, started_at,
    top_routes,
};
use super::sampler::{self, Moment};
use super::series::{Number, Point, Series, derive, num};
use super::status::{Status, counter_delta, current_status};

pub const RUST_VERSION: &str = env!("ELYSIANDB_RUST_VERSION");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listener {
    pub enabled: bool,
    pub address: String,
}

impl Encode for Listener {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "enabled");
        e.write_bool(self.enabled);
        e.write_field_name(&mut first, "address");
        e.write_string(self.address.as_bytes());
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub version: String,
    pub rust_version: String,
    pub engine: String,
    pub started_at: Time,
    pub uptime_seconds: i64,
    pub http: Listener,
    pub tcp: Listener,
    pub auth_mode: String,
    pub features: BTreeMap<String, bool>,
    pub data_folder: String,
}

impl Encode for Instance {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "version");
        e.write_string(self.version.as_bytes());
        e.write_field_name(&mut first, "rust_version");
        e.write_string(self.rust_version.as_bytes());
        e.write_field_name(&mut first, "engine");
        e.write_string(self.engine.as_bytes());
        e.write_field_name(&mut first, "started_at");
        encode_time(&self.started_at, e)?;
        e.write_field_name(&mut first, "uptime_seconds");
        e.write_i64(self.uptime_seconds);
        e.write_field_name(&mut first, "http");
        self.http.encode(e)?;
        e.write_field_name(&mut first, "tcp");
        self.tcp.encode(e)?;
        e.write_field_name(&mut first, "auth_mode");
        e.write_string(self.auth_mode.as_bytes());
        e.write_field_name(&mut first, "features");
        self.features.encode(e)?;
        e.write_field_name(&mut first, "data_folder");
        e.write_string(self.data_folder.as_bytes());
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Traffic {
    pub requests_per_second: Option<Number>,
    pub http_requests_per_second: Option<Number>,
    pub tcp_commands_per_second: Option<Number>,
    pub error_rate: Option<Number>,
    pub p95_ms: Option<Number>,
    pub slow_requests_per_minute: Option<Number>,
}

impl Encode for Traffic {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "requests_per_second");
        self.requests_per_second.encode(e)?;
        e.write_field_name(&mut first, "http_requests_per_second");
        self.http_requests_per_second.encode(e)?;
        e.write_field_name(&mut first, "tcp_commands_per_second");
        self.tcp_commands_per_second.encode(e)?;
        e.write_field_name(&mut first, "error_rate");
        self.error_rate.encode(e)?;
        e.write_field_name(&mut first, "p95_ms");
        self.p95_ms.encode(e)?;
        e.write_field_name(&mut first, "slow_requests_per_minute");
        self.slow_requests_per_minute.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityCount {
    pub name: Vec<u8>,
    pub count: i64,
}

impl Encode for EntityCount {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "name");
        e.write_string(&self.name);
        e.write_field_name(&mut first, "count");
        e.write_i64(self.count);
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub kv_keys: Option<Number>,
    pub kv_keys_with_ttl: Option<Number>,
    pub entity_types: i64,
    pub entities: i64,
    pub disk_bytes: Option<Number>,
    pub top_entity_types: Vec<EntityCount>,
}

impl Encode for Data {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "kv_keys");
        self.kv_keys.encode(e)?;
        e.write_field_name(&mut first, "kv_keys_with_ttl");
        self.kv_keys_with_ttl.encode(e)?;
        e.write_field_name(&mut first, "entity_types");
        e.write_i64(self.entity_types);
        e.write_field_name(&mut first, "entities");
        e.write_i64(self.entities);
        e.write_field_name(&mut first, "disk_bytes");
        self.disk_bytes.encode(e)?;
        e.write_field_name(&mut first, "top_entity_types");
        self.top_entity_types.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeInfo {
    pub allocated_bytes: Option<Number>,
    pub total_bytes: Option<Number>,
    pub threads: Option<Number>,
    pub cpu_percent: Option<Number>,
}

impl Encode for RuntimeInfo {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "allocated_bytes");
        self.allocated_bytes.encode(e)?;
        e.write_field_name(&mut first, "total_bytes");
        self.total_bytes.encode(e)?;
        e.write_field_name(&mut first, "threads");
        self.threads.encode(e)?;
        e.write_field_name(&mut first, "cpu_percent");
        self.cpu_percent.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Connections {
    pub http: Option<Number>,
    pub tcp: Option<Number>,
    pub admin_sessions: Option<Number>,
    pub open_transactions: Option<Number>,
    pub oldest_transaction_seconds: Option<Number>,
}

impl Encode for Connections {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "http");
        self.http.encode(e)?;
        e.write_field_name(&mut first, "tcp");
        self.tcp.encode(e)?;
        e.write_field_name(&mut first, "admin_sessions");
        self.admin_sessions.encode(e)?;
        e.write_field_name(&mut first, "open_transactions");
        self.open_transactions.encode(e)?;
        e.write_field_name(&mut first, "oldest_transaction_seconds");
        self.oldest_transaction_seconds.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogSummary {
    pub level: String,
    pub errors_1h: i64,
    pub warnings_1h: i64,
    pub errors_24h: i64,
    pub warnings_24h: i64,
}

impl Encode for LogSummary {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "level");
        e.write_string(self.level.as_bytes());
        e.write_field_name(&mut first, "errors_1h");
        e.write_i64(self.errors_1h);
        e.write_field_name(&mut first, "warnings_1h");
        e.write_i64(self.warnings_1h);
        e.write_field_name(&mut first, "errors_24h");
        e.write_i64(self.errors_24h);
        e.write_field_name(&mut first, "warnings_24h");
        e.write_i64(self.warnings_24h);
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Overview {
    pub generated_at: Time,
    pub instance: Instance,
    pub status: Status,
    pub traffic: Traffic,
    pub data: Data,
    pub persistence: PersistenceStatus,
    pub runtime: RuntimeInfo,
    pub connections: Connections,
    pub logs: LogSummary,
}

impl Encode for Overview {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "generated_at");
        encode_time(&self.generated_at, e)?;
        e.write_field_name(&mut first, "instance");
        self.instance.encode(e)?;
        e.write_field_name(&mut first, "status");
        self.status.encode(e)?;
        e.write_field_name(&mut first, "traffic");
        self.traffic.encode(e)?;
        e.write_field_name(&mut first, "data");
        self.data.encode(e)?;
        e.write_field_name(&mut first, "persistence");
        self.persistence.encode(e)?;
        e.write_field_name(&mut first, "runtime");
        self.runtime.encode(e)?;
        e.write_field_name(&mut first, "connections");
        self.connections.encode(e)?;
        e.write_field_name(&mut first, "logs");
        self.logs.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

pub fn auth_mode() -> String {
    match globals::config() {
        Some(cfg) if cfg.security.authentication.enabled => {
            cfg.security.authentication.mode.clone()
        }
        _ => "disabled".to_string(),
    }
}

pub fn current_instance() -> Instance {
    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };
    let now = Moment::now();
    let started = started_at();

    let features = [
        ("cache", cfg.api.cache.enabled),
        ("hooks", cfg.api.hooks.enabled),
        ("schema", cfg.api.schema.enabled),
        ("crash_recovery", cfg.store.crash_recovery.enabled),
        ("admin_ui", cfg.admin_ui.enabled),
        ("stats", cfg.stats.enabled),
        ("prometheus", cfg.monitoring.prometheus.enabled),
        ("access_log", cfg.log.access_log),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    Instance {
        version: globals::version(),
        rust_version: RUST_VERSION.to_string(),
        engine: globals::engine(),
        started_at: started.wall.clone(),
        uptime_seconds: now.sub(&started).seconds() as i64,
        auth_mode: auth_mode(),
        data_folder: cfg.store.folder.clone(),
        http: Listener {
            enabled: cfg.server.http.enabled,
            address: format!("{}:{}", cfg.server.http.host, cfg.server.http.port),
        },
        tcp: Listener {
            enabled: cfg.server.tcp.enabled,
            address: format!("{}:{}", cfg.server.tcp.host, cfg.server.tcp.port),
        },
        features,
    }
}

pub fn current_overview() -> Overview {
    let now = Time::now();
    let state = sampler::overview_state();

    let mut overview = Overview {
        generated_at: now.clone(),
        instance: current_instance(),
        status: current_status(),
        traffic: Traffic::default(),
        data: Data::default(),
        persistence: persistence(),
        runtime: RuntimeInfo::default(),
        connections: Connections::default(),
        logs: LogSummary {
            level: log::current_level().level.to_string(),
            ..LogSummary::default()
        },
    };

    if let Some((prev, cur)) = &state.traffic {
        let elapsed = cur.at.sub(&prev.at).seconds();
        let http_count = counter_delta(prev, cur, C_HTTP);
        let tcp_count = counter_delta(prev, cur, C_TCP);

        overview.traffic = Traffic {
            requests_per_second: num((http_count + tcp_count) / elapsed),
            http_requests_per_second: num(http_count / elapsed),
            tcp_commands_per_second: num(tcp_count / elapsed),
            error_rate: None,
            p95_ms: num(((cur.http - prev.http) + (cur.tcp - prev.tcp)).quantile(0.95)),
            slow_requests_per_minute: num(counter_delta(prev, cur, C_SLOW) / elapsed * 60.0),
        };

        if http_count > 0.0 {
            overview.traffic.error_rate = num(counter_delta(prev, cur, C_HTTP5XX) / http_count);
        }
    }

    if let Some(latest) = &state.latest {
        overview.data.kv_keys = num(latest.gauge_or_zero(Series::KvKeys));
        overview.data.kv_keys_with_ttl = num(latest.gauge_or_zero(Series::KvKeysTtl));
        overview.data.disk_bytes = num(latest.gauge_or_zero(Series::DiskBytes));
        overview.runtime = RuntimeInfo {
            allocated_bytes: num(latest.gauge_or_zero(Series::AllocatedBytes)),
            total_bytes: num(latest.gauge_or_zero(Series::TotalBytes)),
            threads: num(latest.gauge_or_zero(Series::Threads)),
            cpu_percent: None,
        };
        overview.connections = Connections {
            http: num(latest.gauge_or_zero(Series::HttpConns)),
            tcp: num(latest.gauge_or_zero(Series::TcpConns)),
            admin_sessions: num(latest.gauge_or_zero(Series::AdminSessions)),
            open_transactions: num(latest.gauge_or_zero(Series::OpenTransactions)),
            oldest_transaction_seconds: num(latest.gauge_or_zero(Series::OldestTransaction)),
        };
    }

    if let Some((prev, cur)) = &state.cpu {
        overview.runtime.cpu_percent = num(derive(prev, cur).get(Series::CpuPercent));
    }

    let mut types = Vec::with_capacity(state.slow.entity_counts.len());
    for (name, count) in state.slow.entity_counts.iter() {
        types.push(EntityCount {
            name: name.clone(),
            count: *count,
        });
        overview.data.entities = overview.data.entities.wrapping_add(*count);
    }
    overview.data.entity_types = types.len() as i64;

    sort_entity_counts(&mut types);
    types.truncate(5);
    overview.data.top_entity_types = types;

    let (errors, warnings) = sum_logs(&state.ten_seconds, &now.add(-Duration::HOUR), 10.0);
    overview.logs.errors_1h = errors;
    overview.logs.warnings_1h = warnings;
    let (errors, warnings) = sum_logs(&state.minutes, &now.add(Duration::HOUR * -24), 60.0);
    overview.logs.errors_24h = errors;
    overview.logs.warnings_24h = warnings;

    overview
}

fn sort_entity_counts(types: &mut [EntityCount]) {
    sort::slice(types, |a, b| {
        if a.count != b.count {
            return a.count > b.count;
        }
        a.name < b.name
    });
}

fn sum_logs(points: &[Point], since: &Time, step_seconds: f64) -> (i64, i64) {
    let mut errors_count = 0.0f64;
    let mut warnings_count = 0.0f64;
    let limit = since.unix_milli();
    for p in points {
        if p.t < limit {
            continue;
        }
        let v = p.get(Series::LogErrorsRps);
        if !v.is_nan() {
            errors_count += v * step_seconds;
        }
        let v = p.get(Series::LogWarningsRps);
        if !v.is_nan() {
            warnings_count += v * step_seconds;
        }
    }

    (errors_count.round() as i64, warnings_count.round() as i64)
}

#[derive(Clone, Debug, Default)]
pub struct Routes {
    pub since_startup: Vec<RouteSummary>,
    pub last_15m: Vec<RouteSummary>,
}

impl Encode for Routes {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "since_startup");
        self.since_startup.encode(e)?;
        e.write_field_name(&mut first, "last_15m");
        self.last_15m.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Details {
    pub routes: Routes,
    pub hooks: Vec<HookSummary>,
    pub entity_types: Vec<EntityCount>,
    pub indexes: Vec<IndexState>,
}

impl Encode for Details {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "routes");
        self.routes.encode(e)?;
        e.write_field_name(&mut first, "hooks");
        self.hooks.encode(e)?;
        e.write_field_name(&mut first, "entity_types");
        self.entity_types.encode(e)?;
        e.write_field_name(&mut first, "indexes");
        self.indexes.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct IndexState {
    pub entity: Vec<u8>,
    pub index: Vec<u8>,
    pub state: String,
    pub entries: i64,
    pub last_used_at: Option<Time>,
    pub kind: String,
    pub origin: String,
    pub pinned: bool,
    pub bytes: i64,
    pub uses: i64,
}

impl Encode for IndexState {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "entity");
        e.write_string(&self.entity);
        e.write_field_name(&mut first, "index");
        e.write_string(&self.index);
        e.write_field_name(&mut first, "state");
        e.write_string(self.state.as_bytes());
        e.write_field_name(&mut first, "entries");
        e.write_i64(self.entries);
        e.write_field_name(&mut first, "last_used_at");
        encode_optional_time(&self.last_used_at, e)?;
        if !self.kind.is_empty() {
            e.write_field_name(&mut first, "kind");
            e.write_string(self.kind.as_bytes());
        }
        if !self.origin.is_empty() {
            e.write_field_name(&mut first, "origin");
            e.write_string(self.origin.as_bytes());
        }
        if self.pinned {
            e.write_field_name(&mut first, "pinned");
            e.write_bool(self.pinned);
        }
        if self.bytes != 0 {
            e.write_field_name(&mut first, "bytes");
            e.write_i64(self.bytes);
        }
        if self.uses != 0 {
            e.write_field_name(&mut first, "uses");
            e.write_i64(self.uses);
        }
        e.end_struct(first);
        Ok(())
    }
}

pub fn current_details() -> Details {
    let current = snapshot_routes();
    let state = sampler::details_state();
    let empty = BTreeMap::new();

    let mut details = Details {
        routes: Routes {
            since_startup: top_routes(&current, &empty, 10),
            last_15m: top_routes(&current, state.baseline.as_ref().unwrap_or(&empty), 10),
        },
        hooks: hook_summaries(),
        entity_types: Vec::new(),
        indexes: Vec::new(),
    };

    if let Some(indexes) = &state.indexes {
        details.indexes = indexes();
    }

    for (name, count) in state.counts.iter() {
        details.entity_types.push(EntityCount {
            name: name.clone(),
            count: *count,
        });
    }

    sort_entity_counts(&mut details.entity_types);

    details
}
