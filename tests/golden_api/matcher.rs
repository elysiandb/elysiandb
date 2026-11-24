use elysiandb::api;

use super::common::{read_jsonl, str_field};
use super::{array, boolean, go_decode, go_filters, go_map, integer, text};

#[test]
fn matcher_corpus() {
    let rows = read_jsonl("api/matcher_corpus.jsonl");
    let mut checked = 0;
    for row in &rows {
        let raw = str_field(row, "raw");
        let doc = go_map(raw.as_bytes());
        let di = integer(row, "doc");

        for case in array(row, "filters") {
            let Some([fs, strict, stored]) = case.as_array().map(Vec::as_slice) else {
                panic!("bad filter case {case:?}");
            };
            let filters = go_filters(&go_decode(text(fs).as_bytes()));
            let got = api::filters_match_entity(&doc, &filters);
            if got != boolean(strict) {
                panic!("doc {di} filter {}: FiltersMatchEntity {got}", text(fs));
            }
            let got = api::stored_filters_match_entity(&doc, &filters);
            if got != boolean(stored) {
                panic!(
                    "doc {di} filter {}: StoredFiltersMatchEntity {got}",
                    text(fs)
                );
            }
        }

        for case in array(row, "search") {
            let Some([p, entity, matched, ok]) = case.as_array().map(Vec::as_slice) else {
                panic!("bad search case {case:?}");
            };
            let pattern = text(p).as_bytes();
            let got = api::search_matches_entity(&doc, pattern);
            if got != boolean(entity) {
                panic!("doc {di} search {:?}: SearchMatchesEntity {got}", text(p));
            }
            let (got_matched, got_ok) = api::search_matches_bytes(raw.as_bytes(), pattern);
            if got_matched != boolean(matched) || got_ok != boolean(ok) {
                panic!(
                    "doc {di} search {:?}: SearchMatchesBytes ({got_matched}, {got_ok})",
                    text(p)
                );
            }
        }

        checked += 1;
    }

    assert_eq!(checked, 206);
    assert_eq!(checked, rows.len());
}
