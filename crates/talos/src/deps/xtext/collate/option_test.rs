use super::collate_test::ignore;
use super::option::{FORCE, IGNORE_CASE, IGNORE_DIACRITICS, IGNORE_WIDTH, LOOSE, Option, Options};
use crate::deps::xtext::colltab::Level;

#[test]
fn test_options() {
    let default_ignore = ignore(Level::Tertiary as i32);
    let ignore_diacritics = IGNORE_DIACRITICS;
    let opts = |ignore, case_level| Options {
        ignore,
        case_level,
        ..Options::default()
    };
    let tests: Vec<(Vec<Option>, Options)> = vec![
        (vec![], opts(default_ignore, false)),
        (
            vec![IGNORE_DIACRITICS],
            opts([false, true, false, true, true], false),
        ),
        (
            vec![IGNORE_CASE, IGNORE_DIACRITICS],
            opts(ignore(Level::Primary as i32), false),
        ),
        (
            vec![ignore_diacritics, IGNORE_WIDTH],
            opts(ignore(Level::Primary as i32), true),
        ),
        (
            vec![IGNORE_WIDTH, ignore_diacritics],
            opts(ignore(Level::Primary as i32), true),
        ),
        (
            vec![IGNORE_CASE, IGNORE_WIDTH],
            opts(ignore(Level::Secondary as i32), false),
        ),
        (
            vec![IGNORE_CASE, IGNORE_WIDTH, LOOSE],
            opts(ignore(Level::Primary as i32), false),
        ),
        (
            vec![FORCE, IGNORE_CASE, IGNORE_WIDTH, LOOSE],
            opts([false, true, true, true, false], false),
        ),
        (
            vec![IGNORE_DIACRITICS, IGNORE_CASE],
            opts(ignore(Level::Primary as i32), false),
        ),
    ];
    for (i, (input, out)) in tests.iter().enumerate() {
        let mut c = Options::new();
        c.set_options(input);
        assert_eq!(c, *out, "{i}");
    }
}
