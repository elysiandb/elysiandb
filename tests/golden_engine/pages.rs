use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::common;
use elysiandb::gocompat::errors::{self, BoxError};
use elysiandb::gocompat::json::Value;
use elysiandb::pagetree::{self, Loader, Page, Ref, Token, Tree, UpsertOut};
use elysiandb::sync::Mutex;

const REL: &str = "engine/pages.jsonl";

#[derive(Default)]
struct MemStore {
    inner: Mutex<(u64, HashMap<u64, Vec<u8>>)>,
}

impl Loader for MemStore {
    fn load(&self, r: &Arc<Ref>) -> Result<Arc<Page>, BoxError> {
        let b = self.inner.lock().1.get(&r.id()).cloned();
        let Some(b) = b else {
            return Err(errors::new(format!("page {} missing", r.id())));
        };
        let p = pagetree::decode(&b).map_err(|e| Box::new(e) as BoxError)?;
        Ok(Arc::new(p))
    }
}

impl MemStore {
    fn flush(&self, tr: &Tree) -> Vec<(u64, usize, Vec<u8>)> {
        let mut written = Vec::new();
        let res = tr.for_each_dirty(|r, p| {
            let enc = pagetree::encode(Vec::new(), p)?;
            let mut g = self.inner.lock();
            g.0 += 1;
            let id = g.0;
            g.1.insert(id, enc.clone());
            drop(g);
            r.persisted(id, enc.len() as u32);
            written.push((id, enc.len(), enc));
            Ok(())
        });
        if let Err(err) = res {
            panic!("{err}");
        }
        written
    }
}

fn fbytes(n: usize, shift: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (((i + shift) * 31 + 7) & 0xff) as u8)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
struct Op {
    op: &'static str,
    key: Vec<u8>,
    val: Vec<u8>,
}

fn op(op: &'static str, key: &[u8], val: &[u8]) -> Op {
    Op {
        op,
        key: key.to_vec(),
        val: val.to_vec(),
    }
}

fn run_ops(mut tr: Tree, ops: &[Op]) -> Tree {
    for o in ops {
        let v = o.val.clone();
        match o.op {
            "put" => tr = tr.put(&o.key, &v).0,
            "delete" => tr = tr.delete(&o.key).0,
            "upsert" => {
                tr = tr.upsert(&o.key, move |cur| match cur {
                    Some(cur) => UpsertOut::Replace([&*cur, &v[..]].concat()),
                    None => UpsertOut::Replace(v),
                })
            }
            "update" => {
                tr = tr
                    .update(&o.key, move |cur| {
                        UpsertOut::Replace([&v[..], &*cur].concat())
                    })
                    .0
            }
            other => panic!("unknown op {other}"),
        }
    }
    tr
}

fn scripts() -> Vec<(&'static str, Vec<Op>)> {
    let mut out = Vec::new();
    let mut ops = Vec::new();
    for i in 0..200 {
        ops.push(op("put", format!("s{i:04}").as_bytes(), &fbytes(8, i)));
    }
    out.push(("splits-by-count", ops));
    let mut ops = Vec::new();
    for i in 0..40 {
        ops.push(op(
            "put",
            format!("b{:04}", (i * 13) % 40).as_bytes(),
            &fbytes(1000, i),
        ));
    }
    out.push(("splits-by-bytes", ops));
    let mut ops = Vec::new();
    for i in 0..300 {
        ops.push(op("put", format!("m{i:04}").as_bytes(), &[0x00]));
    }
    for i in 0..300 {
        if i % 6 != 0 {
            ops.push(op("delete", format!("m{i:04}").as_bytes(), &[]));
        }
    }
    out.push(("merges", ops));
    let mut ops = Vec::new();
    for i in 0..100 {
        ops.push(op("put", format!("r{i:04}").as_bytes(), &[0x01]));
    }
    for i in 1..100 {
        ops.push(op("delete", format!("r{i:04}").as_bytes(), &[]));
    }
    out.push(("root-collapse", ops));
    out.push((
        "upsert-update",
        vec![
            op("upsert", b"a", &[0x01]),
            op("upsert", b"a", &[0x02]),
            op("update", b"a", &[0x03]),
            op("update", b"missing", &[0x04]),
            op("delete", b"missing", &[]),
            op("put", b"b", &[]),
            op("upsert", b"b", &[0x05]),
        ],
    ));
    out.push((
        "keys-00-ff",
        vec![
            op("put", &[0x00], &[0xaa]),
            op("put", &[0xff], &[0xbb]),
            op("put", &[0x00, 0xff], &[0xcc]),
            op("put", &[0xff, 0x00], &[0xdd]),
            op("put", &[], &[0xee]),
            op("put", &[0x00, 0x00], &[0x11]),
            op("delete", &[0xff], &[]),
            op("put", &[0xff, 0xff, 0xff], &[0x22]),
        ],
    ));
    out
}

fn cow_ops() -> Vec<Op> {
    let mut cow = Vec::new();
    for i in (0..200).step_by(7) {
        cow.push(op("put", format!("s{i:04}").as_bytes(), &[0xff]));
    }
    cow.push(op("delete", b"s0001", &[]));
    cow.push(op("put", b"s9999", &[0xee]));
    cow
}

fn keys_of(row: &Value) -> BTreeSet<String> {
    row.as_object()
        .expect("object row")
        .keys()
        .cloned()
        .collect()
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::String(s) => s.parse().expect("integer string"),
        other => panic!("not an integer: {other:?}"),
    }
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing field {key} in {row:?}"))
}

fn check_ops(name: &str, got: &Value, want: &[Op]) {
    let got = got.as_array().expect("ops array");
    assert_eq!(got.len(), want.len(), "{name}: op count");
    for (g, w) in got.iter().zip(want) {
        assert_eq!(common::str_field(g, "op"), w.op, "{name}: op");
        assert_eq!(
            common::unhex(common::str_field(g, "key")),
            w.key,
            "{name}: key"
        );
        match g.get("val") {
            Some(v) => {
                assert_eq!(
                    common::unhex(v.as_str().expect("val")),
                    w.val,
                    "{name}: val"
                );
                assert!(!w.val.is_empty(), "{name}: an empty val is omitted");
            }
            None => assert!(w.val.is_empty(), "{name}: missing val"),
        }
        let expected = if g.get("val").is_some() {
            set(&["op", "key", "val"])
        } else {
            set(&["op", "key"])
        };
        assert_eq!(keys_of(g), expected, "{name}: op fields");
    }
}

fn check_tree(name: &str, row: &Value, tr: &Tree, pages: &[(u64, usize, Vec<u8>)]) {
    let want_pages = field(row, "pages").as_array().expect("pages");
    assert_eq!(want_pages.len(), pages.len(), "{name}: page count");
    for (w, (id, size, bytes)) in want_pages.iter().zip(pages) {
        assert_eq!(
            keys_of(w),
            set(&["id", "size", "bytes"]),
            "{name}: page fields"
        );
        assert_eq!(int(field(w, "id")) as u64, *id, "{name}: page id");
        assert_eq!(
            int(field(w, "size")) as usize,
            *size,
            "{name}: page {id} size"
        );
        assert_eq!(
            common::str_field(w, "bytes"),
            common::hex(bytes),
            "{name}: page {id} bytes"
        );
    }

    match (field(row, "root"), tr.root()) {
        (Value::Null, None) => {}
        (Value::Object(_), Some(r)) => {
            let w = field(row, "root");
            assert_eq!(keys_of(w), set(&["id", "size"]), "{name}: root fields");
            assert_eq!(int(field(w, "id")) as u64, r.id(), "{name}: root id");
            assert_eq!(int(field(w, "size")) as u32, r.size(), "{name}: root size");
        }
        (w, r) => panic!("{name}: root {w:?} vs {:?}", r.map(|r| r.id())),
    }

    let n = tr.len();
    assert_eq!(int(field(row, "len")) as usize, n, "{name}: len");

    let mut dump: Vec<(String, String)> = Vec::new();
    tr.scan(|k, v| {
        dump.push((common::hex(k), common::hex(v)));
        true
    });
    let pairs = |v: &Value| -> Vec<(String, String)> {
        v.as_array()
            .expect("dump")
            .iter()
            .map(|p| {
                let p = p.as_array().expect("pair");
                assert_eq!(p.len(), 2, "{name}: dump pair");
                (
                    p[0].as_str().expect("key").to_string(),
                    p[1].as_str().expect("val").to_string(),
                )
            })
            .collect()
    };
    if dump.len() > 5000 {
        assert!(row.get("dump").is_none(), "{name}: unexpected full dump");
        assert_eq!(
            pairs(field(row, "dump_head")),
            dump[..50].to_vec(),
            "{name}: dump head"
        );
        assert_eq!(
            pairs(field(row, "dump_tail")),
            dump[dump.len() - 50..].to_vec(),
            "{name}: dump tail"
        );
    } else {
        assert_eq!(pairs(field(row, "dump")), dump, "{name}: dump");
    }

    let probes = field(row, "probes");
    assert_eq!(
        keys_of(probes),
        set(&["rank", "at"]),
        "{name}: probe fields"
    );
    let n = n as i64;
    let mut ranks: Vec<(String, i64)> = Vec::new();
    let mut ats: Vec<(i64, bool, String, String)> = Vec::new();
    for i in [0, 1, n / 3, n / 2, n - 1, n, n + 5, -1] {
        match tr.at(i) {
            Some((k, v)) => {
                ats.push((i, true, common::hex(&k), common::hex(&v)));
                ranks.push((common::hex(&k), tr.rank(&k) as i64));
                let mut k0 = k.to_vec();
                k0.push(0);
                ranks.push((common::hex(&k0), tr.rank(&k0) as i64));
            }
            None => ats.push((i, false, String::new(), String::new())),
        }
    }
    ranks.push((String::new(), tr.rank(b"") as i64));
    ranks.push(("ffff".to_string(), tr.rank(&[0xff, 0xff]) as i64));

    let want_ats = field(probes, "at").as_array().expect("at");
    assert_eq!(want_ats.len(), ats.len(), "{name}: at probes");
    for (w, (i, ok, k, v)) in want_ats.iter().zip(&ats) {
        assert_eq!(
            keys_of(w),
            set(&["index", "ok", "key", "val"]),
            "{name}: at fields"
        );
        assert_eq!(int(field(w, "index")), *i, "{name}: at index");
        assert_eq!(field(w, "ok").as_bool(), Some(*ok), "{name}: At({i}) ok");
        assert_eq!(common::str_field(w, "key"), k, "{name}: At({i}) key");
        assert_eq!(common::str_field(w, "val"), v, "{name}: At({i}) val");
    }

    let want_ranks = field(probes, "rank").as_array().expect("rank");
    assert_eq!(want_ranks.len(), ranks.len(), "{name}: rank probes");
    for (w, (k, r)) in want_ranks.iter().zip(&ranks) {
        assert_eq!(keys_of(w), set(&["key", "rank"]), "{name}: rank fields");
        assert_eq!(common::str_field(w, "key"), k, "{name}: rank key");
        assert_eq!(int(field(w, "rank")), *r, "{name}: Rank({k})");
    }
}

fn check_from_sorted(row: &Value, n: usize, with_vals: bool, heavy: Option<usize>) {
    let name = common::str_field(row, "scenario");
    let store = Arc::new(MemStore::default());
    let keys: Vec<Vec<u8>> = (0..n).map(|i| format!("k{i:06}").into_bytes()).collect();
    let vals: Option<Vec<Vec<u8>>> = match heavy {
        Some(size) => Some((0..n).map(|i| fbytes(size, i)).collect()),
        None if with_vals => Some((0..n).map(|i| fbytes(i % 9, i)).collect()),
        None => None,
    };
    let ld: Arc<dyn Loader> = store.clone();
    let tr = Tree::from_sorted(Some(ld), &keys, vals.as_deref());
    let pages = store.flush(&tr);

    let ops = field(row, "ops");
    assert_eq!(
        keys_of(ops),
        set(&["from_sorted", "keys", "values"]),
        "{name}: ops"
    );
    assert_eq!(
        int(field(ops, "from_sorted")) as usize,
        n,
        "{name}: from_sorted"
    );
    assert_eq!(
        common::str_field(ops, "keys"),
        "k%06d",
        "{name}: key format"
    );
    let values = field(ops, "values");
    match heavy {
        Some(size) => assert_eq!(
            values.as_str(),
            Some(format!("{size} bytes, byte j = ((j+i)*31+7)&0xff").as_str()),
            "{name}: values"
        ),
        None if with_vals => assert_eq!(
            values.as_str(),
            Some("i%9 bytes, byte j = ((j+i)*31+7)&0xff"),
            "{name}: values"
        ),
        None => assert!(values.is_null(), "{name}: values"),
    }

    check_tree(name, row, &tr, &pages);
}

fn check_script(row: &Value, ops: &[Op]) {
    let name = common::str_field(row, "scenario");
    check_ops(name, field(row, "ops"), ops);
    let store = Arc::new(MemStore::default());
    let ld: Arc<dyn Loader> = store.clone();
    let tok = Token::new();
    let tr = run_ops(Tree::new(Some(ld)).with_token(Some(&tok)), ops);
    let pages = store.flush(&tr);
    check_tree(name, row, &tr, &pages);
}

fn check_copy_on_write(row: &Value, first_ops: &[Op]) {
    let name = common::str_field(row, "scenario");
    let ops = field(row, "ops");
    assert_eq!(keys_of(ops), set(&["first", "second"]), "{name}: ops");
    let cow = cow_ops();
    check_ops(name, field(ops, "first"), first_ops);
    check_ops(name, field(ops, "second"), &cow);

    let store = Arc::new(MemStore::default());
    let ld: Arc<dyn Loader> = store.clone();
    let tok = Token::new();
    let base = run_ops(Tree::new(Some(ld)).with_token(Some(&tok)), first_ops);
    let mut pages = store.flush(&base);
    tok.freeze();
    let next_tok = Token::new();
    let next = run_ops(base.with_token(Some(&next_tok)), &cow);
    pages.extend(store.flush(&next));
    check_tree(name, row, &next, &pages);
}

fn check_decode(row: &Value) {
    let bytes = common::unhex(common::str_field(row, "bytes"));
    match pagetree::decode(&bytes) {
        Ok(_) => {
            assert_eq!(keys_of(row), set(&["bytes", "ok"]), "decode fields");
            assert_eq!(field(row, "ok").as_bool(), Some(true), "decode ok");
        }
        Err(err) => {
            assert_eq!(keys_of(row), set(&["bytes", "error"]), "decode fields");
            assert_eq!(
                common::str_field(row, "error"),
                err.to_string(),
                "decode of {}",
                common::hex(&bytes)
            );
        }
    }
}

#[test]
fn golden_pages() {
    let rows = common::read_jsonl(REL);
    assert_eq!(rows.len(), common::lines(&common::read_golden(REL)).len());

    let scripts = scripts();
    let mut scenarios: Vec<String> = Vec::new();
    let mut checked = 0;
    for row in &rows {
        let Some(name) = row.get("scenario").and_then(Value::as_str) else {
            check_decode(row);
            checked += 1;
            continue;
        };

        let mut expected = set(&["scenario", "ops", "pages", "root", "len", "probes"]);
        if int(field(row, "len")) > 5000 {
            expected.extend(set(&["dump_head", "dump_tail"]));
        } else {
            expected.insert("dump".to_string());
        }
        assert_eq!(keys_of(row), expected, "{name}: fields");
        scenarios.push(name.to_string());

        if let Some(rest) = name.strip_prefix("from-sorted-heavy-") {
            check_from_sorted(row, 40, true, Some(rest.parse().expect("size")));
        } else if let Some(rest) = name.strip_prefix("from-sorted-") {
            let (n, with_vals) = rest.split_once('-').expect("from-sorted name");
            check_from_sorted(row, n.parse().expect("count"), with_vals == "true", None);
        } else if name == "copy-on-write" {
            let first = &scripts
                .iter()
                .find(|(n, _)| *n == "splits-by-count")
                .expect("splits-by-count")
                .1;
            check_copy_on_write(row, first);
        } else {
            let ops = &scripts
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("unknown scenario {name}"))
                .1;
            check_script(row, ops);
        }
        checked += 1;
    }

    let mut want: Vec<String> = Vec::new();
    for n in [0, 1, 47, 48, 49, 2305, 20000] {
        for v in [false, true] {
            want.push(format!("from-sorted-{n}-{v}"));
        }
    }
    for size in [300, 4000, 4097, 9000] {
        want.push(format!("from-sorted-heavy-{size}"));
    }
    for (name, _) in &scripts {
        want.push(name.to_string());
    }
    want.push("copy-on-write".to_string());
    assert_eq!(scenarios, want, "scenario list");

    assert_eq!(checked, rows.len());
    assert_eq!(checked, 34);
}
