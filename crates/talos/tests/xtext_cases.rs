mod xtext_common;

use std::collections::HashMap;

use gocompat::utf8::Rune;
use talos::deps::xtext::cases::{self, Caser};
use talos::deps::xtext::language;
use xtext_common::{Digest, go_string, hex, hex_field, int_field, read_jsonl, runes, str_field};

struct Casers {
    upper: Caser,
    lower: Caser,
    lower_nosigma: Caser,
}

fn casers() -> Casers {
    Casers {
        upper: cases::upper(language::UND, &[]),
        lower: cases::lower(language::UND, &[]),
        lower_nosigma: cases::lower(language::UND, &[cases::handle_final_sigma(false)]),
    }
}

fn check_rows(rel: &str, count: usize) {
    let rows = read_jsonl(rel);
    assert_eq!(rows.len(), count);
    let c = casers();
    let mut checked = 0;
    for row in &rows {
        let s = hex_field(row, "in");
        let ctx = hex(&s);
        assert_eq!(
            hex(&c.upper.string(&s)),
            str_field(row, "upper"),
            "upper of {ctx}"
        );
        assert_eq!(
            hex(&c.lower.string(&s)),
            str_field(row, "lower"),
            "lower of {ctx}"
        );
        assert_eq!(
            hex(&c.lower_nosigma.string(&s)),
            str_field(row, "lower_nosigma"),
            "lower without final sigma of {ctx}"
        );
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn hooks_cases_vectors() {
    let rows = read_jsonl("hooks/xtext/cases/vectors.jsonl");
    assert_eq!(rows.len(), 2939);
    let c = casers();
    let mut by_cp: HashMap<Rune, usize> = HashMap::new();
    let mut checked = 0;
    for (i, row) in rows.iter().enumerate() {
        if row.get("cp").is_some() {
            by_cp.insert(int_field(row, "cp") as Rune, i);
        } else {
            let s = hex_field(row, "in");
            assert_eq!(
                hex(&c.upper.string(&s)),
                str_field(row, "upper"),
                "upper of {}",
                hex(&s)
            );
            assert_eq!(
                hex(&c.lower.string(&s)),
                str_field(row, "lower"),
                "lower of {}",
                hex(&s)
            );
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
    let mut seen = 0;
    runes(0, 0x110000, |r| {
        let s = go_string(r);
        let (want_upper, want_lower) = match by_cp.get(&r) {
            Some(&i) => {
                seen += 1;
                (hex_field(&rows[i], "upper"), hex_field(&rows[i], "lower"))
            }
            None => (s.clone(), s.clone()),
        };
        assert_eq!(
            c.upper.string(&s).as_ref(),
            want_upper.as_slice(),
            "upper of U+{r:04X}"
        );
        assert_eq!(
            c.lower.string(&s).as_ref(),
            want_lower.as_slice(),
            "lower of U+{r:04X}"
        );
    });
    assert_eq!(seen, by_cp.len());
}

#[test]
fn xtext_cases_strings() {
    check_rows("xtext/cases/strings.jsonl", 2000);
}

#[test]
fn xtext_cases_sigma() {
    check_rows("xtext/cases/sigma.jsonl", 2752);
}

#[test]
fn xtext_cases_context() {
    let rows = read_jsonl("xtext/cases/context.jsonl");
    assert_eq!(rows.len(), 272);
    let c = casers();
    let mut checked = 0;
    for row in &rows {
        let block = int_field(row, "block") as Rune;
        let (mut dl, mut du) = (Digest::new(), Digest::new());
        runes(block << 12, (block + 1) << 12, |r| {
            let s = go_string(r);
            let variants: [Vec<u8>; 5] = [
                ["A\u{03a3}".as_bytes(), &s].concat(),
                ["A\u{03a3}'".as_bytes(), &s].concat(),
                [s.as_slice(), "\u{03a3}".as_bytes()].concat(),
                [b"A".as_slice(), &s, "\u{03a3} ".as_bytes()].concat(),
                ["\u{03a3}".as_bytes(), &s, "\u{03a3}".as_bytes()].concat(),
            ];
            for t in &variants {
                dl.add(&c.lower.string(t));
                du.add(&c.upper.string(t));
            }
        });
        assert_eq!(dl.sum(), str_field(row, "lower"), "lower block {block:#x}");
        assert_eq!(du.sum(), str_field(row, "upper"), "upper block {block:#x}");
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn xtext_cases_invalid() {
    let rows = read_jsonl("xtext/cases/invalid.jsonl");
    assert_eq!(rows.len(), 256);
    let c = casers();
    let mut checked = 0;
    for row in &rows {
        let b0 = int_field(row, "b0") as u8;
        let (mut dl, mut du, mut dn) = (Digest::new(), Digest::new(), Digest::new());
        for b1 in 0..=255u8 {
            let pair = [b0, b1];
            let variants: [Vec<u8>; 5] = [
                pair.to_vec(),
                [pair.as_slice(), b"\x80\x80"].concat(),
                ["a\u{03a3}".as_bytes(), &pair].concat(),
                ["\u{03a3}".as_bytes(), &pair, b"a"].concat(),
                ["\u{0130}".as_bytes(), &pair].concat(),
            ];
            for s in &variants {
                dl.add(&c.lower.string(s));
                du.add(&c.upper.string(s));
                dn.add(&c.lower_nosigma.string(s));
            }
        }
        assert_eq!(dl.sum(), str_field(row, "lower"), "lower b0 {b0:#x}");
        assert_eq!(du.sum(), str_field(row, "upper"), "upper b0 {b0:#x}");
        assert_eq!(
            dn.sum(),
            str_field(row, "lower_nosigma"),
            "lower_nosigma b0 {b0:#x}"
        );
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}
