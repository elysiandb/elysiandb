mod xtext_common;

use gocompat::utf8::Rune;
use talos::deps::xtext::collate::{Buffer, Collator};
use talos::deps::xtext::language;
use talos::deps::xtext::norm::Form;
use xtext_common::{Digest, go_string, hex, hex_field, int_field, read_jsonl, runes, str_field};

fn collator() -> Collator {
    Collator::new(language::UND, &[])
}

fn key(c: &mut Collator, buf: &mut Buffer, s: &[u8]) -> Vec<u8> {
    buf.reset();
    c.key_from_string(buf, s).to_vec()
}

fn nfd(s: &[u8]) -> Vec<u8> {
    Form::Nfd.string(s).into_owned()
}

#[test]
fn hooks_collate_vectors() {
    let rows = read_jsonl("hooks/xtext/collate/vectors.jsonl");
    assert_eq!(rows.len(), 2759);
    let mut c = collator();
    let mut buf = Buffer::default();
    let mut checked = 0;
    for row in &rows {
        if row.get("key_nfd").is_some() {
            let s = hex_field(row, "in");
            assert_eq!(
                hex(&key(&mut c, &mut buf, &nfd(&s))),
                str_field(row, "key_nfd"),
                "key of {}",
                hex(&s)
            );
        } else {
            let a = nfd(&hex_field(row, "a"));
            let b = nfd(&hex_field(row, "b"));
            assert_eq!(
                i64::from(c.compare_string(&a, &b)),
                int_field(row, "compare_nfd"),
                "compare {} {}",
                hex(&a),
                hex(&b)
            );
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn xtext_collate_strings() {
    let rows = read_jsonl("xtext/collate/strings.jsonl");
    assert_eq!(rows.len(), 2000);
    let mut c = collator();
    let mut buf = Buffer::default();
    let mut prev: Vec<u8> = Vec::new();
    let mut checked = 0;
    for row in &rows {
        let s = hex_field(row, "in");
        let ctx = hex(&s);
        let n = nfd(&s);
        assert_eq!(
            hex(&key(&mut c, &mut buf, &s)),
            str_field(row, "key"),
            "key of {ctx}"
        );
        assert_eq!(
            hex(&key(&mut c, &mut buf, &n)),
            str_field(row, "key_nfd"),
            "key_nfd of {ctx}"
        );
        assert_eq!(
            i64::from(c.compare_string(&prev, &s)),
            int_field(row, "compare"),
            "compare of {ctx}"
        );
        assert_eq!(
            i64::from(c.compare_string(&nfd(&prev), &n)),
            int_field(row, "compare_nfd"),
            "compare_nfd of {ctx}"
        );
        assert_eq!(c.compare(&prev, &s), c.compare_string(&prev, &s));
        prev = s;
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn xtext_collate_runes() {
    let rows = read_jsonl("xtext/collate/runes.jsonl");
    assert_eq!(rows.len(), 272);
    let mut c = collator();
    let mut buf = Buffer::default();
    let mut checked = 0;
    for row in &rows {
        let block = int_field(row, "block") as Rune;
        let (mut dk, mut dn, mut dc, mut dcn) =
            (Digest::new(), Digest::new(), Digest::new(), Digest::new());
        runes(block << 12, (block + 1) << 12, |ch| {
            let s = go_string(ch);
            let t = go_string(ch + 1);
            dk.add(&key(&mut c, &mut buf, &s));
            dn.add(&key(&mut c, &mut buf, &nfd(&s)));
            dc.add_int(c.compare_string(&s, &t));
            dcn.add_int(c.compare_string(&nfd(&s), &nfd(&t)));
        });
        assert_eq!(dk.sum(), str_field(row, "key"), "key block {block:#x}");
        assert_eq!(
            dn.sum(),
            str_field(row, "key_nfd"),
            "key_nfd block {block:#x}"
        );
        assert_eq!(
            dc.sum(),
            str_field(row, "compare"),
            "compare block {block:#x}"
        );
        assert_eq!(
            dcn.sum(),
            str_field(row, "compare_nfd"),
            "compare_nfd block {block:#x}"
        );
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

const COLLATE_MARKS: [&str; 32] = [
    "\u{0300}",
    "\u{0301}",
    "\u{0306}",
    "\u{0308}",
    "\u{0323}",
    "\u{0327}",
    "\u{0334}",
    "\u{0653}",
    "\u{0654}",
    "\u{0655}",
    "\u{05b0}",
    "\u{0e38}",
    "\u{0e48}",
    "\u{0f71}",
    "\u{0f72}",
    "\u{0f80}",
    "\u{102e}",
    "\u{1b35}",
    "\u{3099}",
    "\u{11127}",
    "\u{110ba}",
    "\u{1133e}",
    "\u{114b0}",
    "\u{115af}",
    "\u{11930}",
    "\u{0323}\u{0306}",
    "\u{0306}\u{0323}",
    "\u{0301}\u{0306}",
    "\u{0334}\u{0308}",
    "\u{0f71}\u{0f72}",
    "\u{0f72}\u{0f71}",
    "\u{0f80}\u{0f71}",
];

#[test]
fn xtext_collate_marks() {
    let rows = read_jsonl("xtext/collate/marks.jsonl");
    assert_eq!(rows.len(), 128);
    let mut c = collator();
    let mut buf = Buffer::default();
    let mut checked = 0;
    for row in &rows {
        let block = int_field(row, "block") as Rune;
        let m = int_field(row, "mark") as usize;
        let mark = COLLATE_MARKS[m].as_bytes();
        let (lo, hi) = if block == 3 {
            (0x10000, 0x20000)
        } else {
            (block << 12, (block + 1) << 12)
        };
        let (mut dk, mut dn, mut dc, mut dcn) =
            (Digest::new(), Digest::new(), Digest::new(), Digest::new());
        runes(lo, hi, |ch| {
            let s = [go_string(ch).as_slice(), mark].concat();
            let t = [go_string(ch + 1).as_slice(), mark].concat();
            dk.add(&key(&mut c, &mut buf, &s));
            dn.add(&key(&mut c, &mut buf, &nfd(&s)));
            dc.add_int(c.compare_string(&s, &t));
            dcn.add_int(c.compare_string(&nfd(&s), &go_string(ch)));
        });
        assert_eq!(
            dk.sum(),
            str_field(row, "key"),
            "key block {block} mark {m}"
        );
        assert_eq!(
            dn.sum(),
            str_field(row, "key_nfd"),
            "key_nfd block {block} mark {m}"
        );
        assert_eq!(
            dc.sum(),
            str_field(row, "compare"),
            "compare block {block} mark {m}"
        );
        assert_eq!(
            dcn.sum(),
            str_field(row, "compare_nfd"),
            "compare_nfd block {block} mark {m}"
        );
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn xtext_collate_invalid() {
    let rows = read_jsonl("xtext/collate/invalid.jsonl");
    assert_eq!(rows.len(), 256);
    let mut c = collator();
    let mut buf = Buffer::default();
    let mut checked = 0;
    for row in &rows {
        let b0 = int_field(row, "b0") as u8;
        let (mut dk, mut dc) = (Digest::new(), Digest::new());
        for b1 in 0..=255u8 {
            let pair = [b0, b1];
            let pa = [pair.as_slice(), b"a"].concat();
            let variants: [Vec<u8>; 4] = [
                pair.to_vec(),
                [pair.as_slice(), b"\x80"].concat(),
                [b"a".as_slice(), &pair, "\u{0301}".as_bytes()].concat(),
                ["\u{0438}".as_bytes(), &pair, "\u{0306}".as_bytes()].concat(),
            ];
            for s in &variants {
                dk.add(&key(&mut c, &mut buf, s));
                dc.add_int(c.compare_string(s, &pa));
            }
        }
        assert_eq!(dk.sum(), str_field(row, "key"), "key b0 {b0:#x}");
        assert_eq!(dc.sum(), str_field(row, "compare"), "compare b0 {b0:#x}");
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}
