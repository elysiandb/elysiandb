use std::error::Error as StdError;
use std::fmt;

use elysiandb::gocompat::errors::{self, BoxError, Error, Joined, Wrapped};

#[derive(Debug, PartialEq)]
struct Code(u32);

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "code {}", self.0)
    }
}

impl StdError for Code {}

fn boxed(e: impl StdError + Send + Sync + 'static) -> BoxError {
    Box::new(e)
}

#[test]
fn test_new_keeps_the_message() {
    let e = Error::new("storage write failed");
    assert_eq!(e.to_string(), "storage write failed");
    assert_eq!(e.message(), "storage write failed");
    assert_eq!(errors::new("x").to_string(), "x");
}

#[test]
fn test_join_without_errors_is_none() {
    assert!(errors::join(Vec::<Option<BoxError>>::new()).is_none());
    assert!(errors::join(vec![None, None]).is_none());
}

#[test]
fn test_join_skips_missing_errors() {
    let j = errors::join(vec![None, Some(errors::new("a")), None]).unwrap();
    assert_eq!(j.errors().len(), 1);
    assert_eq!(j.to_string(), "a");
}

#[test]
fn test_join_separates_messages_with_newlines() {
    let j = errors::join(vec![
        Some(errors::new("a")),
        Some(errors::new("b")),
        Some(errors::new("c")),
    ])
    .unwrap();
    assert_eq!(j.to_string(), "a\nb\nc");
    assert_eq!(j.into_errors().len(), 3);
}

#[test]
fn test_join_of_joins_nests_messages() {
    let inner = errors::join(vec![Some(errors::new("a")), Some(errors::new("b"))]).unwrap();
    let outer = errors::join(vec![Some(boxed(inner)), Some(errors::new("c"))]).unwrap();
    assert_eq!(outer.to_string(), "a\nb\nc");
}

#[test]
fn test_wrap_prefixes_the_message() {
    let w = errors::wrap("cannot write the manifest: ", errors::new("disk full"));
    assert_eq!(w.to_string(), "cannot write the manifest: disk full");
    let inner = errors::unwrap(w.as_ref()).unwrap();
    assert_eq!(inner.to_string(), "disk full");
}

#[test]
fn test_wrapped_exposes_its_source() {
    let w = Wrapped::new("outer", boxed(Code(7)));
    assert_eq!(w.to_string(), "outer");
    assert_eq!(w.inner().to_string(), "code 7");
    assert_eq!(w.source().unwrap().to_string(), "code 7");
    assert_eq!(w.into_inner().to_string(), "code 7");
}

#[test]
fn test_unwrap_of_a_join_is_none() {
    let j = errors::join(vec![Some(errors::new("a"))]).unwrap();
    assert!(errors::unwrap(&j).is_none());
    assert!(errors::unwrap(&Error::new("plain")).is_none());
}

#[test]
fn test_find_walks_wrap_chains() {
    let w = errors::wrap("a: ", errors::wrap("b: ", boxed(Code(3))));
    assert_eq!(errors::find::<Code>(w.as_ref()), Some(&Code(3)));
    assert!(errors::find::<Joined>(w.as_ref()).is_none());
}

#[test]
fn test_find_walks_join_children_in_order() {
    let j = errors::join(vec![
        Some(errors::new("a")),
        Some(errors::wrap("x: ", boxed(Code(1)))),
        Some(boxed(Code(2))),
    ])
    .unwrap();
    assert_eq!(errors::find::<Code>(&j), Some(&Code(1)));
}

#[test]
fn test_find_matches_the_error_itself_first() {
    let j = errors::join(vec![Some(errors::new("a"))]).unwrap();
    assert!(errors::find::<Joined>(&j).is_some());
}

#[test]
fn test_is_compares_with_the_target() {
    let j = errors::join(vec![
        Some(errors::new("a")),
        Some(errors::wrap("x: ", boxed(Code(2)))),
    ])
    .unwrap();
    assert!(errors::is(&j, &Code(2)));
    assert!(!errors::is(&j, &Code(3)));
}

#[test]
fn test_any_stops_at_the_first_match() {
    let j = errors::join(vec![
        Some(errors::new("a")),
        Some(errors::new("b")),
        Some(errors::new("c")),
    ])
    .unwrap();
    let mut seen = Vec::new();
    let hit = errors::any(&j, &mut |e| {
        seen.push(e.to_string());
        e.to_string() == "b"
    });
    assert!(hit);
    assert_eq!(seen, vec!["a\nb\nc", "a", "b"]);
}
