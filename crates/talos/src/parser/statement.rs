use super::error::Place;
use super::parser::Parser;
use crate::ast::*;
use crate::deps::sourcemap::{self, Consumer};
use crate::file::{Idx, resolve_sourcemap_url};
use crate::token::{self, Token};
use gocompat::base64::STD_ENCODING;
use gocompat::fmt::Arg;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

fn bad_statement(from: Idx, to: Idx) -> Statement {
    Statement::BadStatement(Box::new(BadStatement { from, to }))
}

fn read_file(path: &[u8]) -> Result<Vec<u8>, String> {
    let name = String::from_utf8_lossy(path).into_owned();
    let os_path = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
    let mut f = std::fs::File::open(os_path).map_err(|e| format!("open {name}: {e}"))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| format!("read {name}: {e}"))?;
    Ok(data)
}

pub fn extract_source_map_line(s: &[u8]) -> Vec<u8> {
    let mut s = s;
    loop {
        let p = s.iter().rposition(|&c| c == b'\n');
        let line = match p {
            Some(p) => &s[p + 1..],
            None => s,
        };
        if !line.is_empty() && line != b"})" {
            if line.starts_with(b"//# sourceMappingURL=") {
                return line.to_vec();
            }
            break;
        }
        match p {
            Some(p) => s = &s[..p],
            None => break,
        }
    }
    Vec::new()
}

impl Parser {
    pub(super) fn parse_block_statement(&mut self) -> Box<BlockStatement> {
        let left_brace = self.expect(token::LEFT_BRACE);
        let list = self.parse_statement_list();
        let right_brace = self.expect(token::RIGHT_BRACE);
        Box::new(BlockStatement {
            left_brace,
            list,
            right_brace,
        })
    }

    fn parse_empty_statement(&mut self) -> Statement {
        let idx = self.expect(token::SEMICOLON);
        Statement::EmptyStatement(Box::new(EmptyStatement { semicolon: idx }))
    }

    fn parse_statement_list(&mut self) -> Vec<Statement> {
        let mut list = Vec::new();
        while self.token != token::RIGHT_BRACE && self.token != token::EOF {
            self.scope.allow_let = true;
            list.push(self.parse_statement());
        }
        list
    }

    pub(super) fn parse_statement(&mut self) -> Statement {
        if self.token == token::EOF {
            self.error_unexpected_token(self.token);
            return bad_statement(self.idx, self.idx + 1);
        }
        match self.token {
            token::SEMICOLON => return self.parse_empty_statement(),
            token::LEFT_BRACE => return Statement::BlockStatement(self.parse_block_statement()),
            token::IF => return self.parse_if_statement(),
            token::DO => return self.parse_do_while_statement(),
            token::WHILE => return self.parse_while_statement(),
            token::FOR => return self.parse_for_or_for_in_statement(),
            token::BREAK => return self.parse_break_statement(),
            token::CONTINUE => return self.parse_continue_statement(),
            token::DEBUGGER => return self.parse_debugger_statement(),
            token::WITH => return self.parse_with_statement(),
            token::VAR => return Statement::VariableStatement(self.parse_variable_statement()),
            token::LET => {
                let tok = self.peek();
                if tok == token::LEFT_BRACKET
                    || self.scope.allow_let && (token::is_id(tok) || tok == token::LEFT_BRACE)
                {
                    return Statement::LexicalDeclaration(
                        self.parse_lexical_declaration(self.token),
                    );
                }
                self.insert_semicolon = true;
            }
            token::CONST => {
                return Statement::LexicalDeclaration(self.parse_lexical_declaration(self.token));
            }
            token::ASYNC => {
                if let Some(f) = self.parse_maybe_async_function(true) {
                    return Statement::FunctionDeclaration(Box::new(FunctionDeclaration {
                        function: f,
                    }));
                }
            }
            token::FUNCTION => {
                let idx = self.idx;
                let f = self.parse_function(true, false, idx);
                return Statement::FunctionDeclaration(Box::new(FunctionDeclaration {
                    function: f,
                }));
            }
            token::CLASS => {
                let c = self.parse_class(true);
                return Statement::ClassDeclaration(Box::new(ClassDeclaration { class: c }));
            }
            token::SWITCH => return self.parse_switch_statement(),
            token::RETURN => return self.parse_return_statement(),
            token::THROW => return self.parse_throw_statement(),
            token::TRY => return self.parse_try_statement(),
            _ => {}
        }
        let expression = self.parse_expression();
        if let Expression::Identifier(identifier) = expression {
            if self.token == token::COLON {
                let colon = self.idx;
                self.next();
                let label = identifier.name.clone();
                for value in self.scope.labels.clone() {
                    if label == value {
                        let s = label.string();
                        self.error(
                            Place::Idx(identifier.idx0()),
                            "Label '%s' already exists",
                            &[Arg::Str(s.as_bytes())],
                        );
                    }
                }
                self.scope.labels.push(label);
                self.scope.allow_let = false;
                let statement = self.parse_statement();
                self.scope.labels.pop();
                return Statement::LabelledStatement(Box::new(LabelledStatement {
                    label: identifier,
                    colon,
                    statement,
                }));
            }
            self.optional_semicolon();
            return Statement::ExpressionStatement(Box::new(ExpressionStatement {
                expression: Expression::Identifier(identifier),
            }));
        }
        self.optional_semicolon();
        Statement::ExpressionStatement(Box::new(ExpressionStatement { expression }))
    }

    fn parse_try_statement(&mut self) -> Statement {
        let try_ = self.expect(token::TRY);
        let body = self.parse_block_statement();
        let mut catch = None;
        if self.token == token::CATCH {
            let c = self.idx;
            self.next();
            let mut parameter = None;
            if self.token == token::LEFT_PARENTHESIS {
                self.next();
                parameter = Some(self.parse_binding_target());
                self.expect(token::RIGHT_PARENTHESIS);
            }
            catch = Some(Box::new(CatchStatement {
                catch: c,
                parameter,
                body: self.parse_block_statement(),
            }));
        }
        let mut finally = None;
        if self.token == token::FINALLY {
            self.next();
            finally = Some(self.parse_block_statement());
        }
        if catch.is_none() && finally.is_none() {
            self.error_idx(try_, "Missing catch or finally after try");
            return bad_statement(try_, body.idx1());
        }
        Statement::TryStatement(Box::new(TryStatement {
            try_,
            body,
            catch,
            finally,
        }))
    }

    pub(super) fn parse_function_parameter_list(&mut self) -> ParameterList {
        let opening = self.expect(token::LEFT_PARENTHESIS);
        let mut list: Vec<Arc<Binding>> = Vec::new();
        let mut rest = None;
        let restore = !self.scope.in_func_params;
        if restore {
            self.scope.in_func_params = true;
        }
        while self.token != token::RIGHT_PARENTHESIS && self.token != token::EOF {
            if self.token == token::ELLIPSIS {
                self.next();
                let e = self.parse_assignment_expression();
                rest = self
                    .reinterpret_as_destruct_binding_target(Some(e))
                    .map(Expression::from);
                break;
            }
            self.parse_variable_declaration(&mut list);
            if self.token != token::RIGHT_PARENTHESIS {
                self.expect(token::COMMA);
            }
        }
        let closing = self.expect(token::RIGHT_PARENTHESIS);
        if restore {
            self.scope.in_func_params = false;
        }
        ParameterList {
            opening,
            list,
            rest,
            closing,
        }
    }

    pub(super) fn parse_maybe_async_function(
        &mut self,
        declaration: bool,
    ) -> Option<Box<FunctionLiteral>> {
        if self.peek() == token::FUNCTION {
            let idx = self.idx;
            self.next();
            return Some(self.parse_function(declaration, true, idx));
        }
        None
    }

    pub(super) fn parse_function(
        &mut self,
        declaration: bool,
        async_: bool,
        start: Idx,
    ) -> Box<FunctionLiteral> {
        self.expect(token::FUNCTION);
        let mut generator = false;
        if self.token == token::MULTIPLY {
            generator = true;
            self.next();
        }
        let mut restore_await = false;
        let mut restore_yield = false;
        if !declaration {
            if async_ != self.scope.allow_await {
                self.scope.allow_await = async_;
                restore_await = true;
            }
            if generator != self.scope.allow_yield {
                self.scope.allow_yield = generator;
                restore_yield = true;
            }
        }
        self.token_to_binding_id();
        let mut name = None;
        if self.token == token::IDENTIFIER {
            name = Some(self.parse_identifier());
        } else if declaration {
            self.expect(token::IDENTIFIER);
        }
        if declaration {
            if async_ != self.scope.allow_await {
                self.scope.allow_await = async_;
                restore_await = true;
            }
            if generator != self.scope.allow_yield {
                self.scope.allow_yield = generator;
                restore_yield = true;
            }
        }
        let parameter_list = self.parse_function_parameter_list();
        let allow_yield = self.scope.allow_yield;
        let (body, declaration_list) = self.parse_function_block(async_, async_, allow_yield);
        let source = self.slice(start, body.idx1());
        if restore_yield {
            self.scope.allow_yield = !generator;
        }
        if restore_await {
            self.scope.allow_await = !async_;
        }
        Box::new(FunctionLiteral {
            function: start,
            name,
            parameter_list: Box::new(parameter_list),
            body,
            source,
            declaration_list,
            async_,
            generator,
        })
    }

    pub(super) fn parse_function_block(
        &mut self,
        async_: bool,
        allow_await: bool,
        allow_yield: bool,
    ) -> (Box<BlockStatement>, Vec<VariableDeclaration>) {
        self.open_scope();
        self.scope.in_function = true;
        self.scope.in_async = async_;
        self.scope.allow_await = allow_await;
        self.scope.allow_yield = allow_yield;
        let body = self.parse_block_statement();
        let declaration_list = std::mem::take(&mut self.scope.declaration_list);
        self.close_scope();
        (body, declaration_list)
    }

    pub(super) fn parse_arrow_function_body(
        &mut self,
        async_: bool,
    ) -> (ConciseBody, Vec<VariableDeclaration>) {
        if self.token == token::LEFT_BRACE {
            let (b, d) = self.parse_function_block(async_, async_, false);
            return (ConciseBody::BlockStatement(b), d);
        }
        let mut saved = None;
        if async_ != self.scope.in_async || async_ != self.scope.allow_await {
            saved = Some((
                self.scope.in_async,
                self.scope.allow_await,
                self.scope.allow_yield,
            ));
            self.scope.in_async = async_;
            self.scope.allow_await = async_;
            self.scope.allow_yield = false;
        }
        let expression = self.parse_assignment_expression();
        if let Some((in_async, allow_await, allow_yield)) = saved {
            self.scope.in_async = in_async;
            self.scope.allow_await = allow_await;
            self.scope.allow_yield = allow_yield;
        }
        (
            ConciseBody::ExpressionBody(Box::new(ExpressionBody { expression })),
            Vec::new(),
        )
    }

    pub(super) fn parse_class(&mut self, declaration: bool) -> Box<ClassLiteral> {
        if declaration && !self.scope.allow_let && self.token == token::CLASS {
            self.error_unexpected_token(token::CLASS);
        }
        let class = self.expect(token::CLASS);
        self.token_to_binding_id();
        let mut name = None;
        if self.token == token::IDENTIFIER {
            name = Some(self.parse_identifier());
        } else if declaration {
            self.expect(token::IDENTIFIER);
        }
        let mut super_class = None;
        if self.token != token::LEFT_BRACE {
            self.expect(token::EXTENDS);
            super_class = Some(self.parse_left_hand_side_expression_allow_call());
        }
        self.expect(token::LEFT_BRACE);
        let mut body: Vec<ClassElement> = Vec::new();
        while self.token != token::RIGHT_BRACE && self.token != token::EOF {
            if self.token == token::SEMICOLON {
                self.next();
                continue;
            }
            let start = self.idx;
            let mut static_ = false;
            if self.token == token::STATIC {
                match self.peek() {
                    token::ASSIGN
                    | token::SEMICOLON
                    | token::RIGHT_BRACE
                    | token::LEFT_PARENTHESIS => {}
                    _ => {
                        self.next();
                        if self.token == token::LEFT_BRACE {
                            let (block, declaration_list) =
                                self.parse_function_block(false, true, false);
                            let source = self.slice(block.left_brace, block.idx1());
                            body.push(ClassElement::ClassStaticBlock(Box::new(ClassStaticBlock {
                                static_: start,
                                block,
                                source,
                                declaration_list,
                            })));
                            continue;
                        }
                        static_ = true;
                    }
                }
            }
            let mut kind: Option<PropertyKind> = None;
            let mut async_ = false;
            let method_body_start = self.idx;
            if self.literal == b"get" || self.literal == b"set" {
                let tok = self.peek();
                if tok != token::SEMICOLON && tok != token::LEFT_PARENTHESIS {
                    if self.literal == b"get" {
                        kind = Some(PropertyKind::Get);
                    } else {
                        kind = Some(PropertyKind::Set);
                    }
                    self.next();
                }
            } else if self.token == token::ASYNC {
                let tok = self.peek();
                if tok != token::SEMICOLON && tok != token::LEFT_PARENTHESIS {
                    async_ = true;
                    kind = Some(PropertyKind::Method);
                    self.next();
                }
            }
            let mut generator = false;
            if self.token == token::MULTIPLY
                && (kind.is_none() || kind == Some(PropertyKind::Method))
            {
                generator = true;
                kind = Some(PropertyKind::Method);
                self.next();
            }
            let (_, key_name, value, tkn) = self.parse_object_property_key();
            let Some(value) = value else {
                continue;
            };
            let computed = tkn == token::ILLEGAL;
            let private = matches!(value, Expression::PrivateIdentifier(_));
            if static_ && !private && key_name == "prototype" {
                self.error_idx(
                    value.idx0(),
                    "Classes may not have a static property named 'prototype'",
                );
            }
            if kind.is_none() && self.token == token::LEFT_PARENTHESIS {
                kind = Some(PropertyKind::Method);
            }
            if let Some(kind) = kind {
                if key_name == "constructor" && !computed {
                    if !static_ {
                        if kind != PropertyKind::Method {
                            self.error_idx(
                                value.idx0(),
                                "Class constructor may not be an accessor",
                            );
                        } else if async_ {
                            self.error_idx(
                                value.idx0(),
                                "Class constructor may not be an async method",
                            );
                        } else if generator {
                            self.error_idx(
                                value.idx0(),
                                "Class constructor may not be a generator",
                            );
                        }
                    } else if private {
                        self.error_idx(
                            value.idx0(),
                            "Class constructor may not be a private method",
                        );
                    }
                }
                let f = self.parse_method_definition(method_body_start, kind, generator, async_);
                body.push(ClassElement::MethodDefinition(Box::new(MethodDefinition {
                    idx: start,
                    key: value,
                    kind,
                    body: f,
                    computed,
                    static_,
                })));
            } else {
                let mut is_ctor = !computed && key_name == "constructor";
                if !is_ctor && let Expression::PrivateIdentifier(name) = &value {
                    is_ctor = name.identifier.name == "constructor";
                }
                if is_ctor {
                    self.error_idx(
                        value.idx0(),
                        "Classes may not have a field named 'constructor'",
                    );
                }
                let mut initializer = None;
                if self.token == token::ASSIGN {
                    self.next();
                    initializer = Some(self.parse_expression());
                }
                if !self.implicit_semicolon
                    && self.token != token::SEMICOLON
                    && self.token != token::RIGHT_BRACE
                {
                    self.error_unexpected_token(self.token);
                    break;
                }
                body.push(ClassElement::FieldDefinition(Box::new(FieldDefinition {
                    idx: start,
                    key: value,
                    initializer,
                    computed,
                    static_,
                })));
            }
        }
        let right_brace = self.expect(token::RIGHT_BRACE);
        let source = self.slice(class, right_brace + 1);
        Box::new(ClassLiteral {
            class,
            right_brace,
            name,
            super_class,
            body,
            source,
        })
    }

    fn parse_debugger_statement(&mut self) -> Statement {
        let idx = self.expect(token::DEBUGGER);
        let node = Statement::DebuggerStatement(Box::new(DebuggerStatement { debugger: idx }));
        self.semicolon();
        node
    }

    fn parse_return_statement(&mut self) -> Statement {
        let idx = self.expect(token::RETURN);
        if !self.scope.in_function {
            self.error_idx(idx, "Illegal return statement");
            self.next_statement();
            return bad_statement(idx, self.idx);
        }
        let mut argument = None;
        if !self.implicit_semicolon
            && self.token != token::SEMICOLON
            && self.token != token::RIGHT_BRACE
            && self.token != token::EOF
        {
            argument = Some(self.parse_expression());
        }
        self.semicolon();
        Statement::ReturnStatement(Box::new(ReturnStatement {
            return_: idx,
            argument,
        }))
    }

    fn parse_throw_statement(&mut self) -> Statement {
        let idx = self.expect(token::THROW);
        if self.implicit_semicolon {
            if self.chr == -1 {
                self.error_idx(idx, "Unexpected end of input");
            } else {
                self.error_idx(idx, "Illegal newline after throw");
            }
            self.next_statement();
            return bad_statement(idx, self.idx);
        }
        let node = Statement::ThrowStatement(Box::new(ThrowStatement {
            throw: idx,
            argument: self.parse_expression(),
        }));
        self.semicolon();
        node
    }

    fn parse_switch_statement(&mut self) -> Statement {
        let idx = self.expect(token::SWITCH);
        self.expect(token::LEFT_PARENTHESIS);
        let discriminant = self.parse_expression();
        let mut node = SwitchStatement {
            switch: idx,
            discriminant,
            default: -1,
            body: Vec::new(),
            right_brace: 0,
        };
        self.expect(token::RIGHT_PARENTHESIS);
        self.expect(token::LEFT_BRACE);
        let in_switch = self.scope.in_switch;
        self.scope.in_switch = true;
        let mut index = 0;
        while self.token != token::EOF {
            if self.token == token::RIGHT_BRACE {
                node.right_brace = self.idx;
                self.next();
                break;
            }
            let clause = self.parse_case_statement();
            if clause.test.is_none() {
                if node.default != -1 {
                    self.error_idx(clause.case, "Already saw a default in switch");
                }
                node.default = index;
            }
            node.body.push(clause);
            index += 1;
        }
        self.scope.in_switch = in_switch;
        Statement::SwitchStatement(Box::new(node))
    }

    fn parse_with_statement(&mut self) -> Statement {
        let with = self.expect(token::WITH);
        self.expect(token::LEFT_PARENTHESIS);
        let object = self.parse_expression();
        self.expect(token::RIGHT_PARENTHESIS);
        self.scope.allow_let = false;
        let body = self.parse_statement();
        Statement::WithStatement(Box::new(WithStatement { with, object, body }))
    }

    fn parse_case_statement(&mut self) -> CaseStatement {
        let case = self.idx;
        let mut test = None;
        if self.token == token::DEFAULT {
            self.next();
        } else {
            self.expect(token::CASE);
            test = Some(self.parse_expression());
        }
        self.expect(token::COLON);
        let mut consequent = Vec::new();
        loop {
            if self.token == token::EOF
                || self.token == token::RIGHT_BRACE
                || self.token == token::CASE
                || self.token == token::DEFAULT
            {
                break;
            }
            self.scope.allow_let = true;
            consequent.push(self.parse_statement());
        }
        CaseStatement {
            case,
            test,
            consequent,
        }
    }

    fn parse_iteration_statement(&mut self) -> Statement {
        let in_iteration = self.scope.in_iteration;
        self.scope.in_iteration = true;
        self.scope.allow_let = false;
        let s = self.parse_statement();
        self.scope.in_iteration = in_iteration;
        s
    }

    fn parse_for_in(&mut self, idx: Idx, into: ForInto) -> Statement {
        let source = self.parse_expression();
        self.expect(token::RIGHT_PARENTHESIS);
        let body = self.parse_iteration_statement();
        Statement::ForInStatement(Box::new(ForInStatement {
            for_: idx,
            into,
            source,
            body,
        }))
    }

    fn parse_for_of(&mut self, idx: Idx, into: ForInto) -> Statement {
        let source = self.parse_assignment_expression();
        self.expect(token::RIGHT_PARENTHESIS);
        let body = self.parse_iteration_statement();
        Statement::ForOfStatement(Box::new(ForOfStatement {
            for_: idx,
            into,
            source,
            body,
        }))
    }

    fn parse_for(&mut self, idx: Idx, initializer: Option<ForLoopInitializer>) -> Statement {
        let mut test = None;
        let mut update = None;
        if self.token != token::SEMICOLON {
            test = Some(self.parse_expression());
        }
        self.expect(token::SEMICOLON);
        if self.token != token::RIGHT_PARENTHESIS {
            update = Some(self.parse_expression());
        }
        self.expect(token::RIGHT_PARENTHESIS);
        let body = self.parse_iteration_statement();
        Statement::ForStatement(Box::new(ForStatement {
            for_: idx,
            initializer,
            update,
            test,
            body,
        }))
    }

    fn parse_for_or_for_in_statement(&mut self) -> Statement {
        let idx = self.expect(token::FOR);
        self.expect(token::LEFT_PARENTHESIS);
        let mut initializer: Option<ForLoopInitializer> = None;
        let mut for_in = false;
        let mut for_of = false;
        let mut into: Option<ForInto> = None;
        if self.token != token::SEMICOLON {
            let allow_in = self.scope.allow_in;
            self.scope.allow_in = false;
            let mut tok = self.token;
            if tok == token::LET {
                match self.peek() {
                    token::IDENTIFIER | token::LEFT_BRACKET | token::LEFT_BRACE => {}
                    _ => tok = token::IDENTIFIER,
                }
            }
            if tok == token::VAR || tok == token::LET || tok == token::CONST {
                let idx = self.idx;
                self.next();
                let list = if tok == token::VAR {
                    self.parse_var_declaration_list(idx)
                } else {
                    self.parse_variable_declaration_list()
                };
                if list.len() == 1 {
                    if self.token == token::IN {
                        self.next();
                        for_in = true;
                    } else if self.token == token::IDENTIFIER && self.literal == b"of" {
                        self.next();
                        for_of = true;
                    }
                }
                if for_in || for_of {
                    if let Some(init) = &list[0].initializer {
                        let i0 = init.idx0();
                        self.error_idx(
                            i0,
                            "for-in loop variable declaration may not have an initializer",
                        );
                    }
                    if tok == token::VAR {
                        into = Some(ForInto::ForIntoVar(Box::new(ForIntoVar {
                            binding: Arc::clone(&list[0]),
                        })));
                    } else {
                        into = Some(ForInto::ForDeclaration(Box::new(ForDeclaration {
                            idx,
                            is_const: tok == token::CONST,
                            target: list[0].target.clone(),
                        })));
                    }
                } else {
                    self.ensure_pattern_init(&list);
                    if tok == token::VAR {
                        initializer = Some(ForLoopInitializer::ForLoopInitializerVarDeclList(
                            Box::new(ForLoopInitializerVarDeclList { var: 0, list }),
                        ));
                    } else {
                        initializer = Some(ForLoopInitializer::ForLoopInitializerLexicalDecl(
                            Box::new(ForLoopInitializerLexicalDecl {
                                lexical_declaration: LexicalDeclaration {
                                    idx,
                                    token: tok,
                                    list,
                                },
                            }),
                        ));
                    }
                }
            } else {
                let mut expr = self.parse_expression();
                if self.token == token::IN {
                    self.next();
                    for_in = true;
                } else if self.token == token::IDENTIFIER && self.literal == b"of" {
                    self.next();
                    for_of = true;
                }
                if for_in || for_of {
                    expr = match expr {
                        e @ (Expression::Identifier(_)
                        | Expression::DotExpression(_)
                        | Expression::PrivateDotExpression(_)
                        | Expression::BracketExpression(_)) => e,
                        Expression::ObjectLiteral(e) => {
                            self.reinterpret_as_object_assignment_pattern(*e)
                        }
                        Expression::ArrayLiteral(e) => {
                            self.reinterpret_as_array_assignment_pattern(*e)
                        }
                        _ => {
                            self.error_idx(idx, "Invalid left-hand side in for-in or for-of");
                            self.next_statement();
                            return bad_statement(idx, self.idx);
                        }
                    };
                    into = Some(ForInto::ForIntoExpression(Box::new(ForIntoExpression {
                        expression: expr,
                    })));
                } else {
                    initializer = Some(ForLoopInitializer::ForLoopInitializerExpression(Box::new(
                        ForLoopInitializerExpression { expression: expr },
                    )));
                }
            }
            self.scope.allow_in = allow_in;
        }
        if for_in {
            return self.parse_for_in(idx, into.expect("for-in target"));
        }
        if for_of {
            return self.parse_for_of(idx, into.expect("for-of target"));
        }
        self.expect(token::SEMICOLON);
        self.parse_for(idx, initializer)
    }

    fn ensure_pattern_init(&mut self, list: &[Arc<Binding>]) {
        for item in list {
            if matches!(
                item.target,
                BindingTarget::ArrayPattern(_) | BindingTarget::ObjectPattern(_)
            ) && item.initializer.is_none()
            {
                self.error_idx(
                    item.idx1(),
                    "Missing initializer in destructuring declaration",
                );
                break;
            }
        }
    }

    fn parse_variable_statement(&mut self) -> Box<VariableStatement> {
        let idx = self.expect(token::VAR);
        let list = self.parse_var_declaration_list(idx);
        self.ensure_pattern_init(&list);
        self.semicolon();
        Box::new(VariableStatement { var: idx, list })
    }

    fn parse_lexical_declaration(&mut self, tok: Token) -> Box<LexicalDeclaration> {
        let idx = self.expect(tok);
        if !self.scope.allow_let {
            self.error_idx(
                idx,
                "Lexical declaration cannot appear in a single-statement context",
            );
        }
        let list = self.parse_variable_declaration_list();
        self.ensure_pattern_init(&list);
        self.semicolon();
        Box::new(LexicalDeclaration {
            idx,
            token: tok,
            list,
        })
    }

    fn parse_do_while_statement(&mut self) -> Statement {
        let in_iteration = self.scope.in_iteration;
        self.scope.in_iteration = true;
        let do_ = self.expect(token::DO);
        let body = if self.token == token::LEFT_BRACE {
            Statement::BlockStatement(self.parse_block_statement())
        } else {
            self.scope.allow_let = false;
            self.parse_statement()
        };
        self.expect(token::WHILE);
        self.expect(token::LEFT_PARENTHESIS);
        let test = self.parse_expression();
        let right_parenthesis = self.expect(token::RIGHT_PARENTHESIS);
        if self.token == token::SEMICOLON {
            self.next();
        }
        self.scope.in_iteration = in_iteration;
        Statement::DoWhileStatement(Box::new(DoWhileStatement {
            do_,
            test,
            body,
            right_parenthesis,
        }))
    }

    fn parse_while_statement(&mut self) -> Statement {
        let idx = self.expect(token::WHILE);
        self.expect(token::LEFT_PARENTHESIS);
        let test = self.parse_expression();
        self.expect(token::RIGHT_PARENTHESIS);
        let body = self.parse_iteration_statement();
        Statement::WhileStatement(Box::new(WhileStatement {
            while_: idx,
            test,
            body,
        }))
    }

    fn parse_if_statement(&mut self) -> Statement {
        self.expect(token::IF);
        self.expect(token::LEFT_PARENTHESIS);
        let test = self.parse_expression();
        self.expect(token::RIGHT_PARENTHESIS);
        let consequent = if self.token == token::LEFT_BRACE {
            Statement::BlockStatement(self.parse_block_statement())
        } else {
            self.scope.allow_let = false;
            self.parse_statement()
        };
        let mut alternate = None;
        if self.token == token::ELSE {
            self.next();
            self.scope.allow_let = false;
            alternate = Some(self.parse_statement());
        }
        Statement::IfStatement(Box::new(IfStatement {
            if_: 0,
            test,
            consequent,
            alternate,
        }))
    }

    fn parse_source_elements(&mut self) -> Vec<Statement> {
        let mut body = Vec::new();
        while self.token != token::EOF {
            self.scope.allow_let = true;
            body.push(self.parse_statement());
        }
        body
    }

    pub(super) fn parse_program(&mut self) -> Program {
        let body = self.parse_source_elements();
        let declaration_list = std::mem::take(&mut self.scope.declaration_list);
        let sm = self.parse_source_map();
        self.file.set_source_map(sm);
        Program {
            body,
            declaration_list,
            file: Some(Arc::clone(&self.file)),
        }
    }

    fn parse_source_map(&mut self) -> Option<Consumer> {
        if self.opts.disable_source_maps {
            return None;
        }
        let sm_line = extract_source_map_line(&self.str);
        if sm_line.is_empty() {
            return None;
        }
        let url_index = sm_line.iter().position(|&c| c == b'=').map_or(0, |i| i + 1);
        let url_str = &sm_line[url_index..];
        let mut data: Option<Vec<u8>> = None;
        let mut err: Option<String> = None;
        if url_str.starts_with(b"data:application/json") {
            let b64_index = url_str.iter().position(|&c| c == b',').map_or(0, |i| i + 1);
            let b64 = &url_str[b64_index..];
            match STD_ENCODING.decode_string(b64) {
                Ok(d) => data = Some(d),
                Err(e) => err = Some(e.to_string()),
            }
        } else if let Some(source_url) = resolve_sourcemap_url(self.file.name().as_bytes(), url_str)
        {
            if let Some(loader) = self.opts.source_map_loader.clone() {
                match loader(&source_url.string()) {
                    Ok(d) => data = d,
                    Err(e) => err = Some(e.to_string()),
                }
            } else if source_url.scheme.is_empty() || source_url.scheme == b"file" {
                match read_file(&source_url.path) {
                    Ok(d) => data = Some(d),
                    Err(e) => err = Some(e),
                }
            } else {
                err = Some(format!(
                    "unsupported source map URL scheme: {}",
                    String::from_utf8_lossy(&source_url.scheme)
                ));
            }
        }
        if let Some(e) = err {
            self.error(
                Place::Idx(0),
                "Could not load source map: %v",
                &[Arg::Str(e.as_bytes())],
            );
            return None;
        }
        let data = data?;
        match sourcemap::parse(self.file.name(), &data) {
            Ok(sm) => Some(sm),
            Err(e) => {
                let msg = e.to_string();
                self.error(
                    Place::Idx(0),
                    "Could not parse source map: %v",
                    &[Arg::Str(msg.as_bytes())],
                );
                None
            }
        }
    }

    fn parse_break_statement(&mut self) -> Statement {
        let idx = self.expect(token::BREAK);
        let mut semicolon = self.implicit_semicolon;
        if self.token == token::SEMICOLON {
            semicolon = true;
            self.next();
        }
        if semicolon || self.token == token::RIGHT_BRACE {
            self.implicit_semicolon = false;
            if !self.scope.in_iteration && !self.scope.in_switch {
                return self.illegal_branch(idx, "Illegal break statement");
            }
            return Statement::BranchStatement(Box::new(BranchStatement {
                idx,
                token: token::BREAK,
                label: None,
            }));
        }
        self.token_to_binding_id();
        if self.token == token::IDENTIFIER {
            let identifier = self.parse_identifier();
            if !self.scope.has_label(&identifier.name) {
                let s = identifier.name.string();
                self.error(
                    Place::Idx(idx),
                    "Undefined label '%s'",
                    &[Arg::Str(s.as_bytes())],
                );
                return bad_statement(idx, identifier.idx1());
            }
            self.semicolon();
            return Statement::BranchStatement(Box::new(BranchStatement {
                idx,
                token: token::BREAK,
                label: Some(identifier),
            }));
        }
        self.expect(token::IDENTIFIER);
        self.illegal_branch(idx, "Illegal break statement")
    }

    fn illegal_branch(&mut self, idx: Idx, msg: &str) -> Statement {
        self.error_idx(idx, msg);
        self.next_statement();
        bad_statement(idx, self.idx)
    }

    fn parse_continue_statement(&mut self) -> Statement {
        let idx = self.expect(token::CONTINUE);
        let mut semicolon = self.implicit_semicolon;
        if self.token == token::SEMICOLON {
            semicolon = true;
            self.next();
        }
        if semicolon || self.token == token::RIGHT_BRACE {
            self.implicit_semicolon = false;
            if !self.scope.in_iteration {
                return self.illegal_branch(idx, "Illegal continue statement");
            }
            return Statement::BranchStatement(Box::new(BranchStatement {
                idx,
                token: token::CONTINUE,
                label: None,
            }));
        }
        self.token_to_binding_id();
        if self.token == token::IDENTIFIER {
            let identifier = self.parse_identifier();
            if !self.scope.has_label(&identifier.name) {
                let s = identifier.name.string();
                self.error(
                    Place::Idx(idx),
                    "Undefined label '%s'",
                    &[Arg::Str(s.as_bytes())],
                );
                return bad_statement(idx, identifier.idx1());
            }
            if !self.scope.in_iteration {
                return self.illegal_branch(idx, "Illegal continue statement");
            }
            self.semicolon();
            return Statement::BranchStatement(Box::new(BranchStatement {
                idx,
                token: token::CONTINUE,
                label: Some(identifier),
            }));
        }
        self.expect(token::IDENTIFIER);
        self.illegal_branch(idx, "Illegal continue statement")
    }

    pub(super) fn next_statement(&mut self) {
        loop {
            match self.token {
                token::BREAK
                | token::CONTINUE
                | token::FOR
                | token::IF
                | token::RETURN
                | token::SWITCH
                | token::VAR
                | token::DO
                | token::TRY
                | token::WITH
                | token::WHILE
                | token::THROW
                | token::CATCH
                | token::FINALLY => {
                    if self.idx == self.recover.idx && self.recover.count < 10 {
                        self.recover.count += 1;
                        return;
                    }
                    if self.idx > self.recover.idx {
                        self.recover.idx = self.idx;
                        self.recover.count = 0;
                        return;
                    }
                }
                token::EOF => return,
                _ => {}
            }
            self.next();
        }
    }
}
