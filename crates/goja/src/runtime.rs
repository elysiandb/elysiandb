use crate::ast;
use crate::compiler::{Compiler, CompilerError, CompilerSyntaxError, EvalContext, Program};
use crate::parser::{self, ParserOption};
use std::rc::Rc;
use std::sync::Arc;

pub fn compile(name: &str, src: &[u8], strict: bool) -> Result<Arc<Program>, CompilerSyntaxError> {
    compile_with(name, src, strict, true, None, &[])
}

pub fn compile_ast(prg: &ast::Program, strict: bool) -> Result<Arc<Program>, CompilerSyntaxError> {
    compile_ast_with(prg, strict, true, None)
}

pub fn must_compile(name: &str, src: &[u8], strict: bool) -> Arc<Program> {
    match compile(name, src, strict) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

pub fn parse(
    name: &str,
    src: &[u8],
    options: &[ParserOption],
) -> Result<ast::Program, CompilerSyntaxError> {
    let (prg, err) = parser::parse_file(None, name, src, 0, options);
    match err {
        Some(err1) => Err(CompilerSyntaxError(CompilerError {
            message: err1.error().into_bytes(),
            file: None,
            offset: 0,
        })),
        None => Ok(prg),
    }
}

pub fn compile_with(
    name: &str,
    src: &[u8],
    strict: bool,
    in_global: bool,
    eval_vm: Option<Rc<dyn EvalContext>>,
    parser_options: &[ParserOption],
) -> Result<Arc<Program>, CompilerSyntaxError> {
    let prg = parse(name, src, parser_options)?;
    compile_ast_with(&prg, strict, in_global, eval_vm)
}

pub fn compile_ast_with(
    prg: &ast::Program,
    strict: bool,
    in_global: bool,
    eval_vm: Option<Rc<dyn EvalContext>>,
) -> Result<Arc<Program>, CompilerSyntaxError> {
    let mut c = Compiler::new();
    c.compile(prg, strict, in_global, eval_vm)?;
    Ok(c.link())
}
