use super::error::Place;
use super::lexer::{is_line_terminator, is_line_white_space, parse_number_literal};
use super::parser::Parser;
use crate::ast::*;
use crate::file::Idx;
use crate::token::{self, Token};
use crate::unistring;
use std::sync::Arc;

fn bad(from: Idx, to: Idx) -> Expression {
    Expression::BadExpression(Box::new(BadExpression { from, to }))
}

fn bad_target(from: Idx, to: Idx) -> BindingTarget {
    BindingTarget::BadExpression(Box::new(BadExpression { from, to }))
}

fn ident(name: unistring::String, idx: Idx) -> Identifier {
    Identifier { name, idx }
}

fn is_lhs(e: &Expression) -> bool {
    matches!(
        e,
        Expression::Identifier(_)
            | Expression::DotExpression(_)
            | Expression::PrivateDotExpression(_)
            | Expression::BracketExpression(_)
    )
}

fn is_logical_and_expr(expr: &Expression) -> bool {
    matches!(expr, Expression::BinaryExpression(b) if b.operator == token::LOGICAL_AND)
}

impl Parser {
    pub(super) fn parse_identifier(&mut self) -> Box<Identifier> {
        let literal = self.parsed_literal.clone();
        let idx = self.idx;
        self.next();
        Box::new(ident(literal, idx))
    }

    pub(super) fn parse_primary_expression(&mut self) -> Expression {
        let literal = self.literal.clone();
        let parsed_literal = self.parsed_literal.clone();
        let idx = self.idx;
        match self.token {
            token::IDENTIFIER => {
                self.next();
                return Expression::Identifier(Box::new(ident(parsed_literal, idx)));
            }
            token::NULL => {
                self.next();
                return Expression::NullLiteral(Box::new(NullLiteral {
                    idx,
                    literal: literal.clone(),
                }));
            }
            token::BOOLEAN => {
                self.next();
                let mut value = false;
                if parsed_literal == "true" {
                    value = true;
                } else if parsed_literal == "false" {
                    value = false;
                } else {
                    self.error_idx(idx, "Illegal boolean literal");
                }
                return Expression::BooleanLiteral(Box::new(BooleanLiteral {
                    idx,
                    literal: literal.clone(),
                    value,
                }));
            }
            token::STRING => {
                self.next();
                return Expression::StringLiteral(Box::new(StringLiteral {
                    idx,
                    literal: literal.clone(),
                    value: parsed_literal,
                }));
            }
            token::NUMBER => {
                self.next();
                let value = match parse_number_literal(&literal) {
                    Ok(v) => v,
                    Err(e) => {
                        self.error_idx(idx, &e.to_string());
                        NumberValue::Int(0)
                    }
                };
                return Expression::NumberLiteral(Box::new(NumberLiteral {
                    idx,
                    literal: literal.clone(),
                    value,
                }));
            }
            token::SLASH | token::QUOTIENT_ASSIGN => {
                return Expression::RegExpLiteral(Box::new(self.parse_reg_exp_literal()));
            }
            token::LEFT_BRACE => {
                return Expression::ObjectLiteral(Box::new(self.parse_object_literal()));
            }
            token::LEFT_BRACKET => {
                return Expression::ArrayLiteral(Box::new(self.parse_array_literal()));
            }
            token::LEFT_PARENTHESIS => return self.parse_parenthesised_expression(),
            token::BACKTICK => {
                return Expression::TemplateLiteral(Box::new(self.parse_template_literal(false)));
            }
            token::THIS => {
                self.next();
                return Expression::ThisExpression(Box::new(ThisExpression { idx }));
            }
            token::SUPER => return self.parse_super_property(),
            token::ASYNC => {
                if let Some(f) = self.parse_maybe_async_function(false) {
                    return Expression::FunctionLiteral(f);
                }
            }
            token::FUNCTION => {
                return Expression::FunctionLiteral(self.parse_function(false, false, idx));
            }
            token::CLASS => return Expression::ClassLiteral(self.parse_class(false)),
            _ => {}
        }
        if self.is_binding_id(self.token) {
            self.next();
            return Expression::Identifier(Box::new(ident(parsed_literal, idx)));
        }
        self.error_unexpected_token(self.token);
        self.next_statement();
        bad(idx, self.idx)
    }

    fn parse_super_property(&mut self) -> Expression {
        let idx = self.idx;
        self.next();
        match self.token {
            token::PERIOD => {
                self.next();
                if !token::is_id(self.token) {
                    self.expect(token::IDENTIFIER);
                    self.next_statement();
                    return bad(idx, self.idx);
                }
                let id_idx = self.idx;
                let parsed_literal = self.parsed_literal.clone();
                self.next();
                Expression::DotExpression(Box::new(DotExpression {
                    left: Expression::SuperExpression(Box::new(SuperExpression { idx })),
                    identifier: ident(parsed_literal, id_idx),
                }))
            }
            token::LEFT_BRACKET => {
                self.parse_bracket_member(Expression::SuperExpression(Box::new(SuperExpression {
                    idx,
                })))
            }
            token::LEFT_PARENTHESIS => {
                self.parse_call_expression(Expression::SuperExpression(Box::new(SuperExpression {
                    idx,
                })))
            }
            _ => {
                self.error_idx(idx, "'super' keyword unexpected here");
                self.next_statement();
                bad(idx, self.idx)
            }
        }
    }

    fn reinterpret_sequence_as_arrow_func_params(
        &mut self,
        list: Vec<Expression>,
    ) -> ParameterList {
        let mut first_rest: Option<Expression> = None;
        let mut params: Vec<Arc<Binding>> = Vec::with_capacity(list.len());
        for item in list {
            if matches!(item, Expression::SpreadElement(_)) && first_rest.is_none() {
                first_rest = Some(item);
                continue;
            }
            if let Some(rest) = &first_rest {
                let idx = rest.idx0();
                self.error_idx(idx, "Rest parameter must be last formal parameter");
                return ParameterList::default();
            }
            params.push(Arc::new(self.reinterpret_as_binding(item)));
        }
        let rest = first_rest.map(|r| self.reinterpret_as_binding_rest_element(r));
        ParameterList {
            opening: 0,
            list: params,
            rest,
            closing: 0,
        }
    }

    fn parse_parenthesised_expression(&mut self) -> Expression {
        let opening = self.idx;
        self.expect(token::LEFT_PARENTHESIS);
        let mut list: Vec<Expression> = Vec::new();
        if self.token != token::RIGHT_PARENTHESIS {
            loop {
                if self.token == token::ELLIPSIS {
                    let start = self.idx;
                    self.error_unexpected_token(token::ELLIPSIS);
                    self.next();
                    let expr = self.parse_assignment_expression();
                    list.push(bad(start, expr.idx1()));
                } else {
                    list.push(self.parse_assignment_expression());
                }
                if self.token != token::COMMA {
                    break;
                }
                self.next();
                if self.token == token::RIGHT_PARENTHESIS {
                    self.error_unexpected_token(token::RIGHT_PARENTHESIS);
                    break;
                }
            }
        }
        self.expect(token::RIGHT_PARENTHESIS);
        if list.len() == 1 && self.errors.is_empty() {
            return list.pop().unwrap_or_else(|| bad(opening, opening));
        }
        if list.is_empty() {
            self.error_unexpected_token(token::RIGHT_PARENTHESIS);
            return bad(opening, self.idx);
        }
        Expression::SequenceExpression(Box::new(SequenceExpression { sequence: list }))
    }

    fn parse_reg_exp_literal(&mut self) -> RegExpLiteral {
        let mut offset = self.chr_offset - 1;
        if self.token == token::QUOTIENT_ASSIGN {
            offset -= 1;
        }
        let idx = self.idx_of(offset);
        let (mut pattern, _, err) = self.scan_string(offset, false);
        let mut end_offset = self.chr_offset;
        if err.is_empty() {
            pattern = pattern[1..pattern.len() - 1].to_vec();
        }
        let mut flags: Vec<u8> = Vec::new();
        if !is_line_terminator(self.chr) && !is_line_white_space(self.chr) {
            self.next();
            if self.token == token::IDENTIFIER {
                flags = self.literal.clone();
                self.next();
                end_offset = self.chr_offset - 1;
            }
        } else {
            self.next();
        }
        let literal = self.str[offset..end_offset].to_vec();
        RegExpLiteral {
            idx,
            literal: literal.clone(),
            pattern: pattern.clone(),
            flags: flags.clone(),
        }
    }

    pub(super) fn is_binding_id(&self, tok: Token) -> bool {
        if tok == token::IDENTIFIER {
            return true;
        }
        if tok == token::AWAIT {
            return !self.scope.allow_await;
        }
        if tok == token::YIELD {
            return !self.scope.allow_yield;
        }
        token::is_unreserved_word(tok)
    }

    pub(super) fn token_to_binding_id(&mut self) {
        if self.is_binding_id(self.token) {
            self.token = token::IDENTIFIER;
        }
    }

    pub(super) fn parse_binding_target(&mut self) -> BindingTarget {
        self.token_to_binding_id();
        match self.token {
            token::IDENTIFIER => {
                let target = BindingTarget::Identifier(Box::new(ident(
                    self.parsed_literal.clone(),
                    self.idx,
                )));
                self.next();
                target
            }
            token::LEFT_BRACKET => self.parse_array_binding_pattern(),
            token::LEFT_BRACE => self.parse_object_binding_pattern(),
            _ => {
                let idx = self.expect(token::IDENTIFIER);
                self.next_statement();
                bad_target(idx, self.idx)
            }
        }
    }

    pub(super) fn parse_variable_declaration(&mut self, declaration_list: &mut Vec<Arc<Binding>>) {
        let target = self.parse_binding_target();
        let mut initializer = None;
        if self.token == token::ASSIGN {
            self.next();
            initializer = Some(self.parse_assignment_expression());
        }
        declaration_list.push(Arc::new(Binding {
            target,
            initializer,
        }));
    }

    pub(super) fn parse_variable_declaration_list(&mut self) -> Vec<Arc<Binding>> {
        let mut declaration_list = Vec::new();
        loop {
            self.parse_variable_declaration(&mut declaration_list);
            if self.token != token::COMMA {
                break;
            }
            self.next();
        }
        declaration_list
    }

    pub(super) fn parse_var_declaration_list(&mut self, var: Idx) -> Vec<Arc<Binding>> {
        let declaration_list = self.parse_variable_declaration_list();
        self.scope.declare(VariableDeclaration {
            var,
            list: declaration_list.clone(),
        });
        declaration_list
    }

    pub(super) fn parse_object_property_key(
        &mut self,
    ) -> (Vec<u8>, unistring::String, Option<Expression>, Token) {
        if self.token == token::LEFT_BRACKET {
            self.next();
            let expr = self.parse_assignment_expression();
            self.expect(token::RIGHT_BRACKET);
            return (
                Vec::new(),
                unistring::String::default(),
                Some(expr),
                token::ILLEGAL,
            );
        }
        let (idx, tkn, literal, parsed_literal) = (
            self.idx,
            self.token,
            self.literal.clone(),
            self.parsed_literal.clone(),
        );
        let mut value: Option<Expression> = None;
        self.next();
        match tkn {
            token::IDENTIFIER | token::STRING | token::KEYWORD | token::ESCAPED_RESERVED_WORD => {
                value = Some(Expression::StringLiteral(Box::new(StringLiteral {
                    idx,
                    literal: literal.clone(),
                    value: parsed_literal.clone(),
                })));
            }
            token::NUMBER => match parse_number_literal(&literal) {
                Err(e) => self.error_idx(idx, &e.to_string()),
                Ok(num) => {
                    value = Some(Expression::NumberLiteral(Box::new(NumberLiteral {
                        idx,
                        literal: literal.clone(),
                        value: num,
                    })));
                }
            },
            token::PRIVATE_IDENTIFIER => {
                value = Some(Expression::PrivateIdentifier(Box::new(PrivateIdentifier {
                    identifier: ident(parsed_literal.clone(), idx),
                })));
            }
            _ => {
                if token::is_id(tkn) {
                    value = Some(Expression::StringLiteral(Box::new(StringLiteral {
                        idx,
                        literal: literal.clone(),
                        value: unistring::String::from_bytes(&literal),
                    })));
                } else {
                    self.error_unexpected_token(tkn);
                }
            }
        }
        (literal, parsed_literal, value, tkn)
    }

    fn parse_object_property(&mut self) -> Option<Property> {
        if self.token == token::ELLIPSIS {
            self.next();
            return Some(Property::SpreadElement(Box::new(SpreadElement {
                expression: self.parse_assignment_expression(),
            })));
        }
        let key_start_idx = self.idx;
        let mut generator = false;
        if self.token == token::MULTIPLY {
            generator = true;
            self.next();
        }
        let (literal, parsed_literal, value, tkn) = self.parse_object_property_key();
        let value = value?;
        if token::is_id(tkn)
            || tkn == token::STRING
            || tkn == token::NUMBER
            || tkn == token::ILLEGAL
        {
            if generator {
                let f =
                    self.parse_method_definition(key_start_idx, PropertyKind::Method, true, false);
                return Some(Property::PropertyKeyed(Box::new(PropertyKeyed {
                    key: value,
                    kind: PropertyKind::Method,
                    value: Expression::FunctionLiteral(f),
                    computed: tkn == token::ILLEGAL,
                })));
            }
            if self.token == token::LEFT_PARENTHESIS {
                let f =
                    self.parse_method_definition(key_start_idx, PropertyKind::Method, false, false);
                return Some(Property::PropertyKeyed(Box::new(PropertyKeyed {
                    key: value,
                    kind: PropertyKind::Method,
                    value: Expression::FunctionLiteral(f),
                    computed: tkn == token::ILLEGAL,
                })));
            } else if self.token == token::COMMA
                || self.token == token::RIGHT_BRACE
                || self.token == token::ASSIGN
            {
                if self.is_binding_id(tkn) {
                    let mut initializer = None;
                    if self.token == token::ASSIGN {
                        self.next();
                        initializer = Some(self.parse_assignment_expression());
                    }
                    return Some(Property::PropertyShort(Box::new(PropertyShort {
                        name: ident(parsed_literal, value.idx0()),
                        initializer,
                    })));
                } else {
                    self.error_unexpected_token(self.token);
                }
            } else if (literal == b"get" || literal == b"set" || tkn == token::ASYNC)
                && self.token != token::COLON
            {
                let (_, _, key_value, tkn1) = self.parse_object_property_key();
                let key_value = key_value?;
                let (kind, async_) = if tkn == token::ASYNC {
                    (PropertyKind::Method, true)
                } else if literal == b"get" {
                    (PropertyKind::Get, false)
                } else {
                    (PropertyKind::Set, false)
                };
                let f = self.parse_method_definition(key_start_idx, kind, false, async_);
                return Some(Property::PropertyKeyed(Box::new(PropertyKeyed {
                    key: key_value,
                    kind,
                    value: Expression::FunctionLiteral(f),
                    computed: tkn1 == token::ILLEGAL,
                })));
            }
        }
        self.expect(token::COLON);
        Some(Property::PropertyKeyed(Box::new(PropertyKeyed {
            key: value,
            kind: PropertyKind::Value,
            value: self.parse_assignment_expression(),
            computed: tkn == token::ILLEGAL,
        })))
    }

    pub(super) fn parse_method_definition(
        &mut self,
        key_start_idx: Idx,
        kind: PropertyKind,
        generator: bool,
        async_: bool,
    ) -> Box<FunctionLiteral> {
        let idx1 = self.idx;
        let restore_yield = generator != self.scope.allow_yield;
        if restore_yield {
            self.scope.allow_yield = generator;
        }
        let restore_await = async_ != self.scope.allow_await;
        if restore_await {
            self.scope.allow_await = async_;
        }
        let parameter_list = self.parse_function_parameter_list();
        match kind {
            PropertyKind::Get => {
                if !parameter_list.list.is_empty() || parameter_list.rest.is_some() {
                    self.error_idx(idx1, "Getter must not have any formal parameters.");
                }
            }
            PropertyKind::Set => {
                if parameter_list.list.len() != 1 || parameter_list.rest.is_some() {
                    self.error_idx(idx1, "Setter must have exactly one formal parameter.");
                }
            }
            _ => {}
        }
        let (body, declaration_list) = self.parse_function_block(async_, async_, generator);
        let source = self.slice(key_start_idx, body.idx1());
        if restore_await {
            self.scope.allow_await = !async_;
        }
        if restore_yield {
            self.scope.allow_yield = !generator;
        }
        Box::new(FunctionLiteral {
            function: key_start_idx,
            name: None,
            parameter_list: Box::new(parameter_list),
            body,
            source,
            declaration_list,
            async_,
            generator,
        })
    }

    pub(super) fn parse_object_literal(&mut self) -> ObjectLiteral {
        let mut value: Vec<Property> = Vec::new();
        let idx0 = self.expect(token::LEFT_BRACE);
        while self.token != token::RIGHT_BRACE && self.token != token::EOF {
            if let Some(property) = self.parse_object_property() {
                value.push(property);
            }
            if self.token != token::RIGHT_BRACE {
                self.expect(token::COMMA);
            } else {
                break;
            }
        }
        let idx1 = self.expect(token::RIGHT_BRACE);
        ObjectLiteral {
            left_brace: idx0,
            right_brace: idx1,
            value,
        }
    }

    pub(super) fn parse_array_literal(&mut self) -> ArrayLiteral {
        let idx0 = self.expect(token::LEFT_BRACKET);
        let mut value: Vec<Option<Expression>> = Vec::new();
        while self.token != token::RIGHT_BRACKET && self.token != token::EOF {
            if self.token == token::COMMA {
                self.next();
                value.push(None);
                continue;
            }
            if self.token == token::ELLIPSIS {
                self.next();
                value.push(Some(Expression::SpreadElement(Box::new(SpreadElement {
                    expression: self.parse_assignment_expression(),
                }))));
            } else {
                value.push(Some(self.parse_assignment_expression()));
            }
            if self.token != token::RIGHT_BRACKET {
                self.expect(token::COMMA);
            }
        }
        let idx1 = self.expect(token::RIGHT_BRACKET);
        ArrayLiteral {
            left_bracket: idx0,
            right_bracket: idx1,
            value,
        }
    }

    pub(super) fn parse_template_literal(&mut self, tagged: bool) -> TemplateLiteral {
        let mut res = TemplateLiteral {
            open_quote: self.idx,
            close_quote: 0,
            tag: None,
            elements: Vec::new(),
            expressions: Vec::new(),
        };
        loop {
            let start = self.offset;
            let (literal, parsed, finished, parse_err, err) = self.parse_template_characters();
            if !err.is_empty() {
                self.error(Place::Offset(self.offset), &err, &[]);
            }
            res.elements.push(TemplateElement {
                idx: self.idx_of(start),
                literal: literal.clone(),
                parsed,
                valid: parse_err.is_empty(),
            });
            if !tagged && !parse_err.is_empty() {
                self.error(Place::Offset(self.offset), &parse_err, &[]);
            }
            let end = self.chr_offset.wrapping_sub(1);
            self.next();
            if finished {
                res.close_quote = self.idx_of(end);
                break;
            }
            let expr = self.parse_expression();
            res.expressions.push(expr);
            if self.token != token::RIGHT_BRACE {
                self.error_unexpected_token(self.token);
            }
        }
        res
    }

    fn parse_tagged_template_literal(&mut self, tag: Expression) -> Expression {
        let mut l = self.parse_template_literal(true);
        l.tag = Some(tag);
        Expression::TemplateLiteral(Box::new(l))
    }

    fn parse_argument_list(&mut self) -> (Vec<Expression>, Idx, Idx) {
        let idx0 = self.expect(token::LEFT_PARENTHESIS);
        let mut argument_list = Vec::new();
        while self.token != token::RIGHT_PARENTHESIS {
            let item = if self.token == token::ELLIPSIS {
                self.next();
                Expression::SpreadElement(Box::new(SpreadElement {
                    expression: self.parse_assignment_expression(),
                }))
            } else {
                self.parse_assignment_expression()
            };
            argument_list.push(item);
            if self.token != token::COMMA {
                break;
            }
            self.next();
        }
        let idx1 = self.expect(token::RIGHT_PARENTHESIS);
        (argument_list, idx0, idx1)
    }

    fn parse_call_expression(&mut self, left: Expression) -> Expression {
        let (argument_list, idx0, idx1) = self.parse_argument_list();
        Expression::CallExpression(Box::new(CallExpression {
            callee: left,
            left_parenthesis: idx0,
            argument_list,
            right_parenthesis: idx1,
        }))
    }

    fn parse_dot_member(&mut self, left: Expression) -> Expression {
        let period = self.idx;
        self.next();
        let literal = self.parsed_literal.clone();
        let idx = self.idx;
        if self.token == token::PRIVATE_IDENTIFIER {
            self.next();
            return Expression::PrivateDotExpression(Box::new(PrivateDotExpression {
                left,
                identifier: PrivateIdentifier {
                    identifier: ident(literal, idx),
                },
            }));
        }
        if !token::is_id(self.token) {
            self.expect(token::IDENTIFIER);
            self.next_statement();
            return bad(period, self.idx);
        }
        self.next();
        Expression::DotExpression(Box::new(DotExpression {
            left,
            identifier: ident(literal, idx),
        }))
    }

    fn parse_bracket_member(&mut self, left: Expression) -> Expression {
        let idx0 = self.expect(token::LEFT_BRACKET);
        let member = self.parse_expression();
        let idx1 = self.expect(token::RIGHT_BRACKET);
        Expression::BracketExpression(Box::new(BracketExpression {
            left_bracket: idx0,
            left,
            member,
            right_bracket: idx1,
        }))
    }

    fn parse_new_expression(&mut self) -> Expression {
        let idx = self.expect(token::NEW);
        if self.token == token::PERIOD {
            self.next();
            if self.literal == b"target" {
                let property = self.parse_identifier();
                return Expression::MetaProperty(Box::new(MetaProperty {
                    meta: Box::new(ident(unistring::String::new(&token::NEW.string()), idx)),
                    property,
                    idx: 0,
                }));
            }
            self.error_unexpected_token(token::IDENTIFIER);
        }
        let callee = self.parse_left_hand_side_expression();
        if let Expression::BadExpression(mut b) = callee {
            b.from = idx;
            return Expression::BadExpression(b);
        }
        let mut node = NewExpression {
            new: idx,
            callee,
            left_parenthesis: 0,
            argument_list: Vec::new(),
            right_parenthesis: 0,
        };
        if self.token == token::LEFT_PARENTHESIS {
            let (argument_list, idx0, idx1) = self.parse_argument_list();
            node.argument_list = argument_list;
            node.left_parenthesis = idx0;
            node.right_parenthesis = idx1;
        }
        Expression::NewExpression(Box::new(node))
    }

    fn parse_left_hand_side_expression(&mut self) -> Expression {
        let mut left = if self.token == token::NEW {
            self.parse_new_expression()
        } else {
            self.parse_primary_expression()
        };
        loop {
            match self.token {
                token::PERIOD => left = self.parse_dot_member(left),
                token::LEFT_BRACKET => left = self.parse_bracket_member(left),
                token::BACKTICK => left = self.parse_tagged_template_literal(left),
                _ => break,
            }
        }
        left
    }

    pub(super) fn parse_left_hand_side_expression_allow_call(&mut self) -> Expression {
        let allow_in = self.scope.allow_in;
        self.scope.allow_in = true;
        let r = self.parse_left_hand_side_expression_allow_call_inner();
        self.scope.allow_in = allow_in;
        r
    }

    fn parse_left_hand_side_expression_allow_call_inner(&mut self) -> Expression {
        let start = self.idx;
        let mut left = if self.token == token::NEW {
            self.parse_new_expression()
        } else {
            self.parse_primary_expression()
        };
        let mut optional_chain = false;
        loop {
            match self.token {
                token::PERIOD => left = self.parse_dot_member(left),
                token::LEFT_BRACKET => left = self.parse_bracket_member(left),
                token::LEFT_PARENTHESIS => left = self.parse_call_expression(left),
                token::BACKTICK => {
                    if optional_chain {
                        self.error_idx(self.idx, "Invalid template literal on optional chain");
                        self.next_statement();
                        return bad(start, self.idx);
                    }
                    left = self.parse_tagged_template_literal(left);
                }
                token::QUESTION_DOT => {
                    optional_chain = true;
                    left = Expression::Optional(Box::new(Optional { expression: left }));
                    match self.peek() {
                        token::LEFT_BRACKET | token::LEFT_PARENTHESIS | token::BACKTICK => {
                            self.next()
                        }
                        _ => left = self.parse_dot_member(left),
                    }
                }
                _ => break,
            }
        }
        if optional_chain {
            left = Expression::OptionalChain(Box::new(OptionalChain { expression: left }));
        }
        left
    }

    fn parse_update_expression(&mut self) -> Expression {
        match self.token {
            token::INCREMENT | token::DECREMENT => {
                let tkn = self.token;
                let idx = self.idx;
                self.next();
                let operand = self.parse_unary_expression();
                if !is_lhs(&operand) {
                    self.error_idx(idx, "Invalid left-hand side in assignment");
                    self.next_statement();
                    return bad(idx, self.idx);
                }
                Expression::UnaryExpression(Box::new(UnaryExpression {
                    operator: tkn,
                    idx,
                    operand,
                    postfix: false,
                }))
            }
            _ => {
                let operand = self.parse_left_hand_side_expression_allow_call();
                if self.token == token::INCREMENT || self.token == token::DECREMENT {
                    if self.implicit_semicolon {
                        return operand;
                    }
                    let tkn = self.token;
                    let idx = self.idx;
                    self.next();
                    if !is_lhs(&operand) {
                        self.error_idx(idx, "Invalid left-hand side in assignment");
                        self.next_statement();
                        return bad(idx, self.idx);
                    }
                    return Expression::UnaryExpression(Box::new(UnaryExpression {
                        operator: tkn,
                        idx,
                        operand,
                        postfix: true,
                    }));
                }
                operand
            }
        }
    }

    fn parse_unary_expression(&mut self) -> Expression {
        match self.token {
            token::PLUS
            | token::MINUS
            | token::NOT
            | token::BITWISE_NOT
            | token::DELETE
            | token::VOID
            | token::TYPEOF => {
                let tkn = self.token;
                let idx = self.idx;
                self.next();
                return Expression::UnaryExpression(Box::new(UnaryExpression {
                    operator: tkn,
                    idx,
                    operand: self.parse_unary_expression(),
                    postfix: false,
                }));
            }
            token::AWAIT => {
                if self.scope.allow_await {
                    let idx = self.idx;
                    self.next();
                    if !self.scope.in_async {
                        self.error_unexpected_token(token::AWAIT);
                        return bad(idx, self.idx);
                    }
                    if self.scope.in_func_params {
                        self.error_idx(
                            idx,
                            "Illegal await-expression in formal parameters of async function",
                        );
                    }
                    return Expression::AwaitExpression(Box::new(AwaitExpression {
                        await_: idx,
                        argument: self.parse_unary_expression(),
                    }));
                }
            }
            _ => {}
        }
        self.parse_update_expression()
    }

    fn parse_exponentiation_expression(&mut self) -> Expression {
        let parenthesis = self.token == token::LEFT_PARENTHESIS;
        let mut left = self.parse_unary_expression();
        if self.token == token::EXPONENT {
            if !parenthesis
                && let Expression::UnaryExpression(u) = &left
                && u.operator != token::INCREMENT
                && u.operator != token::DECREMENT
            {
                self.error_idx(
                    self.idx,
                    "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
                );
            }
            loop {
                self.next();
                left = Expression::BinaryExpression(Box::new(BinaryExpression {
                    operator: token::EXPONENT,
                    left,
                    right: self.parse_exponentiation_expression(),
                    comparison: false,
                }));
                if self.token != token::EXPONENT {
                    break;
                }
            }
        }
        left
    }

    fn binary(
        &mut self,
        operator: Token,
        left: Expression,
        right: Expression,
        comparison: bool,
    ) -> Expression {
        Expression::BinaryExpression(Box::new(BinaryExpression {
            operator,
            left,
            right,
            comparison,
        }))
    }

    fn parse_multiplicative_expression(&mut self) -> Expression {
        let mut left = self.parse_exponentiation_expression();
        while self.token == token::MULTIPLY
            || self.token == token::SLASH
            || self.token == token::REMAINDER
        {
            let tkn = self.token;
            self.next();
            let right = self.parse_exponentiation_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_additive_expression(&mut self) -> Expression {
        let mut left = self.parse_multiplicative_expression();
        while self.token == token::PLUS || self.token == token::MINUS {
            let tkn = self.token;
            self.next();
            let right = self.parse_multiplicative_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_shift_expression(&mut self) -> Expression {
        let mut left = self.parse_additive_expression();
        while self.token == token::SHIFT_LEFT
            || self.token == token::SHIFT_RIGHT
            || self.token == token::UNSIGNED_SHIFT_RIGHT
        {
            let tkn = self.token;
            self.next();
            let right = self.parse_additive_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_relational_expression(&mut self) -> Expression {
        if self.scope.allow_in && self.token == token::PRIVATE_IDENTIFIER {
            let left = Expression::PrivateIdentifier(Box::new(PrivateIdentifier {
                identifier: ident(self.parsed_literal.clone(), self.idx),
            }));
            self.next();
            if self.token == token::IN {
                self.next();
                let right = self.parse_shift_expression();
                let operator = self.token;
                return self.binary(operator, left, right, false);
            }
            return left;
        }
        let left = self.parse_shift_expression();
        let allow_in = self.scope.allow_in;
        self.scope.allow_in = true;
        let r = match self.token {
            token::LESS | token::LESS_OR_EQUAL | token::GREATER | token::GREATER_OR_EQUAL => {
                let tkn = self.token;
                self.next();
                let right = self.parse_relational_expression();
                self.binary(tkn, left, right, true)
            }
            token::INSTANCEOF => {
                let tkn = self.token;
                self.next();
                let right = self.parse_relational_expression();
                self.binary(tkn, left, right, false)
            }
            token::IN => {
                if !allow_in {
                    left
                } else {
                    let tkn = self.token;
                    self.next();
                    let right = self.parse_relational_expression();
                    self.binary(tkn, left, right, false)
                }
            }
            _ => left,
        };
        self.scope.allow_in = allow_in;
        r
    }

    fn parse_equality_expression(&mut self) -> Expression {
        let mut left = self.parse_relational_expression();
        while self.token == token::EQUAL
            || self.token == token::NOT_EQUAL
            || self.token == token::STRICT_EQUAL
            || self.token == token::STRICT_NOT_EQUAL
        {
            let tkn = self.token;
            self.next();
            let right = self.parse_relational_expression();
            left = self.binary(tkn, left, right, true);
        }
        left
    }

    fn parse_bitwise_and_expression(&mut self) -> Expression {
        let mut left = self.parse_equality_expression();
        while self.token == token::AND {
            let tkn = self.token;
            self.next();
            let right = self.parse_equality_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_bitwise_exclusive_or_expression(&mut self) -> Expression {
        let mut left = self.parse_bitwise_and_expression();
        while self.token == token::EXCLUSIVE_OR {
            let tkn = self.token;
            self.next();
            let right = self.parse_bitwise_and_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_bitwise_or_expression(&mut self) -> Expression {
        let mut left = self.parse_bitwise_exclusive_or_expression();
        while self.token == token::OR {
            let tkn = self.token;
            self.next();
            let right = self.parse_bitwise_exclusive_or_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_logical_and_expression(&mut self) -> Expression {
        let mut left = self.parse_bitwise_or_expression();
        while self.token == token::LOGICAL_AND {
            let tkn = self.token;
            self.next();
            let right = self.parse_bitwise_or_expression();
            left = self.binary(tkn, left, right, false);
        }
        left
    }

    fn parse_logical_or_expression(&mut self) -> Expression {
        let idx: Idx;
        let parenthesis = self.token == token::LEFT_PARENTHESIS;
        let mut left = self.parse_logical_and_expression();
        if self.token == token::LOGICAL_OR || !parenthesis && is_logical_and_expr(&left) {
            loop {
                match self.token {
                    token::LOGICAL_OR => {
                        self.next();
                        let right = self.parse_logical_and_expression();
                        left = self.binary(token::LOGICAL_OR, left, right, false);
                    }
                    token::COALESCE => {
                        idx = self.idx;
                        break;
                    }
                    _ => return left,
                }
            }
        } else {
            loop {
                match self.token {
                    token::COALESCE => {
                        let i = self.idx;
                        self.next();
                        let parenthesis = self.token == token::LEFT_PARENTHESIS;
                        let right = self.parse_logical_and_expression();
                        if !parenthesis && is_logical_and_expr(&right) {
                            idx = i;
                            break;
                        }
                        left = self.binary(token::COALESCE, left, right, false);
                    }
                    token::LOGICAL_OR => {
                        idx = self.idx;
                        break;
                    }
                    _ => return left,
                }
            }
        }
        self.error_idx(
            idx,
            "Logical expressions and coalesce expressions cannot be mixed. Wrap either by parentheses",
        );
        left
    }

    fn parse_conditional_expression(&mut self) -> Expression {
        let left = self.parse_logical_or_expression();
        if self.token == token::QUESTION_MARK {
            self.next();
            let allow_in = self.scope.allow_in;
            self.scope.allow_in = true;
            let consequent = self.parse_assignment_expression();
            self.scope.allow_in = allow_in;
            self.expect(token::COLON);
            return Expression::ConditionalExpression(Box::new(ConditionalExpression {
                test: left,
                consequent,
                alternate: self.parse_assignment_expression(),
            }));
        }
        left
    }

    fn parse_arrow_function(
        &mut self,
        start: Idx,
        param_list: ParameterList,
        async_: bool,
    ) -> Expression {
        self.expect(token::ARROW);
        let (body, declaration_list) = self.parse_arrow_function_body(async_);
        let source = self.slice(start, body.idx1());
        Expression::ArrowFunctionLiteral(Box::new(ArrowFunctionLiteral {
            start,
            parameter_list: Box::new(param_list),
            body,
            source,
            declaration_list,
            async_,
        }))
    }

    fn parse_single_arg_arrow_function(&mut self, start: Idx, async_: bool) -> Expression {
        let restore = async_ != self.scope.allow_await;
        if restore {
            self.scope.allow_await = async_;
        }
        let r = self.parse_single_arg_arrow_function_inner(start, async_);
        if restore {
            self.scope.allow_await = !async_;
        }
        r
    }

    fn parse_single_arg_arrow_function_inner(&mut self, start: Idx, async_: bool) -> Expression {
        self.token_to_binding_id();
        if self.token != token::IDENTIFIER {
            self.error_unexpected_token(self.token);
            self.next();
            return bad(start, self.idx);
        }
        let id = self.parse_identifier();
        let param_list = ParameterList {
            opening: id.idx,
            closing: id.idx1(),
            list: vec![Arc::new(Binding {
                target: BindingTarget::Identifier(id),
                initializer: None,
            })],
            rest: None,
        };
        self.parse_arrow_function(start, param_list, async_)
    }

    pub(super) fn parse_assignment_expression(&mut self) -> Expression {
        let start = self.idx;
        let mut parenthesis = false;
        let mut async_ = false;
        let mut state = super::lexer::ParserState::default();
        match self.token {
            token::LEFT_PARENTHESIS => {
                state = self.mark();
                parenthesis = true;
            }
            token::ASYNC => {
                let tok = self.peek();
                if self.is_binding_id(tok) {
                    self.next();
                    return self.parse_single_arg_arrow_function(start, true);
                } else if tok == token::LEFT_PARENTHESIS {
                    state = self.mark();
                    async_ = true;
                }
            }
            token::YIELD if self.scope.allow_yield => return self.parse_yield_expression(),
            _ => self.token_to_binding_id(),
        }
        let left = self.parse_conditional_expression();
        let operator = match self.token {
            token::ASSIGN => self.token,
            token::ADD_ASSIGN => token::PLUS,
            token::SUBTRACT_ASSIGN => token::MINUS,
            token::MULTIPLY_ASSIGN => token::MULTIPLY,
            token::EXPONENT_ASSIGN => token::EXPONENT,
            token::QUOTIENT_ASSIGN => token::SLASH,
            token::REMAINDER_ASSIGN => token::REMAINDER,
            token::AND_ASSIGN => token::AND,
            token::OR_ASSIGN => token::OR,
            token::EXCLUSIVE_OR_ASSIGN => token::EXCLUSIVE_OR,
            token::SHIFT_LEFT_ASSIGN => token::SHIFT_LEFT,
            token::SHIFT_RIGHT_ASSIGN => token::SHIFT_RIGHT,
            token::UNSIGNED_SHIFT_RIGHT_ASSIGN => token::UNSIGNED_SHIFT_RIGHT,
            token::LOGICAL_AND_ASSIGN => token::LOGICAL_AND,
            token::LOGICAL_OR_ASSIGN => token::LOGICAL_OR,
            token::COALESCE_ASSIGN => token::COALESCE,
            token::ARROW => {
                let mut restore_await = false;
                let mut param_list: Option<ParameterList> = None;
                let (l0, l1) = (left.idx0(), left.idx1());
                if let Expression::Identifier(id) = left {
                    param_list = Some(ParameterList {
                        opening: id.idx,
                        closing: id.idx1() - 1,
                        list: vec![Arc::new(Binding {
                            target: BindingTarget::Identifier(id),
                            initializer: None,
                        })],
                        rest: None,
                    });
                } else if parenthesis {
                    match left {
                        Expression::SequenceExpression(seq) if self.errors.is_empty() => {
                            param_list =
                                Some(self.reinterpret_sequence_as_arrow_func_params(seq.sequence));
                        }
                        _ => {
                            self.restore(&state);
                            param_list = Some(self.parse_function_parameter_list());
                        }
                    }
                } else if async_ {
                    if !self.scope.allow_await {
                        self.scope.allow_await = true;
                        restore_await = true;
                    }
                    if let Expression::CallExpression(_) = left {
                        self.restore(&state);
                        self.next();
                        param_list = Some(self.parse_function_parameter_list());
                    }
                }
                let r = match param_list {
                    None => {
                        self.error_idx(l0, "Malformed arrow function parameter list");
                        bad(l0, l1)
                    }
                    Some(p) => self.parse_arrow_function(start, p, async_),
                };
                if restore_await {
                    self.scope.allow_await = false;
                }
                return r;
            }
            _ => Token(0),
        };
        if operator != Token(0) {
            let idx = self.idx;
            self.next();
            let left0 = left.idx0();
            let target = match left {
                l @ (Expression::Identifier(_)
                | Expression::DotExpression(_)
                | Expression::PrivateDotExpression(_)
                | Expression::BracketExpression(_)) => Ok(l),
                Expression::ArrayLiteral(l) if !parenthesis && operator == token::ASSIGN => {
                    Ok(self.reinterpret_as_array_assignment_pattern(*l))
                }
                Expression::ObjectLiteral(l) if !parenthesis && operator == token::ASSIGN => {
                    Ok(self.reinterpret_as_object_assignment_pattern(*l))
                }
                l => Err(l),
            };
            return match target {
                Ok(left) => Expression::AssignExpression(Box::new(AssignExpression {
                    left,
                    operator,
                    right: self.parse_assignment_expression(),
                })),
                Err(_) => {
                    self.error_idx(left0, "Invalid left-hand side in assignment");
                    self.next_statement();
                    bad(idx, self.idx)
                }
            };
        }
        left
    }

    fn parse_yield_expression(&mut self) -> Expression {
        let idx = self.expect(token::YIELD);
        if self.scope.in_func_params {
            self.error_idx(idx, "Yield expression not allowed in formal parameter");
        }
        let mut node = YieldExpression {
            yield_: idx,
            argument: None,
            delegate: false,
        };
        if !self.implicit_semicolon && self.token == token::MULTIPLY {
            node.delegate = true;
            self.next();
        }
        if !self.implicit_semicolon
            && self.token != token::SEMICOLON
            && self.token != token::RIGHT_BRACE
            && self.token != token::EOF
        {
            let state = self.mark();
            let expr = self.parse_assignment_expression();
            if let Expression::BadExpression(_) = expr {
                self.restore(&state);
                node.argument = None;
            } else {
                node.argument = Some(expr);
            }
        }
        Expression::YieldExpression(Box::new(node))
    }

    pub(super) fn parse_expression(&mut self) -> Expression {
        let left = self.parse_assignment_expression();
        if self.token == token::COMMA {
            let mut sequence = vec![left];
            loop {
                if self.token != token::COMMA {
                    break;
                }
                self.next();
                sequence.push(self.parse_assignment_expression());
            }
            return Expression::SequenceExpression(Box::new(SequenceExpression { sequence }));
        }
        left
    }

    fn check_comma(&mut self, from: Idx, to: Idx) {
        let s = &self.str[(from - self.base) as usize..(to - self.base) as usize];
        if let Some(pos) = s.iter().position(|&c| c == b',') {
            self.error_idx(from + pos as Idx, "Comma is not allowed here");
        }
    }

    pub(super) fn reinterpret_as_array_assignment_pattern(
        &mut self,
        left: ArrayLiteral,
    ) -> Expression {
        let (l0, l1) = (left.left_bracket, left.right_bracket + 1);
        let mut value = left.value;
        let mut rest = None;
        let n = value.len();
        let mut i = 0;
        while i < value.len() {
            if let Some(Expression::SpreadElement(_)) = &value[i] {
                if i != n - 1 {
                    let idx = value[i].as_ref().map_or(0, |v| v.idx0());
                    self.error_idx(idx, "Rest element must be last element");
                    return bad(l0, l1);
                }
                let Some(Expression::SpreadElement(spread)) = value.pop().flatten() else {
                    unreachable!("checked above");
                };
                self.check_comma(spread.expression.idx1(), left.right_bracket);
                rest = self.reinterpret_as_destruct_assign_target(Some(spread.expression));
                break;
            }
            let item = value[i].take();
            value[i] = self.reinterpret_as_assignment_element(item);
            i += 1;
        }
        Expression::ArrayPattern(Box::new(ArrayPattern {
            left_bracket: left.left_bracket,
            right_bracket: left.right_bracket,
            elements: value,
            rest,
        }))
    }

    fn reinterpret_array_assign_pattern_as_binding(
        &mut self,
        mut pattern: ArrayPattern,
    ) -> ArrayPattern {
        for i in 0..pattern.elements.len() {
            let item = pattern.elements[i].take();
            pattern.elements[i] = self
                .reinterpret_as_destruct_binding_target(item)
                .map(Expression::from);
        }
        if pattern.rest.is_some() {
            let rest = pattern.rest.take();
            pattern.rest = self
                .reinterpret_as_destruct_binding_target(rest)
                .map(Expression::from);
        }
        pattern
    }

    fn reinterpret_as_array_binding_pattern(&mut self, left: ArrayLiteral) -> BindingTarget {
        let (l0, l1) = (left.left_bracket, left.right_bracket + 1);
        let mut value = left.value;
        let mut rest = None;
        let n = value.len();
        let mut i = 0;
        while i < value.len() {
            if let Some(Expression::SpreadElement(_)) = &value[i] {
                if i != n - 1 {
                    let idx = value[i].as_ref().map_or(0, |v| v.idx0());
                    self.error_idx(idx, "Rest element must be last element");
                    return bad_target(l0, l1);
                }
                let Some(Expression::SpreadElement(spread)) = value.pop().flatten() else {
                    unreachable!("checked above");
                };
                self.check_comma(spread.expression.idx1(), left.right_bracket);
                rest = self
                    .reinterpret_as_destruct_binding_target(Some(spread.expression))
                    .map(Expression::from);
                break;
            }
            let item = value[i].take();
            value[i] = self.reinterpret_as_assignment_element(item);
            i += 1;
        }
        BindingTarget::ArrayPattern(Box::new(ArrayPattern {
            left_bracket: left.left_bracket,
            right_bracket: left.right_bracket,
            elements: value,
            rest,
        }))
    }

    fn parse_array_binding_pattern(&mut self) -> BindingTarget {
        let l = self.parse_array_literal();
        self.reinterpret_as_array_binding_pattern(l)
    }

    fn parse_object_binding_pattern(&mut self) -> BindingTarget {
        let l = self.parse_object_literal();
        self.reinterpret_as_object_binding_pattern(l)
    }

    fn reinterpret_array_object_pattern_as_binding(
        &mut self,
        mut pattern: ObjectPattern,
    ) -> ObjectPattern {
        for prop in pattern.properties.iter_mut() {
            if let Property::PropertyKeyed(keyed) = prop {
                let v = std::mem::replace(&mut keyed.value, bad(0, 0));
                keyed.value = self.reinterpret_as_binding_element(v);
            }
        }
        if let Some(rest) = pattern.rest.take() {
            pattern.rest = Some(self.reinterpret_as_binding_rest_element(rest));
        }
        pattern
    }

    fn reinterpret_as_object_binding_pattern(&mut self, expr: ObjectLiteral) -> BindingTarget {
        let (e0, e1) = (expr.left_brace, expr.right_brace + 1);
        let mut rest = None;
        let mut value = expr.value;
        let n = value.len();
        let mut i = 0;
        while i < value.len() {
            let ok = match &mut value[i] {
                Property::PropertyKeyed(prop) => {
                    if prop.kind == PropertyKind::Value {
                        let v = std::mem::replace(&mut prop.value, bad(0, 0));
                        prop.value = self.reinterpret_as_binding_element(v);
                        true
                    } else {
                        false
                    }
                }
                Property::PropertyShort(_) => true,
                Property::SpreadElement(_) => {
                    if i != n - 1 {
                        let idx = value[i].idx0();
                        self.error_idx(idx, "Rest element must be last element");
                        return bad_target(e0, e1);
                    }
                    let Some(Property::SpreadElement(spread)) = value.pop() else {
                        unreachable!("checked above");
                    };
                    rest = Some(self.reinterpret_as_binding_rest_element(spread.expression));
                    break;
                }
            };
            if !ok {
                let idx = value[i].idx0();
                self.error_idx(idx, "Invalid destructuring binding target");
                return bad_target(e0, e1);
            }
            i += 1;
        }
        BindingTarget::ObjectPattern(Box::new(ObjectPattern {
            left_brace: expr.left_brace,
            right_brace: expr.right_brace,
            properties: value,
            rest,
        }))
    }

    pub(super) fn reinterpret_as_object_assignment_pattern(
        &mut self,
        l: ObjectLiteral,
    ) -> Expression {
        let (e0, e1) = (l.left_brace, l.right_brace + 1);
        let mut rest = None;
        let mut value = l.value;
        let n = value.len();
        let mut i = 0;
        while i < value.len() {
            let ok = match &mut value[i] {
                Property::PropertyKeyed(prop) => {
                    if prop.kind == PropertyKind::Value {
                        let v = std::mem::replace(&mut prop.value, bad(0, 0));
                        prop.value = self
                            .reinterpret_as_assignment_element(Some(v))
                            .unwrap_or_else(|| bad(0, 0));
                        true
                    } else {
                        false
                    }
                }
                Property::PropertyShort(_) => true,
                Property::SpreadElement(_) => {
                    if i != n - 1 {
                        let idx = value[i].idx0();
                        self.error_idx(idx, "Rest element must be last element");
                        return bad(e0, e1);
                    }
                    let Some(Property::SpreadElement(spread)) = value.pop() else {
                        unreachable!("checked above");
                    };
                    rest = Some(spread.expression);
                    break;
                }
            };
            if !ok {
                let idx = value[i].idx0();
                self.error_idx(idx, "Invalid destructuring assignment target");
                return bad(e0, e1);
            }
            i += 1;
        }
        Expression::ObjectPattern(Box::new(ObjectPattern {
            left_brace: l.left_brace,
            right_brace: l.right_brace,
            properties: value,
            rest,
        }))
    }

    fn reinterpret_as_assignment_element(
        &mut self,
        expr: Option<Expression>,
    ) -> Option<Expression> {
        match expr {
            Some(Expression::AssignExpression(mut e)) => {
                if e.operator == token::ASSIGN {
                    let left = std::mem::replace(&mut e.left, bad(0, 0));
                    e.left = self
                        .reinterpret_as_destruct_assign_target(Some(left))
                        .unwrap_or_else(|| bad(0, 0));
                    Some(Expression::AssignExpression(e))
                } else {
                    let ex = Expression::AssignExpression(e);
                    let (i0, i1) = (ex.idx0(), ex.idx1());
                    self.error_idx(i0, "Invalid destructuring assignment target");
                    Some(bad(i0, i1))
                }
            }
            other => self.reinterpret_as_destruct_assign_target(other),
        }
    }

    fn reinterpret_as_binding_element(&mut self, expr: Expression) -> Expression {
        match expr {
            Expression::AssignExpression(mut e) => {
                if e.operator == token::ASSIGN {
                    let left = std::mem::replace(&mut e.left, bad(0, 0));
                    e.left = self
                        .reinterpret_as_destruct_binding_target(Some(left))
                        .map(Expression::from)
                        .unwrap_or_else(|| bad(0, 0));
                    Expression::AssignExpression(e)
                } else {
                    let ex = Expression::AssignExpression(e);
                    let (i0, i1) = (ex.idx0(), ex.idx1());
                    self.error_idx(i0, "Invalid destructuring assignment target");
                    bad(i0, i1)
                }
            }
            other => self
                .reinterpret_as_destruct_binding_target(Some(other))
                .map(Expression::from)
                .unwrap_or_else(|| bad(0, 0)),
        }
    }

    fn reinterpret_as_binding(&mut self, expr: Expression) -> Binding {
        match expr {
            Expression::AssignExpression(e) => {
                if e.operator == token::ASSIGN {
                    let e = *e;
                    let target = self
                        .reinterpret_as_destruct_binding_target(Some(e.left))
                        .unwrap_or_else(|| bad_target(0, 0));
                    Binding {
                        target,
                        initializer: Some(e.right),
                    }
                } else {
                    let ex = Expression::AssignExpression(e);
                    let (i0, i1) = (ex.idx0(), ex.idx1());
                    self.error_idx(i0, "Invalid destructuring assignment target");
                    Binding {
                        target: bad_target(i0, i1),
                        initializer: None,
                    }
                }
            }
            other => Binding {
                target: self
                    .reinterpret_as_destruct_binding_target(Some(other))
                    .unwrap_or_else(|| bad_target(0, 0)),
                initializer: None,
            },
        }
    }

    fn reinterpret_as_destruct_assign_target(
        &mut self,
        item: Option<Expression>,
    ) -> Option<Expression> {
        let item = item?;
        match item {
            Expression::ArrayLiteral(l) => Some(self.reinterpret_as_array_assignment_pattern(*l)),
            Expression::ObjectLiteral(l) => Some(self.reinterpret_as_object_assignment_pattern(*l)),
            e @ (Expression::ArrayPattern(_)
            | Expression::ObjectPattern(_)
            | Expression::Identifier(_)
            | Expression::DotExpression(_)
            | Expression::PrivateDotExpression(_)
            | Expression::BracketExpression(_)) => Some(e),
            e => {
                let (i0, i1) = (e.idx0(), e.idx1());
                self.error_idx(i0, "Invalid destructuring assignment target");
                Some(bad(i0, i1))
            }
        }
    }

    pub(super) fn reinterpret_as_destruct_binding_target(
        &mut self,
        item: Option<Expression>,
    ) -> Option<BindingTarget> {
        let item = item?;
        match item {
            Expression::ArrayPattern(p) => {
                return Some(BindingTarget::ArrayPattern(Box::new(
                    self.reinterpret_array_assign_pattern_as_binding(*p),
                )));
            }
            Expression::ObjectPattern(p) => {
                return Some(BindingTarget::ObjectPattern(Box::new(
                    self.reinterpret_array_object_pattern_as_binding(*p),
                )));
            }
            Expression::ArrayLiteral(l) => {
                return Some(self.reinterpret_as_array_binding_pattern(*l));
            }
            Expression::ObjectLiteral(l) => {
                return Some(self.reinterpret_as_object_binding_pattern(*l));
            }
            Expression::Identifier(id) => {
                if !self.scope.allow_await || id.name != "await" {
                    return Some(BindingTarget::Identifier(id));
                }
                let (i0, i1) = (id.idx0(), id.idx1());
                self.error_idx(i0, "Invalid destructuring binding target");
                return Some(bad_target(i0, i1));
            }
            _ => {}
        }
        let (i0, i1) = (item.idx0(), item.idx1());
        self.error_idx(i0, "Invalid destructuring binding target");
        Some(bad_target(i0, i1))
    }

    fn reinterpret_as_binding_rest_element(&mut self, expr: Expression) -> Expression {
        if let Expression::Identifier(_) = expr {
            return expr;
        }
        let (i0, i1) = (expr.idx0(), expr.idx1());
        self.error_idx(i0, "Invalid binding rest");
        bad(i0, i1)
    }
}
