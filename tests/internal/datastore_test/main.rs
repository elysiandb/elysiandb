#[path = "../tempdir.rs"]
mod tempdir;

mod build_test;
mod checkpoint_test;
mod endurance_test;
mod model_test;

use std::path::Path;
use std::sync::Arc;

use elysiandb::datastore::{self, Engine, Options};
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::journal;

pub fn options(dir: &Path) -> Options {
    Options {
        folder: dir.to_path_buf(),
        shards: 8,
        durability: journal::Mode::Write,
        segment_size: 8 << 10,
        ..Options::default()
    }
}

pub fn open(opts: &Options) -> Arc<Engine> {
    datastore::open(opts.clone()).unwrap_or_else(|e| panic!("{e}"))
}

pub fn doc(pairs: Vec<(&str, Value)>) -> Map {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    m
}

pub fn s(v: impl Into<String>) -> Value {
    Value::String(v.into())
}

pub fn n(v: impl Into<f64>) -> Value {
    Value::Number(v.into())
}

pub struct Closer(pub Arc<Engine>);

impl Drop for Closer {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}
