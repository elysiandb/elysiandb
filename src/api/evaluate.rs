use crate::datastore::{
    Bitset, IndexView, KEY_BOOL, KEY_DATE, KEY_ELEMENT, KEY_NUMBER, KEY_PRESENT, KEY_RAW,
    append_time,
};
use crate::gocompat::time::Time;
use crate::query::Ops;

use super::filter::{day, match_boolean, match_number, match_string_or_date, parse_date};
use super::plan::{Candidates, collect, kind_bounds, unescape_payload};

const EVAL_MIN_PENDING: i64 = 64;

fn kind_count(view: &IndexView, kind: u8) -> i64 {
    view.rank(&[kind + 1]) - view.rank(&[kind])
}

fn pending_count(view: &IndexView, c: &Candidates) -> i64 {
    if c.all {
        return kind_count(view, KEY_PRESENT);
    }

    let mut n = c.set.count() as i64;
    if let Some(exact) = &c.exact {
        n -= exact.count() as i64;
    }

    n
}

pub(crate) fn evaluate_leaf(
    view: &IndexView,
    ops: &Ops,
    current: &Candidates,
) -> Option<Candidates> {
    if ops.is_empty() || pending_count(view, current) < EVAL_MIN_PENDING {
        return None;
    }

    if let Some(c) = date_range_on_index(view, ops) {
        return Some(c);
    }

    let scalars =
        kind_count(view, KEY_RAW) + kind_count(view, KEY_NUMBER) + kind_count(view, KEY_BOOL);
    if pending_count(view, current) * 4 < scalars {
        return None;
    }

    let mut exact = Bitset::default();
    scan_kind(view, KEY_RAW, |key, seq| {
        if !exact.has(seq) && match_string_or_date(&unescape_payload(key), ops) {
            exact.set(seq);
        }
    });
    scan_kind(view, KEY_NUMBER, |key, seq| {
        if !exact.has(seq) && match_number(number_of_key(key), ops) {
            exact.set(seq);
        }
    });
    scan_kind(view, KEY_BOOL, |key, seq| {
        if !exact.has(seq) && match_boolean(key[1] == 1, ops) {
            exact.set(seq);
        }
    });

    let mut set = exact.clone();
    if arrays_may_match(ops) {
        let (mut lo, mut hi) = kind_bounds(KEY_ELEMENT);
        if empty_arrays_may_match(ops) {
            (lo, hi) = kind_bounds(KEY_PRESENT);
        }
        collect(view, &lo, &hi, &mut set, None);
    }

    Some(Candidates {
        all: false,
        set,
        exact: Some(exact),
    })
}

fn scan_kind(view: &IndexView, kind: u8, mut f: impl FnMut(&[u8], u32)) {
    let (lo, hi) = kind_bounds(kind);
    view.scan_range(Some(&lo), Some(&hi), |key, seq| {
        f(key, seq);
        true
    });
}

pub(crate) fn number_of_key(key: &[u8]) -> f64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&key[1..9]);
    let mut bits = u64::from_be_bytes(b);
    if bits & (1 << 63) != 0 {
        bits &= !(1 << 63);
    } else {
        bits = !bits;
    }

    f64::from_bits(bits)
}

pub(crate) fn time_of_key(key: &[u8]) -> Time {
    let mut s = [0u8; 8];
    s.copy_from_slice(&key[1..9]);
    let mut n = [0u8; 4];
    n.copy_from_slice(&key[9..13]);
    let sec = (u64::from_be_bytes(s) ^ (1 << 63)) as i64;

    Time::from_unix(sec, i64::from(u32::from_be_bytes(n)))
}

fn arrays_may_match(ops: &Ops) -> bool {
    ops.keys().all(|op| {
        matches!(
            op.as_slice(),
            b"contains" | b"not_contains" | b"all" | b"any" | b"eq" | b"none"
        )
    })
}

fn empty_arrays_may_match(ops: &Ops) -> bool {
    ops.keys()
        .all(|op| op.as_slice() == b"not_contains" || op.as_slice() == b"none")
}

fn date_range_on_index(view: &IndexView, ops: &Ops) -> Option<Candidates> {
    let mut lo: Option<Time> = None;
    let mut hi: Option<Time> = None;
    for (op, cmp) in ops {
        let (mut t, date_only) = parse_date(cmp)?;
        if !date_only {
            return None;
        }

        match op.as_slice() {
            b"gte" | b"gt" => {
                if op.as_slice() == b"gt" {
                    t = t.add(day());
                }
                if lo.as_ref().is_none_or(|l| t.after(l)) {
                    lo = Some(t);
                }
            }
            b"lt" | b"lte" => {
                if op.as_slice() == b"lte" {
                    t = t.add(day());
                }
                if hi.as_ref().is_none_or(|h| t.before(h)) {
                    hi = Some(t);
                }
            }
            _ => return None,
        }
    }

    if kind_count(view, KEY_DATE) != kind_count(view, KEY_RAW) {
        return None;
    }

    let (mut l, mut h) = kind_bounds(KEY_DATE);
    let mut earliest = true;
    let zero = Time::default();
    view.scan_range(Some(&l), Some(&h), |key, _| {
        earliest = !time_of_key(key).before(&zero);
        false
    });
    if !earliest {
        return None;
    }

    if let Some(lo) = &lo {
        l = vec![KEY_DATE];
        append_time(&mut l, lo);
    }
    if let Some(hi) = &hi {
        h = vec![KEY_DATE];
        append_time(&mut h, hi);
    }

    let mut exact = Bitset::default();
    let open = match (&lo, &hi) {
        (Some(lo), Some(hi)) => lo.before(hi),
        _ => true,
    };
    if open {
        collect(view, &l, &h, &mut exact, None);
    }

    Some(Candidates {
        all: false,
        set: exact.clone(),
        exact: Some(exact),
    })
}
