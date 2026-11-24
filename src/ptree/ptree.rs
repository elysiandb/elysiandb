use std::sync::Arc;

const MAX_ENTRIES: usize = 32;
const MIN_ENTRIES: usize = 8;

pub struct Tree<V> {
    root: Option<Arc<Node<V>>>,
}

impl<V> Clone for Tree<V> {
    fn clone(&self) -> Tree<V> {
        Tree {
            root: self.root.clone(),
        }
    }
}

impl<V> Default for Tree<V> {
    fn default() -> Tree<V> {
        Tree { root: None }
    }
}

struct Node<V> {
    keys: Vec<Arc<[u8]>>,
    vals: Vec<V>,
    kids: Vec<Arc<Node<V>>>,
    sizes: Vec<usize>,
    size: usize,
}

impl<V> Node<V> {
    fn leaf(&self) -> bool {
        self.kids.is_empty()
    }

    fn min_key(&self) -> &Arc<[u8]> {
        &self.keys[0]
    }

    fn internal() -> Node<V> {
        Node {
            keys: Vec::new(),
            vals: Vec::new(),
            kids: Vec::new(),
            sizes: Vec::new(),
            size: 0,
        }
    }
}

impl<V: Clone> Node<V> {
    fn leaf_of(keys: Vec<Arc<[u8]>>, vals: Vec<V>, size: usize) -> Node<V> {
        Node {
            keys,
            vals,
            kids: Vec::new(),
            sizes: Vec::new(),
            size,
        }
    }

    fn copy(&self) -> Node<V> {
        let mut c = Node::internal();
        c.size = self.size;
        c.keys = self.keys.clone();
        if self.leaf() {
            c.vals = self.vals.clone();
            return c;
        }

        c.kids = self.kids.clone();
        c.sizes = self.sizes.clone();

        c
    }
}

fn child_index(keys: &[Arc<[u8]>], key: &[u8]) -> usize {
    keys.partition_point(|k| **k <= *key).saturating_sub(1)
}

fn search_leaf(keys: &[Arc<[u8]>], key: &[u8]) -> (usize, bool) {
    let i = keys.partition_point(|k| **k < *key);
    (i, i < keys.len() && *keys[i] == *key)
}

fn insert_at<T: Clone>(s: &[T], i: usize, v: T) -> Vec<T> {
    let mut out = Vec::with_capacity(s.len() + 1);
    out.extend_from_slice(&s[..i]);
    out.push(v);
    out.extend_from_slice(&s[i..]);
    out
}

fn remove_at<T: Clone>(s: &[T], i: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(s.len() - 1);
    out.extend_from_slice(&s[..i]);
    out.extend_from_slice(&s[i + 1..]);
    out
}

type Visit<'a, V> = dyn FnMut(&[u8], &V) -> bool + 'a;

impl<V: Clone> Tree<V> {
    pub fn new() -> Tree<V> {
        Tree { root: None }
    }

    pub fn len(&self) -> usize {
        match &self.root {
            None => 0,
            Some(n) => n.size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &[u8]) -> Option<&V> {
        let mut n = self.root.as_deref();
        while let Some(cur) = n {
            if cur.leaf() {
                let (i, ok) = search_leaf(&cur.keys, key);
                if ok {
                    return Some(&cur.vals[i]);
                }

                break;
            }

            n = Some(&cur.kids[child_index(&cur.keys, key)]);
        }

        None
    }

    pub fn put(&self, key: &[u8], v: V) -> (Tree<V>, bool) {
        let Some(root) = &self.root else {
            let leaf = Node::leaf_of(vec![Arc::from(key)], vec![v], 1);
            return (
                Tree {
                    root: Some(Arc::new(leaf)),
                },
                false,
            );
        };

        let (n, split, replaced) = put(root, key, v);
        let Some(split) = split else {
            return (Tree { root: Some(n) }, replaced);
        };

        let mut top = Node::internal();
        top.keys = vec![n.min_key().clone(), split.min_key().clone()];
        top.sizes = vec![n.size, split.size];
        top.size = n.size + split.size;
        top.kids = vec![n, split];

        (
            Tree {
                root: Some(Arc::new(top)),
            },
            replaced,
        )
    }

    pub fn delete(&self, key: &[u8]) -> (Tree<V>, bool) {
        let Some(root) = &self.root else {
            return (self.clone(), false);
        };

        let (n, removed) = del(root, key);
        if !removed {
            return (self.clone(), false);
        }

        let Some(mut n) = n else {
            return (Tree::default(), true);
        };
        if n.size == 0 {
            return (Tree::default(), true);
        }

        while !n.leaf() && n.kids.len() == 1 {
            n = n.kids[0].clone();
        }

        (Tree { root: Some(n) }, true)
    }

    pub fn rank(&self, key: &[u8]) -> usize {
        let mut rank = 0;
        let mut n = self.root.as_deref();
        while let Some(cur) = n {
            if cur.leaf() {
                return rank + cur.keys.partition_point(|k| **k < *key);
            }

            let i = child_index(&cur.keys, key);
            rank += cur.sizes[..i].iter().sum::<usize>();

            n = Some(&cur.kids[i]);
        }

        rank
    }

    pub fn at(&self, index: i64) -> Option<(&[u8], &V)> {
        let root = self.root.as_deref()?;
        if index < 0 || index as usize >= root.size {
            return None;
        }

        let mut index = index as usize;
        let mut n = root;
        while !n.leaf() {
            let mut i = 0;
            while index >= n.sizes[i] {
                index -= n.sizes[i];
                i += 1;
            }

            n = &n.kids[i];
        }

        Some((&n.keys[index], &n.vals[index]))
    }

    pub fn ascend_at(&self, index: i64, mut f: impl FnMut(&[u8], &V) -> bool) {
        let Some(root) = self.root.as_deref() else {
            return;
        };
        if index >= root.size as i64 {
            return;
        }

        ascend_at(root, index.max(0) as usize, &mut f);
    }

    pub fn ascend_from(&self, key: &[u8], mut f: impl FnMut(&[u8], &V) -> bool) {
        let Some(root) = self.root.as_deref() else {
            return;
        };

        ascend_from(root, key, &mut f);
    }

    pub fn descend_before(&self, key: &[u8], mut f: impl FnMut(&[u8], &V) -> bool) {
        let Some(root) = self.root.as_deref() else {
            return;
        };

        descend_before(root, key, &mut f);
    }

    pub fn descend_at(&self, index: i64, mut f: impl FnMut(&[u8], &V) -> bool) {
        let Some(root) = self.root.as_deref() else {
            return;
        };
        if index < 0 {
            return;
        }

        let index = if index as usize >= root.size {
            root.size as i64 - 1
        } else {
            index
        };

        descend_at(root, index, &mut f);
    }

    pub fn from_sorted<K: AsRef<[u8]>>(keys: &[K], vals: &[V]) -> Tree<V> {
        if keys.is_empty() {
            return Tree::default();
        }

        let mut level: Vec<Arc<Node<V>>> = Vec::new();
        let mut start = 0;
        while start < keys.len() {
            let end = (start + MAX_ENTRIES).min(keys.len());
            let ks = keys[start..end]
                .iter()
                .map(|k| Arc::from(k.as_ref()))
                .collect();
            level.push(Arc::new(Node::leaf_of(
                ks,
                vals[start..end].to_vec(),
                end - start,
            )));
            start += MAX_ENTRIES;
        }

        while level.len() > 1 {
            let mut next = Vec::new();
            for group in level.chunks(MAX_ENTRIES) {
                let mut n = Node::internal();
                for kid in group {
                    n.keys.push(kid.min_key().clone());
                    n.kids.push(kid.clone());
                    n.sizes.push(kid.size);
                    n.size += kid.size;
                }

                next.push(Arc::new(n));
            }

            level = next;
        }

        Tree { root: level.pop() }
    }
}

type Grown<V> = (Arc<Node<V>>, Option<Arc<Node<V>>>, bool);

fn put<V: Clone>(n: &Arc<Node<V>>, key: &[u8], v: V) -> Grown<V> {
    if n.leaf() {
        let (i, found) = search_leaf(&n.keys, key);
        if found {
            let mut c = n.copy();
            c.vals[i] = v;
            return (Arc::new(c), None, true);
        }

        let c = Node::leaf_of(
            insert_at(&n.keys, i, Arc::from(key)),
            insert_at(&n.vals, i, v),
            n.size + 1,
        );
        if c.keys.len() <= MAX_ENTRIES {
            return (Arc::new(c), None, false);
        }

        let (left, right) = split_leaf(&c);

        return (Arc::new(left), Some(Arc::new(right)), false);
    }

    let i = child_index(&n.keys, key);
    let (kid, split, replaced) = put(&n.kids[i], key, v);

    let mut c = n.copy();
    c.sizes[i] = kid.size;
    c.keys[i] = kid.min_key().clone();
    if !replaced {
        c.size += 1;
    }

    if let Some(split) = split {
        c.keys = insert_at(&c.keys, i + 1, split.min_key().clone());
        c.sizes = insert_at(&c.sizes, i + 1, split.size);
        c.sizes[i] = kid.size;
        c.kids[i] = kid;
        c.kids = insert_at(&c.kids, i + 1, split);
    } else {
        c.kids[i] = kid;
    }

    if c.kids.len() <= MAX_ENTRIES {
        return (Arc::new(c), None, replaced);
    }

    let (left, right) = split_internal(&c);

    (Arc::new(left), Some(Arc::new(right)), replaced)
}

fn split_leaf<V: Clone>(n: &Node<V>) -> (Node<V>, Node<V>) {
    let mid = n.keys.len() / 2;
    let left = Node::leaf_of(n.keys[..mid].to_vec(), n.vals[..mid].to_vec(), mid);
    let right = Node::leaf_of(
        n.keys[mid..].to_vec(),
        n.vals[mid..].to_vec(),
        n.keys.len() - mid,
    );

    (left, right)
}

fn split_internal<V: Clone>(n: &Node<V>) -> (Node<V>, Node<V>) {
    let mid = n.kids.len() / 2;
    let mut left = Node::internal();
    left.keys = n.keys[..mid].to_vec();
    left.kids = n.kids[..mid].to_vec();
    left.sizes = n.sizes[..mid].to_vec();
    let mut right = Node::internal();
    right.keys = n.keys[mid..].to_vec();
    right.kids = n.kids[mid..].to_vec();
    right.sizes = n.sizes[mid..].to_vec();
    left.size = left.sizes.iter().sum();
    right.size = right.sizes.iter().sum();

    (left, right)
}

fn del<V: Clone>(n: &Arc<Node<V>>, key: &[u8]) -> (Option<Arc<Node<V>>>, bool) {
    if n.leaf() {
        let (i, found) = search_leaf(&n.keys, key);
        if !found {
            return (Some(n.clone()), false);
        }

        if n.keys.len() == 1 {
            return (None, true);
        }

        let c = Node::leaf_of(remove_at(&n.keys, i), remove_at(&n.vals, i), n.size - 1);

        return (Some(Arc::new(c)), true);
    }

    let i = child_index(&n.keys, key);
    let (kid, removed) = del(&n.kids[i], key);
    if !removed {
        return (Some(n.clone()), false);
    }

    let mut c = n.copy();
    c.size -= 1;

    let Some(kid) = kid else {
        if c.kids.len() == 1 {
            return (None, true);
        }

        c.keys = remove_at(&c.keys, i);
        c.kids = remove_at(&c.kids, i);
        c.sizes = remove_at(&c.sizes, i);

        return (Some(Arc::new(c)), true);
    };

    c.sizes[i] = kid.size;
    c.keys[i] = kid.min_key().clone();
    let small = entries(&kid) < MIN_ENTRIES;
    c.kids[i] = kid;

    if small {
        merge_with_neighbour(&mut c, i);
    }

    (Some(Arc::new(c)), true)
}

fn entries<V>(n: &Node<V>) -> usize {
    n.keys.len()
}

fn merge_with_neighbour<V: Clone>(c: &mut Node<V>, i: usize) {
    let mut j = i + 1;
    if j >= c.kids.len() {
        if i == 0 {
            return;
        }
        j = i - 1;
    }

    let (a, b) = if i < j { (i, j) } else { (j, i) };

    let (left, right) = (&c.kids[a], &c.kids[b]);
    if entries(left) + entries(right) > MAX_ENTRIES {
        return;
    }

    let mut merged = Node::internal();
    merged.size = left.size + right.size;
    merged.keys = [left.keys.as_slice(), right.keys.as_slice()].concat();
    if left.leaf() {
        merged.vals = [left.vals.as_slice(), right.vals.as_slice()].concat();
    } else {
        merged.kids = [left.kids.as_slice(), right.kids.as_slice()].concat();
        merged.sizes = [left.sizes.as_slice(), right.sizes.as_slice()].concat();
    }

    c.sizes[a] = merged.size;
    c.keys[a] = merged.min_key().clone();
    c.kids[a] = Arc::new(merged);
    c.keys = remove_at(&c.keys, b);
    c.kids = remove_at(&c.kids, b);
    c.sizes = remove_at(&c.sizes, b);
}

fn ascend_at<V>(n: &Node<V>, mut index: usize, f: &mut Visit<'_, V>) -> bool {
    if n.leaf() {
        let start = index.min(n.keys.len());
        for (k, v) in n.keys[start..].iter().zip(&n.vals[start..]) {
            if !f(k, v) {
                return false;
            }
        }

        return true;
    }

    let mut i = 0;
    while i < n.kids.len() && index >= n.sizes[i] {
        index -= n.sizes[i];
        i += 1;
    }

    for kid in &n.kids[i..] {
        if !ascend_at(kid, index, f) {
            return false;
        }

        index = 0;
    }

    true
}

fn ascend_from<V>(n: &Node<V>, key: &[u8], f: &mut Visit<'_, V>) -> bool {
    if n.leaf() {
        let start = n.keys.partition_point(|k| **k < *key);
        for (k, v) in n.keys[start..].iter().zip(&n.vals[start..]) {
            if !f(k, v) {
                return false;
            }
        }

        return true;
    }

    for kid in &n.kids[child_index(&n.keys, key)..] {
        if !ascend_from(kid, key, f) {
            return false;
        }
    }

    true
}

fn descend_before<V>(n: &Node<V>, key: &[u8], f: &mut Visit<'_, V>) -> bool {
    if n.leaf() {
        let end = n.keys.partition_point(|k| **k < *key);
        for (k, v) in n.keys[..end].iter().zip(&n.vals[..end]).rev() {
            if !f(k, v) {
                return false;
            }
        }

        return true;
    }

    for kid in n.kids[..=child_index(&n.keys, key)].iter().rev() {
        if !descend_before(kid, key, f) {
            return false;
        }
    }

    true
}

fn descend_at<V>(n: &Node<V>, mut index: i64, f: &mut Visit<'_, V>) -> bool {
    if n.leaf() {
        let mut i = index;
        while i >= 0 {
            if !f(&n.keys[i as usize], &n.vals[i as usize]) {
                return false;
            }
            i -= 1;
        }

        return true;
    }

    let mut i = 0;
    while i + 1 < n.kids.len() && index >= n.sizes[i] as i64 {
        index -= n.sizes[i] as i64;
        i += 1;
    }

    loop {
        if !descend_at(&n.kids[i], index, f) {
            return false;
        }

        if i == 0 {
            break;
        }
        index = n.sizes[i - 1] as i64 - 1;
        i -= 1;
    }

    true
}
