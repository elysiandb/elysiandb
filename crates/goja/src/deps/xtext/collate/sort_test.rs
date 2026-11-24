use super::Collator;
use crate::deps::xtext::language;

#[test]
fn example_collator_strings() {
    let mut c = Collator::new(language::UND, &[]);
    let mut strings = vec!["ad", "ab", "\u{e4}b", "ac"];
    c.sort_strings(&mut strings);
    assert_eq!(strings, ["ab", "\u{e4}b", "ac", "ad"]);
}
