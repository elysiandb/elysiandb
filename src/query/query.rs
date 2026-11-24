use std::collections::{BTreeMap, BTreeSet};

use crate::gocompat::strings;

pub type Ops = BTreeMap<Vec<u8>, Vec<u8>>;
pub type Filters = BTreeMap<Vec<u8>, Ops>;

#[derive(Clone, Default)]
pub struct Query {
    pub entity: Vec<u8>,
    pub offset: i64,
    pub limit: i64,
    pub filter: FilterNode,
    pub sorts: BTreeMap<Vec<u8>, Vec<u8>>,
    pub sort_order: Vec<Vec<u8>>,
}

#[derive(Default)]
pub struct FilterNode {
    pub or: Vec<FilterNode>,
    pub and: Vec<FilterNode>,
    pub leaf: Option<Filters>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sort {
    pub field: Vec<u8>,
    pub ascending: bool,
}

impl Sort {
    pub fn new(field: impl Into<Vec<u8>>, ascending: bool) -> Sort {
        Sort {
            field: field.into(),
            ascending,
        }
    }
}

impl FilterNode {
    pub fn with_leaf(leaf: Filters) -> FilterNode {
        FilterNode {
            or: Vec::new(),
            and: Vec::new(),
            leaf: Some(leaf),
        }
    }

    pub fn with_or(nodes: Vec<FilterNode>) -> FilterNode {
        FilterNode {
            or: nodes,
            and: Vec::new(),
            leaf: None,
        }
    }

    pub fn with_and(nodes: Vec<FilterNode>) -> FilterNode {
        FilterNode {
            or: Vec::new(),
            and: nodes,
            leaf: None,
        }
    }
}

impl Drop for FilterNode {
    fn drop(&mut self) {
        if self.or.is_empty() && self.and.is_empty() {
            return;
        }

        let mut stack: Vec<FilterNode> = Vec::new();
        stack.append(&mut self.or);
        stack.append(&mut self.and);
        while let Some(mut n) = stack.pop() {
            stack.append(&mut n.or);
            stack.append(&mut n.and);
        }
    }
}

struct CloneFrame<'a> {
    src: &'a FilterNode,
    out: FilterNode,
    next: usize,
}

fn clone_frame(src: &FilterNode) -> CloneFrame<'_> {
    CloneFrame {
        src,
        out: FilterNode {
            or: Vec::new(),
            and: Vec::new(),
            leaf: src.leaf.clone(),
        },
        next: 0,
    }
}

impl Clone for FilterNode {
    fn clone(&self) -> FilterNode {
        let mut stack = vec![clone_frame(self)];
        loop {
            let Some(top) = stack.last_mut() else {
                return FilterNode::default();
            };
            let ors = top.src.or.len();
            if top.next < ors + top.src.and.len() {
                let src = top.src;
                let child = if top.next < ors {
                    &src.or[top.next]
                } else {
                    &src.and[top.next - ors]
                };
                top.next += 1;
                stack.push(clone_frame(child));
                continue;
            }

            let Some(done) = stack.pop() else {
                return FilterNode::default();
            };
            let out = done.out;
            match stack.last_mut() {
                None => return out,
                Some(parent) => {
                    if parent.next <= parent.src.or.len() {
                        parent.out.or.push(out);
                    } else {
                        parent.out.and.push(out);
                    }
                }
            }
        }
    }
}

impl Query {
    pub fn primary_sort(&self) -> (Vec<u8>, bool) {
        let sorts = self.ordered_sorts();
        match sorts.into_iter().next() {
            None => (Vec::new(), true),
            Some(s) => (s.field, s.ascending),
        }
    }

    pub fn ordered_sorts(&self) -> Vec<Sort> {
        let mut fields: Vec<&[u8]> = Vec::with_capacity(self.sorts.len());
        let mut seen: BTreeSet<&[u8]> = BTreeSet::new();

        for field in &self.sort_order {
            if self.sorts.contains_key(field) && seen.insert(field.as_slice()) {
                fields.push(field);
            }
        }

        for field in self.sorts.keys() {
            if !seen.contains(field.as_slice()) {
                fields.push(field);
            }
        }

        fields
            .into_iter()
            .map(|field| Sort {
                field: field.to_vec(),
                ascending: self
                    .sorts
                    .get(field)
                    .is_none_or(|v| strings::to_lower(v) != b"desc"),
            })
            .collect()
    }
}
