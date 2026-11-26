use std::collections::BTreeMap;

use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::ptree::Tree;

type Model = BTreeMap<Vec<u8>, i64>;

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn check(tree: &Tree<i64>, m: &Model) {
    let keys: Vec<&Vec<u8>> = m.keys().collect();
    assert_eq!(
        tree.len(),
        keys.len(),
        "len {}, want {}",
        tree.len(),
        keys.len()
    );

    let mut i = 0;
    tree.ascend_at(0, |k, v| {
        if k != keys[i].as_slice() || *v != m[k] {
            panic!(
                "ascend #{i}: {}={v}, want {}={}",
                show(k),
                show(keys[i]),
                m[keys[i]]
            );
        }
        i += 1;
        true
    });
    assert_eq!(i, keys.len(), "ascend visited {i}, want {}", keys.len());

    for (idx, k) in keys.iter().enumerate() {
        match tree.at(idx as i64) {
            Some((got, _)) if got == k.as_slice() => {}
            other => panic!(
                "At({idx})={:?}, want {}",
                other.map(|(g, _)| show(g)),
                show(k)
            ),
        }
        let r = tree.rank(k);
        assert_eq!(r, idx, "Rank({})={r}, want {idx}", show(k));
        match tree.get(k) {
            Some(v) if *v == m[*k] => {}
            other => panic!("Get({})={other:?}", show(k)),
        }
    }

    let mut j = keys.len() as i64 - 1;
    tree.descend_at(keys.len() as i64 - 1, |k, _| {
        if k != keys[j as usize].as_slice() {
            panic!("descend #{j}: {}, want {}", show(k), show(keys[j as usize]));
        }
        j -= 1;
        true
    });
    assert_eq!(j, -1, "descend stopped at {j}");
}

#[test]
fn random_operations_against_model() {
    let mut r = Rand::new(rand::new_source(7));
    let mut tree: Tree<i64> = Tree::new();
    let mut m = Model::new();
    let mut snapshots: Vec<(Tree<i64>, Model)> = Vec::new();

    for step in 0..20000i64 {
        let k = format!("k{:05}", r.intn(3000)).into_bytes();
        match r.intn(10) {
            0..=2 => {
                let (next, removed) = tree.delete(&k);
                tree = next;
                let existed = m.contains_key(&k);
                assert_eq!(
                    removed,
                    existed,
                    "delete {} removed={removed} existed={existed}",
                    show(&k)
                );
                m.remove(&k);
            }
            _ => {
                let (next, replaced) = tree.put(&k, step);
                tree = next;
                let existed = m.contains_key(&k);
                assert_eq!(
                    replaced,
                    existed,
                    "put {} replaced={replaced} existed={existed}",
                    show(&k)
                );
                m.insert(k, step);
            }
        }

        if step % 2000 == 0 {
            snapshots.push((tree.clone(), m.clone()));
            check(&tree, &m);
        }
    }

    check(&tree, &m);
    for (t, sm) in &snapshots {
        check(t, sm);
    }
}

#[test]
fn ranges_and_bulk_load() {
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut vals: Vec<i64> = Vec::new();
    for i in 0..5000i64 {
        keys.push(format!("{:06}", i * 2).into_bytes());
        vals.push(i);
    }

    let mut tree = Tree::from_sorted(&keys, &vals);
    let mut m = Model::new();
    for (i, k) in keys.iter().enumerate() {
        m.insert(k.clone(), vals[i]);
    }
    check(&tree, &m);

    let mut got: Vec<String> = Vec::new();
    tree.ascend_from(b"000101", |k, _| {
        got.push(show(k));
        got.len() < 3
    });
    assert_eq!(
        format!("[{}]", got.join(" ")),
        "[000102 000104 000106]",
        "AscendFrom: {got:?}"
    );

    got.clear();
    tree.descend_before(b"000101", |k, _| {
        got.push(show(k));
        got.len() < 3
    });
    assert_eq!(
        format!("[{}]", got.join(" ")),
        "[000100 000098 000096]",
        "DescendBefore: {got:?}"
    );

    let r = tree.rank(b"000101");
    assert_eq!(r, 51, "Rank: {r}");

    for i in (0..5000).step_by(3) {
        tree = tree.delete(&keys[i]).0;
        m.remove(&keys[i]);
    }
    check(&tree, &m);
}
