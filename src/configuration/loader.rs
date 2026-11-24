use crate::config_struct;
use crate::durable::FsError;
use crate::gocompat::json::{self, Encode, EncodeState, quote_text};
use crate::gocompat::strings::{to_lower, trim_space};
use crate::log::{self, Attr, ToValue, Value};
use crate::yaml::{self, StringList};
use std::fmt;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

config_struct! {
pub struct SecurityConfig {
    authentication: AuthenticationConfig = "authentication",
}
pub struct AuthenticationConfig {
    enabled: bool = "enabled",
    mode: String = "mode",
    token: String = "token",
}
pub struct AdminUiConfig {
    enabled: bool = "enabled",
}
pub struct ServersConfig {
    http: ServerConfig = "http",
    tcp: ServerConfig = "tcp",
}
pub struct LogConfig {
    level: String = "level",
    format: String = "format",
    access_log: bool = "accessLog",
    slow_request_ms: i64 = "slowRequestMs",
    buffer_lines: i64 = "bufferLines",
    flush_interval_seconds: i64 = "flushIntervalSeconds",
}
pub struct PrometheusConfig {
    enabled: bool = "enabled",
    token: String = "token",
}
pub struct MonitoringConfig {
    prometheus: PrometheusConfig = "prometheus",
}
pub struct ServerConfig {
    enabled: bool = "enabled",
    host: String = "host",
    port: i64 = "port",
    cors: CorsConfig = "cors",
    trusted_proxies: StringList = "trustedProxies",
    read_timeout_seconds: i64 = "readTimeoutSeconds",
    write_timeout_seconds: i64 = "writeTimeoutSeconds",
}
pub struct CorsConfig {
    allowed_origins: StringList = "allowedOrigins",
}
pub struct CrashRecoveryConfig {
    enabled: bool = "enabled",
    max_log_mb: i64 = "maxLogMB",
}
pub struct StoreConfig {
    folder: String = "folder",
    shards: i64 = "shards",
    flush_interval_seconds: i64 = "flushIntervalSeconds",
    crash_recovery: CrashRecoveryConfig = "crashRecovery",
    durability: String = "durability",
    memory: StoreMemoryConfig = "memory",
    min_free_disk_mb: i64 = "minFreeDiskMB",
    backup: StoreBackupConfig = "backup",
}
pub struct StoreBackupConfig {
    folder: String = "folder",
    every_minutes: i64 = "everyMinutes",
    keep: i64 = "keep",
}
pub struct StoreMemoryConfig {
    limit_mb: i64 = "limitMB",
}
pub struct StatsConfig {
    enabled: bool = "enabled",
}
pub struct ApiSchemaConfig {
    enabled: bool = "enabled",
    strict: bool = "strict",
}
pub struct ApiConfig {
    index: ApiIndexConfig = "index",
    cache: ApiCacheConfig = "cache",
    schema: ApiSchemaConfig = "schema",
    hooks: HooksConfig = "hooks",
    query: ApiQueryConfig = "query",
    response: ApiResponseConfig = "response",
}
pub struct ApiQueryConfig {
    workers: i64 = "workers",
    timeout_ms: i64 = "timeoutMs",
    max_queued: i64 = "maxQueued",
    queue_timeout_ms: i64 = "queueTimeoutMs",
    cancel_on_disconnect: Option<bool> = "cancelOnDisconnect",
}
pub struct ApiResponseConfig {
    spool_mb: i64 = "spoolMB",
}
pub struct HooksConfig {
    enabled: bool = "enabled",
    timeout_ms: i64 = "timeoutMs",
    request_budget_ms: i64 = "requestBudgetMs",
}
pub struct ApiIndexConfig {
    workers: i64 = "workers",
}
pub struct ApiCacheConfig {
    enabled: bool = "enabled",
    cleanup_interval_seconds: i64 = "cleanupIntervalSeconds",
    max_mb: i64 = "maxMB",
    max_entry_kb: i64 = "maxEntryKB",
}
pub struct EngineConfig {
    name: String = "name",
}
pub struct Config {
    store: StoreConfig = "store",
    server: ServersConfig = "server",
    log: LogConfig = "log",
    security: SecurityConfig = "security",
    stats: StatsConfig = "stats",
    api: ApiConfig = "api",
    admin_ui: AdminUiConfig = "adminui",
    engine: EngineConfig = "engine",
    monitoring: MonitoringConfig = "monitoring",
}
}

macro_rules! go_field_names {
    ($($name:ident { $($field:ident => $go:literal,)* })*) => {
        $(
            impl Encode for $name {
                fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
                    let mut first = true;
                    $(
                        e.write_field_name(&mut first, $go);
                        Encode::encode(&self.$field, e)?;
                    )*
                    e.end_struct(first);
                    Ok(())
                }
            }
        )*
    };
}

go_field_names! {
    SecurityConfig {
        authentication => "Authentication",
    }
    AuthenticationConfig {
        enabled => "Enabled",
        mode => "Mode",
        token => "Token",
    }
    AdminUiConfig {
        enabled => "Enabled",
    }
    ServersConfig {
        http => "HTTP",
        tcp => "TCP",
    }
    LogConfig {
        level => "Level",
        format => "Format",
        access_log => "AccessLog",
        slow_request_ms => "SlowRequestMs",
        buffer_lines => "BufferLines",
        flush_interval_seconds => "FlushIntervalSeconds",
    }
    PrometheusConfig {
        enabled => "Enabled",
        token => "Token",
    }
    MonitoringConfig {
        prometheus => "Prometheus",
    }
    ServerConfig {
        enabled => "Enabled",
        host => "Host",
        port => "Port",
        cors => "CORS",
        trusted_proxies => "TrustedProxies",
        read_timeout_seconds => "ReadTimeoutSeconds",
        write_timeout_seconds => "WriteTimeoutSeconds",
    }
    CorsConfig {
        allowed_origins => "AllowedOrigins",
    }
    CrashRecoveryConfig {
        enabled => "Enabled",
        max_log_mb => "MaxLogMB",
    }
    StoreConfig {
        folder => "Folder",
        shards => "Shards",
        flush_interval_seconds => "FlushIntervalSeconds",
        crash_recovery => "CrashRecovery",
        durability => "Durability",
        memory => "Memory",
        min_free_disk_mb => "MinFreeDiskMB",
        backup => "Backup",
    }
    StoreBackupConfig {
        folder => "Folder",
        every_minutes => "EveryMinutes",
        keep => "Keep",
    }
    StoreMemoryConfig {
        limit_mb => "LimitMB",
    }
    StatsConfig {
        enabled => "Enabled",
    }
    ApiSchemaConfig {
        enabled => "Enabled",
        strict => "Strict",
    }
    ApiConfig {
        index => "Index",
        cache => "Cache",
        schema => "Schema",
        hooks => "Hooks",
        query => "Query",
        response => "Response",
    }
    ApiQueryConfig {
        workers => "Workers",
        timeout_ms => "TimeoutMs",
        max_queued => "MaxQueued",
        queue_timeout_ms => "QueueTimeoutMs",
        cancel_on_disconnect => "CancelOnDisconnect",
    }
    ApiResponseConfig {
        spool_mb => "SpoolMB",
    }
    HooksConfig {
        enabled => "Enabled",
        timeout_ms => "TimeoutMs",
        request_budget_ms => "RequestBudgetMs",
    }
    ApiIndexConfig {
        workers => "Workers",
    }
    ApiCacheConfig {
        enabled => "Enabled",
        cleanup_interval_seconds => "CleanupIntervalSeconds",
        max_mb => "MaxMB",
        max_entry_kb => "MaxEntryKB",
    }
    EngineConfig {
        name => "Name",
    }
    Config {
        store => "Store",
        server => "Server",
        log => "Log",
        security => "Security",
        stats => "Stats",
        api => "Api",
        admin_ui => "AdminUI",
        engine => "Engine",
        monitoring => "Monitoring",
    }
}

impl ApiQueryConfig {
    pub fn cancels_abandoned_requests(&self) -> bool {
        self.cancel_on_disconnect.unwrap_or(true)
    }
}

pub const DEFAULT_SLOW_REQUEST_MS: i64 = 500;
pub const DEFAULT_BUFFER_LINES: i64 = 5000;

pub fn default_config() -> Config {
    let mut c = Config::default();
    c.log.level = "info".to_string();
    c.log.format = "text".to_string();
    c.log.slow_request_ms = DEFAULT_SLOW_REQUEST_MS;
    c.log.buffer_lines = DEFAULT_BUFFER_LINES;
    c
}

pub const REDACTED_VALUE: &str = "***";

impl Config {
    pub fn to_json(&self) -> String {
        let mut copy = self.clone();
        if !copy.security.authentication.token.is_empty() {
            copy.security.authentication.token = REDACTED_VALUE.to_string();
        }

        if !copy.monitoring.prometheus.token.is_empty() {
            copy.monitoring.prometheus.token = REDACTED_VALUE.to_string();
        }

        match json::marshal(&copy) {
            Ok(data) => String::from_utf8_lossy(&data).into_owned(),
            Err(_) => "{}".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Read(FsError),
    Parse(yaml::Error),
    Invalid(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "cannot read the configuration file: {e}"),
            LoadError::Parse(e) => {
                write!(f, "cannot parse the configuration file: {}", e.detail())
            }
            LoadError::Invalid(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Read(e) => Some(e),
            LoadError::Parse(e) => Some(e),
            LoadError::Invalid(_) => None,
        }
    }
}

impl ToValue for LoadError {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, FsError> {
    let mut f = std::fs::File::open(path).map_err(|e| FsError::new("open", path, e))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| FsError::new("read", path, e))?;
    Ok(data)
}

pub fn load_config(path: &Path) -> Result<Config, LoadError> {
    let data = read_file(path).map_err(LoadError::Read)?;

    let mut cfg = default_config();
    yaml::unmarshal(&data, &mut cfg).map_err(LoadError::Parse)?;

    if let Err(err) = yaml::unmarshal_strict(&data, &mut Config::default()) {
        warn_ignored_keys(path, &err);
    }

    validate(&mut cfg)?;

    Ok(cfg)
}

fn warn_ignored_keys(path: &Path, err: &yaml::Error) {
    let keys = err.unknown_keys().join(",");
    let duplicates = err.duplicate_keys().join(",");
    let mut attrs: Vec<Attr<'_>> = vec![
        ("component", Value::Str(b"boot")),
        ("path", Value::Str(path.as_os_str().as_bytes())),
    ];
    if !keys.is_empty() {
        attrs.push(("keys", Value::Str(keys.as_bytes())));
    }
    if !duplicates.is_empty() {
        attrs.push(("duplicate_keys", Value::Str(duplicates.as_bytes())));
    }
    log::warn("unknown configuration keys are ignored", &attrs);
}

fn normalized(s: &str) -> Vec<u8> {
    to_lower(trim_space(s.as_bytes()))
}

fn invalid(msg: String) -> Result<(), LoadError> {
    Err(LoadError::Invalid(msg))
}

pub fn validate(cfg: &mut Config) -> Result<(), LoadError> {
    if let Err(err) = log::parse_level(&cfg.log.level) {
        return invalid(err.message().to_string());
    }

    match normalized(&cfg.log.format).as_slice() {
        b"" | b"text" | b"json" => {}
        _ => {
            return invalid(format!(
                "unknown log format {}: use text or json",
                quote_text(cfg.log.format.as_bytes())
            ));
        }
    }

    if cfg.log.slow_request_ms < 0 {
        return invalid("log.slowRequestMs must be 0 or more".to_string());
    }

    if cfg.log.buffer_lines < 0 {
        return invalid("log.bufferLines must be 0 or more".to_string());
    }

    match cfg.engine.name.as_str() {
        "" | "internal" => {}
        _ => {
            return invalid(format!(
                "unknown engine.name {}: use internal",
                quote_text(cfg.engine.name.as_bytes())
            ));
        }
    }

    match normalized(&cfg.store.durability).as_slice() {
        b"" | b"default" | b"fsync" => {}
        _ => {
            return invalid(format!(
                "unknown store.durability {}: use default or fsync",
                quote_text(cfg.store.durability.as_bytes())
            ));
        }
    }

    for (name, v) in [
        ("api.cache.maxEntryKB", cfg.api.cache.max_entry_kb),
        ("api.cache.maxMB", cfg.api.cache.max_mb),
        ("api.hooks.requestBudgetMs", cfg.api.hooks.request_budget_ms),
        ("api.hooks.timeoutMs", cfg.api.hooks.timeout_ms),
        ("api.query.timeoutMs", cfg.api.query.timeout_ms),
        ("api.query.workers", cfg.api.query.workers),
        ("api.response.spoolMB", cfg.api.response.spool_mb),
        (
            "server.http.readTimeoutSeconds",
            cfg.server.http.read_timeout_seconds,
        ),
        (
            "server.http.writeTimeoutSeconds",
            cfg.server.http.write_timeout_seconds,
        ),
    ] {
        if v < 0 {
            return invalid(format!("{name} must be 0 or more"));
        }
    }

    if cfg.store.memory.limit_mb < 0 {
        return invalid("store.memory.limitMB must be 0 or more".to_string());
    }

    for (name, v) in [
        ("api.query.maxQueued", cfg.api.query.max_queued),
        ("api.query.queueTimeoutMs", cfg.api.query.queue_timeout_ms),
    ] {
        if v < -1 {
            return invalid(format!("{name} must be -1 (no limit), 0 (default) or more"));
        }
    }

    let auth = &mut cfg.security.authentication;
    if auth.enabled {
        auth.mode = String::from_utf8_lossy(&normalized(&auth.mode)).into_owned();

        match auth.mode.as_str() {
            "basic" | "user" => {}
            "token" => {
                if auth.token.is_empty() {
                    return invalid(
                        "token authentication is enabled but no token is provided in the configuration"
                            .to_string(),
                    );
                }
            }
            _ => {
                return invalid(format!(
                    "authentication is enabled with an unknown mode {}: use basic, token or user",
                    quote_text(auth.mode.as_bytes())
                ));
            }
        }
    }

    Ok(())
}
