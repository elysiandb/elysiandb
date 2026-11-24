use crate::docpath;
use crate::gocompat::binary;
use crate::gocompat::json::Value;
use crate::gocompat::strings;
use crate::pagetree::Tree;

use super::engine::IndexView;
use super::indexkey::{
    INDEX_COMPOSITE, IndexSpec, KEY_LOWER, diff_sorted, key_seq, seq_key, sort_key, terminated,
    unique_keys,
};

pub const IRREGULAR_KEY: &[u8] = b"\x00irregular";

pub fn composite_spec(eq_field: &[u8], sort_field: &[u8]) -> IndexSpec {
    let mut field = Vec::with_capacity(eq_field.len() + 1 + sort_field.len());
    field.extend_from_slice(eq_field);
    field.push(b',');
    field.extend_from_slice(sort_field);
    IndexSpec::new(INDEX_COMPOSITE, field)
}

impl IndexSpec {
    pub fn composite_fields(&self) -> Option<(&[u8], &[u8])> {
        if self.kind != INDEX_COMPOSITE {
            return None;
        }

        let i = self.field.iter().position(|&c| c == b',')?;
        if i == 0 || i == self.field.len() - 1 {
            return None;
        }

        Some((&self.field[..i], &self.field[i + 1..]))
    }
}

pub fn composite_prefix(value: &[u8]) -> Vec<u8> {
    terminated(KEY_LOWER, &strings::to_lower(value))
}

#[doc(hidden)]
pub fn composite_keys(spec: &IndexSpec, doc: Option<&Value>) -> (Vec<Vec<u8>>, bool) {
    let Some((eq_field, sort_field)) = spec.composite_fields() else {
        return (Vec::new(), false);
    };
    let Some(doc) = doc else {
        return (Vec::new(), false);
    };

    let values = docpath::resolve_values(doc, eq_field);
    if values.is_empty() {
        return (Vec::new(), false);
    }

    let sk = sort_key(Some(doc), sort_field);
    let mut keys = Vec::with_capacity(values.len());
    let mut irregular = false;
    for v in values {
        let Value::String(s) = v else {
            irregular = true;
            continue;
        };
        let mut k = composite_prefix(s.as_bytes());
        k.extend_from_slice(&sk);
        keys.push(k);
    }

    (unique_keys(keys), irregular)
}

#[doc(hidden)]
pub fn irregular_count(tree: &Tree) -> i64 {
    match tree.get(IRREGULAR_KEY) {
        Some(b) => binary::varint(&b).0,
        None => 0,
    }
}

#[doc(hidden)]
pub fn add_irregular(tree: Tree, delta: i64) -> Tree {
    let n = irregular_count(&tree).wrapping_add(delta);
    if n == 0 {
        return tree.delete(IRREGULAR_KEY).0;
    }

    let mut v = Vec::new();
    binary::append_varint(&mut v, n);
    tree.put(IRREGULAR_KEY, &v).0
}

pub(crate) fn apply_composite_change(
    spec: &IndexSpec,
    mut tree: Tree,
    seq: u32,
    old: Option<&Value>,
    new: Option<&Value>,
) -> (Tree, bool) {
    let (old_keys, old_irregular) = composite_keys(spec, old);
    let (new_keys, new_irregular) = composite_keys(spec, new);
    if old_irregular == new_irregular && old_keys == new_keys {
        return (tree, false);
    }

    let (removed, added) = diff_sorted(&old_keys, &new_keys);
    let sk = seq_key(seq);
    for k in &removed {
        let mut key = k.clone();
        key.extend_from_slice(&sk);
        tree = tree.delete(&key).0;
    }

    for k in &added {
        let mut key = k.clone();
        key.extend_from_slice(&sk);
        tree = tree.put(&key, &[]).0;
    }

    if old_irregular && !new_irregular {
        tree = add_irregular(tree, -1);
    } else if !old_irregular && new_irregular {
        tree = add_irregular(tree, 1);
    }

    (tree, true)
}

impl IndexView {
    pub fn irregular(&self) -> i64 {
        irregular_count(&self.idx.tree)
    }

    pub fn sub(&self, prefix: &[u8]) -> IndexView {
        let base = self.idx.tree.rank(prefix) as i64;
        let end = self
            .idx
            .tree
            .rank(&super::engine::prefix_end(prefix).unwrap_or_default()) as i64;

        IndexView {
            idx: self.idx.clone(),
            prefix: prefix.to_vec(),
            base,
            end,
            sub: true,
        }
    }

    pub(crate) fn sub_len(&self) -> i64 {
        self.end - self.base
    }

    pub(crate) fn sub_rank(&self, key: &[u8]) -> i64 {
        let mut full = self.prefix.clone();
        full.extend_from_slice(key);
        let r = self.idx.tree.rank(&full) as i64 - self.base;
        r.max(0).min(self.sub_len())
    }

    pub(crate) fn sub_at(&self, i: i64) -> Option<(Vec<u8>, u32)> {
        if i < 0 || i >= self.sub_len() {
            return None;
        }

        let (k, _) = self.idx.tree.at(self.base + i)?;
        Some((k[self.prefix.len()..].to_vec(), key_seq(&k)))
    }

    pub(crate) fn sub_ascend_at(&self, i: i64, mut f: impl FnMut(&[u8], u32) -> bool) {
        if i < 0 || i >= self.sub_len() {
            return;
        }

        let mut left = self.sub_len() - i;
        let n = self.prefix.len();
        self.idx.tree.ascend_at(self.base + i, |k, _| {
            if left <= 0 {
                return false;
            }
            left -= 1;
            f(&k[n..], key_seq(k))
        });
    }
}
