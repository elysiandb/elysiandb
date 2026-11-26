use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use elysiandb::gocompat::errors::{self, BoxError};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::pagetree::{self, Loader, Page, Ref, Token, Tree, UpsertOut};
use elysiandb::sync::Mutex;

#[derive(Default)]
struct Inner {
    pages: HashMap<u64, Vec<u8>>,
    next: u64,
    refs: Vec<Arc<Ref>>,
    loads: usize,
}

struct MemStore {
    inner: Mutex<Inner>,
}

impl MemStore {
    fn new() -> Arc<MemStore> {
        Arc::new(MemStore {
            inner: Mutex::new(Inner {
                next: 1,
                ..Inner::default()
            }),
        })
    }

    fn flush(&self, t: &Tree) {
        let res = t.for_each_dirty(|r, p| {
            let b = pagetree::encode(Vec::new(), p)?;
            let mut g = self.inner.lock();
            let id = g.next;
            g.next += 1;
            let size = b.len() as u32;
            g.pages.insert(id, b);
            g.refs.push(r.clone());
            drop(g);
            r.persisted(id, size);
            Ok(())
        });
        if let Err(err) = res {
            panic!("{err}");
        }
    }

    fn evict(&self, r: &mut Rand, share: f64) {
        let refs = self.inner.lock().refs.clone();
        for rf in refs {
            if r.float64() < share {
                rf.evict();
            }
        }
    }

    fn loads(&self) -> usize {
        self.inner.lock().loads
    }

    fn page_count(&self) -> usize {
        self.inner.lock().pages.len()
    }
}

impl Loader for MemStore {
    fn load(&self, r: &Arc<Ref>) -> Result<Arc<Page>, BoxError> {
        let b = {
            let mut g = self.inner.lock();
            g.loads += 1;
            g.pages.get(&r.id()).cloned()
        };
        let Some(b) = b else {
            return Err(errors::new(format!("page {} not found", r.id())));
        };

        let p = pagetree::decode(&b).map_err(|e| Box::new(e) as BoxError)?;

        Ok(r.attach(Arc::new(p)))
    }
}

fn loader(store: &Arc<MemStore>) -> Option<Arc<dyn Loader>> {
    let ld: Arc<dyn Loader> = store.clone();
    Some(ld)
}

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn keys0(keys: &[Vec<u8>]) -> Vec<u8> {
    if keys.is_empty() {
        return b"k1".to_vec();
    }
    keys[keys.len() / 2].clone()
}

fn check(step: usize, tr: &Tree, m: &Model) {
    let keys: Vec<Vec<u8>> = m.keys().cloned().collect();
    let n = keys.len();
    assert_eq!(tr.len(), n, "step {step}: len {}, want {n}", tr.len());

    let mut i = 0;
    tr.ascend_at(0, |k, v| {
        if i >= n || k != keys[i].as_slice() || v != m[k].as_slice() {
            panic!(
                "step {step}: ascend at {i} got {:?}={:?}, want {:?}",
                show(k),
                show(v),
                keys.get(i).map(|k| show(k))
            );
        }
        i += 1;
        true
    });
    assert_eq!(i, n, "step {step}: ascend saw {i} keys, want {n}");

    let probes = [0i64, 1, n as i64 / 3, n as i64 / 2, n as i64 - 1, n as i64];
    for probe in probes {
        if probe < 0 {
            continue;
        }
        let p = probe as usize;

        let got = tr.at(probe);
        if p < n {
            match &got {
                Some((k, v)) if **k == *keys[p] && **v == *m[&keys[p]] => {}
                _ => panic!(
                    "step {step}: At({probe}) = {:?}, want {:?}",
                    got.as_ref().map(|(k, _)| show(k)),
                    show(&keys[p])
                ),
            }
        }
        if p >= n && got.is_some() {
            panic!("step {step}: At({probe}) beyond the end");
        }

        let mut asc: Vec<Vec<u8>> = Vec::new();
        tr.ascend_at(probe, |k, _| {
            asc.push(k.to_vec());
            asc.len() < 5
        });
        let want = &keys[p.min(n)..(p + 5).min(n)];
        if asc.len() != want.len() || (!want.is_empty() && asc[0] != want[0]) {
            panic!(
                "step {step}: AscendAt({probe}) {} keys, want {}",
                asc.len(),
                want.len()
            );
        }

        if p < n {
            let mut desc: Vec<Vec<u8>> = Vec::new();
            tr.descend_at(probe, |k, _| {
                desc.push(k.to_vec());
                desc.len() < 5
            });
            if desc.is_empty() || desc[0] != keys[p] {
                panic!("step {step}: DescendAt({probe}) {} keys", desc.len());
            }
            for (j, key) in desc.iter().enumerate().skip(1) {
                if *key != keys[p - j] {
                    panic!("step {step}: DescendAt({probe}) at {j}");
                }
            }
        }
    }

    let probes: [Vec<u8>; 6] = [
        b"".to_vec(),
        b"k0".to_vec(),
        b"k5000".to_vec(),
        b"k99".to_vec(),
        b"z".to_vec(),
        keys0(&keys),
    ];
    for k in &probes {
        let rank = keys.partition_point(|x| x < k);
        let got = tr.rank(k);
        assert_eq!(
            got,
            rank,
            "step {step}: Rank({:?}) {got}, want {rank}",
            show(k)
        );

        let mut from: Vec<Vec<u8>> = Vec::new();
        tr.ascend_from(k, |key, _| {
            from.push(key.to_vec());
            from.len() < 3
        });
        let want = &keys[rank..(rank + 3).min(n)];
        if from.as_slice() != want && !(from.is_empty() && want.is_empty()) {
            panic!("step {step}: AscendFrom({:?})", show(k));
        }

        let mut before: Vec<Vec<u8>> = Vec::new();
        tr.descend_before(k, |key, _| {
            before.push(key.to_vec());
            before.len() < 3
        });
        for (j, key) in before.iter().enumerate() {
            if *key != keys[rank - 1 - j] {
                panic!("step {step}: DescendBefore({:?})", show(k));
            }
        }

        let v = tr.get(k);
        let mv = m.get(k);
        let same = match (&v, mv) {
            (Some(v), Some(mv)) => **v == **mv,
            (None, None) => true,
            _ => false,
        };
        if !same {
            panic!(
                "step {step}: Get({:?}) {:?}, want {:?}",
                show(k),
                v.as_ref().map(|v| show(v)),
                mv.map(|v| show(v))
            );
        }
    }
}

#[test]
fn random_operations_against_a_sorted_map() {
    let mut r = Rand::new(rand::new_source(1));
    let store = MemStore::new();
    let mut tr = Tree::new(loader(&store));
    let mut m = Model::new();

    let mut versions: Vec<(Tree, Model)> = Vec::new();

    for step in 0..400 {
        let tok = Token::new();
        let mut w = tr.with_token(Some(&tok));
        let mut op = 0;
        while op < 1 + r.intn(200) {
            let k = format!("k{}", r.intn(5000)).into_bytes();
            if r.intn(6) == 0 {
                let grow = r.intn(2) == 0;
                w = w
                    .update(&k, |v| {
                        if grow {
                            let mut next = v.to_vec();
                            next.push(b'g');
                            return UpsertOut::Replace(next);
                        }
                        if !v.is_empty() {
                            v[0] = b'Z';
                        }
                        UpsertOut::InPlace
                    })
                    .0;
                if let Some(v) = m.get_mut(&k) {
                    if grow {
                        v.push(b'g');
                    } else if !v.is_empty() {
                        v[0] = b'Z';
                    }
                }
            } else if r.intn(5) == 0 {
                w = w.upsert(&k, |v| match v {
                    None => UpsertOut::Replace(b"new".to_vec()),
                    Some(v) => {
                        let mut next = v.to_vec();
                        next.push(b'u');
                        UpsertOut::Replace(next)
                    }
                });
                match m.get_mut(&k) {
                    Some(v) => v.push(b'u'),
                    None => {
                        m.insert(k.clone(), b"new".to_vec());
                    }
                }
            } else if r.intn(3) == 0 {
                w = w.delete(&k).0;
                m.remove(&k);
            } else {
                let b = b'a' + r.intn(26) as u8;
                let v = vec![b; r.intn(40) as usize];
                w = w.put(&k, &v).0;
                m.insert(k.clone(), v);
            }
            op += 1;
        }
        tok.freeze();
        tr = w.with_token(None);

        if step % 7 == 0 {
            versions.push((tr.clone(), m.clone()));
        }

        if step % 5 == 0 {
            store.flush(&tr);
            store.evict(&mut r, 0.6);
        }

        check(step, &tr, &m);
    }

    for (i, (tree, data)) in versions.iter().enumerate() {
        check(10000 + i, tree, data);
    }

    assert!(
        store.loads() > 0,
        "evicted pages should have been loaded back"
    );
}

#[test]
fn from_sorted_matches_inserts() {
    let store = MemStore::new();
    let mut m = Model::new();
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut vals: Vec<Vec<u8>> = Vec::new();
    for i in 0..20000 {
        let k = format!("key{:07}", i * 3).into_bytes();
        keys.push(k.clone());
        vals.push(i.to_string().into_bytes());
        m.insert(k, i.to_string().into_bytes());
    }

    let tr = Tree::from_sorted(loader(&store), &keys, Some(&vals));
    check(0, &tr, &m);

    store.flush(&tr);
    store.evict(&mut Rand::new(rand::new_source(2)), 1.0);
    check(1, &tr, &m);

    let tok = Token::new();
    let mut w = tr.with_token(Some(&tok));
    for i in 0..5000 {
        let k = format!("key{:07}", i * 7 + 1).into_bytes();
        w = w.put(&k, b"x").0;
        m.insert(k, b"x".to_vec());
    }
    tok.freeze();
    check(2, &w, &m);
}

#[test]
fn a_published_tree_never_changes() {
    let tok = Token::new();
    let mut tr = Tree::new(None).with_token(Some(&tok));
    for i in 0..3000 {
        tr = tr.put(format!("a{i:05}").as_bytes(), b"v1").0;
    }
    tok.freeze();
    let published = tr.with_token(None);

    let next_tok = Token::new();
    let mut next = published.with_token(Some(&next_tok));
    for i in (0..3000).step_by(2) {
        next = next.put(format!("a{i:05}").as_bytes(), b"v2").0;
        next = next.delete(format!("a{:05}", i + 1).as_bytes()).0;
    }

    assert_eq!(
        published.len(),
        3000,
        "a later batch changed a published tree: len {}",
        published.len()
    );
    published.ascend_at(0, |k, v| {
        assert_eq!(v, b"v1", "a later batch changed {} to {}", show(k), show(v));
        true
    });
    assert_eq!(
        next.len(),
        1500,
        "the new version has {} keys, want 1500",
        next.len()
    );

    let stale = published.with_token(Some(&tok));
    let _stale = stale.put(b"a00000", b"v3").0;
    let v = published.get(b"a00000");
    assert_eq!(
        v.as_deref(),
        Some(&b"v1"[..]),
        "a frozen token must not change a published page, got {:?}",
        v.as_ref().map(|v| show(v))
    );
}

#[test]
fn encode_decode_round_trip() {
    let store = MemStore::new();
    let tok = Token::new();
    let mut tr = Tree::new(loader(&store)).with_token(Some(&tok));
    for i in 0..5000 {
        let mut k = format!("{i:08}").into_bytes();
        k.extend_from_slice(&[0x00, 0xff]);
        tr = tr.put(&k, &[i as u8, 0, 1]).0;
    }
    store.flush(&tr);

    let pages: Vec<(u64, Vec<u8>)> = store
        .inner
        .lock()
        .pages
        .iter()
        .map(|(id, b)| (*id, b.clone()))
        .collect();
    for (id, b) in pages {
        let p = match pagetree::decode(&b) {
            Ok(p) => p,
            Err(err) => panic!("page {id}: {err}"),
        };
        let again = pagetree::encode(Vec::new(), &p);
        assert!(
            matches!(&again, Ok(again) if *again == b),
            "page {id} does not round trip"
        );
    }

    let broken: [&[u8]; 4] = [&[], &[9], &[1, 200], &[1, 1, 5, b'a']];
    for b in broken {
        assert!(
            pagetree::decode(b).is_err(),
            "a corrupt page {b:?} is refused"
        );
    }
}

#[test]
fn relocate_moves_only_victim_pages() {
    let store = MemStore::new();
    let mut m = Model::new();
    let tok = Token::new();
    let mut tr = Tree::new(loader(&store)).with_token(Some(&tok));
    for i in 0..20000 {
        let k = format!("r{i:06}").into_bytes();
        tr = tr.put(&k, &k).0;
        m.insert(k.clone(), k);
    }
    let tr = tr.with_token(None);
    store.flush(&tr);
    store.evict(&mut Rand::new(rand::new_source(3)), 1.0);

    let mut stored: Vec<u64> = Vec::new();
    tr.walk_stored(|id, _| stored.push(id));
    assert_eq!(
        stored.len(),
        store.page_count(),
        "the walk sees {} stored pages, {} were written",
        stored.len(),
        store.page_count()
    );

    let victims: std::collections::HashSet<u64> =
        stored[..stored.len() / 3].iter().copied().collect();

    let loads_before = store.loads();
    let (moved, n) = tr
        .with_token(Some(&Token::new()))
        .relocate(|id| victims.contains(&id), 1 << 30);
    assert_eq!(n, victims.len(), "moved {n} pages, want {}", victims.len());
    let loads = store.loads() - loads_before;
    assert!(
        loads <= victims.len() + stored.len() / 40 + 10,
        "relocation loaded {loads} pages for {} victims",
        victims.len()
    );

    let moved = moved.with_token(None);
    moved.walk_stored(|id, _| {
        assert!(
            !victims.contains(&id),
            "page {id} is still referenced after the relocation"
        );
    });
    check(0, &moved, &m);
    check(1, &tr, &m);
}
