mod regexp_common;

use regexp_common::{FindTest, StringReader, find_tests};
use talos::deps::regexp::must_compile;

fn name(test: &FindTest) -> String {
    format!(
        "pat: {:?} text: {:?}",
        String::from_utf8_lossy(test.pat),
        String::from_utf8_lossy(test.text)
    )
}

fn slice(test: &FindTest, a: isize, b: isize) -> &'static [u8] {
    &test.text[a as usize..b as usize]
}

#[test]
fn test_find() {
    for test in find_tests() {
        let re = must_compile(test.pat);
        assert_eq!(re.string(), test.pat, "String() for {}", name(&test));
        let result = re.find(test.text);
        match (&test.matches, result) {
            (m, r) if m.as_ref().is_none_or(|m| m.is_empty()) && r.is_none_or(|r| r.is_empty()) => {
            }
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => {
                assert_eq!(r, slice(&test, m[0][0], m[0][1]), "{}", name(&test));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn test_find_string() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_string(test.text);
        match &test.matches {
            m if m.as_ref().is_none_or(|m| m.is_empty()) && result.is_empty() => {}
            None => assert!(
                result.is_empty(),
                "expected no match; got one: {}",
                name(&test)
            ),
            Some(m) if result.is_empty() => {
                assert_eq!(
                    m[0][0],
                    m[0][1],
                    "expected match; got none: {}",
                    name(&test)
                );
            }
            Some(m) => assert_eq!(result, slice(&test, m[0][0], m[0][1]), "{}", name(&test)),
        }
    }
}

fn check_find_index(test: &FindTest, result: Option<Vec<isize>>) {
    match (&test.matches, result) {
        (m, r)
            if m.as_ref().is_none_or(|m| m.is_empty())
                && r.as_ref().is_none_or(|r| r.is_empty()) => {}
        (None, Some(_)) => panic!("expected no match; got one: {}", name(test)),
        (Some(_), None) => panic!("expected match; got none: {}", name(test)),
        (Some(m), Some(r)) => {
            assert_eq!((m[0][0], m[0][1]), (r[0], r[1]), "{}", name(test));
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_find_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_index(test.text);
        check_find_index(&test, r);
    }
}

#[test]
fn test_find_string_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_string_index(test.text);
        check_find_index(&test, r);
    }
}

#[test]
fn test_find_reader_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_reader_index(&mut StringReader::new(test.text));
        check_find_index(&test, r);
    }
}

#[test]
fn test_find_all() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_all(test.text, -1);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => {
                assert_eq!(m.len(), r.len(), "{}", name(&test));
                for (k, e) in m.iter().enumerate() {
                    assert_eq!(r[k], slice(&test, e[0], e[1]), "match {k}: {}", name(&test));
                }
            }
        }
    }
}

#[test]
fn test_find_all_string() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_all_string(test.text, -1);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => {
                assert_eq!(m.len(), r.len(), "{}", name(&test));
                for (k, e) in m.iter().enumerate() {
                    assert_eq!(r[k], slice(&test, e[0], e[1]), "{}", name(&test));
                }
            }
        }
    }
}

fn check_find_all_index(test: &FindTest, result: Option<Vec<Vec<isize>>>) {
    match (&test.matches, result) {
        (None, None) => {}
        (None, Some(_)) => panic!("expected no match; got one: {}", name(test)),
        (Some(_), None) => panic!("expected match; got none: {}", name(test)),
        (Some(m), Some(r)) => {
            assert_eq!(m.len(), r.len(), "{}", name(test));
            for (k, e) in m.iter().enumerate() {
                assert_eq!(
                    (e[0], e[1]),
                    (r[k][0], r[k][1]),
                    "match {k}: {}",
                    name(test)
                );
            }
        }
    }
}

#[test]
fn test_find_all_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_all_index(test.text, -1);
        check_find_all_index(&test, r);
    }
}

#[test]
fn test_find_all_string_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_all_string_index(test.text, -1);
        check_find_all_index(&test, r);
    }
}

fn check_submatch_bytes(test: &FindTest, n: usize, submatches: &[isize], result: &[Option<&[u8]>]) {
    assert_eq!(
        submatches.len(),
        result.len() * 2,
        "match {n}: {}",
        name(test)
    );
    for k in (0..submatches.len()).step_by(2) {
        if submatches[k] == -1 {
            assert!(
                result[k / 2].is_none(),
                "match {n}: expected nil: {}",
                name(test)
            );
            continue;
        }
        let got =
            result[k / 2].unwrap_or_else(|| panic!("match {n}: missing submatch: {}", name(test)));
        assert_eq!(
            got,
            slice(test, submatches[k], submatches[k + 1]),
            "match {n}: {}",
            name(test)
        );
    }
}

#[test]
fn test_find_submatch() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_submatch(test.text);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => check_submatch_bytes(&test, 0, &m[0], &r),
        }
    }
}

fn check_submatch_string(test: &FindTest, n: usize, submatches: &[isize], result: &[&[u8]]) {
    assert_eq!(
        submatches.len(),
        result.len() * 2,
        "match {n}: {}",
        name(test)
    );
    for k in (0..submatches.len()).step_by(2) {
        if submatches[k] == -1 {
            assert!(
                result[k / 2].is_empty(),
                "match {n}: expected nil: {}",
                name(test)
            );
            continue;
        }
        assert_eq!(
            result[k / 2],
            slice(test, submatches[k], submatches[k + 1]),
            "match {n}: {}",
            name(test)
        );
    }
}

#[test]
fn test_find_string_submatch() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_string_submatch(test.text);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => check_submatch_string(&test, 0, &m[0], &r),
        }
    }
}

fn check_submatch_indices(test: &FindTest, n: usize, expect: &[isize], result: &[isize]) {
    assert_eq!(expect.len(), result.len(), "match {n}: {}", name(test));
    assert_eq!(expect, result, "match {n}: submatch error: {}", name(test));
}

fn check_find_submatch_index(test: &FindTest, result: Option<Vec<isize>>) {
    match (&test.matches, result) {
        (None, None) => {}
        (None, Some(_)) => panic!("expected no match; got one: {}", name(test)),
        (Some(_), None) => panic!("expected match; got none: {}", name(test)),
        (Some(m), Some(r)) => check_submatch_indices(test, 0, &m[0], &r),
    }
}

#[test]
fn test_find_submatch_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_submatch_index(test.text);
        check_find_submatch_index(&test, r);
    }
}

#[test]
fn test_find_string_submatch_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_string_submatch_index(test.text);
        check_find_submatch_index(&test, r);
    }
}

#[test]
fn test_find_reader_submatch_index() {
    for test in find_tests() {
        let r =
            must_compile(test.pat).find_reader_submatch_index(&mut StringReader::new(test.text));
        check_find_submatch_index(&test, r);
    }
}

#[test]
fn test_find_all_submatch() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_all_submatch(test.text, -1);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => {
                assert_eq!(m.len(), r.len(), "{}", name(&test));
                for (k, mm) in m.iter().enumerate() {
                    check_submatch_bytes(&test, k, mm, &r[k]);
                }
            }
        }
    }
}

#[test]
fn test_find_all_string_submatch() {
    for test in find_tests() {
        let result = must_compile(test.pat).find_all_string_submatch(test.text, -1);
        match (&test.matches, result) {
            (None, None) => {}
            (None, Some(_)) => panic!("expected no match; got one: {}", name(&test)),
            (Some(_), None) => panic!("expected match; got none: {}", name(&test)),
            (Some(m), Some(r)) => {
                assert_eq!(m.len(), r.len(), "{}", name(&test));
                for (k, mm) in m.iter().enumerate() {
                    check_submatch_string(&test, k, mm, &r[k]);
                }
            }
        }
    }
}

fn check_find_all_submatch_index(test: &FindTest, result: Option<Vec<Vec<isize>>>) {
    match (&test.matches, result) {
        (None, None) => {}
        (None, Some(_)) => panic!("expected no match; got one: {}", name(test)),
        (Some(_), None) => panic!("expected match; got none: {}", name(test)),
        (Some(m), Some(r)) => {
            assert_eq!(m.len(), r.len(), "{}", name(test));
            for (k, mm) in m.iter().enumerate() {
                check_submatch_indices(test, k, mm, &r[k]);
            }
        }
    }
}

#[test]
fn test_find_all_submatch_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_all_submatch_index(test.text, -1);
        check_find_all_submatch_index(&test, r);
    }
}

#[test]
fn test_find_all_string_submatch_index() {
    for test in find_tests() {
        let r = must_compile(test.pat).find_all_string_submatch_index(test.text, -1);
        check_find_all_submatch_index(&test, r);
    }
}
