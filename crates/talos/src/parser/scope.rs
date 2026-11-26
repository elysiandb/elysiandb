use super::parser::Parser;
use crate::ast;
use crate::unistring;

#[derive(Default)]
pub struct Scope {
    pub outer: Option<Box<Scope>>,
    pub allow_in: bool,
    pub allow_let: bool,
    pub in_iteration: bool,
    pub in_switch: bool,
    pub in_func_params: bool,
    pub in_function: bool,
    pub in_async: bool,
    pub allow_await: bool,
    pub allow_yield: bool,
    pub declaration_list: Vec<ast::VariableDeclaration>,
    pub labels: Vec<unistring::String>,
}

impl Parser {
    pub fn open_scope(&mut self) {
        let outer = std::mem::take(&mut self.scope);
        self.scope = Box::new(Scope {
            outer: Some(outer),
            allow_in: true,
            ..Scope::default()
        });
    }

    pub fn close_scope(&mut self) -> Box<Scope> {
        let outer = self.scope.outer.take().unwrap_or_default();
        std::mem::replace(&mut self.scope, outer)
    }
}

impl Scope {
    pub fn declare(&mut self, declaration: ast::VariableDeclaration) {
        self.declaration_list.push(declaration);
    }

    pub fn has_label(&self, name: &unistring::String) -> bool {
        if self.labels.iter().any(|l| l == name) {
            return true;
        }
        if let Some(outer) = &self.outer
            && !self.in_function
        {
            return outer.has_label(name);
        }
        false
    }
}
