use std::collections::HashMap;
use std::sync::LazyLock;

use crate::datastore::{Doc, ORIGIN_ADVISOR, composite_prefix, composite_spec};
use crate::query::{Filters, Sort};
use crate::sync::Mutex;

use super::filter::parse_date;
use super::plan::{ListQuery, Planner, node_is_everything, walk_sort_index_from};

struct EqSortShape {
    eq_field: Vec<u8>,
    eq_value: Vec<u8>,
    sort_field: Vec<u8>,
    ascending: bool,
}

fn shape_of(q: &ListQuery<'_, '_>) -> Option<EqSortShape> {
    let sorts: Vec<Sort> = if q.sorts.is_empty() && !q.sort_field.is_empty() {
        vec![Sort::new(q.sort_field, q.sort_asc)]
    } else {
        q.sorts.clone()
    };

    if sorts.len() != 1 || !q.search.is_empty() || q.offset < 0 {
        return None;
    }

    let mut filters: Option<&Filters> = q.filters;
    if let Some(node) = q.node
        && !node_is_everything(node)
    {
        if q.filters.is_some_and(|f| !f.is_empty())
            || node.leaf.is_none()
            || !node.and.is_empty()
            || !node.or.is_empty()
        {
            return None;
        }
        filters = node.leaf.as_ref();
    }

    let filters = filters?;
    if filters.len() != 1 {
        return None;
    }

    let (field, ops) = filters.iter().next()?;
    let cmp = ops.get(b"eq".as_slice())?;
    if ops.len() != 1 || *field == sorts[0].field {
        return None;
    }

    if cmp.iter().any(|&c| c == b'*' || c == b'\\') {
        return None;
    }

    if parse_date(cmp).is_some() {
        return None;
    }

    Some(EqSortShape {
        eq_field: field.clone(),
        eq_value: cmp.clone(),
        sort_field: sorts[0].field.clone(),
        ascending: sorts[0].ascending,
    })
}

const COMPOSITE_MIN_DOCS: usize = 4096;
const COMPOSITE_AFTER_USE: i64 = 8;

type ShapeKey = (Vec<u8>, Vec<u8>, Vec<u8>);

static SHAPE_USES: LazyLock<Mutex<HashMap<ShapeKey, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl Planner {
    pub(crate) fn composite_page(&mut self, q: &ListQuery<'_, '_>) -> Option<(Vec<Doc>, usize)> {
        let shape = shape_of(q)?;

        self.observe_shape(&shape);

        if self.affected_by_includes(&shape.eq_field)
            || (!self.refs.is_empty() && self.sort_path_crosses_reference(&shape.sort_field))
        {
            return None;
        }

        let view = self
            .snap
            .index(&composite_spec(&shape.eq_field, &shape.sort_field))?;
        if view.irregular() != 0 {
            return None;
        }

        let sub = view.sub(&composite_prefix(&shape.eq_value));
        let snap = self.snap.clone();
        let mut docs: Vec<Doc> = Vec::new();
        if !q.count_only || q.limit > 0 || q.offset > 0 {
            let limit = q.limit;
            walk_sort_index_from(&sub, shape.ascending, q.offset, &mut |seq| {
                docs.push(snap.indexed(seq));
                limit <= 0 || (docs.len() as i64) < limit
            });
        }

        Some((docs, sub.len() as usize))
    }

    fn observe_shape(&self, shape: &EqSortShape) {
        if self.snap.len() < COMPOSITE_MIN_DOCS {
            return;
        }

        let spec = composite_spec(&shape.eq_field, &shape.sort_field);
        if self.snap.has_index(&spec) {
            return;
        }

        let uses = {
            let mut m = SHAPE_USES.lock();
            let n = m
                .entry((
                    self.snap.name().to_vec(),
                    shape.eq_field.clone(),
                    shape.sort_field.clone(),
                ))
                .or_insert(0);
            *n += 1;
            *n
        };
        if uses != COMPOSITE_AFTER_USE {
            return;
        }

        let name = self.snap.name().to_vec();
        if self.e.index_names(&name).map_or(0, |n| n.len()) < 64 {
            self.e
                .ensure_index_with(&name, &spec, ORIGIN_ADVISOR.as_bytes());
        }
    }
}
