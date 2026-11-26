mod common;

use elysiandb::gocompat::unicode::{self, RangeTable};
use elysiandb::gocompat::utf8::Rune;

const MAX_RUNE: Rune = 0x10FFFF;

fn line_count(rel: &str) -> usize {
    common::lines(&common::read_golden(rel)).len()
}

fn tsv_rows(rel: &str) -> Vec<Vec<String>> {
    let data = common::read_golden(rel);
    common::lines(&data)
        .into_iter()
        .map(|line| {
            std::str::from_utf8(line)
                .unwrap_or_else(|e| panic!("{rel}: {e}"))
                .split('\t')
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

fn hex_rune(s: &str) -> Rune {
    Rune::from_str_radix(s, 16).unwrap_or_else(|e| panic!("bad hex rune {s}: {e}"))
}

fn ranges_of(f: impl Fn(Rune) -> bool) -> Vec<(Rune, Rune)> {
    let mut out = Vec::new();
    let mut start = None;
    for c in 0..=MAX_RUNE + 1 {
        let v = c <= MAX_RUNE && f(c);
        match (v, start) {
            (true, None) => start = Some(c),
            (false, Some(s)) => {
                out.push((s, c - 1));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn flatten(t: &RangeTable) -> Vec<(Rune, Rune)> {
    let mut pts: Vec<(Rune, Rune)> = Vec::new();
    let mut push = |lo: Rune, hi: Rune| {
        if let Some(last) = pts.last_mut()
            && last.1 + 1 == lo
        {
            last.1 = hi;
            return;
        }
        pts.push((lo, hi));
    };
    for r in t.r16 {
        let (lo, hi, stride) = (Rune::from(r.lo), Rune::from(r.hi), Rune::from(r.stride));
        if stride == 1 {
            push(lo, hi);
            continue;
        }
        let mut c = lo;
        while c <= hi {
            push(c, c);
            c += stride;
        }
    }
    for r in t.r32 {
        let (lo, hi, stride) = (r.lo as Rune, r.hi as Rune, r.stride as Rune);
        if stride == 1 {
            push(lo, hi);
            continue;
        }
        let mut c = lo;
        while c <= hi {
            push(c, c);
            c += stride;
        }
    }
    pts
}

struct Group {
    name: String,
    ranges: Vec<(Rune, Rune)>,
}

fn range_groups(rel: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for row in tsv_rows(rel) {
        assert_eq!(row.len(), 3, "{rel}: {row:?}");
        let r = (hex_rune(&row[1]), hex_rune(&row[2]));
        match groups.last_mut() {
            Some(g) if g.name == row[0] => g.ranges.push(r),
            _ => groups.push(Group {
                name: row[0].clone(),
                ranges: vec![r],
            }),
        }
    }
    groups
}

fn check_ranges(rel: &str, name: &str, want: &[(Rune, Rune)], got: &[(Rune, Rune)]) -> usize {
    assert_eq!(got.len(), want.len(), "{rel}: {name}: number of ranges");
    let mut checked = 0;
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        assert_eq!(w, g, "{rel}: {name}: range {i}");
        checked += 1;
    }
    checked
}

fn check_tables(rel: &str, tables: &[(&str, &RangeTable)]) {
    let groups = range_groups(rel);
    let file_names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    let names: Vec<&str> = tables.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, file_names, "{rel}: table names");
    let mut checked = 0;
    for (group, (name, table)) in groups.iter().zip(tables) {
        assert_eq!(
            table.latin_offset,
            table.r16.iter().filter(|r| r.hi <= 0xFF).count(),
            "{rel}: {name}: latin offset"
        );
        assert_eq!(flatten(table), group.ranges, "{rel}: {name}: table data");
        let got = ranges_of(|c| unicode::is(table, c));
        checked += check_ranges(rel, name, &group.ranges, &got);
    }
    assert_eq!(checked, line_count(rel), "{rel}: rows checked");
}

#[test]
fn version() {
    let v = common::read_json("gocompat/unicode/version.json");
    let object = v.as_object().expect("version object");
    assert_eq!(object.len(), 1);
    assert_eq!(common::str_field(&v, "unicode"), unicode::VERSION);
}

#[test]
fn case_mappings() {
    let rel = "gocompat/unicode/case.tsv";
    let rows = tsv_rows(rel);
    let mut listed = Vec::with_capacity(rows.len());
    let mut checked = 0;
    for row in &rows {
        assert_eq!(row.len(), 5, "{rel}: {row:?}");
        let c = hex_rune(&row[0]);
        assert_eq!(
            unicode::to_lower(c),
            hex_rune(&row[1]),
            "{rel}: ToLower {c:04x}"
        );
        assert_eq!(
            unicode::to_upper(c),
            hex_rune(&row[2]),
            "{rel}: ToUpper {c:04x}"
        );
        assert_eq!(
            unicode::to_title(c),
            hex_rune(&row[3]),
            "{rel}: ToTitle {c:04x}"
        );
        assert_eq!(
            unicode::simple_fold(c),
            hex_rune(&row[4]),
            "{rel}: SimpleFold {c:04x}"
        );
        listed.push(c);
        checked += 1;
    }
    let mapped: Vec<Rune> = (0..=MAX_RUNE)
        .filter(|&c| {
            unicode::to_lower(c) != c
                || unicode::to_upper(c) != c
                || unicode::to_title(c) != c
                || unicode::simple_fold(c) != c
        })
        .collect();
    assert_eq!(mapped, listed, "{rel}: runes with a mapping");
    assert_eq!(checked, line_count(rel), "{rel}: rows checked");
}

#[test]
fn case_ranges() {
    let rel = "gocompat/unicode/case_ranges.tsv";
    let rows = tsv_rows(rel);
    assert_eq!(
        rows.len(),
        unicode::CASE_RANGES.len(),
        "{rel}: number of case ranges"
    );
    let mut checked = 0;
    for (i, (row, cr)) in rows.iter().zip(unicode::CASE_RANGES).enumerate() {
        assert_eq!(row.len(), 5, "{rel}: {row:?}");
        assert_eq!(cr.lo as Rune, hex_rune(&row[0]), "{rel}: row {i} lo");
        assert_eq!(cr.hi as Rune, hex_rune(&row[1]), "{rel}: row {i} hi");
        for (k, delta) in cr.delta.iter().enumerate() {
            let want: Rune = row[2 + k].parse().expect("decimal delta");
            assert_eq!(*delta, want, "{rel}: row {i} delta {k}");
        }
        checked += 1;
    }
    assert_eq!(checked, line_count(rel), "{rel}: rows checked");
}

#[test]
fn predicates() {
    let rel = "gocompat/unicode/predicates.tsv";
    let groups = range_groups(rel);
    let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "IsControl",
            "IsDigit",
            "IsGraphic",
            "IsLetter",
            "IsLower",
            "IsMark",
            "IsNumber",
            "IsPrint",
            "IsPunct",
            "IsSpace",
            "IsSymbol",
            "IsTitle",
            "IsUpper",
            "strconv.IsGraphic",
            "strconv.IsPrint",
        ],
        "{rel}: predicate names"
    );
    let mut checked = 0;
    for group in &groups {
        let f: fn(Rune) -> bool = match group.name.as_str() {
            "IsControl" => unicode::is_control,
            "IsDigit" => unicode::is_digit,
            "IsGraphic" | "strconv.IsGraphic" => unicode::is_graphic,
            "IsLetter" => unicode::is_letter,
            "IsLower" => unicode::is_lower,
            "IsMark" => unicode::is_mark,
            "IsNumber" => unicode::is_number,
            "IsPrint" | "strconv.IsPrint" => unicode::is_print,
            "IsPunct" => unicode::is_punct,
            "IsSpace" => unicode::is_space,
            "IsSymbol" => unicode::is_symbol,
            "IsTitle" => unicode::is_title,
            "IsUpper" => unicode::is_upper,
            other => panic!("{rel}: unknown predicate {other}"),
        };
        checked += check_ranges(rel, &group.name, &group.ranges, &ranges_of(f));
    }
    assert_eq!(checked, line_count(rel), "{rel}: rows checked");
}

#[test]
fn categories() {
    check_tables("gocompat/unicode/categories.tsv", unicode::CATEGORIES);
}

#[test]
fn scripts() {
    check_tables("gocompat/unicode/scripts.tsv", unicode::SCRIPTS);
}

#[test]
fn properties() {
    check_tables("gocompat/unicode/properties.tsv", unicode::PROPERTIES);
}

#[test]
fn fold_category() {
    check_tables("gocompat/unicode/fold_category.tsv", unicode::FOLD_CATEGORY);
}

#[test]
fn fold_script() {
    check_tables("gocompat/unicode/fold_script.tsv", unicode::FOLD_SCRIPT);
}
