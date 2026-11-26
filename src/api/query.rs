use crate::datastore::{Doc, Snapshot};
use crate::gocompat::json::Map;
use crate::query::{FilterNode, Filters, Query};

use super::filter::{match_leaf_strict, stored_filters_match_entity};
use super::guard::{self, Guard, GuardError};
use super::plan::{ListQuery, ListResult, plan};

pub fn execute_query(q: &Query) -> (Vec<Map>, Option<GuardError>) {
    (plan_query(q, false).decode(), None)
}

pub fn execute_query_guarded(q: &Query, guard: Option<&Guard>) -> (Vec<Map>, Option<GuardError>) {
    let res = plan_query_guarded(q, false, guard);
    if let Some(err) = res.err {
        return (Vec::new(), Some(err));
    }

    (res.decode(), guard::err(guard))
}

pub fn plan_query(q: &Query, count_only: bool) -> ListResult<'static> {
    plan_query_guarded(q, count_only, None)
}

pub fn stream_query_guarded(
    q: &Query,
    guard: Option<&Guard>,
    snap: Option<Snapshot>,
    f: &mut dyn FnMut(Doc) -> bool,
) -> Option<GuardError> {
    plan(ListQuery {
        entity: &q.entity,
        sorts: q.ordered_sorts(),
        node: Some(&q.filter),
        guard,
        snapshot: snap,
        each: Some(f),
        ..ListQuery::default()
    })
    .err
}

pub fn plan_query_guarded<'g>(
    q: &Query,
    count_only: bool,
    guard: Option<&'g Guard>,
) -> ListResult<'g> {
    plan(ListQuery {
        entity: &q.entity,
        offset: q.offset,
        limit: q.limit,
        sorts: q.ordered_sorts(),
        node: Some(&q.filter),
        count_only,
        guard,
        ..ListQuery::default()
    })
}

pub fn apply_query_filter(data: Vec<Map>, filter: &FilterNode) -> Vec<Map> {
    data.into_iter()
        .filter(|e| match_filter_node(filter, e))
        .collect()
}

pub fn query_filter_is_empty(filter: &FilterNode) -> bool {
    filter.leaf.as_ref().is_some_and(|l| l.is_empty())
        && filter.and.is_empty()
        && filter.or.is_empty()
}

pub fn query_filter_matches_entity(filter: &FilterNode, entity: &Map) -> bool {
    match_filter_node(filter, entity)
}

pub fn apply_stored_query_filter(data: Vec<Map>, filter: &FilterNode) -> Vec<Map> {
    data.into_iter()
        .filter(|e| match_filter_node_with(filter, e, stored_filters_match_entity))
        .collect()
}

pub(crate) fn match_filter_node(node: &FilterNode, entity: &Map) -> bool {
    match_filter_node_with(node, entity, match_leaf_strict)
}

fn match_filter_node_with(
    node: &FilterNode,
    entity: &Map,
    match_leaf: fn(&Map, &Filters) -> bool,
) -> bool {
    struct Frame<'a> {
        children: &'a [FilterNode],
        is_and: bool,
        next: usize,
    }

    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut cur = node;
    loop {
        let mut result = if let Some(leaf) = &cur.leaf {
            match_leaf(entity, leaf)
        } else if !cur.and.is_empty() {
            stack.push(Frame {
                children: &cur.and,
                is_and: true,
                next: 0,
            });
            cur = &cur.and[0];
            continue;
        } else if !cur.or.is_empty() {
            stack.push(Frame {
                children: &cur.or,
                is_and: false,
                next: 0,
            });
            cur = &cur.or[0];
            continue;
        } else {
            false
        };

        loop {
            let Some(top) = stack.last_mut() else {
                return result;
            };

            if result != top.is_and {
                stack.pop();
                continue;
            }

            top.next += 1;
            if top.next < top.children.len() {
                cur = &top.children[top.next];
                break;
            }

            result = top.is_and;
            stack.pop();
        }
    }
}
