use super::lower;
use crate::deps::xtext::language::UND;

#[test]
fn example() {
    let src = [
        "hello world!",
        "i with dot",
        "'n ijsberg",
        "here comes O'Brian",
    ];
    let want = [
        "hello world!",
        "i with dot",
        "'n ijsberg",
        "here comes o'brian",
    ];
    let c = lower(UND, &[]);
    for (s, w) in src.iter().zip(want) {
        assert_eq!(c.string(s.as_bytes()).as_ref(), w.as_bytes());
    }
}
