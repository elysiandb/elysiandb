mod regexp_common;

use goja::deps::regexp::syntax::Flags;
use goja::deps::regexp::{self, Regexp, must_compile};
use regexp_common::*;

type Variants = (Regexp, Regexp, Regexp, Regexp);

fn variants(q: &[u8]) -> Variants {
    let re = regexp::compile(q)
        .unwrap_or_else(|e| panic!("compile {:?}: {e:?}", String::from_utf8_lossy(q)));
    let mut full = br"\A(?:".to_vec();
    full.extend_from_slice(q);
    full.extend_from_slice(br")\z");
    let refull = regexp::compile(&full).unwrap_or_else(|e| panic!("compile full: {e:?}"));
    let mut relong = re.copy();
    relong.longest();
    let mut refulllong = refull.copy();
    refulllong.longest();
    (re, refull, relong, refulllong)
}

fn run_re2(v: &Variants, text: &[u8]) -> ([Option<Vec<isize>>; 4], [bool; 4]) {
    let (re, refull, relong, refulllong) = v;
    (
        [
            refull.find_string_submatch_index(text),
            re.find_string_submatch_index(text),
            refulllong.find_string_submatch_index(text),
            relong.find_string_submatch_index(text),
        ],
        [
            refull.match_string(text),
            re.match_string(text),
            refulllong.match_string(text),
            relong.match_string(text),
        ],
    )
}

#[test]
fn test_re2_search() {
    let rel = "re2-search.jsonl";
    let rows = read_jsonl(rel);
    let mut current: Option<(Vec<u8>, Variants)> = None;
    let mut checked = 0;
    for row in &rows {
        let q = pattern(row);
        let ctx = format!("{:?}", String::from_utf8_lossy(&q));
        if row.get("error").is_some() {
            let err = regexp::compile(&q).expect_err(&ctx);
            check_error(&err, row, &ctx);
            checked += 1;
            continue;
        }
        if current.as_ref().is_none_or(|(p, _)| *p != q) {
            current = Some((q.clone(), variants(&q)));
        }
        let (_, v) = current.as_ref().expect("compiled");
        let text = bytes_field(row, "text").expect("text");
        let (have, matched) = run_re2(v, &text);
        let keys = ["full", "partial", "full_longest", "partial_longest"];
        let want_match: Vec<bool> = val(row, "match")
            .as_array()
            .expect("match")
            .iter()
            .map(|b| b.as_bool().expect("bool"))
            .collect();
        for k in 0..4 {
            assert_eq!(
                have[k],
                ints(val(row, keys[k])),
                "{ctx} {} on {:?}",
                keys[k],
                text
            );
            assert_eq!(
                matched[k], want_match[k],
                "{ctx} match {} on {:?}",
                keys[k], text
            );
        }
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}

#[test]
fn test_re2_exhaustive() {
    let rel = "re2-exhaustive.jsonl";
    let rows = read_jsonl(rel);
    let mut texts: Vec<Vec<u8>> = Vec::new();
    let mut checked = 0;
    let mut cases = 0;
    for row in &rows {
        if row.get("strings").is_some() || row.get("strings_hex").is_some() {
            texts = pieces_field(row, "strings").expect("strings");
            checked += 1;
            continue;
        }
        let q = pattern(row);
        let ctx = format!("{:?}", String::from_utf8_lossy(&q));
        if row.get("error").is_some() {
            let err = regexp::compile(&q).expect_err(&ctx);
            check_error(&err, row, &ctx);
            checked += 1;
            continue;
        }
        let v = variants(&q);
        let mut b = compile_summary(&Ok(v.0.clone()));
        b.push(b'\n');
        b.extend_from_slice(&compile_summary(&Ok(v.1.clone())));
        b.push(b'\n');
        for text in &texts {
            let (have, matched) = run_re2(&v, text);
            for r in &have {
                b.extend_from_slice(res_text(r).as_bytes());
            }
            for m in matched {
                b.extend_from_slice(bool_text(m).as_bytes());
            }
            b.push(b'\n');
        }
        assert_eq!(int_field(row, "cases") as usize, texts.len(), "{ctx}");
        assert_eq!(fnv(&b), str_field(row, "fnv"), "{ctx}");
        cases += texts.len();
        checked += 1;
    }
    assert!(cases > 0);
    assert_eq!(checked, read_lines(rel).len());
}

#[test]
fn test_fowler() {
    let rel = "fowler.jsonl";
    let rows = read_jsonl(rel);
    let mut checked = 0;
    for row in &rows {
        let pat = pattern(row);
        let text = bytes_field(row, "text").expect("text");
        let flags = int_field(row, "flags") as Flags;
        let ctx = format!(
            "{}:{} {:?} on {:?}",
            str_field(row, "file"),
            int_field(row, "line"),
            String::from_utf8_lossy(&pat),
            String::from_utf8_lossy(&text)
        );
        match regexp::compile_mode(&pat, flags, true) {
            Err(err) => {
                assert!(row.get("error").is_some(), "{ctx}: did not compile");
                check_error(&err, row, &ctx);
            }
            Ok(re) => {
                assert!(row.get("error").is_none(), "{ctx}: should not compile");
                let matched = re.match_string(&text);
                assert_eq!(matched, bool_field(row, "match"), "{ctx}: match");
                let have = re.find_string_submatch_index(&text);
                assert_eq!(have.is_some(), matched, "{ctx}: consistency");
                assert_eq!(have, ints(val(row, "have")), "{ctx}: submatches");
            }
        }
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}

#[test]
fn test_longest() {
    let mut re = regexp::compile(b"a(|b)").expect("compile");
    assert_eq!(re.find_string(b"ab"), b"a", "first match");
    re.longest();
    assert_eq!(re.find_string(b"ab"), b"ab", "longest match");
}

#[test]
fn test_program_too_long_for_backtrack() {
    let long_regex = must_compile(b"(one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|twentyone|twentytwo|twentythree|twentyfour|twentyfive|twentysix|twentyseven|twentyeight|twentynine|thirty|thirtyone|thirtytwo|thirtythree|thirtyfour|thirtyfive|thirtysix|thirtyseven|thirtyeight|thirtynine|forty|fortyone|fortytwo|fortythree|fortyfour|fortyfive|fortysix|fortyseven|fortyeight|fortynine|fifty|fiftyone|fiftytwo|fiftythree|fiftyfour|fiftyfive|fiftysix|fiftyseven|fiftyeight|fiftynine|sixty|sixtyone|sixtytwo|sixtythree|sixtyfour|sixtyfive|sixtysix|sixtyseven|sixtyeight|sixtynine|seventy|seventyone|seventytwo|seventythree|seventyfour|seventyfive|seventysix|seventyseven|seventyeight|seventynine|eighty|eightyone|eightytwo|eightythree|eightyfour|eightyfive|eightysix|eightyseven|eightyeight|eightynine|ninety|ninetyone|ninetytwo|ninetythree|ninetyfour|ninetyfive|ninetysix|ninetyseven|ninetyeight|ninetynine|onehundred)");
    assert!(long_regex.match_string(b"two"));
    assert!(!long_regex.match_string(b"xxx"));
}
