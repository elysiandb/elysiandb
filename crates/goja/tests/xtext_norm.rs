mod xtext_common;

use std::collections::HashMap;

use gocompat::utf8::Rune;
use goja::deps::xtext::norm::transform::Error;
use goja::deps::xtext::norm::{Form, Iter};
use xtext_common::{
    Digest, array_field, bools, go_string, hex, hex_field, int_field, ints, read_jsonl, runes,
    str_field, strs, unhex,
};

const FORMS: [Form; 4] = [Form::Nfc, Form::Nfd, Form::Nfkc, Form::Nfkd];
const FORM_NAMES: [&str; 4] = ["NFC", "NFD", "NFKC", "NFKD"];

fn span_code(err: Result<(), Error>) -> i64 {
    match err {
        Ok(()) => 0,
        Err(Error::EndOfSpan) => 1,
        Err(Error::ShortSrc) => 2,
        Err(Error::ShortDst) => 3,
    }
}

fn iterate(f: Form, s: &[u8]) -> Option<(Vec<i64>, Vec<u8>)> {
    std::panic::catch_unwind(|| {
        let mut it = Iter::new();
        it.init_string(f, s);
        let mut segs = Vec::new();
        let mut cat = Vec::new();
        while !it.done() {
            let seg = it.next();
            segs.push(seg.len() as i64);
            cat.extend_from_slice(seg);
        }
        (segs, cat)
    })
    .ok()
}

#[test]
fn hooks_norm_vectors() {
    let rows = read_jsonl("hooks/xtext/norm/vectors.jsonl");
    assert_eq!(rows.len(), 17960);
    let mut by_cp: HashMap<Rune, usize> = HashMap::new();
    let mut checked = 0;
    for (i, row) in rows.iter().enumerate() {
        if row.get("cp").is_some() {
            by_cp.insert(int_field(row, "cp") as Rune, i);
            checked += 1;
            continue;
        }
        let input = hex_field(row, "in");
        for (k, f) in FORMS.iter().enumerate() {
            assert_eq!(
                hex(&f.string(&input)),
                str_field(row, FORM_NAMES[k]),
                "{} of {}",
                FORM_NAMES[k],
                hex(&input)
            );
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
    let mut seen = 0;
    runes(0, 0x110000, |c| {
        let s = go_string(c);
        let row = by_cp.get(&c).map(|&i| &rows[i]);
        for (k, f) in FORMS.iter().enumerate() {
            let want = row
                .and_then(|r| r.get(FORM_NAMES[k]))
                .and_then(|v| v.as_str())
                .map(unhex)
                .unwrap_or_else(|| s.clone());
            assert_eq!(
                f.string(&s).as_ref(),
                want.as_slice(),
                "{} of U+{c:04X}",
                FORM_NAMES[k]
            );
        }
        let ccc = row
            .and_then(|r| r.get("ccc"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        assert_eq!(
            i64::from(Form::Nfc.properties_string(&s).ccc()),
            ccc,
            "ccc of U+{c:04X}"
        );
        if row.is_some() {
            seen += 1;
        }
    });
    assert_eq!(seen, by_cp.len());
}

#[test]
fn xtext_norm_strings() {
    let rows = read_jsonl("xtext/norm/strings.jsonl");
    assert_eq!(rows.len(), 1200);
    let mut checked = 0;
    for row in &rows {
        let s = hex_field(row, "in");
        let ctx = hex(&s);
        let outs = strs(row.get("out").unwrap());
        let normal = bools(row.get("normal").unwrap());
        let normal_bytes = bools(row.get("normal_bytes").unwrap());
        let quick = ints(row.get("quick_span").unwrap());
        let first = ints(row.get("first_boundary").unwrap());
        let last = ints(row.get("last_boundary").unwrap());
        let next_eof = ints(row.get("next_boundary_eof").unwrap());
        let next = ints(row.get("next_boundary").unwrap());
        let span = array_field(row, "span");
        let split = int_field(row, "split") as usize;
        let iters = array_field(row, "iter");
        for (k, f) in FORMS.iter().enumerate() {
            let out = f.string(&s);
            assert_eq!(hex(&out), outs[k], "{} string of {ctx}", FORM_NAMES[k]);
            assert_eq!(
                f.is_normal_string(&s),
                normal[k],
                "{} is_normal_string of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.is_normal(&s),
                normal_bytes[k],
                "{} is_normal of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.quick_span_string(&s) as i64,
                quick[k],
                "{} quick_span of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.first_boundary_in_string(&s) as i64,
                first[k],
                "{} first_boundary of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.last_boundary(&s) as i64,
                last[k],
                "{} last_boundary of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.next_boundary_in_string(&s, true) as i64,
                next_eof[k],
                "{} next_boundary eof of {ctx}",
                FORM_NAMES[k]
            );
            assert_eq!(
                f.next_boundary_in_string(&s, false) as i64,
                next[k],
                "{} next_boundary of {ctx}",
                FORM_NAMES[k]
            );
            let want_span = ints(&span[k]);
            let (n1, e1) = f.span_string(&s, true);
            let (n2, e2) = f.span(&s, false);
            assert_eq!(
                vec![n1 as i64, span_code(e1), n2 as i64, span_code(e2)],
                want_span,
                "{} span of {ctx}",
                FORM_NAMES[k]
            );
            let appended = f.append_string(f.bytes(&s[..split]).into_owned(), &s[split..]);
            let want_append = row
                .get("append_out")
                .and_then(|m| m.get(&k.to_string()))
                .and_then(|v| v.as_str())
                .unwrap_or(outs[k]);
            assert_eq!(
                hex(&appended),
                want_append,
                "{} append at {split} of {ctx}",
                FORM_NAMES[k]
            );
            let got = iterate(*f, &s);
            if iters[k].is_null() {
                assert!(
                    got.is_none(),
                    "{} iter of {ctx} should panic",
                    FORM_NAMES[k]
                );
                continue;
            }
            let (segs, cat) =
                got.unwrap_or_else(|| panic!("{} iter of {ctx} panicked", FORM_NAMES[k]));
            assert_eq!(
                segs,
                ints(&iters[k]),
                "{} iter segments of {ctx}",
                FORM_NAMES[k]
            );
            let want_cat = row
                .get("iter_out")
                .and_then(|m| m.get(&k.to_string()))
                .and_then(|v| v.as_str())
                .unwrap_or(outs[k]);
            assert_eq!(
                hex(&cat),
                want_cat,
                "{} iter output of {ctx}",
                FORM_NAMES[k]
            );
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

const NORM_MARKS: [&str; 12] = [
    "\u{0300}",
    "\u{0301}",
    "\u{0302}",
    "\u{0308}",
    "\u{030a}",
    "\u{0323}",
    "\u{0327}",
    "\u{0345}",
    "\u{3099}",
    "\u{1161}",
    "\u{11a8}",
    "\u{110ba}",
];

const NORM_MARKS_HIGH: [&str; 3] = ["\u{0301}", "\u{0308}", "\u{3099}"];

#[test]
fn xtext_norm_marks() {
    let rows = read_jsonl("xtext/norm/marks.jsonl");
    assert_eq!(rows.len(), 1104);
    let mut checked = 0;
    for row in &rows {
        let block = int_field(row, "block") as Rune;
        let m = int_field(row, "mark") as usize;
        let mark = if block >= 0x20 {
            NORM_MARKS_HIGH[m]
        } else {
            NORM_MARKS[m]
        };
        let mut ds: Vec<Digest> = (0..4).map(|_| Digest::new()).collect();
        runes(block << 12, (block + 1) << 12, |c| {
            let mut s = go_string(c);
            s.extend_from_slice(mark.as_bytes());
            for (k, f) in FORMS.iter().enumerate() {
                ds[k].add(&f.string(&s));
            }
        });
        let want = strs(row.get("digests").unwrap());
        for k in 0..4 {
            assert_eq!(
                ds[k].sum(),
                want[k],
                "{} block {block:#x} mark {m}",
                FORM_NAMES[k]
            );
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}

#[test]
fn xtext_norm_invalid() {
    let rows = read_jsonl("xtext/norm/invalid.jsonl");
    assert_eq!(rows.len(), 256);
    let mut checked = 0;
    for row in &rows {
        let b0 = int_field(row, "b0") as u8;
        let mut ds: Vec<Digest> = (0..4).map(|_| Digest::new()).collect();
        for b1 in 0..=255u8 {
            let pair = [b0, b1];
            let variants: [Vec<u8>; 4] = [
                pair.to_vec(),
                [b"a\xcc\x81".as_slice(), &pair, b"\xcc\x80"].concat(),
                [pair.as_slice(), b"\x80\x80"].concat(),
                [b"\xe1\x84\x80".as_slice(), &pair, b"\xe1\x86\xa8"].concat(),
            ];
            for s in &variants {
                for (k, f) in FORMS.iter().enumerate() {
                    ds[k].add(&f.string(s));
                }
            }
        }
        let want = strs(row.get("digests").unwrap());
        for k in 0..4 {
            assert_eq!(ds[k].sum(), want[k], "{} b0 {b0:#x}", FORM_NAMES[k]);
        }
        checked += 1;
    }
    assert_eq!(checked, rows.len());
}
