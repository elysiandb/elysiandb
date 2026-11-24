mod common;

use common::dump::{dump_program, node_type};
use common::{b, opt_s, read_jsonl, s, unhex};
use gocompat::json::Value;
use goja::ast::{Node, walk_nodes};
use goja::parser::{ParserOption, parse_file, with_disable_source_maps};
use std::sync::Arc;

fn script(row: &Value) -> Vec<u8> {
    match opt_s(row, "script") {
        Some(sc) => sc.as_bytes().to_vec(),
        None => unhex(s(row, "script_hex")),
    }
}

fn walk(p: &goja::ast::Program, pruned: bool) -> String {
    let mut types: Vec<&'static str> = Vec::new();
    walk_nodes(Node::Program(p), &mut |n| {
        types.push(node_type(n));
        !(pruned
            && matches!(
                n,
                Node::FunctionLiteral(_) | Node::PropertyKeyed(_) | Node::CallExpression(_)
            ))
    });
    types.join(" ")
}

fn check_file(rel: &str) -> usize {
    let disable: ParserOption = Arc::new(with_disable_source_maps);
    let rows = read_jsonl(rel);
    let mut failures = Vec::new();
    for row in &rows {
        let label = s(row, "label");
        let src = script(row);
        let (program, err) = parse_file(None, "", &src, 0, &[Arc::clone(&disable)]);
        let err_text = err.map(|e| e.error()).unwrap_or_default();
        if err_text != s(row, "error") {
            failures.push(format!(
                "{label}: error {err_text:?} != {:?}",
                s(row, "error")
            ));
            continue;
        }
        let dump = dump_program(&program);
        if dump != s(row, "dump") {
            failures.push(format!(
                "{label}: dump\n  rust {dump}\n  go   {}",
                s(row, "dump")
            ));
            continue;
        }
        if walk(&program, false) != s(row, "walk") {
            failures.push(format!("{label}: walk {}", walk(&program, false)));
        }
        if walk(&program, true) != s(row, "walk_pruned") {
            failures.push(format!("{label}: walk_pruned {}", walk(&program, true)));
        }
        let (_, default_err) = parse_file(None, "", &src, 0, &[]);
        if default_err.is_some() != b(row, "default_fails") {
            failures.push(format!("{label}: default parse {default_err:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    rows.len()
}

#[test]
fn ast_of_the_hooks_corpus_matches_go() {
    assert_eq!(check_file("goja-front/ast/corpus.jsonl"), 230);
}

#[test]
fn ast_of_extra_scripts_matches_go() {
    assert!(check_file("goja-front/ast/extra.jsonl") > 400);
}

#[test]
fn corpus_syntax_errors_have_the_expected_text() {
    let ast_rows = read_jsonl("goja-front/ast/corpus.jsonl");
    let corpus = read_jsonl("hooks/corpus-utc.jsonl");
    assert_eq!(ast_rows.len(), corpus.len());
    let mut checked = 0;
    for (row, case) in ast_rows.iter().zip(corpus.iter()) {
        assert_eq!(s(row, "label"), s(case, "label"));
        let (_, err) = parse_file(None, "", s(case, "script").as_bytes(), 0, &[]);
        assert_eq!(
            err.is_some(),
            b(row, "default_fails"),
            "{}",
            s(case, "label")
        );
        if let Some(err) = err {
            let expected = case
                .get("expected_rust")
                .and_then(|e| e.get("error"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| s(case, "error"));
            assert_eq!(
                format!("SyntaxError: {}", err.error()),
                expected,
                "{}",
                s(case, "label")
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 9);
}
