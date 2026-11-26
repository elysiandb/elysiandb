use std::collections::BTreeMap;
use std::sync::Arc;

use elysiandb::durable::{self, PageStore, filepath};
use elysiandb::gocompat::binary;
use elysiandb::gocompat::json::Value;
use elysiandb::pagetree::{Loader, Ref, Token, Tree};

use super::common;
use super::support::{array, assert_same_tree, copy_tree, dec_u64, field, formula_bytes, int};
use super::support::{read_tree, string, uint};
use super::tempdir::TempDir;

const TREES: &str = "persistence/pages/trees.json";
const SEGMENTS: &str = "persistence/pages/pgs";

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

fn key(i: usize) -> Vec<u8> {
    format!("key-{i:08}").into_bytes()
}

fn encode_slot(lsn: u64, segment: u32, offset: u32, length: u32, id: &[u8]) -> Vec<u8> {
    let mut dst = Vec::new();
    binary::append_uvarint(&mut dst, lsn);
    binary::append_uvarint(&mut dst, u64::from(segment));
    binary::append_uvarint(&mut dst, u64::from(offset));
    binary::append_uvarint(&mut dst, u64::from(length));
    dst.extend_from_slice(id);
    dst
}

fn loader(store: &Arc<PageStore>) -> Option<Arc<dyn Loader>> {
    let ld: Arc<dyn Loader> = store.clone();
    Some(ld)
}

fn contents(tr: &Tree) -> Model {
    let mut out = Model::new();
    tr.scan(|k, v| {
        out.insert(k.to_vec(), v.to_vec());
        true
    });
    out
}

fn assert_tree(label: &str, tr: &Tree, model: &Model) {
    assert_eq!(tr.len(), model.len(), "{label}: len");
    let got = contents(tr);
    assert!(got == *model, "{label}: contents differ");
    for (k, v) in model {
        match tr.get(k) {
            Some(found) => assert!(*found == **v, "{label}: value of {k:?}"),
            None => panic!("{label}: {k:?} is missing"),
        }
    }
}

struct Built {
    name: &'static str,
    step: &'static str,
    tree: Tree,
    model: Model,
}

fn check_row(row: &Value, b: &Built) {
    assert_eq!(string(row, "tree"), b.name);
    assert_eq!(string(row, "step"), b.step, "{}", b.name);
    assert_eq!(uint(row, "len") as usize, b.tree.len(), "{}: len", b.name);
    assert_eq!(b.model.len(), b.tree.len(), "{}: model", b.name);
    match (field(row, "root"), b.tree.root()) {
        (Value::Null, None) => {}
        (Value::Null, Some(r)) => panic!("{}: unexpected root {r:?}", b.name),
        (root, Some(r)) => {
            assert_eq!(dec_u64(root, "id"), r.id(), "{}: root id", b.name);
            assert_eq!(
                uint(root, "size"),
                u64::from(r.size()),
                "{}: root size",
                b.name
            );
        }
        (root, None) => panic!("{}: missing root {root:?}", b.name),
    }
}

#[test]
fn golden_page_store_writes_the_go_segments() {
    let doc = common::read_json(TREES);
    let cfg = field(&doc, "store");
    let limit = dec_u64(cfg, "limit") as i64;
    let segment_size = int(cfg, "segment_size");
    assert_eq!(limit, 1 << 30);
    assert_eq!(segment_size, 16 << 10);
    let rows = array(&doc, "steps");

    let tmp = TempDir::new();
    let work = filepath::join(tmp.path(), "pages");
    let store = durable::open_page_store(None, &work, limit, segment_size)
        .unwrap_or_else(|e| panic!("{e}"));
    let ld = loader(&store);

    let mut built: Vec<Built> = Vec::new();
    let mut flush = |b: Built| {
        store
            .flush_tree(&b.tree)
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        store.sync().unwrap_or_else(|e| panic!("{}: {e}", b.name));
        check_row(&rows[built.len()], &b);
        built.push(b);
    };

    let tok = Token::new();
    let mut a = Tree::new(ld.clone()).with_token(Some(&tok));
    let mut a_model = Model::new();
    for i in 0..2000 {
        a = a.put(&key(i), &formula_bytes(32, i)).0;
        a_model.insert(key(i), formula_bytes(32, i));
    }
    tok.freeze();
    flush(Built {
        name: "a",
        step: "put",
        tree: a.clone(),
        model: a_model.clone(),
    });

    let tok = Token::new();
    let mut b = Tree::new(ld.clone()).with_token(Some(&tok));
    let mut b_model = Model::new();
    for i in 0..500 {
        b = b.put(&key(i), &formula_bytes(32, i)).0;
        b_model.insert(key(i), formula_bytes(32, i));
    }
    for i in (0..500).step_by(5) {
        b = b.delete(&key(i)).0;
        b = b.delete(&key(i + 1)).0;
        b_model.remove(&key(i));
        b_model.remove(&key(i + 1));
    }
    tok.freeze();
    flush(Built {
        name: "b",
        step: "put-delete",
        tree: b,
        model: b_model,
    });

    let mut idx_keys: Vec<Vec<u8>> = (0..5000usize)
        .map(|i| format!("f\x00title\x00{:08}\x00{:06}", i * 7 % 5000, i).into_bytes())
        .collect();
    idx_keys.sort();
    let c1 = Tree::from_sorted::<Vec<u8>, Vec<u8>>(ld.clone(), &idx_keys, None);
    let c1_model: Model = idx_keys.iter().map(|k| (k.clone(), Vec::new())).collect();
    flush(Built {
        name: "c-index",
        step: "from-sorted",
        tree: c1,
        model: c1_model,
    });

    let mut seq_keys: Vec<Vec<u8>> = Vec::new();
    let mut seq_vals: Vec<Vec<u8>> = Vec::new();
    for i in 0..3000usize {
        seq_keys.push(((i + 1) as u32).to_be_bytes().to_vec());
        seq_vals.push(encode_slot(
            (i + 10) as u64,
            (1 + i / 100) as u32,
            (16 + i * 40) as u32,
            (i % 97) as u32,
            &key(i),
        ));
    }
    let c2 = Tree::from_sorted(ld.clone(), &seq_keys, Some(&seq_vals));
    let c2_model: Model = seq_keys
        .iter()
        .cloned()
        .zip(seq_vals.iter().cloned())
        .collect();
    flush(Built {
        name: "c-slots",
        step: "from-sorted",
        tree: c2,
        model: c2_model,
    });

    flush(Built {
        name: "d",
        step: "empty",
        tree: Tree::new(ld.clone()),
        model: Model::new(),
    });

    let tok = Token::new();
    let mut e = a.with_token(Some(&tok));
    let mut e_model = a_model.clone();
    for i in 0..100 {
        e = e.put(&key(i * 17), &formula_bytes(48, i + 1000)).0;
        e_model.insert(key(i * 17), formula_bytes(48, i + 1000));
    }
    tok.freeze();
    flush(Built {
        name: "e",
        step: "update-after-flush",
        tree: e,
        model: e_model,
    });

    assert_eq!(built.len(), rows.len());
    assert_eq!(rows.len(), 6);

    store.close().unwrap_or_else(|e| panic!("{e}"));

    let golden = common::golden_path(SEGMENTS);
    let segments = read_tree(&golden);
    assert_eq!(segments.len(), 26);
    assert_eq!(read_tree(&work).len(), segments.len());
    assert_same_tree(&work, &golden, "page segments");

    for b in &built {
        assert_tree(&format!("{} (Rust)", b.name), &b.tree, &b.model);
    }

    let go = filepath::join(tmp.path(), "go-pages");
    copy_tree(&golden, &go);
    let go_store =
        durable::open_page_store(None, &go, limit, segment_size).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(go_store.current_segment(), 26);
    assert_eq!(go_store.segment_sizes().len(), 26);
    for (seg, size) in go_store.segment_sizes() {
        let name = format!("{seg:06}.pgs");
        assert_eq!(size as usize, segments[&name].len(), "{name}");
    }
    let go_ld = loader(&go_store);

    let mut loaded = 0;
    for b in &built {
        let root = b.tree.root().map(|r| Ref::stored(r.id(), r.size()));
        let tr = Tree::open(go_ld.clone(), root);
        assert_tree(&format!("{} (Go pages)", b.name), &tr, &b.model);
        loaded += 1;
    }
    assert_eq!(loaded, rows.len());
    assert!(go_store.stats().loads > 0);
    assert_eq!(go_store.stats().writes, 0);
    go_store.close().unwrap_or_else(|e| panic!("{e}"));
}
