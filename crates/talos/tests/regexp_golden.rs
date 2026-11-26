mod regexp_common;

use gocompat::json::Value;
use regexp_common::*;
use std::collections::HashMap;
use talos::deps::regexp::syntax::{self, Flags};
use talos::deps::regexp::{self, Regexp};

fn short(p: &[u8]) -> String {
    let n = p.len().min(80);
    format!("{:?}", String::from_utf8_lossy(&p[..n]))
}

fn compile(pat: &[u8], posix: bool) -> Result<Regexp, syntax::Error> {
    if posix {
        regexp::compile_posix(pat)
    } else {
        regexp::compile(pat)
    }
}

fn string_list(row: &Value, key: &str) -> Vec<Vec<u8>> {
    row.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing {key} in {row:?}"))
        .iter()
        .map(|v| v.as_str().expect("string").as_bytes().to_vec())
        .collect()
}

#[test]
fn golden_compile() {
    let rel = "compile.jsonl";
    let rows = read_jsonl(rel);
    let mut checked = 0;
    for row in &rows {
        let pat = pattern(row);
        let posix = bool_field(row, "posix");
        let ctx = format!("compile {} posix={posix}", short(&pat));
        let res = compile(&pat, posix);
        if row.get("error").is_some() {
            let err = res.expect_err(&ctx);
            check_error(&err, row, &ctx);
            checked += 1;
            continue;
        }
        let re = res.unwrap_or_else(|e| panic!("{ctx}: {e:?}"));
        assert_eq!(
            re.string() == pat.as_slice(),
            bool_field(row, "string"),
            "{ctx}"
        );
        assert_eq!(
            re.num_subexp() as i64,
            int_field(row, "num_subexp"),
            "{ctx}"
        );
        assert_eq!(
            re.subexp_names().to_vec(),
            string_list(row, "names"),
            "{ctx}"
        );
        let (prefix, complete) = re.literal_prefix();
        assert_eq!(
            Some(prefix.to_vec()),
            bytes_field(row, "prefix"),
            "{ctx}: prefix"
        );
        assert_eq!(complete, bool_field(row, "complete"), "{ctx}: complete");
        assert_eq!(
            i64::from(re.prefix_rune()),
            int_field(row, "prefix_rune"),
            "{ctx}"
        );
        assert_eq!(
            i64::from(re.prefix_end()),
            int_field(row, "prefix_end"),
            "{ctx}"
        );
        assert_eq!(
            re.max_bit_state_len() as i64,
            int_field(row, "max_bitstate_len"),
            "{ctx}"
        );
        assert_eq!(
            re.min_input_len() as i64,
            int_field(row, "min_input_len"),
            "{ctx}"
        );
        assert_eq!(i64::from(re.cond()), int_field(row, "cond"), "{ctx}");
        assert_eq!(re.matchcap() as i64, int_field(row, "matchcap"), "{ctx}");
        assert_eq!(re.mpool() as i64, int_field(row, "mpool"), "{ctx}");
        let (insts, runes) = prog_size(&re);
        assert_eq!(insts as i64, int_field(row, "insts"), "{ctx}");
        assert_eq!(runes as i64, int_field(row, "runes"), "{ctx}");
        if huge_prog(&re) {
            assert!(
                row.get("prog").is_none() && row.get("prog_fnv").is_none(),
                "{ctx}"
            );
        } else {
            prog_dump(&re).check(row, "prog", &ctx);
            match row.get("onepass") {
                Some(v) if v.is_null() => assert!(re.onepass().is_none(), "{ctx}: onepass"),
                _ => onepass_dump(&re)
                    .unwrap_or_else(|| panic!("{ctx}: onepass expected"))
                    .check(row, "onepass", &ctx),
            }
        }
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}

const TEMPLATES: [&[u8]; 5] = [
    b"<$0>",
    b"[$1|${2}]",
    b"$1x${1}x",
    b"$$$x${name}${",
    b"${0}$00$01$10",
];

fn owned(v: Option<Vec<&[u8]>>) -> Option<Vec<Vec<u8>>> {
    v.map(|v| v.into_iter().map(|s| s.to_vec()).collect())
}

#[test]
fn golden_exec() {
    let rel = "exec.jsonl";
    let rows = read_jsonl(rel);
    let mut cache: HashMap<(bool, Vec<u8>), Regexp> = HashMap::new();
    let mut checked = 0;
    for row in &rows {
        let pat = pattern(row);
        let posix = bool_field(row, "posix");
        let text = bytes_field(row, "text").expect("text");
        let repeat = row.get("repeat").and_then(Value::as_i64).unwrap_or(1);
        let full = text.repeat(repeat as usize);
        let ctx = format!(
            "exec {} posix={posix} text={} x{repeat}",
            short(&pat),
            short(&text)
        );
        let re = cache
            .entry((posix, pat.clone()))
            .or_insert_with(|| compile(&pat, posix).unwrap_or_else(|e| panic!("{ctx}: {e:?}")))
            .clone();
        let mut long = re.copy();
        long.longest();
        assert_eq!(
            re.find_string_submatch_index(&full),
            ints(val(row, "submatch")),
            "{ctx}: submatch"
        );
        assert_eq!(
            re.find_reader_submatch_index(&mut StringReader::new(&full)),
            ints(val(row, "reader")),
            "{ctx}: reader"
        );
        assert_eq!(
            long.find_string_submatch_index(&full),
            ints(val(row, "longest")),
            "{ctx}: longest"
        );
        assert_eq!(
            long.find_reader_submatch_index(&mut StringReader::new(&full)),
            ints(val(row, "longest_reader")),
            "{ctx}: longest reader"
        );
        assert_eq!(
            re.match_string(&full),
            bool_field(row, "match"),
            "{ctx}: match"
        );
        assert_eq!(
            re.match_reader(&mut StringReader::new(&full)),
            bool_field(row, "match_reader"),
            "{ctx}: match reader"
        );
        assert_eq!(
            re.find_all_index(&full, 2),
            intss(val(row, "all_index_2")),
            "{ctx}: all 2"
        );
        if repeat <= 1 {
            assert_eq!(
                re.find_all_string_submatch_index(&full, -1),
                intss(val(row, "all")),
                "{ctx}: all"
            );
            assert_eq!(
                long.find_all_string_submatch_index(&full, -1),
                intss(val(row, "all_longest")),
                "{ctx}: all longest"
            );
            assert_eq!(
                owned(re.split(&full, -1)),
                pieces_field(row, "split"),
                "{ctx}: split"
            );
            assert_eq!(
                owned(re.split(&full, 2)),
                pieces_field(row, "split_2"),
                "{ctx}: split 2"
            );
            let mut reps: Vec<Vec<u8>> = TEMPLATES
                .iter()
                .map(|t| re.replace_all_string(&full, t))
                .collect();
            reps.push(re.replace_all_literal_string(&full, b"<$1>"));
            reps.push(re.replace_all_string_func(&full, |s| {
                let mut v = b"<".to_vec();
                v.extend_from_slice(s);
                v.push(b'>');
                v
            }));
            assert_eq!(Some(reps), pieces_field(row, "replace"), "{ctx}: replace");
        }
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}

#[test]
fn golden_syntax() {
    let rel = "syntax.jsonl";
    let rows = read_jsonl(rel);
    let mut checked = 0;
    for row in &rows {
        let pat = pattern(row);
        let flags = int_field(row, "flags") as Flags;
        let ctx = format!("syntax {} flags={flags}", short(&pat));
        match syntax::parse(&pat, flags) {
            Err(err) => {
                assert!(
                    row.get("error").is_some(),
                    "{ctx}: unexpected error {err:?}"
                );
                check_error(&err, row, &ctx);
            }
            Ok(re) => {
                assert!(row.get("error").is_none(), "{ctx}: expected an error");
                check_big(&dump(&re), row, "dump", &ctx);
                check_big(&re.string(), row, "string", &ctx);
                assert_eq!(i64::from(re.max_cap()), int_field(row, "max_cap"), "{ctx}");
                assert_eq!(re.cap_names(), string_list(row, "cap_names"), "{ctx}");
                let simple = re.simplify();
                check_big(&simple.string(), row, "simplify", &ctx);
                check_big(&dump(&simple), row, "simplify_dump", &ctx);
                let prog = syntax::compile(&simple).expect("compile");
                check_big(prog.to_string().as_bytes(), row, "prog", &ctx);
                let (prefix, complete) = prog.prefix();
                assert_eq!(Some(prefix), bytes_field(row, "prefix"), "{ctx}: prefix");
                assert_eq!(complete, bool_field(row, "complete"), "{ctx}: complete");
                assert_eq!(
                    i64::from(prog.start_cond()),
                    int_field(row, "start_cond"),
                    "{ctx}"
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}

#[test]
fn golden_fuzz() {
    let rel = "fuzz.jsonl";
    let rows = read_jsonl(rel);
    let mut checked = 0;
    for row in &rows {
        let pat = pattern(row);
        let texts = pieces_field(row, "texts").expect("texts");
        let res = regexp::compile(&pat);
        let mut b = compile_summary(&res);
        b.push(b'\n');
        if let Ok(re) = &res {
            let mut long = re.copy();
            long.longest();
            for t in &texts {
                b.extend_from_slice(res_text(&re.find_string_submatch_index(t)).as_bytes());
                b.extend_from_slice(
                    res_text(&re.find_reader_submatch_index(&mut StringReader::new(t))).as_bytes(),
                );
                b.extend_from_slice(res_text(&long.find_string_submatch_index(t)).as_bytes());
                b.extend_from_slice(
                    res_text(&long.find_reader_submatch_index(&mut StringReader::new(t)))
                        .as_bytes(),
                );
                b.extend_from_slice(all_text(&re.find_all_string_submatch_index(t, -1)).as_bytes());
                b.extend_from_slice(
                    all_text(&long.find_all_string_submatch_index(t, -1)).as_bytes(),
                );
                b.extend_from_slice(bool_text(re.match_string(t)).as_bytes());
                b.push(b'\n');
            }
        }
        assert_eq!(fnv(&b), str_field(row, "fnv"), "fuzz {}", short(&pat));
        checked += 1;
    }
    assert_eq!(checked, read_lines(rel).len());
}
