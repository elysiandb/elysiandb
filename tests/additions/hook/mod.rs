mod bench;
mod chain;
mod corpus;
mod deep;

use std::sync::Arc;

use elysiandb::api;
use elysiandb::configuration::{
    AdminUiConfig, ApiCacheConfig, ApiConfig, ApiIndexConfig, ApiSchemaConfig,
    AuthenticationConfig, Config, CrashRecoveryConfig, HooksConfig, SecurityConfig, StatsConfig,
    StoreConfig,
};
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::hook;
use elysiandb::storage;
use elysiandb::sync::MutexGuard;
use talos::host::{DocValue, MapRef};

use crate::tempdir::TempDir;

pub struct Setup {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn config(folder: &str, hooks_enabled: bool) -> Config {
    Config {
        store: StoreConfig {
            folder: folder.to_string(),
            shards: 2,
            crash_recovery: CrashRecoveryConfig {
                enabled: false,
                max_log_mb: 1,
            },
            ..StoreConfig::default()
        },
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                enabled: false,
                mode: "basic".to_string(),
                token: String::new(),
            },
        },
        stats: StatsConfig { enabled: false },
        api: ApiConfig {
            schema: ApiSchemaConfig {
                enabled: false,
                strict: false,
            },
            cache: ApiCacheConfig {
                enabled: false,
                cleanup_interval_seconds: 0,
                ..ApiCacheConfig::default()
            },
            index: ApiIndexConfig { workers: 0 },
            hooks: HooksConfig {
                enabled: hooks_enabled,
                ..HooksConfig::default()
            },
            ..ApiConfig::default()
        },
        admin_ui: AdminUiConfig { enabled: false },
        ..Config::default()
    }
}

pub fn setup(hooks_enabled: bool) -> Setup {
    setup_with(|cfg| cfg.api.hooks.enabled = hooks_enabled)
}

pub fn setup_with(f: impl FnOnce(&mut Config)) -> Setup {
    let serial = crate::serial();
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = config(&dir.path().to_string_lossy(), true);
    f(&mut cfg);
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();

    Setup {
        _dir: dir,
        _serial: serial,
    }
}

fn value(text: &str) -> Value {
    json::unmarshal_value(text.as_bytes())
        .unwrap_or_else(|e| panic!("invalid test JSON {text}: {e}"))
}

fn doc(text: &str) -> Map {
    match &mut value(text) {
        Value::Object(m) => std::mem::take(m),
        _ => panic!("not an object: {text}"),
    }
}

pub fn entity(text: &str) -> MapRef {
    hook::document_from_json(&doc(text))
}

fn sprint(v: &DocValue) -> String {
    talos::host::json::format_v(v)
}

pub fn fmt_field(e: &MapRef, key: &str) -> String {
    match e.borrow().get(key) {
        Some(v) => sprint(v),
        None => "<nil>".to_string(),
    }
}

pub fn rng(seed: i64) -> Rand {
    Rand::new(rand::new_source(seed))
}

pub fn obj(items: Vec<(&str, Value)>) -> Map {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

pub fn write(entity: &str, mut d: Map) {
    api::write_entity(entity.as_bytes(), &mut d);
}

pub const BUDGET_SCRIPT: &str = r#"function monthsBetween(from, to) {
  var a = from.split("-"), b = to.split("-")
  return (parseInt(b[0], 10) - parseInt(a[0], 10)) * 12 + parseInt(b[1], 10) - parseInt(a[1], 10)
}
function postRead(ctx) {
  var month = ctx.entity.id
  var purchases = ctx.query("purchase", { status: { neq: "annule" } })
  var total = 0, count = 0
  for (var i = 0; i < purchases.length; i++) {
    var p = purchases[i]
    if (!p.budget_month || p.status === "wishlist") { continue }
    var n = p.installments || 1
    if (n > 1) {
      var k = monthsBetween(p.budget_month, month)
      if (k < 0 || k >= n) { continue }
      total += p.installment_amount || 0
    } else {
      if (p.budget_month !== month) { continue }
      total += p.price || 0
    }
    count++
  }
  total += ctx.entity.spent_manual_adjust || 0
  ctx.entity.spent = Math.round(total * 100) / 100
  ctx.entity.remaining = Math.round(((ctx.entity.cap || 0) - total) * 100) / 100
  ctx.entity.purchase_count = count
  ctx.entity.raw_total = total
}"#;

fn month_id(i: i64) -> String {
    format!("20{:02}-{:02}", 20 + i / 12, 1 + i % 12)
}

pub fn random_purchase(r: &mut Rand, id: &str) -> Map {
    let mut doc = obj(vec![
        ("id", s(id)),
        ("budget_month", s(&month_id(r.intn(36)))),
    ]);
    match r.intn(6) {
        0 => doc.insert("status".to_string(), s("annule")),
        1 => doc.insert("status".to_string(), s("wishlist")),
        _ => doc.insert("status".to_string(), s("achete")),
    };

    doc.insert(
        "price".to_string(),
        Value::Number(r.intn(100000) as f64 / 100.0),
    );
    if r.intn(4) == 0 {
        doc.insert(
            "installments".to_string(),
            Value::Number((2 + r.intn(10)) as f64),
        );
        doc.insert(
            "installment_amount".to_string(),
            Value::Number(r.intn(10000) as f64 / 100.0),
        );
    }

    match r.intn(40) {
        0 => {
            doc.remove("price");
        }
        1 => {
            doc.insert("budget_month".to_string(), s(""));
        }
        2 => {
            doc.remove("status");
        }
        3 => {
            doc.insert("budget_month".to_string(), Value::Null);
        }
        _ => {}
    }

    doc
}
