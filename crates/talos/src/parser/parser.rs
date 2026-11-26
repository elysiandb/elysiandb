use super::error::ErrorList;
use super::scope::Scope;
use crate::ast;
use crate::file::{File, FileSet, Idx, Position};
use crate::token::{self, Token};
use crate::unistring;
use gocompat::utf8::Rune;
use std::sync::Arc;

pub type Mode = u32;

pub const IGNORE_REG_EXP_ERRORS: Mode = 1;

pub type LoaderError = Box<dyn std::error::Error + Send + Sync>;

pub type SourceMapLoader = Arc<dyn Fn(&str) -> Result<Option<Vec<u8>>, LoaderError> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Options {
    pub disable_source_maps: bool,
    pub source_map_loader: Option<SourceMapLoader>,
}

pub type ParserOption = Arc<dyn Fn(&mut Options) + Send + Sync>;

pub fn with_disable_source_maps(opts: &mut Options) {
    opts.disable_source_maps = true;
}

pub fn with_source_map_loader(loader: SourceMapLoader) -> ParserOption {
    Arc::new(move |opts: &mut Options| {
        opts.source_map_loader = Some(Arc::clone(&loader));
    })
}

#[derive(Default, Clone, Copy)]
pub(super) struct Recover {
    pub idx: Idx,
    pub count: i64,
}

pub struct Parser {
    pub str: Vec<u8>,
    pub length: usize,
    pub base: i64,
    pub chr: Rune,
    pub chr_offset: usize,
    pub offset: usize,
    pub idx: Idx,
    pub token: Token,
    pub literal: Vec<u8>,
    pub parsed_literal: unistring::String,
    pub scope: Box<Scope>,
    pub insert_semicolon: bool,
    pub implicit_semicolon: bool,
    pub errors: ErrorList,
    pub(super) recover: Recover,
    pub mode: Mode,
    pub opts: Options,
    pub file: Arc<File>,
}

impl Parser {
    pub fn with_options(filename: &str, src: &[u8], base: i64, options: &[ParserOption]) -> Parser {
        let mut p = Parser {
            str: src.to_vec(),
            length: src.len(),
            base,
            chr: Rune::from(b' '),
            chr_offset: 0,
            offset: 0,
            idx: 0,
            token: Token(0),
            literal: Vec::new(),
            parsed_literal: unistring::String::default(),
            scope: Box::default(),
            insert_semicolon: false,
            implicit_semicolon: false,
            errors: ErrorList::default(),
            recover: Recover::default(),
            mode: 0,
            opts: Options::default(),
            file: Arc::new(File::new(filename, src, base)),
        };
        for opt in options {
            opt(&mut p.opts);
        }
        p
    }

    pub fn new(filename: &str, src: &[u8]) -> Parser {
        Parser::with_options(filename, src, 1, &[])
    }

    pub fn slice(&self, idx0: Idx, idx1: Idx) -> Vec<u8> {
        let from = idx0 - self.base;
        let to = idx1 - self.base;
        if from >= 0 && to <= self.str.len() as i64 && from <= to {
            return self.str[from as usize..to as usize].to_vec();
        }
        if from >= 0 && to <= self.str.len() as i64 {
            panic!("slice bounds out of range [{from}:{to}]");
        }
        Vec::new()
    }

    pub fn parse(&mut self) -> (ast::Program, Option<ErrorList>) {
        self.open_scope();
        self.next();
        let program = self.parse_program();
        self.close_scope();
        (program, self.errors.clone().err())
    }

    pub fn next(&mut self) {
        let (tkn, literal, parsed, idx) = self.scan();
        self.token = tkn;
        self.literal = literal;
        self.parsed_literal = parsed;
        self.idx = idx;
    }

    pub(super) fn optional_semicolon(&mut self) {
        if self.token == token::SEMICOLON {
            self.next();
            return;
        }
        if self.implicit_semicolon {
            self.implicit_semicolon = false;
            return;
        }
        if self.token != token::EOF && self.token != token::RIGHT_BRACE {
            self.expect(token::SEMICOLON);
        }
    }

    pub(super) fn semicolon(&mut self) {
        if self.token != token::RIGHT_PARENTHESIS && self.token != token::RIGHT_BRACE {
            if self.implicit_semicolon {
                self.implicit_semicolon = false;
                return;
            }
            self.expect(token::SEMICOLON);
        }
    }

    pub fn idx_of(&self, offset: usize) -> Idx {
        self.base + offset as Idx
    }

    pub fn expect(&mut self, value: Token) -> Idx {
        let idx = self.idx;
        if self.token != value {
            self.error_unexpected_token(self.token);
        }
        self.next();
        idx
    }

    pub fn position(&self, idx: Idx) -> Position {
        self.file.position(idx - self.base)
    }
}

pub fn parse_file(
    file_set: Option<&mut FileSet>,
    filename: &str,
    src: &[u8],
    mode: Mode,
    options: &[ParserOption],
) -> (ast::Program, Option<ErrorList>) {
    let mut base = 1;
    if let Some(fs) = file_set {
        base = fs.add_file(filename, src);
    }
    let mut parser = Parser::with_options(filename, src, base, options);
    parser.mode = mode;
    parser.parse()
}

pub fn parse_function(
    parameter_list: &str,
    body: &str,
    options: &[ParserOption],
) -> Result<ast::FunctionLiteral, ErrorList> {
    let src = format!("(function({parameter_list}) {{\n{body}\n}})");
    let mut parser = Parser::with_options("", src.as_bytes(), 1, options);
    let (mut program, err) = parser.parse();
    if let Some(err) = err {
        return Err(err);
    }
    let stmt = program.body.swap_remove(0);
    let ast::Statement::ExpressionStatement(stmt) = stmt else {
        panic!("interface conversion: the first statement is not an expression statement");
    };
    let ast::Expression::FunctionLiteral(f) = stmt.expression else {
        panic!("interface conversion: the expression is not a function literal");
    };
    Ok(*f)
}
