use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Once;

use gocompat::json::{self, Value};
use gocompat::utf8::{self, Rune};
use goja::deps::regexp2::syntax::{self, Code, ErrorArg, RegexTree};
use goja::deps::regexp2::{Error, Match, Regexp};

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

fn install_quiet_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                default(info);
            }
        }));
    });
}

fn quietly<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    install_quiet_hook();
    QUIET.with(|q| q.set(true));
    let r = panic::catch_unwind(AssertUnwindSafe(f));
    QUIET.with(|q| q.set(false));
    r
}

fn golden_lines(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/regexp2")
        .join(name);
    let data =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| json::unmarshal_value(l).unwrap_or_else(|e| panic!("{name}:{}: {e}", i + 1)))
        .collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn text(row: &Value, field: &str) -> Vec<u8> {
    if let Some(s) = row.get(field).and_then(Value::as_str) {
        return s.as_bytes().to_vec();
    }
    let hex = row
        .get(&format!("{field}_hex"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing {field} in {row:?}"));
    unhex(hex)
}

fn int(v: &Value) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("not an integer: {v:?}"))
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing {key} in {row:?}"))
}

fn arr(v: &Value) -> &Vec<Value> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v:?}"))
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v:?}"))
}

fn decode(b: &[u8]) -> Vec<Rune> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let (r, n) = utf8::decode_rune(&b[i..]);
        out.push(r);
        i += n;
    }
    out
}

fn error_text(e: &Error) -> String {
    match e {
        Error::Syntax(se) => {
            let args: Vec<String> = se
                .args
                .iter()
                .map(|a| match a {
                    ErrorArg::Int(i) => format!("int:{i}"),
                    ErrorArg::Str(s) => format!("str:{s}"),
                    ErrorArg::Rune(r) => format!("rune:{r}"),
                })
                .collect();
            format!("{:?}{:?}", se.code, args)
        }
        Error::StartAtTooLarge => "StartAtTooLarge[]".to_string(),
        Error::StartAtNotAligned => "StartAtNotAligned[]".to_string(),
        Error::CountTooSmall => "CountTooSmall[]".to_string(),
        Error::UnknownRunnerState => "UnknownRunnerState[]".to_string(),
    }
}

fn golden_error(v: &Value) -> String {
    let code = s(field(v, "code"));
    let code = if code == "InvalidUTF8" {
        "InvalidUtf8"
    } else {
        code
    };
    let args: Vec<String> = v
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|x| {
                    if let Some(i) = x.get("int") {
                        format!("int:{}", int(i))
                    } else if let Some(t) = x.get("str") {
                        format!("str:{}", s(t))
                    } else {
                        format!("rune:{}", int(field(x, "rune")))
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    format!("{code}{args:?}")
}

type CodeRecord = String;

type GroupRecord = (String, isize, isize, Vec<(isize, isize)>);

fn code_record(tree: &RegexTree, code: &Code, with_dumps: bool) -> CodeRecord {
    let caps: Option<Vec<(i64, i64)>> = code.caps.as_ref().map(|m| {
        let mut v: Vec<(i64, i64)> = m.iter().map(|(&k, &v)| (k as i64, v as i64)).collect();
        v.sort();
        v
    });
    let capnames: Option<Vec<(String, i64)>> = tree.capnames.as_ref().map(|m| {
        let mut v: Vec<(String, i64)> = m.iter().map(|(k, &v)| (k.clone(), v as i64)).collect();
        v.sort();
        v
    });
    let codes: Vec<i64> = code.codes.iter().map(|&c| c as i64).collect();
    let strings: Vec<Vec<i64>> = code
        .strings
        .iter()
        .map(|s| s.iter().map(|&r| i64::from(r)).collect())
        .collect();
    let sets: Vec<String> = code.sets.iter().map(|c| c.string()).collect();
    let fc = code
        .fc_prefix
        .as_ref()
        .map(|p| (p.prefix_set.string(), p.case_insensitive));
    let bm = code.bm_prefix.as_ref().map(|b| b.dump(""));
    let mut out = format!(
        "codes={codes:?}\nstrings={strings:?}\nsets={sets:?}\ntrackcount={}\ncapsize={}\ncaps={caps:?}\nfc={fc:?}\nbm={bm:?}\nanchors={}\nrtl={}\ncapnames={capnames:?}\ncaplist={:?}\n",
        code.track_count, code.capsize, code.anchors, code.right_to_left, tree.caplist
    );
    if with_dumps {
        out.push_str(&format!("tree={}\ndump={}\n", tree.dump(), code.dump()));
    }
    out
}

fn golden_code_record(v: &Value, with_dumps: bool) -> CodeRecord {
    let codes: Vec<i64> = arr(field(v, "codes")).iter().map(int).collect();
    let strings: Vec<Vec<i64>> = arr(field(v, "strings"))
        .iter()
        .map(|x| arr(x).iter().map(int).collect())
        .collect();
    let sets: Vec<String> = arr(field(v, "sets"))
        .iter()
        .map(|x| s(x).to_string())
        .collect();
    let caps: Option<Vec<(i64, i64)>> = field(v, "caps").as_array().map(|a| {
        a.iter()
            .map(|p| (int(&arr(p)[0]), int(&arr(p)[1])))
            .collect()
    });
    let capnames: Option<Vec<(String, i64)>> = field(v, "capnames").as_array().map(|a| {
        a.iter()
            .map(|p| (s(&arr(p)[0]).to_string(), int(&arr(p)[1])))
            .collect()
    });
    let caplist: Option<Vec<String>> = field(v, "caplist")
        .as_array()
        .map(|a| a.iter().map(|x| s(x).to_string()).collect());
    let fc_v = field(v, "fc");
    let fc = if fc_v.is_null() {
        None
    } else {
        Some((
            s(field(fc_v, "set")).to_string(),
            field(fc_v, "ci").as_bool().expect("bool"),
        ))
    };
    let bm = field(v, "bm").as_str().map(str::to_string);
    let mut out = format!(
        "codes={codes:?}\nstrings={strings:?}\nsets={sets:?}\ntrackcount={}\ncapsize={}\ncaps={caps:?}\nfc={fc:?}\nbm={bm:?}\nanchors={}\nrtl={}\ncapnames={capnames:?}\ncaplist={caplist:?}\n",
        int(field(v, "trackcount")),
        int(field(v, "capsize")),
        int(field(v, "anchors")),
        field(v, "rtl").as_bool().expect("bool"),
    );
    if with_dumps {
        out.push_str(&format!(
            "tree={}\ndump={}\n",
            s(field(v, "tree")),
            s(field(v, "dump"))
        ));
    }
    out
}

fn check_code(label: &str, pattern: &[u8], options: i32, golden: &Value, with_dumps: bool) {
    if golden.get("panic").is_some() {
        let r = quietly(|| {
            let tree = syntax::parse(pattern, options);
            if let Ok(tree) = tree {
                let _ = syntax::write(&tree);
            }
        });
        assert!(r.is_err(), "{label}: expected a panic while compiling");
        return;
    }
    if let Some(e) = golden.get("error") {
        let got = syntax::parse(pattern, options)
            .map(|_| ())
            .map_err(Error::Syntax);
        let got = got.expect_err(&format!("{label}: expected a compile error"));
        assert_eq!(error_text(&got), golden_error(e), "{label}");
        return;
    }
    let tree = syntax::parse(pattern, options).unwrap_or_else(|e| panic!("{label}: {e}"));
    let code = syntax::write(&tree);
    let got = code_record(&tree, &code, with_dumps);
    let want = golden_code_record(golden, with_dumps);
    assert_eq!(got, want, "{label}: code differs");
}

fn match_record(m: &Match<'_>) -> String {
    let groups: Vec<GroupRecord> = m
        .groups()
        .iter()
        .map(|g| {
            (
                g.name.clone(),
                g.index,
                g.length,
                g.captures.iter().map(|c| (c.index, c.length)).collect(),
            )
        })
        .collect();
    format!("{}/{}/{}/{groups:?}", m.index, m.length, m.group_count())
}

fn golden_match_record(v: &Value) -> String {
    let groups: Vec<GroupRecord> = arr(field(v, "g"))
        .iter()
        .map(|g| {
            (
                s(field(g, "n")).to_string(),
                int(field(g, "i")) as isize,
                int(field(g, "l")) as isize,
                arr(field(g, "c"))
                    .iter()
                    .map(|c| (int(&arr(c)[0]) as isize, int(&arr(c)[1]) as isize))
                    .collect(),
            )
        })
        .collect();
    format!(
        "{}/{}/{}/{groups:?}",
        int(field(v, "i")),
        int(field(v, "l")),
        int(field(v, "n"))
    )
}

fn compact_record(m: &Match<'_>) -> String {
    let groups: Vec<(isize, isize, Vec<isize>)> = m
        .groups()
        .iter()
        .map(|g| {
            (
                g.index,
                g.length,
                g.captures
                    .iter()
                    .flat_map(|c| [c.index, c.length])
                    .collect(),
            )
        })
        .collect();
    format!("{}/{}/{groups:?}", m.index, m.length)
}

fn golden_compact_record(v: &Value) -> String {
    let a = arr(v);
    let groups: Vec<(isize, isize, Vec<isize>)> = arr(&a[2])
        .iter()
        .map(|g| {
            let g = arr(g);
            (
                int(&g[0]) as isize,
                int(&g[1]) as isize,
                arr(&g[2]).iter().map(|x| int(x) as isize).collect(),
            )
        })
        .collect();
    format!("{}/{}/{groups:?}", int(&a[0]), int(&a[1]))
}

struct Found {
    matches: Vec<String>,
    more: bool,
    panicked: bool,
}

fn find_all(
    re: &Regexp,
    runes: &[Rune],
    start: isize,
    limit: usize,
    record: fn(&Match<'_>) -> String,
) -> Found {
    let mut matches = Vec::new();
    let mut more = false;
    let r = quietly(|| {
        let mut m = re
            .find_runes_match_starting_at(runes, start)
            .expect("no error without a timeout");
        while let Some(cur) = &m {
            if matches.len() >= limit {
                break;
            }
            matches.push(record(cur));
            m = re.find_next_match(cur).expect("no error without a timeout");
        }
        more = m.is_some();
    });
    Found {
        matches,
        more,
        panicked: r.is_err(),
    }
}

fn input_runes(parts: &Value) -> Vec<Rune> {
    let mut out = Vec::new();
    for p in arr(parts) {
        let r: Vec<Rune> = arr(field(p, "r")).iter().map(|x| int(x) as Rune).collect();
        for _ in 0..int(field(p, "n")) {
            out.extend_from_slice(&r);
        }
    }
    out
}

#[test]
fn goja_cases_replay() {
    let lines = golden_lines("goja.jsonl");
    let mut count = 0;
    for row in &lines {
        let label = s(field(row, "label"));
        let pattern = text(row, "pattern");
        let options = int(field(row, "options")) as i32;
        let compile = field(row, "compile");
        if compile.as_str() == Some("panic") {
            let r = quietly(|| Regexp::compile(&pattern, options).map(|_| ()));
            assert!(r.is_err(), "{label}: expected a panic");
            count += 1;
            continue;
        }
        if compile.as_str() != Some("ok") {
            let err = Regexp::compile(&pattern, options).expect_err(label);
            assert_eq!(error_text(&err), golden_error(compile), "{label}");
            if let Error::Syntax(se) = &err {
                assert!(!se.reason().contains("error parsing regexp"), "{label}");
                assert!(!se.reason().contains("regexp/syntax"), "{label}");
            }
            count += 1;
            continue;
        }
        check_code(label, &pattern, options, field(row, "code"), true);
        let re = Regexp::compile(&pattern, options).unwrap_or_else(|e| panic!("{label}: {e}"));
        let info = field(row, "info");
        let names: Vec<String> = arr(field(info, "names"))
            .iter()
            .map(|x| s(x).to_string())
            .collect();
        assert_eq!(re.get_group_names(), names, "{label}: names");
        let numbers: Vec<isize> = arr(field(info, "numbers"))
            .iter()
            .map(|x| int(x) as isize)
            .collect();
        assert_eq!(re.get_group_numbers(), numbers, "{label}: numbers");
        let max_num = numbers.iter().copied().max().unwrap_or(0).max(0);
        let name_of: Vec<String> = arr(field(info, "name_of"))
            .iter()
            .map(|x| s(x).to_string())
            .collect();
        let got_name_of: Vec<String> = (-1..=max_num + 2)
            .map(|i| re.group_name_from_number(i))
            .collect();
        assert_eq!(got_name_of, name_of, "{label}: name_of");
        for pair in arr(field(info, "number_of")) {
            let p = arr(pair);
            assert_eq!(
                re.group_number_from_name(s(&p[0])),
                int(&p[1]) as isize,
                "{label}: number_of {}",
                s(&p[0])
            );
        }
        for run in arr(field(row, "runs")) {
            let runes = input_runes(field(run, "input"));
            for find in arr(field(run, "finds")) {
                let start = int(field(find, "start")) as isize;
                let got = find_all(&re, &runes, start, 20, match_record);
                let want: Vec<String> = arr(field(find, "matches"))
                    .iter()
                    .map(golden_match_record)
                    .collect();
                assert_eq!(got.matches, want, "{label}: matches from {start}");
                let want_panic = find.get("panic").is_some();
                assert_eq!(got.panicked, want_panic, "{label}: panic from {start}");
                if !want_panic {
                    assert_eq!(
                        got.more,
                        field(find, "more").as_bool().expect("bool"),
                        "{label}: more from {start}"
                    );
                }
            }
            let quick = field(run, "quick");
            let got = quietly(|| re.match_runes(runes.as_slice()).expect("no error"));
            match quick.as_bool() {
                Some(b) => assert_eq!(got.ok(), Some(b), "{label}: quick"),
                None => {
                    assert_eq!(quick.as_str(), Some("panic"), "{label}");
                    assert!(got.is_err(), "{label}: quick should panic");
                }
            }
        }
        count += 1;
    }
    assert_eq!(count, lines.len());
    assert_eq!(count, 224);
}

#[test]
fn corpus_replay() {
    let lines = golden_lines("corpus.jsonl");
    let fixed: [&str; 2] = ["aB1 _-\n\u{e9}x(", "zz\u{130}\u{212a}\u{1F600}a"];
    let mut count = 0;
    let mut runs = 0;
    for row in &lines {
        let name = s(field(row, "name"));
        let pattern = text(row, "pattern");
        let options = int(field(row, "options")) as i32;
        let label = format!("{name}/{options}");
        let code = field(row, "code");
        check_code(&label, &pattern, options, code, false);
        if let Some(expected) = row.get("runs") {
            let re = Regexp::compile(&pattern, options).unwrap_or_else(|e| panic!("{label}: {e}"));
            let mut probes: Vec<Vec<Rune>> = fixed.iter().map(|t| decode(t.as_bytes())).collect();
            probes.push(decode(&pattern));
            for (i, want) in arr(expected).iter().enumerate() {
                let mut runes = probes[i].clone();
                runes.truncate(40);
                if want.is_null() {
                    continue;
                }
                let got = find_all(&re, &runes, 0, 3, compact_record);
                let (want_matches, want_panic) = match want.as_array() {
                    Some(a) => (
                        a.iter().map(golden_compact_record).collect::<Vec<_>>(),
                        false,
                    ),
                    None => (
                        arr(field(want, "matches"))
                            .iter()
                            .map(golden_compact_record)
                            .collect(),
                        true,
                    ),
                };
                assert_eq!(got.matches, want_matches, "{label}: probe {i}");
                assert_eq!(got.panicked, want_panic, "{label}: probe {i} panic");
                runs += 1;
            }
        }
        count += 1;
    }
    assert_eq!(count, lines.len());
    assert_eq!(count, 7532);
    assert!(runs > 0);
}

#[test]
fn replace_replay() {
    let lines = golden_lines("replace.jsonl");
    let mut count = 0;
    for row in &lines {
        let pattern = text(row, "pattern");
        let options = int(field(row, "options")) as i32;
        let input = String::from_utf8(text(row, "input")).expect("utf-8 input");
        let repl = String::from_utf8(text(row, "replacement")).expect("utf-8 replacement");
        let start_at = int(field(row, "start_at")) as isize;
        let n = int(field(row, "count")) as isize;
        let label = format!("{} {repl}", String::from_utf8_lossy(&pattern));
        let re = Regexp::must_compile(&pattern, options);
        match re.replace(&input, &repl, start_at, n) {
            Ok(out) => assert_eq!(out.as_bytes(), text(row, "output").as_slice(), "{label}"),
            Err(e) => assert_eq!(error_text(&e), golden_error(field(row, "error")), "{label}"),
        }
        let evaluated = re.replace_func(
            &input,
            |m| format!("<{},{},{}>", m.index, m.length, m.group_count()),
            start_at,
            n,
        );
        match evaluated {
            Ok(out) => assert_eq!(
                out.as_bytes(),
                text(row, "func_output").as_slice(),
                "{label}"
            ),
            Err(e) => assert_eq!(
                error_text(&e),
                golden_error(field(row, "func_error")),
                "{label}"
            ),
        }
        count += 1;
    }
    assert_eq!(count, lines.len());
    assert_eq!(count, 32);
}

#[test]
fn escape_replay() {
    let lines = golden_lines("escape.jsonl");
    let mut count = 0;
    for row in &lines {
        let input = text(row, "input");
        match s(field(row, "kind")) {
            "escape" => {
                let esc = goja::deps::regexp2::escape(&input);
                assert_eq!(esc.as_bytes(), text(row, "output").as_slice(), "{input:?}");
                match goja::deps::regexp2::unescape(&esc) {
                    Ok(back) => assert_eq!(
                        back.as_bytes(),
                        text(row, "unescaped").as_slice(),
                        "{input:?}"
                    ),
                    Err(e) => assert_eq!(
                        error_text(&e),
                        golden_error(field(row, "error")),
                        "{input:?}"
                    ),
                }
            }
            _ => {
                let input = String::from_utf8(input).expect("utf-8 input");
                match goja::deps::regexp2::unescape(&input) {
                    Ok(out) => {
                        assert_eq!(out.as_bytes(), text(row, "output").as_slice(), "{input:?}")
                    }
                    Err(e) => assert_eq!(
                        error_text(&e),
                        golden_error(field(row, "error")),
                        "{input:?}"
                    ),
                }
            }
        }
        count += 1;
    }
    assert_eq!(count, lines.len());
    assert_eq!(count, 801);
}

#[test]
fn strings_replay() {
    let lines = golden_lines("strings.jsonl");
    let mut count = 0;
    for row in &lines {
        let pattern = text(row, "pattern");
        let options = int(field(row, "options")) as i32;
        let input = text(row, "input");
        let re = Regexp::must_compile(&pattern, options);
        let start = field(row, "start_at");
        let got = if start.is_null() {
            let quick = re.match_string(&input).expect("no error");
            assert_eq!(quick, field(row, "quick").as_bool().expect("bool"));
            re.find_string_match(&input)
        } else {
            re.find_string_match_starting_at(&input, int(start) as isize)
        };
        let label = format!("{} at {start:?}", String::from_utf8_lossy(&pattern));
        match got {
            Err(e) => assert_eq!(error_text(&e), golden_error(field(row, "error")), "{label}"),
            Ok(None) => assert!(field(row, "match").is_null(), "{label}"),
            Ok(Some(m)) => {
                assert_eq!(
                    match_record(&m),
                    golden_match_record(field(row, "match")),
                    "{label}"
                );
                assert_eq!(
                    m.string().as_bytes(),
                    text(row, "text").as_slice(),
                    "{label}"
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, lines.len());
    assert_eq!(count, 39);
}

#[test]
fn hooks_corpus_regexp2_reasons() {
    let mut checked = 0;
    for corpus in ["corpus-utc.jsonl", "corpus-europe-paris.jsonl"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/golden/hooks")
            .join(corpus);
        let data =
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for line in data.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            let row = json::unmarshal_value(line).unwrap_or_else(|e| panic!("{corpus}: {e}"));
            let label = s(field(&row, "label"));
            if !label.starts_with("re-bad-") {
                continue;
            }
            let Some(expected) = row.get("expected_rust") else {
                continue;
            };
            if label == "re-bad-class" {
                continue;
            }
            let message = s(field(expected, "error"));
            let rest = message
                .strip_prefix("SyntaxError: Invalid regular expression: /")
                .unwrap_or_else(|| panic!("{label}: {message}"));
            let rest = &rest[..rest.rfind(" at postRead").expect("a stack suffix")];
            let split = rest.rfind("/: ").expect("a reason");
            let (pattern, reason) = (&rest[..split], &rest[split + 3..]);
            let err = Regexp::compile(pattern, goja::deps::regexp2::ECMASCRIPT).expect_err(label);
            assert_eq!(err.reason(), reason, "{corpus} {label}");
            checked += 1;
        }
    }
    assert_eq!(checked, 4);
}
