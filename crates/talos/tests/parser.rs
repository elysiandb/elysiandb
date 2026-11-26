use std::sync::{Arc, Mutex};
use talos::ast;
use talos::deps::big::Int;
use talos::parser::{
    ErrorList, IGNORE_REG_EXP_ERRORS, Parser, ParserOption, SourceMapLoader,
    extract_source_map_line, parse_file, parse_function, parse_number_literal,
    with_disable_source_maps, with_source_map_loader,
};
use talos::token;

fn first_err(err: &Option<ErrorList>) -> Option<String> {
    err.as_ref().map(|e| e.0[0].error())
}

fn empty_semicolon(s: &ast::Statement) -> i64 {
    let ast::Statement::EmptyStatement(e) = s else {
        panic!("not an empty statement");
    };
    e.semicolon
}

fn number_literal(s: &ast::Statement) -> Vec<u8> {
    let ast::Statement::ExpressionStatement(e) = s else {
        panic!("not an expression statement");
    };
    let ast::Expression::NumberLiteral(n) = &e.expression else {
        panic!("not a number literal");
    };
    n.literal.clone()
}

#[test]
fn test_parse_file() {
    let (_, err) = parse_file(None, "", b"/abc/", 0, &[]);
    assert_eq!(first_err(&err), None);
    let (_, err) = parse_file(None, "", b"/(?!def)abc/", IGNORE_REG_EXP_ERRORS, &[]);
    assert_eq!(first_err(&err), None);
    let (_, err) = parse_file(
        None,
        "",
        b"/(?!def)abc/; return",
        IGNORE_REG_EXP_ERRORS,
        &[],
    );
    assert_eq!(
        err.map(|e| e.error()).as_deref(),
        Some("(anonymous): Line 1:15 Illegal return statement")
    );
}

#[test]
fn test_parse_function() {
    let test = |prm: &str, bdy: &str, expect: Option<&str>| {
        let r = parse_function(prm, bdy, &[]);
        assert_eq!(
            r.as_ref().err().map(|e| e.0[0].error()).as_deref(),
            expect,
            "{prm:?} {bdy:?}"
        );
        r.ok()
    };
    test("a, b,c,d", "", None);
    test(
        "a, b;,c,d",
        "",
        Some("(anonymous): Line 1:15 Unexpected token ;"),
    );
    test(
        "this",
        "",
        Some("(anonymous): Line 1:11 Unexpected token this"),
    );
    test(
        "a, b, c, null",
        "",
        Some("(anonymous): Line 1:20 Unexpected token null"),
    );
    test("a, b,c,d", "return;", None);
    test(
        "a, b,c,d",
        "break;",
        Some("(anonymous): Line 2:1 Illegal break statement"),
    );
    test("a, b,c,d", "{}", None);
}

#[test]
fn test_parser_err() {
    fn test(input: impl AsRef<[u8]>, expect: Option<&str>) -> (ast::Program, Parser) {
        let input = input.as_ref();
        let mut parser = Parser::new("", input);
        let (program, err) = parser.parse();
        assert_eq!(
            first_err(&err).as_deref(),
            expect,
            "{:?}",
            String::from_utf8_lossy(input)
        );
        (program, parser)
    }
    test("", None);
    let (program, parser) = test(
        "\n        var abc;\n        break; do {\n        } while(true);\n    ",
        Some("(anonymous): Line 3:9 Illegal break statement"),
    );
    {
        let ast::Statement::BadStatement(stmt) = &program.body[1] else {
            panic!("not a bad statement");
        };
        assert_eq!(parser.position(stmt.from).column, 9);
        assert_eq!(parser.position(stmt.to).column, 16);
        assert_eq!(parser.slice(stmt.from, stmt.to), b"break; ");
    }
    let s: [u8; 37] = [
        0x22, 0x25, 0x21, 0x63, 0x28, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x3d, 0x25, 0x63, 0x25,
        0x9c, 0x29, 0x25, 0x21, 0x5c, 0x28, 0x73, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x3d, 0x5c, 0xe2,
        0x80, 0xa9, 0x29, 0x78, 0x39, 0x63, 0x22,
    ];
    test(s, Some("(anonymous): Line 1:16 Invalid UTF-8 character"));
    test("{", Some("(anonymous): Line 1:2 Unexpected end of input"));
    test("}", Some("(anonymous): Line 1:1 Unexpected token }"));
    test(
        "3ea",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "3in",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "3in []",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test("3e", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test(
        "3e+",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "3e-",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test("3x", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test(
        "3x0",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test("0x", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test("09", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test(
        "018",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test("01.0", Some("(anonymous): Line 1:3 Unexpected number"));
    test(".0.9", Some("(anonymous): Line 1:3 Unexpected number"));
    test(
        "0o3e1",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "01a",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "0x3in[]",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\"Hello\nWorld\"",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "‿ = 10",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "x\\",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "x\\\\",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "x\\u005c",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "x\\u002a",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "x\\\\u002a",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "/\n",
        Some("(anonymous): Line 1:1 Invalid regular expression: missing /"),
    );
    test(
        "0 = 1",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "func() = 1",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "(1 + 1) = 2",
        Some("(anonymous): Line 1:2 Invalid left-hand side in assignment"),
    );
    test(
        "1++",
        Some("(anonymous): Line 1:2 Invalid left-hand side in assignment"),
    );
    test(
        "1--",
        Some("(anonymous): Line 1:2 Invalid left-hand side in assignment"),
    );
    test(
        "--1",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "for((1 + 1) in abc) def();",
        Some("(anonymous): Line 1:1 Invalid left-hand side in for-in or for-of"),
    );
    test("[", Some("(anonymous): Line 1:2 Unexpected end of input"));
    test("[,", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test(
        "1 + {",
        Some("(anonymous): Line 1:6 Unexpected end of input"),
    );
    test(
        "1 + { abc:abc",
        Some("(anonymous): Line 1:14 Unexpected end of input"),
    );
    test(
        "1 + { abc:abc,",
        Some("(anonymous): Line 1:15 Unexpected end of input"),
    );
    test(
        "var abc = /\n/",
        Some("(anonymous): Line 1:11 Invalid regular expression: missing /"),
    );
    test(
        "var abc = \"\n",
        Some("(anonymous): Line 1:11 Unexpected token ILLEGAL"),
    );
    test(
        "var if = 0",
        Some("(anonymous): Line 1:5 Unexpected token if"),
    );
    test(
        "abc + 0 = 1",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "+abc = 1",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "1 + (",
        Some("(anonymous): Line 1:6 Unexpected end of input"),
    );
    test(
        "\n\n\n{",
        Some("(anonymous): Line 4:2 Unexpected end of input"),
    );
    test(
        "\n/* Some multiline\ncomment */\n)",
        Some("(anonymous): Line 4:1 Unexpected token )"),
    );
    test(
        "+1 ** 2",
        Some(
            "(anonymous): Line 1:4 Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
        ),
    );
    test(
        "typeof 1 ** 2",
        Some(
            "(anonymous): Line 1:10 Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
        ),
    );
    test(
        "function abc(if) {}",
        Some("(anonymous): Line 1:14 Unexpected token if"),
    );
    test(
        "function abc(true) {}",
        Some("(anonymous): Line 1:14 Unexpected token true"),
    );
    test(
        "function abc(false) {}",
        Some("(anonymous): Line 1:14 Unexpected token false"),
    );
    test(
        "function abc(null) {}",
        Some("(anonymous): Line 1:14 Unexpected token null"),
    );
    test(
        "function null() {}",
        Some("(anonymous): Line 1:10 Unexpected token null"),
    );
    test(
        "function true() {}",
        Some("(anonymous): Line 1:10 Unexpected token true"),
    );
    test(
        "function false() {}",
        Some("(anonymous): Line 1:10 Unexpected token false"),
    );
    test(
        "function if() {}",
        Some("(anonymous): Line 1:10 Unexpected token if"),
    );
    test("a b;", Some("(anonymous): Line 1:3 Unexpected identifier"));
    test("if.a", Some("(anonymous): Line 1:3 Unexpected token ."));
    test("a if", Some("(anonymous): Line 1:3 Unexpected token if"));
    test(
        "a class",
        Some("(anonymous): Line 1:3 Unexpected token class"),
    );
    test(
        "break\n",
        Some("(anonymous): Line 1:1 Illegal break statement"),
    );
    test("break 1;", Some("(anonymous): Line 1:7 Unexpected number"));
    test(
        "for (;;) { break 1; }",
        Some("(anonymous): Line 1:18 Unexpected number"),
    );
    test(
        "continue\n",
        Some("(anonymous): Line 1:1 Illegal continue statement"),
    );
    test(
        "continue 1;",
        Some("(anonymous): Line 1:10 Unexpected number"),
    );
    test(
        "for (;;) { continue 1; }",
        Some("(anonymous): Line 1:21 Unexpected number"),
    );
    test(
        "throw",
        Some("(anonymous): Line 1:1 Unexpected end of input"),
    );
    test("throw;", Some("(anonymous): Line 1:6 Unexpected token ;"));
    test(
        "throw \n",
        Some("(anonymous): Line 1:1 Unexpected end of input"),
    );
    test(
        "for (var abc, def in {});",
        Some("(anonymous): Line 1:19 Unexpected token in"),
    );
    test("for ((abc in {});;);", None);
    test(
        "for ((abc in {}));",
        Some("(anonymous): Line 1:17 Unexpected token )"),
    );
    test(
        "for (+abc in {});",
        Some("(anonymous): Line 1:1 Invalid left-hand side in for-in or for-of"),
    );
    test(
        "if (false)",
        Some("(anonymous): Line 1:11 Unexpected end of input"),
    );
    test(
        "if (false) abc(); else",
        Some("(anonymous): Line 1:23 Unexpected end of input"),
    );
    test("do", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test(
        "while (false)",
        Some("(anonymous): Line 1:14 Unexpected end of input"),
    );
    test(
        "for (;;)",
        Some("(anonymous): Line 1:9 Unexpected end of input"),
    );
    test(
        "with (abc)",
        Some("(anonymous): Line 1:11 Unexpected end of input"),
    );
    test(
        "try {}",
        Some("(anonymous): Line 1:1 Missing catch or finally after try"),
    );
    test(
        "try {} catch () {}",
        Some("(anonymous): Line 1:15 Unexpected token )"),
    );
    test(
        "‿ = 1",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "new abc().\"def\"",
        Some("(anonymous): Line 1:11 Unexpected string"),
    );
    test("/*", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test("/**", Some("(anonymous): Line 1:4 Unexpected end of input"));
    test(
        "/*\n\n\n",
        Some("(anonymous): Line 4:1 Unexpected end of input"),
    );
    test(
        "/*\n\n\n*",
        Some("(anonymous): Line 4:2 Unexpected end of input"),
    );
    test(
        "/*abc",
        Some("(anonymous): Line 1:6 Unexpected end of input"),
    );
    test(
        "/*abc  *",
        Some("(anonymous): Line 1:9 Unexpected end of input"),
    );
    test("\n]", Some("(anonymous): Line 2:1 Unexpected token ]"));
    test("\r\n]", Some("(anonymous): Line 2:1 Unexpected token ]"));
    test("\n\r]", Some("(anonymous): Line 3:1 Unexpected token ]"));
    test("//\r\n]", Some("(anonymous): Line 2:1 Unexpected token ]"));
    test("//\n\r]", Some("(anonymous): Line 3:1 Unexpected token ]"));
    test(
        "/abc\\\n/",
        Some("(anonymous): Line 1:1 Invalid regular expression: missing /"),
    );
    test("//\r \n]", Some("(anonymous): Line 3:1 Unexpected token ]"));
    test(
        "/*\r\n*/]",
        Some("(anonymous): Line 2:3 Unexpected token ]"),
    );
    test(
        "/*\r \n*/]",
        Some("(anonymous): Line 3:3 Unexpected token ]"),
    );
    test(
        "\\\\",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\\u005c",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\\abc",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\\u0000",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\\u200c = []",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\\u200D = []",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\"\\",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "\"\\u",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "return",
        Some("(anonymous): Line 1:1 Illegal return statement"),
    );
    test(
        "continue",
        Some("(anonymous): Line 1:1 Illegal continue statement"),
    );
    test(
        "break",
        Some("(anonymous): Line 1:1 Illegal break statement"),
    );
    test(
        "switch (abc) { default: continue; }",
        Some("(anonymous): Line 1:25 Illegal continue statement"),
    );
    test(
        "do { abc } *",
        Some("(anonymous): Line 1:12 Unexpected token *"),
    );
    test(
        "while (true) { break abc; }",
        Some("(anonymous): Line 1:16 Undefined label 'abc'"),
    );
    test(
        "while (true) { continue abc; }",
        Some("(anonymous): Line 1:16 Undefined label 'abc'"),
    );
    test(
        "abc: while (true) { (function(){ break abc; }); }",
        Some("(anonymous): Line 1:34 Undefined label 'abc'"),
    );
    test(
        "abc: while (true) { (function(){ abc: break abc; }); }",
        None,
    );
    test(
        "abc: while (true) { (function(){ continue abc; }); }",
        Some("(anonymous): Line 1:34 Undefined label 'abc'"),
    );
    test("abc: if (0) break abc; else {}", None);
    test("abc: if (0) { break abc; } else {}", None);
    test("abc: if (0) { break abc } else {}", None);
    test(
        "abc: while (true) { abc: while (true) {} }",
        Some("(anonymous): Line 1:21 Label 'abc' already exists"),
    );
    test("if(0) { do { } while(0) } else { do { } while(0) }", None);
    test("if(0) do { } while(0); else do { } while(0)", None);
    test(
        "_: _: while (true) {]",
        Some("(anonymous): Line 1:4 Label '_' already exists"),
    );
    test(
        "_:\n_:\nwhile (true) {]",
        Some("(anonymous): Line 2:1 Label '_' already exists"),
    );
    test(
        "_:\n   _:\nwhile (true) {]",
        Some("(anonymous): Line 2:4 Label '_' already exists"),
    );
    test(
        "function(){}",
        Some("(anonymous): Line 1:9 Unexpected token ("),
    );
    test(
        "\n/*/",
        Some("(anonymous): Line 2:4 Unexpected end of input"),
    );
    test(
        "/*/.source",
        Some("(anonymous): Line 1:11 Unexpected end of input"),
    );
    test(
        "var class",
        Some("(anonymous): Line 1:5 Unexpected token class"),
    );
    test("var if", Some("(anonymous): Line 1:5 Unexpected token if"));
    test(
        "object Object",
        Some("(anonymous): Line 1:8 Unexpected identifier"),
    );
    test(
        "[object Object]",
        Some("(anonymous): Line 1:9 Unexpected identifier"),
    );
    test(
        "\\u0xyz",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test(
        "for (var abc, def in {}) {}",
        Some("(anonymous): Line 1:19 Unexpected token in"),
    );
    test(
        "for (abc, def in {}) {}",
        Some("(anonymous): Line 1:1 Invalid left-hand side in for-in or for-of"),
    );
    test("for (var abc=def, ghi=(\"abc\" in {}); true;) {}", None);
    test("this\nif (1);", None);
    test("while (1) { break\nif (1); }", None);
    test(
        "throw\nif (1);",
        Some("(anonymous): Line 1:1 Illegal newline after throw"),
    );
    test("(function(){ return\nif (1); })", None);
    test("while (1) { continue\nif (1); }", None);
    test("debugger\nif (1);", None);
    test(
        "class",
        Some("(anonymous): Line 1:6 Unexpected end of input"),
    );
    test("abc.class = 1", None);
    test(
        "var class;",
        Some("(anonymous): Line 1:5 Unexpected token class"),
    );
    test(
        "const",
        Some("(anonymous): Line 1:6 Unexpected end of input"),
    );
    test("abc.const = 1", None);
    test(
        "var const;",
        Some("(anonymous): Line 1:5 Unexpected token const"),
    );
    test(
        "enum",
        Some("(anonymous): Line 1:1 Unexpected reserved word"),
    );
    test("abc.enum = 1", None);
    test(
        "var enum;",
        Some("(anonymous): Line 1:5 Unexpected reserved word"),
    );
    test(
        "export",
        Some("(anonymous): Line 1:1 Unexpected reserved word"),
    );
    test("abc.export = 1", None);
    test(
        "var export;",
        Some("(anonymous): Line 1:5 Unexpected reserved word"),
    );
    test(
        "extends",
        Some("(anonymous): Line 1:1 Unexpected token extends"),
    );
    test("abc.extends = 1", None);
    test(
        "var extends;",
        Some("(anonymous): Line 1:5 Unexpected token extends"),
    );
    test(
        "import",
        Some("(anonymous): Line 1:1 Unexpected reserved word"),
    );
    test("abc.import = 1", None);
    test(
        "var import;",
        Some("(anonymous): Line 1:5 Unexpected reserved word"),
    );
    test(
        "super",
        Some("(anonymous): Line 1:1 'super' keyword unexpected here"),
    );
    test("abc.super = 1", None);
    test(
        "var super;",
        Some("(anonymous): Line 1:5 Unexpected token super"),
    );
    test(
        "\n\t\t\tobj = {\n\t\t\t  aaa: 1\n\t\t\t  bbb: \"string\"\n\t\t\t};",
        Some("(anonymous): Line 4:6 Unexpected identifier"),
    );
    test("{}", None);
    test("{a: 1}", None);
    test("{a: 1,}", Some("(anonymous): Line 1:7 Unexpected token }"));
    test(
        "{a: 1, b: 2}",
        Some("(anonymous): Line 1:9 Unexpected token :"),
    );
    test(
        "{a: 1, b: 2,}",
        Some("(anonymous): Line 1:9 Unexpected token :"),
    );
    test(
        "let f = () => new import('');",
        Some("(anonymous): Line 1:19 Unexpected reserved word"),
    );
    test("implements", None);
    test("abc.implements = 1", None);
    test("var implements;", None);
    test("interface", None);
    test("abc.interface = 1", None);
    test("var interface;", None);
    test("let", None);
    test("abc.let = 1", None);
    test("var let;", None);
    test("package", None);
    test("abc.package = 1", None);
    test("var package;", None);
    test("private", None);
    test("abc.private = 1", None);
    test("var private;", None);
    test("protected", None);
    test("abc.protected = 1", None);
    test("var protected;", None);
    test("public", None);
    test("abc.public = 1", None);
    test("var public;", None);
    test("static", None);
    test("abc.static = 1", None);
    test("var static;", None);
    test("yield", None);
    test("abc.yield = 1", None);
    test("var yield;", None);
    test(
        "0, { get a(param = null) {} };",
        Some("(anonymous): Line 1:11 Getter must not have any formal parameters."),
    );
    test(
        "let{f(",
        Some("(anonymous): Line 1:7 Unexpected end of input"),
    );
    test("`", Some("(anonymous): Line 1:2 Unexpected end of input"));
    test(" `", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test("` ", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test(
        "var{..(",
        Some("(anonymous): Line 1:7 Unexpected token ILLEGAL"),
    );
    test(
        "var{get..(",
        Some("(anonymous): Line 1:10 Unexpected token ILLEGAL"),
    );
    test(
        "var{set..(",
        Some("(anonymous): Line 1:10 Unexpected token ILLEGAL"),
    );
    test(
        "(0 ?? 0 || true)",
        Some(
            "(anonymous): Line 1:9 Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
        ),
    );
    test(
        "(a || b ?? c)",
        Some(
            "(anonymous): Line 1:9 Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
        ),
    );
    test(
        "2 ?? 2 && 3 + 3",
        Some(
            "(anonymous): Line 1:3 Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
        ),
    );
    test(
        "\n\t\tclass C {\n            st\\u0061tic m() {}\n\t\t}\n\t\t",
        Some("(anonymous): Line 3:25 Unexpected identifier"),
    );
}

#[test]
fn test_parser() {
    fn test(source: impl AsRef<[u8]>, chk: Option<&str>) -> ast::Program {
        let source = source.as_ref();
        let mut parser = Parser::new("", source);
        let (program, err) = parser.parse();
        assert_eq!(
            first_err(&err).as_deref(),
            chk,
            "{:?}",
            String::from_utf8_lossy(source)
        );
        program
    }
    test("new (() => {});", None);
    test(
        "\n            abc\n            --\n            []\n        ",
        Some("(anonymous): Line 3:13 Invalid left-hand side in assignment"),
    );
    test("\n            abc--\n            []\n        ", None);
    test("1\n[]\n", Some("(anonymous): Line 2:2 Unexpected token ]"));
    test(
        "\n            function abc() {\n            }\n            abc()\n        ",
        None,
    );
    test("", None);
    test("//", None);
    test("/* */", None);
    test("/** **/", None);
    test("/*****/", None);
    test("/*", Some("(anonymous): Line 1:3 Unexpected end of input"));
    test("#", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test(
        "/**/#",
        Some("(anonymous): Line 1:5 Unexpected token ILLEGAL"),
    );
    test("new +", Some("(anonymous): Line 1:5 Unexpected token +"));
    let program = test(";", None);
    assert_eq!(program.body.len(), 1);
    assert_eq!(empty_semicolon(&program.body[0]), 1);
    let program = test(";;", None);
    assert_eq!(program.body.len(), 2);
    assert_eq!(empty_semicolon(&program.body[0]), 1);
    assert_eq!(empty_semicolon(&program.body[1]), 2);
    let program = test("1.2", None);
    assert_eq!(program.body.len(), 1);
    assert_eq!(number_literal(&program.body[0]), b"1.2");
    let program = test("/* */1.2", None);
    assert_eq!(program.body.len(), 1);
    assert_eq!(number_literal(&program.body[0]), b"1.2");
    let program = test("\n", None);
    assert_eq!(program.body.len(), 0);
    test(
        "\n            if (0) {\n                abc = 0\n            }\n            else abc = 0\n        ",
        None,
    );
    test(
        "if (0) abc = 0 else abc = 0",
        Some("(anonymous): Line 1:16 Unexpected token else"),
    );
    test(
        "\n            if (0) {\n                abc = 0\n            } else abc = 0\n        ",
        None,
    );
    test(
        "\n            if (0) {\n                abc = 1\n            } else {\n            }\n        ",
        None,
    );
    test(
        "\n            do {\n            } while (true)\n        ",
        None,
    );
    test(
        "\n            try {\n            } finally {\n            }\n        ",
        None,
    );
    test(
        "\n            try {\n            } catch (abc) {\n            } finally {\n            }\n        ",
        None,
    );
    test(
        "\n            try {\n            }\n            catch (abc) {\n            }\n            finally {\n            }\n        ",
        None,
    );
    test("try {} catch (abc) {} finally {}", None);
    test("try {} catch {}", None);
    test(
        "\n            do {\n                do {\n                } while (0)\n            } while (0)\n        ",
        None,
    );
    test(
        "\n            (function(){\n                try {\n                    if (\n                        1\n                    ) {\n                        return 1\n                    }\n                    return 0\n                } finally {\n                }\n            })()\n        ",
        None,
    );
    test("abc = ''\ndef", None);
    test("abc = 1\ndef", None);
    test("abc = Math\ndef", None);
    test("\"\\'\"", None);
    test(
        "\n            abc = function(){\n            }\n            abc = 0\n        ",
        None,
    );
    test("abc.null = 0", None);
    test("0x41", None);
    test("\"\\d\"", None);
    test("(function(){return this})", None);
    test(
        "\n            Object.defineProperty(Array.prototype, \"0\", {\n                value: 100,\n                writable: false,\n                configurable: true\n            });\n            abc = [101];\n            abc.hasOwnProperty(\"0\") && abc[0] === 101;\n        ",
        None,
    );
    test("new abc()", None);
    test("new {}", None);
    test(
        "\n            limit = 4\n            result = 0\n            while (limit) {\n                limit = limit - 1\n                if (limit) {\n                }\n                else {\n                    break\n                }\n                result = result + 1\n            }\n        ",
        None,
    );
    test(
        "\n            while (0) {\n                if (0) {\n                    continue\n                }\n            }\n        ",
        None,
    );
    test("var abc = 0", None);
    test("var test7_3_1\nabc = 66;", None);
    test("var test7_3_1\u{2028}abc = 66;", None);
    test(
        "//\u{2028} =;",
        Some("(anonymous): Line 2:2 Unexpected token ="),
    );
    test(
        "var abc = \u{2029};",
        Some("(anonymous): Line 2:1 Unexpected token ;"),
    );
    test(
        "var abc = \\u2029;",
        Some("(anonymous): Line 1:11 Unexpected token ILLEGAL"),
    );
    test("var \\u0061\\u0062\\u0063 = 0;", None);
    test("'", Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"));
    test(
        "'\nstr\ning\n'",
        Some("(anonymous): Line 1:1 Unexpected token ILLEGAL"),
    );
    test("var $\\u0030 = 0;", None);
    test(
        "switch = 1",
        Some("(anonymous): Line 1:8 Unexpected token ="),
    );
    test(".0 === 0.0", None);
    test(
        "var regExp = /\\\rn/;",
        Some("(anonymous): Line 1:14 Invalid regular expression: missing /"),
    );
    test("var regExp = /=/;", None);
    test("/*/", Some("(anonymous): Line 1:4 Unexpected end of input"));
    test(
        "\n            do {\n            ;\n            } while (false) true\n        ",
        None,
    );
    test("\n            {a:1\n            } 3\n        ", None);
    test("\n            abc\n            ++def\n        ", None);
    test(
        "\n            for(false;false\n            ) {\n            break;\n            }\n        ",
        Some("(anonymous): Line 3:13 Unexpected token )"),
    );
    test(
        "\n            do {};\n            while (false)\n        ",
        Some("(anonymous): Line 2:18 Unexpected token ;"),
    );
    test("\n            \"x\\0y\"\n        ", None);
    test("\n            10e10000\n        ", None);
    test("\n            \"abc\\\n            def\"\n        ", None);
    test("'\\\n'", None);
    test("'\\\r\n'", None);
    test(
        "42 = 42;",
        Some("(anonymous): Line 1:1 Invalid left-hand side in assignment"),
    );
    test(
        "s &^= 42;",
        Some("(anonymous): Line 1:4 Unexpected token ^="),
    );
    test("\n            abc /= \"1\"\n        ", None);
    test(
        "\n            try{};catch(){}\n        ",
        Some("(anonymous): Line 2:13 Missing catch or finally after try"),
    );
    test(
        "\n            try{};finally{}\n        ",
        Some("(anonymous): Line 2:13 Missing catch or finally after try"),
    );
    test(
        "\n            while (true) {\n                break abc;\n            }\n        ",
        Some("(anonymous): Line 3:17 Undefined label 'abc'"),
    );
    test(
        "var x / = 1;",
        Some("(anonymous): Line 1:7 Unexpected token /"),
    );
    test(
        "\n            function abc() {\n                if (0)\n                    return;\n                else {\n                }\n            }\n        ",
        None,
    );
    test(
        "//\u{2028} var =;",
        Some("(anonymous): Line 2:6 Unexpected token ="),
    );
    test(
        "\n            throw\n            {}\n        ",
        Some("(anonymous): Line 2:13 Illegal newline after throw"),
    );
    test(
        "\n            function = 1\n        ",
        Some("(anonymous): Line 2:22 Unexpected token ="),
    );
    test("0e1", None);
    test(
        "abc = 1; abc\n++",
        Some("(anonymous): Line 2:3 Unexpected end of input"),
    );
    test("({ get abc() {} })", None);
    test("for (abc.def in {}) {}", None);
    test("while (true) { break }", None);
    test("while (true) { continue }", None);
    test(
        "abc=/^(?:(\\w+:)\\/{2}(\\w+(?:\\.\\w+)*\\/?)|(.{0,2}\\/{1}))?([/.]*?(?:[^?]+)?\\/)?((?:[^/?]+)\\.(\\w+))(?:\\?(\\S+)?)?$/,def=/^(?:(\\w+:)\\/{2})|(.{0,2}\\/{1})?([/.]*?(?:[^?]+)?\\/?)?$/",
        None,
    );
    test(
        "(function() { try {} catch (err) {} finally {} return })",
        None,
    );
    test("0xde0b6b3a7640080.toFixed(0)", None);
    test(
        "/[^-._0-9A-Za-z\\xb7\\xc0-\\xd6\\xd8-\\xf6\\xf8-\\u037d\\u37f-\\u1fff\\u200c-\\u200d\\u203f\\u2040\\u2070-\\u218f]/",
        None,
    );
    test(
        "/[\\u0000-\\u0008\\u000B-\\u000C\\u000E-\\u001F\\uD800-\\uDFFF\\uFFFE-\\uFFFF]/",
        None,
    );
    test("var abc = 1;\u{feff}", None);
    test("\u{feff}/* var abc = 1; */", None);
    test(
        "if (-0x8000000000000000<=abc&&abc<=0x8000000000000000) {}",
        None,
    );
    test("(function(){debugger;return this;})", None);
    test("\n\n        ", None);
    test(
        "\n            var abc = \"\"\n            debugger\n        ",
        None,
    );
    test(
        "\n            var abc = /\\[\\]$/\n            debugger\n        ",
        None,
    );
    test(
        "\n            var abc = 1 /\n                2\n            debugger\n        ",
        None,
    );
    test("'ё\\\u{2029}'", None);
    test("[a, b] = [1, 2]", None);
    test("({\"a b\": {}} = {})", None);
    test("ref = (a, b = 39,) => {\n\t\t};", None);
    test("(a,) => {}", None);
    test("2 ?? (2 && 3) + 3", None);
    test("(2 ?? 2) && 3 + 3", None);
    let program = test("a ?? b ?? c", None);
    assert_eq!(program.body.len(), 1);
    let ast::Statement::ExpressionStatement(st) = &program.body[0] else {
        panic!("not an expression statement");
    };
    let ast::Expression::BinaryExpression(bin) = &st.expression else {
        panic!("not a binary expression");
    };
    let ast::Expression::Identifier(id) = &bin.right else {
        panic!("not an identifier");
    };
    assert_eq!(id.name.string(), "c");
    let program = test(
        "\n\t\tclass C {\n\t\t\ta\n\t\t\tb\n\t\t\t#c\n\t\t\tm() {\n\t\t\t\treturn this.#c;\n\t\t\t}\n\t\t}\n\t\t",
        None,
    );
    assert_eq!(program.body.len(), 1);
    test(
        "function f() { if (true) { return class A {} } let A; if (true) { A = class A {} } }",
        None,
    );
    test("function f() { if (true) return class A {} }", None);
    test("function f() { let A; if (true) A = class A {} }", None);
    test(
        "function f() { if (false) {} else return class A {} }",
        None,
    );
    test(
        "function f() { let A; if (false) {} else A = class A {} }",
        None,
    );
    {
        let program = test("(-2)**53", None);
        let ast::Statement::ExpressionStatement(st) = &program.body[0] else {
            panic!("not an expression statement");
        };
        let ast::Expression::BinaryExpression(st) = &st.expression else {
            panic!("not a binary expression");
        };
        assert_eq!(st.operator, token::EXPONENT);
        let ast::Expression::UnaryExpression(left) = &st.left else {
            panic!("not a unary expression");
        };
        assert_eq!(left.operator, token::MINUS);
        let ast::Expression::NumberLiteral(op1) = &left.operand else {
            panic!("not a number literal");
        };
        assert_eq!(op1.literal, b"2");
        let ast::Expression::NumberLiteral(right) = &st.right else {
            panic!("not a number literal");
        };
        assert_eq!(right.literal, b"53");
    }
}

#[test]
fn test_parse_destruct() {
    let mut parser = Parser::new("", b"({a: (a.b), ...spread,} = {})");
    let (_, err) = parser.parse();
    if let Some(err) = err {
        panic!("{err}");
    }
}

#[test]
fn test_parse_string_literal() {
    let test = |have: &str, want: &str| {
        let mut parser = Parser::new("", have.as_bytes());
        parser.read();
        parser.read();
        let (_, res, err) = parser.scan_string(0, true);
        assert_eq!(err, "", "{have:?}");
        assert_eq!(res.string(), want, "{have:?}");
    };
    test(r#""""#, "");
    test("/=/", "=");
    test("'1(\\\\d+)'", "1(\\d+)");
    test("'\\u2029'", "\u{2029}");
    test("'abc\\uFFFFabc'", "abc\u{FFFF}abc");
    test(
        "'[First line \\\nSecond line \\\n Third line\\\n.     ]'",
        "[First line Second line  Third line.     ]",
    );
    test("'\\u007a\\x79\\u000a\\x78'", "zy\nx");
    test("'\\a'", "a");
    test("'\u{0410}'", "\u{0410}");
    test("'\\0'", "\u{0}");
    test("'\u{0}'", "\u{0}");
    test("\"'abc'\\\n'def'\"", "'abc''def'");
    test("\"'abc'\\\r\n'def'\"", "'abc''def'");
    test("'\\0'", "\u{0}");
    test("'\\00'", "\u{0}");
    test("'\\000'", "\u{0}");
    test("'\\09'", "\u{0}9");
    test("'\\009'", "\u{0}9");
    test("'\\0009'", "\u{0}9");
    test("'\\1'", "\u{1}");
    test("'\\01'", "\u{1}");
    test("'\\001'", "\u{1}");
    test("'\\0011'", "\u{1}1");
    test("'\\1abc'", "\u{1}abc");
    test("'\\\u{4e16}'", "\u{4e16}");

    let test = |have: &str, want: &str| {
        let mut parser = Parser::new("", have.as_bytes());
        parser.read();
        parser.read();
        let (_, res, err) = parser.scan_string(0, true);
        assert_eq!(err, want, "{have:?}");
        assert_eq!(res.string(), "", "{have:?}");
    };
    test(r#""\u""#, r#"invalid escape: \u: len("") != 4"#);
    test(r#""\u0""#, r#"invalid escape: \u: len("0") != 4"#);
    test(r#""\u00""#, r#"invalid escape: \u: len("00") != 4"#);
    test(r#""\u000""#, r#"invalid escape: \u: len("000") != 4"#);
    test(r#""\x""#, r#"invalid escape: \x: len("") != 2"#);
    test(r#""\x0""#, r#"invalid escape: \x: len("0") != 2"#);
}

#[test]
fn test_parse_number_literal() {
    let test = |input: &str, expect: &str| {
        let result =
            parse_number_literal(input.as_bytes()).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(result.to_string(), expect, "{input}");
    };
    test("0", "0");
    test("0x8000000000000000", "9.223372036854776e+18");
    test("1n", &Int::new_int(1).string());
    test("-1n", &Int::new_int(-1).string());
    test("0x23n", &Int::new_int(35).string());
    test("0xabcdef01n", &Int::new_int(2882400001).string());
    let n = Int::set_string(b"0xabcdef0123456789abcdef0123", 0).expect("big");
    test("0xabcdef0123456789abcdef0123n", &n.string());
}

#[test]
fn test_position() {
    let parser = Parser::new("", b"// Lorem ipsum");
    assert_eq!(parser.slice(0, 1), b"");
    assert_eq!(parser.slice(0, 10), b"");
    assert_eq!(parser.slice(1, 128), b"");
    assert_eq!(&parser.str[0..0], b"");
    assert_eq!(parser.slice(1, 1), b"");
    assert_eq!(&parser.str[0..1], b"/");
    assert_eq!(parser.slice(1, 2), b"/");
    assert_eq!(&parser.str[0..14], b"// Lorem ipsum");
    assert_eq!(parser.slice(1, 15), b"// Lorem ipsum");

    let parse = |src: &str| -> (Parser, ast::Program) {
        let mut parser = Parser::new("", src.as_bytes());
        let (program, err) = parser.parse();
        assert!(err.is_none(), "{src}: {err:?}");
        (parser, program)
    };
    let expr = |program: &ast::Program| -> ast::Expression {
        let ast::Statement::ExpressionStatement(s) = &program.body[0] else {
            panic!("not an expression statement");
        };
        s.expression.clone()
    };
    let func_body = |program: &ast::Program| -> ast::Statement {
        let ast::Expression::FunctionLiteral(f) = expr(program) else {
            panic!("not a function literal");
        };
        f.body.list[0].clone()
    };

    let (parser, program) = parse("(function(){ return 0; })");
    let node = expr(&program);
    assert_eq!(node.idx0(), 2);
    assert_eq!(node.idx1(), 25);
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"function(){ return 0; }"
    );
    assert_eq!(
        parser.slice(node.idx0(), node.idx1() + 1),
        b"function(){ return 0; })"
    );
    assert_eq!(parser.slice(node.idx0(), node.idx1() + 2), b"");
    let ast::Expression::FunctionLiteral(f) = &node else {
        panic!("not a function literal");
    };
    assert_eq!(f.source, b"function(){ return 0; }");
    assert_eq!(program.idx0(), 2);
    assert_eq!(program.idx1(), 25);
    assert_eq!(
        parser.slice(program.idx0(), program.idx1()),
        b"function(){ return 0; }"
    );

    let (parser, program) = parse("(function(){ return abc; })");
    let ast::Expression::FunctionLiteral(f) = expr(&program) else {
        panic!("not a function literal");
    };
    assert_eq!(f.source, b"function(){ return abc; }");
    let node = func_body(&program);
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"return abc");

    let (parser, program) = parse("(function(){ return; })");
    let node = func_body(&program);
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"return");

    let (parser, program) = parse("true ? 1 : 2");
    let node = expr(&program);
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"true ? 1 : 2");

    let (parser, program) = parse("a++");
    let node = expr(&program);
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"a++");

    let (parser, program) = parse("++a");
    let node = expr(&program);
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"++a");

    let (parser, program) = parse("xyz: for (i = 0; i < 10; i++) { if (i == 5) continue xyz; }");
    let node = &program.body[0];
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"xyz: for (i = 0; i < 10; i++) { if (i == 5) continue xyz; }"
    );
    let ast::Statement::LabelledStatement(l) = node else {
        panic!("not a labelled statement");
    };
    let node = &l.statement;
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"for (i = 0; i < 10; i++) { if (i == 5) continue xyz; }"
    );
    let ast::Statement::ForStatement(f) = node else {
        panic!("not a for statement");
    };
    let ast::Statement::BlockStatement(block) = &f.body else {
        panic!("not a block");
    };
    let ast::Statement::IfStatement(i) = &block.list[0] else {
        panic!("not an if statement");
    };
    let node = &i.consequent;
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"continue xyz");

    let (parser, program) = parse("for (p in o) { break; }");
    let node = &program.body[0];
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"for (p in o) { break; }"
    );
    let ast::Statement::ForInStatement(f) = node else {
        panic!("not a for-in statement");
    };
    let ast::Statement::BlockStatement(block) = &f.body else {
        panic!("not a block");
    };
    let node = &block.list[0];
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"break");

    let (parser, program) = parse("while (i < 10) { i++; }");
    let node = &program.body[0];
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"while (i < 10) { i++; }"
    );

    let (parser, program) = parse("do { i++; } while (i < 10 )");
    let node = &program.body[0];
    assert_eq!(
        parser.slice(node.idx0(), node.idx1()),
        b"do { i++; } while (i < 10 )"
    );

    let (parser, program) = parse("with (1) {}");
    let node = &program.body[0];
    assert_eq!(parser.slice(node.idx0(), node.idx1()), b"with (1) {}");

    let src = "switch (a) {\n\tcase 1: x--;\n\tcase 2:\n\tdefault: x++;\n}";
    let (parser, program) = parse(src);
    let node = &program.body[0];
    assert_eq!(parser.slice(node.idx0(), node.idx1()), src.as_bytes());
}

#[test]
fn test_extract_source_map_line() {
    assert_eq!(extract_source_map_line(b""), b"");
    assert_eq!(extract_source_map_line(b"\n"), b"");
    assert_eq!(extract_source_map_line(b" "), b"");
    assert_eq!(extract_source_map_line(b"1\n2\n3\n4\n"), b"");
    let src = "\"use strict\";\nvar x = {};\n//# sourceMappingURL=delme.js.map";
    let mod_src = format!("(function(exports, require, module) {{{src}\n}})");
    assert_eq!(
        extract_source_map_line(mod_src.as_bytes()),
        b"//# sourceMappingURL=delme.js.map"
    );
    assert_eq!(
        extract_source_map_line(format!("{mod_src}\n\n\n\n").as_bytes()),
        b"//# sourceMappingURL=delme.js.map"
    );
}

#[test]
fn test_source_map_options() {
    let state = Arc::new(Mutex::new((0, String::new())));
    let s2 = Arc::clone(&state);
    let loader: SourceMapLoader = Arc::new(move |p: &str| {
        let mut st = s2.lock().expect("lock");
        st.0 += 1;
        st.1 = p.to_string();
        Ok(None)
    });
    let opt = with_source_map_loader(loader);
    let src = b"\"use strict\";\nvar x = {};\n//# sourceMappingURL=delme.js.map";
    let count = || state.lock().expect("lock").0;
    let requested = || state.lock().expect("lock").1.clone();
    let reset = || state.lock().expect("lock").0 = 0;

    let (_, err) = parse_file(None, "delme.js", src, 0, &[Arc::clone(&opt)]);
    assert!(err.is_none());
    assert_eq!(count(), 1);
    assert_eq!(requested(), "delme.js.map");

    reset();
    let (_, err) = parse_file(None, "", src, 0, &[Arc::clone(&opt)]);
    assert!(err.is_none());
    assert_eq!(count(), 1);
    assert_eq!(requested(), "delme.js.map");

    reset();
    let disable: ParserOption = Arc::new(with_disable_source_maps);
    let (_, err) = parse_file(None, "delme.js", src, 0, &[disable]);
    assert!(err.is_none());
    assert_eq!(count(), 0);

    let (_, err) = parse_file(None, "/home/user/src/delme.js", src, 0, &[Arc::clone(&opt)]);
    assert!(err.is_none());
    assert_eq!(count(), 1);
    assert_eq!(requested(), "/home/user/src/delme.js.map");

    reset();
    let (_, err) = parse_file(
        None,
        "https://site.com/delme.js",
        src,
        0,
        &[Arc::clone(&opt)],
    );
    assert!(err.is_none());
    assert_eq!(count(), 1);
    assert_eq!(requested(), "https://site.com/delme.js.map");
}

#[test]
fn test_parse_template_characters() {
    let mut parser = Parser::new("", b"`test\\\r\\\n${a}`");
    parser.next();
    assert_eq!(parser.token, token::BACKTICK, "Token: {}", parser.token);
    let check = |parser: &mut Parser,
                 expected_literal: &[u8],
                 expected_parsed: &str,
                 expected_finished: bool,
                 expect_parse_err: bool,
                 expect_err: bool| {
        let (literal, parsed, finished, parse_err, err) = parser.parse_template_characters();
        assert_eq!(!err.is_empty(), expect_err, "{err}");
        assert_eq!(
            literal,
            expected_literal,
            "Literal: {:?}",
            String::from_utf8_lossy(&literal)
        );
        assert_eq!(parsed.string(), expected_parsed, "Parsed: {parsed:?}");
        assert_eq!(finished, expected_finished);
        assert_eq!(
            !parse_err.is_empty(),
            expect_parse_err,
            "parseErr: {parse_err}"
        );
    };
    check(&mut parser, b"test\\\n\\\n", "test", false, false, false);
    parser.next();
    parser.expect(token::IDENTIFIER);
    assert!(parser.errors.is_empty(), "{:?}", parser.errors);
    assert_eq!(parser.token, token::RIGHT_BRACE, "Expected }}");
    assert!(parser.errors.is_empty(), "{:?}", parser.errors);
    check(&mut parser, b"", "", true, false, false);
    assert_eq!(parser.chr, -1, "Expected EOF");
}

#[test]
fn test_parse_template_literal() {
    let mut parser = Parser::new("", b"f()\n`test${a}`");
    let (prg, err) = parser.parse();
    assert!(err.is_none(), "{err:?}");
    let ast::Statement::ExpressionStatement(st) = &prg.body[0] else {
        panic!("{:?}", prg.body[0]);
    };
    let ast::Expression::TemplateLiteral(expr) = &st.expression else {
        panic!("{st:?}");
    };
    let tag = expr.tag.as_ref().expect("tag is nil");
    assert_eq!(tag.idx0(), 1, "Tag.Idx0(): {}", tag.idx0());
    assert_eq!(expr.open_quote, 5, "OpenQuote: {}", expr.open_quote);
    assert_eq!(expr.close_quote, 14, "CloseQuote: {}", expr.close_quote);
    assert_eq!(
        expr.elements.len(),
        2,
        "len elements: {}",
        expr.elements.len()
    );
    assert_eq!(
        expr.expressions.len(),
        1,
        "len expressions: {}",
        expr.expressions.len()
    );
}

#[test]
fn test_parse_template_literal_with_tail() {
    let mut parser = Parser::new("", b"f()\n`test${a}tail` ");
    let (prg, err) = parser.parse();
    assert!(err.is_none(), "{err:?}");
    let ast::Statement::ExpressionStatement(st) = &prg.body[0] else {
        panic!("{:?}", prg.body[0]);
    };
    let ast::Expression::TemplateLiteral(expr) = &st.expression else {
        panic!("{st:?}");
    };
    assert_eq!(expr.close_quote, 18, "CloseQuote: {}", expr.close_quote);
}
