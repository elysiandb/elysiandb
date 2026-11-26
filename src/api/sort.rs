use std::cmp::Ordering;

use crate::docpath::{self, SortValue};
use crate::gocompat::json::{Map, Value};
use crate::gocompat::sort as gosort;
use crate::query::Sort;

pub fn sort_entities(data: &mut [Map], sorts: &[Sort]) {
    gosort::slice_stable(data, |a, b| {
        for s in sorts {
            let va = get_sort_nested_value(a, &s.field);
            let vb = get_sort_nested_value(b, &s.field);
            if let Some(less) = sort_step(&va, &vb, s.ascending) {
                return less;
            }
        }

        false
    });
}

pub(crate) fn sort_step(
    a: &SortValue<'_, Value>,
    b: &SortValue<'_, Value>,
    ascending: bool,
) -> Option<bool> {
    let a_nil = matches!(a, SortValue::Nil);
    let b_nil = matches!(b, SortValue::Nil);
    if a_nil || b_nil {
        if a_nil && b_nil {
            return None;
        }

        return Some(b_nil);
    }

    let c = compare_sort_values(a, b);
    if c == 0 {
        return None;
    }

    if ascending {
        return Some(c < 0);
    }

    Some(c > 0)
}

pub(crate) fn compare_sort_values(a: &SortValue<'_, Value>, b: &SortValue<'_, Value>) -> i64 {
    let (rank_a, rank_b) = (sort_type_rank(a), sort_type_rank(b));
    if rank_a != rank_b {
        return rank_a - rank_b;
    }

    match (a, b) {
        (SortValue::Time(ta), SortValue::Time(tb)) => return ordering(ta.compare(tb)),
        (SortValue::Value(Value::String(sa)), SortValue::Value(Value::String(sb))) => {
            return ordering(sa.as_bytes().cmp(sb.as_bytes()));
        }
        (SortValue::Value(Value::Bool(ba)), SortValue::Value(Value::Bool(bb))) => {
            return ordering(ba.cmp(bb));
        }
        _ => {}
    }

    if let (Some(na), Some(nb)) = (sort_number(a), sort_number(b)) {
        if na < nb {
            return -1;
        }
        if na > nb {
            return 1;
        }
    }

    0
}

fn ordering(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

fn sort_type_rank(v: &SortValue<'_, Value>) -> i64 {
    if sort_number(v).is_some() {
        return 0;
    }

    match v {
        SortValue::Time(_) => 1,
        SortValue::Value(Value::String(_)) => 2,
        SortValue::Value(Value::Bool(_)) => 3,
        _ => 4,
    }
}

fn sort_number(v: &SortValue<'_, Value>) -> Option<f64> {
    match v {
        SortValue::Value(Value::Number(n)) => Some(*n),
        SortValue::Value(Value::Int(n)) => Some(*n as f64),
        _ => None,
    }
}

pub(crate) fn get_sort_nested_value<'a>(m: &'a Map, path: &[u8]) -> SortValue<'a, Value> {
    docpath::sort_map_value(m, path)
}
