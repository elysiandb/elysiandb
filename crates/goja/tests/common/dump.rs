use gocompat::strconv::quote;
use goja::ast::*;
use goja::token::Token;
use goja::unistring;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Dumper {
    b: String,
    ids: HashMap<usize, usize>,
    next: usize,
}

pub fn dump_program(p: &Program) -> String {
    let mut d = Dumper {
        b: String::new(),
        ids: HashMap::new(),
        next: 0,
    };
    d.ptr(None);
    d.program(p);
    d.b
}

impl Dumper {
    fn w(&mut self, s: &str) {
        self.b.push_str(s);
    }

    fn ptr(&mut self, key: Option<usize>) -> bool {
        if let Some(k) = key
            && let Some(id) = self.ids.get(&k)
        {
            let s = format!("*#{id}");
            self.w(&s);
            return false;
        }
        self.next += 1;
        if let Some(k) = key {
            self.ids.insert(k, self.next);
        }
        let s = format!("*#{} ", self.next);
        self.w(&s);
        true
    }

    fn open(&mut self, name: &str) {
        self.w(name);
        self.w("{");
    }

    fn field(&mut self, first: bool, name: &str) {
        if !first {
            self.w(" ");
        }
        self.w(name);
        self.w(":");
    }

    fn close(&mut self) {
        self.w("}");
    }

    fn idx(&mut self, v: i64) {
        let s = v.to_string();
        self.w(&s);
    }

    fn tok(&mut self, t: Token) {
        let s = format!("T{}", t.0);
        self.w(&s);
    }

    fn boolean(&mut self, v: bool) {
        self.w(if v { "true" } else { "false" });
    }

    fn string(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    fn bytes(&mut self, s: &[u8]) {
        let q = quote(s);
        self.w(&q);
    }

    fn uni(&mut self, s: &unistring::String) {
        let q = quote(s.string().as_bytes());
        if s.as_utf16().is_some() {
            self.w("u");
        }
        self.w(&q);
    }

    fn nil(&mut self) {
        self.w("nil");
    }

    fn opt_expr(&mut self, e: &Option<Expression>) {
        match e {
            Some(e) => self.expr(e),
            None => self.nil(),
        }
    }

    fn exprs(&mut self, list: &[Expression]) {
        self.w("[");
        for (i, e) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.expr(e);
        }
        self.w("]");
    }

    fn opt_exprs(&mut self, list: &[Option<Expression>]) {
        self.w("[");
        for (i, e) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.opt_expr(e);
        }
        self.w("]");
    }

    fn stmts(&mut self, list: &[Statement]) {
        self.w("[");
        for (i, s) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.stmt(s);
        }
        self.w("]");
    }

    fn bindings(&mut self, list: &[Arc<Binding>]) {
        self.w("[");
        for (i, b) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.binding(b);
        }
        self.w("]");
    }

    fn binding(&mut self, b: &Arc<Binding>) {
        if !self.ptr(Some(Arc::as_ptr(b) as usize)) {
            return;
        }
        self.open("Binding");
        self.field(true, "Target");
        self.target(&b.target);
        self.field(false, "Initializer");
        self.opt_expr(&b.initializer);
        self.close();
    }

    fn decls(&mut self, list: &[VariableDeclaration]) {
        self.w("[");
        for (i, d) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.ptr(None);
            self.open("VariableDeclaration");
            self.field(true, "Var");
            self.idx(d.var);
            self.field(false, "List");
            self.bindings(&d.list);
            self.close();
        }
        self.w("]");
    }

    fn program(&mut self, p: &Program) {
        self.open("Program");
        self.field(true, "Body");
        self.stmts(&p.body);
        self.field(false, "DeclarationList");
        self.decls(&p.declaration_list);
        self.field(false, "File");
        if p.file.is_some() {
            self.w("<file>");
        } else {
            self.nil();
        }
        self.close();
    }

    fn ident_value(&mut self, i: &Identifier) {
        self.open("Identifier");
        self.field(true, "Name");
        self.uni(&i.name);
        self.field(false, "Idx");
        self.idx(i.idx);
        self.close();
    }

    fn ident_ptr(&mut self, i: &Identifier) {
        self.ptr(None);
        self.ident_value(i);
    }

    fn opt_ident(&mut self, i: &Option<Box<Identifier>>) {
        match i {
            Some(i) => self.ident_ptr(i),
            None => self.nil(),
        }
    }

    fn private_ident_value(&mut self, p: &PrivateIdentifier) {
        self.open("PrivateIdentifier");
        self.field(true, "Identifier");
        self.ident_value(&p.identifier);
        self.close();
    }

    fn target(&mut self, t: &BindingTarget) {
        match t {
            BindingTarget::Identifier(i) => self.ident_ptr(i),
            BindingTarget::ArrayPattern(p) => self.array_pattern(p),
            BindingTarget::ObjectPattern(p) => self.object_pattern(p),
            BindingTarget::BadExpression(b) => self.bad_expr(b),
        }
    }

    fn opt_target(&mut self, t: &Option<BindingTarget>) {
        match t {
            Some(t) => self.target(t),
            None => self.nil(),
        }
    }

    fn bad_expr(&mut self, b: &BadExpression) {
        self.ptr(None);
        self.open("BadExpression");
        self.field(true, "From");
        self.idx(b.from);
        self.field(false, "To");
        self.idx(b.to);
        self.close();
    }

    fn array_pattern(&mut self, p: &ArrayPattern) {
        self.ptr(None);
        self.open("ArrayPattern");
        self.field(true, "LeftBracket");
        self.idx(p.left_bracket);
        self.field(false, "RightBracket");
        self.idx(p.right_bracket);
        self.field(false, "Elements");
        self.opt_exprs(&p.elements);
        self.field(false, "Rest");
        self.opt_expr(&p.rest);
        self.close();
    }

    fn object_pattern(&mut self, p: &ObjectPattern) {
        self.ptr(None);
        self.open("ObjectPattern");
        self.field(true, "LeftBrace");
        self.idx(p.left_brace);
        self.field(false, "RightBrace");
        self.idx(p.right_brace);
        self.field(false, "Properties");
        self.props(&p.properties);
        self.field(false, "Rest");
        self.opt_expr(&p.rest);
        self.close();
    }

    fn props(&mut self, list: &[Property]) {
        self.w("[");
        for (i, p) in list.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.ptr(None);
            match p {
                Property::PropertyShort(s) => {
                    self.open("PropertyShort");
                    self.field(true, "Name");
                    self.ident_value(&s.name);
                    self.field(false, "Initializer");
                    self.opt_expr(&s.initializer);
                    self.close();
                }
                Property::PropertyKeyed(k) => {
                    self.open("PropertyKeyed");
                    self.field(true, "Key");
                    self.expr(&k.key);
                    self.field(false, "Kind");
                    self.string(k.kind.as_str());
                    self.field(false, "Value");
                    self.expr(&k.value);
                    self.field(false, "Computed");
                    self.boolean(k.computed);
                    self.close();
                }
                Property::SpreadElement(s) => {
                    self.open("SpreadElement");
                    self.field(true, "Expression");
                    self.expr(&s.expression);
                    self.close();
                }
            }
        }
        self.w("]");
    }

    fn params(&mut self, p: &ParameterList) {
        self.ptr(None);
        self.open("ParameterList");
        self.field(true, "Opening");
        self.idx(p.opening);
        self.field(false, "List");
        self.bindings(&p.list);
        self.field(false, "Rest");
        self.opt_expr(&p.rest);
        self.field(false, "Closing");
        self.idx(p.closing);
        self.close();
    }

    fn block(&mut self, b: &BlockStatement) {
        self.ptr(None);
        self.open("BlockStatement");
        self.field(true, "LeftBrace");
        self.idx(b.left_brace);
        self.field(false, "List");
        self.stmts(&b.list);
        self.field(false, "RightBrace");
        self.idx(b.right_brace);
        self.close();
    }

    fn opt_block(&mut self, b: &Option<Box<BlockStatement>>) {
        match b {
            Some(b) => self.block(b),
            None => self.nil(),
        }
    }

    fn function(&mut self, f: &FunctionLiteral) {
        self.ptr(None);
        self.open("FunctionLiteral");
        self.field(true, "Function");
        self.idx(f.function);
        self.field(false, "Name");
        self.opt_ident(&f.name);
        self.field(false, "ParameterList");
        self.params(&f.parameter_list);
        self.field(false, "Body");
        self.block(&f.body);
        self.field(false, "Source");
        self.bytes(&f.source);
        self.field(false, "DeclarationList");
        self.decls(&f.declaration_list);
        self.field(false, "Async");
        self.boolean(f.async_);
        self.field(false, "Generator");
        self.boolean(f.generator);
        self.close();
    }

    fn class(&mut self, c: &ClassLiteral) {
        self.ptr(None);
        self.open("ClassLiteral");
        self.field(true, "Class");
        self.idx(c.class);
        self.field(false, "RightBrace");
        self.idx(c.right_brace);
        self.field(false, "Name");
        self.opt_ident(&c.name);
        self.field(false, "SuperClass");
        self.opt_expr(&c.super_class);
        self.field(false, "Body");
        self.w("[");
        for (i, e) in c.body.iter().enumerate() {
            if i > 0 {
                self.w(" ");
            }
            self.ptr(None);
            match e {
                ClassElement::FieldDefinition(f) => {
                    self.open("FieldDefinition");
                    self.field(true, "Idx");
                    self.idx(f.idx);
                    self.field(false, "Key");
                    self.expr(&f.key);
                    self.field(false, "Initializer");
                    self.opt_expr(&f.initializer);
                    self.field(false, "Computed");
                    self.boolean(f.computed);
                    self.field(false, "Static");
                    self.boolean(f.static_);
                    self.close();
                }
                ClassElement::MethodDefinition(m) => {
                    self.open("MethodDefinition");
                    self.field(true, "Idx");
                    self.idx(m.idx);
                    self.field(false, "Key");
                    self.expr(&m.key);
                    self.field(false, "Kind");
                    self.string(m.kind.as_str());
                    self.field(false, "Body");
                    self.function(&m.body);
                    self.field(false, "Computed");
                    self.boolean(m.computed);
                    self.field(false, "Static");
                    self.boolean(m.static_);
                    self.close();
                }
                ClassElement::ClassStaticBlock(s) => {
                    self.open("ClassStaticBlock");
                    self.field(true, "Static");
                    self.idx(s.static_);
                    self.field(false, "Block");
                    self.block(&s.block);
                    self.field(false, "Source");
                    self.bytes(&s.source);
                    self.field(false, "DeclarationList");
                    self.decls(&s.declaration_list);
                    self.close();
                }
            }
        }
        self.w("]");
        self.field(false, "Source");
        self.bytes(&c.source);
        self.close();
    }

    fn expr(&mut self, e: &Expression) {
        match e {
            Expression::ArrayPattern(p) => return self.array_pattern(p),
            Expression::ObjectPattern(p) => return self.object_pattern(p),
            Expression::BadExpression(b) => return self.bad_expr(b),
            Expression::FunctionLiteral(f) => return self.function(f),
            Expression::ClassLiteral(c) => return self.class(c),
            Expression::Identifier(i) => return self.ident_ptr(i),
            _ => {}
        }
        self.ptr(None);
        match e {
            Expression::ArrayLiteral(n) => {
                self.open("ArrayLiteral");
                self.field(true, "LeftBracket");
                self.idx(n.left_bracket);
                self.field(false, "RightBracket");
                self.idx(n.right_bracket);
                self.field(false, "Value");
                self.opt_exprs(&n.value);
            }
            Expression::ArrowFunctionLiteral(n) => {
                self.open("ArrowFunctionLiteral");
                self.field(true, "Start");
                self.idx(n.start);
                self.field(false, "ParameterList");
                self.params(&n.parameter_list);
                self.field(false, "Body");
                match &n.body {
                    ConciseBody::BlockStatement(b) => self.block(b),
                    ConciseBody::ExpressionBody(b) => {
                        self.ptr(None);
                        self.open("ExpressionBody");
                        self.field(true, "Expression");
                        self.expr(&b.expression);
                        self.close();
                    }
                }
                self.field(false, "Source");
                self.bytes(&n.source);
                self.field(false, "DeclarationList");
                self.decls(&n.declaration_list);
                self.field(false, "Async");
                self.boolean(n.async_);
            }
            Expression::AssignExpression(n) => {
                self.open("AssignExpression");
                self.field(true, "Operator");
                self.tok(n.operator);
                self.field(false, "Left");
                self.expr(&n.left);
                self.field(false, "Right");
                self.expr(&n.right);
            }
            Expression::AwaitExpression(n) => {
                self.open("AwaitExpression");
                self.field(true, "Await");
                self.idx(n.await_);
                self.field(false, "Argument");
                self.expr(&n.argument);
            }
            Expression::BinaryExpression(n) => {
                self.open("BinaryExpression");
                self.field(true, "Operator");
                self.tok(n.operator);
                self.field(false, "Left");
                self.expr(&n.left);
                self.field(false, "Right");
                self.expr(&n.right);
                self.field(false, "Comparison");
                self.boolean(n.comparison);
            }
            Expression::BooleanLiteral(n) => {
                self.open("BooleanLiteral");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Literal");
                self.bytes(&n.literal);
                self.field(false, "Value");
                self.boolean(n.value);
            }
            Expression::BracketExpression(n) => {
                self.open("BracketExpression");
                self.field(true, "Left");
                self.expr(&n.left);
                self.field(false, "Member");
                self.expr(&n.member);
                self.field(false, "LeftBracket");
                self.idx(n.left_bracket);
                self.field(false, "RightBracket");
                self.idx(n.right_bracket);
            }
            Expression::CallExpression(n) => {
                self.open("CallExpression");
                self.field(true, "Callee");
                self.expr(&n.callee);
                self.field(false, "LeftParenthesis");
                self.idx(n.left_parenthesis);
                self.field(false, "ArgumentList");
                self.exprs(&n.argument_list);
                self.field(false, "RightParenthesis");
                self.idx(n.right_parenthesis);
            }
            Expression::ConditionalExpression(n) => {
                self.open("ConditionalExpression");
                self.field(true, "Test");
                self.expr(&n.test);
                self.field(false, "Consequent");
                self.expr(&n.consequent);
                self.field(false, "Alternate");
                self.expr(&n.alternate);
            }
            Expression::DotExpression(n) => {
                self.open("DotExpression");
                self.field(true, "Left");
                self.expr(&n.left);
                self.field(false, "Identifier");
                self.ident_value(&n.identifier);
            }
            Expression::MetaProperty(n) => {
                self.open("MetaProperty");
                self.field(true, "Meta");
                self.ident_ptr(&n.meta);
                self.field(false, "Property");
                self.ident_ptr(&n.property);
                self.field(false, "Idx");
                self.idx(n.idx);
            }
            Expression::NewExpression(n) => {
                self.open("NewExpression");
                self.field(true, "New");
                self.idx(n.new);
                self.field(false, "Callee");
                self.expr(&n.callee);
                self.field(false, "LeftParenthesis");
                self.idx(n.left_parenthesis);
                self.field(false, "ArgumentList");
                self.exprs(&n.argument_list);
                self.field(false, "RightParenthesis");
                self.idx(n.right_parenthesis);
            }
            Expression::NullLiteral(n) => {
                self.open("NullLiteral");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Literal");
                self.bytes(&n.literal);
            }
            Expression::NumberLiteral(n) => {
                self.open("NumberLiteral");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Literal");
                self.bytes(&n.literal);
                self.field(false, "Value");
                let v = match &n.value {
                    NumberValue::Int64(v) => format!("int64({v})"),
                    NumberValue::Int(v) => format!("int({v})"),
                    NumberValue::Float64(f) => format!("float64({:016x})", f.to_bits()),
                    NumberValue::BigInt(b) => format!("bigint({b})"),
                };
                self.w(&v);
            }
            Expression::ObjectLiteral(n) => {
                self.open("ObjectLiteral");
                self.field(true, "LeftBrace");
                self.idx(n.left_brace);
                self.field(false, "RightBrace");
                self.idx(n.right_brace);
                self.field(false, "Value");
                self.props(&n.value);
            }
            Expression::Optional(n) => {
                self.open("Optional");
                self.field(true, "Expression");
                self.expr(&n.expression);
            }
            Expression::OptionalChain(n) => {
                self.open("OptionalChain");
                self.field(true, "Expression");
                self.expr(&n.expression);
            }
            Expression::PrivateDotExpression(n) => {
                self.open("PrivateDotExpression");
                self.field(true, "Left");
                self.expr(&n.left);
                self.field(false, "Identifier");
                self.private_ident_value(&n.identifier);
            }
            Expression::PrivateIdentifier(n) => {
                self.open("PrivateIdentifier");
                self.field(true, "Identifier");
                self.ident_value(&n.identifier);
            }
            Expression::RegExpLiteral(n) => {
                self.open("RegExpLiteral");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Literal");
                self.bytes(&n.literal);
                self.field(false, "Pattern");
                self.bytes(&n.pattern);
                self.field(false, "Flags");
                self.bytes(&n.flags);
            }
            Expression::SequenceExpression(n) => {
                self.open("SequenceExpression");
                self.field(true, "Sequence");
                self.exprs(&n.sequence);
            }
            Expression::SpreadElement(n) => {
                self.open("SpreadElement");
                self.field(true, "Expression");
                self.expr(&n.expression);
            }
            Expression::StringLiteral(n) => {
                self.open("StringLiteral");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Literal");
                self.bytes(&n.literal);
                self.field(false, "Value");
                self.uni(&n.value);
            }
            Expression::SuperExpression(n) => {
                self.open("SuperExpression");
                self.field(true, "Idx");
                self.idx(n.idx);
            }
            Expression::TemplateLiteral(n) => {
                self.open("TemplateLiteral");
                self.field(true, "OpenQuote");
                self.idx(n.open_quote);
                self.field(false, "CloseQuote");
                self.idx(n.close_quote);
                self.field(false, "Tag");
                self.opt_expr(&n.tag);
                self.field(false, "Elements");
                self.w("[");
                for (i, el) in n.elements.iter().enumerate() {
                    if i > 0 {
                        self.w(" ");
                    }
                    self.ptr(None);
                    self.open("TemplateElement");
                    self.field(true, "Idx");
                    self.idx(el.idx);
                    self.field(false, "Literal");
                    self.bytes(&el.literal);
                    self.field(false, "Parsed");
                    self.uni(&el.parsed);
                    self.field(false, "Valid");
                    self.boolean(el.valid);
                    self.close();
                }
                self.w("]");
                self.field(false, "Expressions");
                self.exprs(&n.expressions);
            }
            Expression::ThisExpression(n) => {
                self.open("ThisExpression");
                self.field(true, "Idx");
                self.idx(n.idx);
            }
            Expression::UnaryExpression(n) => {
                self.open("UnaryExpression");
                self.field(true, "Operator");
                self.tok(n.operator);
                self.field(false, "Idx");
                self.idx(n.idx);
                self.field(false, "Operand");
                self.expr(&n.operand);
                self.field(false, "Postfix");
                self.boolean(n.postfix);
            }
            Expression::YieldExpression(n) => {
                self.open("YieldExpression");
                self.field(true, "Yield");
                self.idx(n.yield_);
                self.field(false, "Argument");
                self.opt_expr(&n.argument);
                self.field(false, "Delegate");
                self.boolean(n.delegate);
            }
            _ => unreachable!(),
        }
        self.close();
    }

    fn opt_stmt(&mut self, s: &Option<Statement>) {
        match s {
            Some(s) => self.stmt(s),
            None => self.nil(),
        }
    }

    fn for_into(&mut self, i: &ForInto) {
        self.ptr(None);
        match i {
            ForInto::ForIntoVar(v) => {
                self.open("ForIntoVar");
                self.field(true, "Binding");
                self.binding(&v.binding);
            }
            ForInto::ForDeclaration(d) => {
                self.open("ForDeclaration");
                self.field(true, "Idx");
                self.idx(d.idx);
                self.field(false, "IsConst");
                self.boolean(d.is_const);
                self.field(false, "Target");
                self.target(&d.target);
            }
            ForInto::ForIntoExpression(e) => {
                self.open("ForIntoExpression");
                self.field(true, "Expression");
                self.expr(&e.expression);
            }
        }
        self.close();
    }

    fn lexical_value(&mut self, l: &LexicalDeclaration) {
        self.open("LexicalDeclaration");
        self.field(true, "Idx");
        self.idx(l.idx);
        self.field(false, "Token");
        self.tok(l.token);
        self.field(false, "List");
        self.bindings(&l.list);
        self.close();
    }

    fn stmt(&mut self, s: &Statement) {
        if let Statement::BlockStatement(b) = s {
            return self.block(b);
        }
        self.ptr(None);
        match s {
            Statement::BadStatement(n) => {
                self.open("BadStatement");
                self.field(true, "From");
                self.idx(n.from);
                self.field(false, "To");
                self.idx(n.to);
            }
            Statement::BranchStatement(n) => {
                self.open("BranchStatement");
                self.field(true, "Idx");
                self.idx(n.idx);
                self.field(false, "Token");
                self.tok(n.token);
                self.field(false, "Label");
                self.opt_ident(&n.label);
            }
            Statement::ClassDeclaration(n) => {
                self.open("ClassDeclaration");
                self.field(true, "Class");
                self.class(&n.class);
            }
            Statement::DebuggerStatement(n) => {
                self.open("DebuggerStatement");
                self.field(true, "Debugger");
                self.idx(n.debugger);
            }
            Statement::DoWhileStatement(n) => {
                self.open("DoWhileStatement");
                self.field(true, "Do");
                self.idx(n.do_);
                self.field(false, "Test");
                self.expr(&n.test);
                self.field(false, "Body");
                self.stmt(&n.body);
                self.field(false, "RightParenthesis");
                self.idx(n.right_parenthesis);
            }
            Statement::EmptyStatement(n) => {
                self.open("EmptyStatement");
                self.field(true, "Semicolon");
                self.idx(n.semicolon);
            }
            Statement::ExpressionStatement(n) => {
                self.open("ExpressionStatement");
                self.field(true, "Expression");
                self.expr(&n.expression);
            }
            Statement::ForInStatement(n) => {
                self.open("ForInStatement");
                self.field(true, "For");
                self.idx(n.for_);
                self.field(false, "Into");
                self.for_into(&n.into);
                self.field(false, "Source");
                self.expr(&n.source);
                self.field(false, "Body");
                self.stmt(&n.body);
            }
            Statement::ForOfStatement(n) => {
                self.open("ForOfStatement");
                self.field(true, "For");
                self.idx(n.for_);
                self.field(false, "Into");
                self.for_into(&n.into);
                self.field(false, "Source");
                self.expr(&n.source);
                self.field(false, "Body");
                self.stmt(&n.body);
            }
            Statement::ForStatement(n) => {
                self.open("ForStatement");
                self.field(true, "For");
                self.idx(n.for_);
                self.field(false, "Initializer");
                match &n.initializer {
                    None => self.nil(),
                    Some(i) => {
                        self.ptr(None);
                        match i {
                            ForLoopInitializer::ForLoopInitializerExpression(e) => {
                                self.open("ForLoopInitializerExpression");
                                self.field(true, "Expression");
                                self.expr(&e.expression);
                            }
                            ForLoopInitializer::ForLoopInitializerVarDeclList(v) => {
                                self.open("ForLoopInitializerVarDeclList");
                                self.field(true, "Var");
                                self.idx(v.var);
                                self.field(false, "List");
                                self.bindings(&v.list);
                            }
                            ForLoopInitializer::ForLoopInitializerLexicalDecl(l) => {
                                self.open("ForLoopInitializerLexicalDecl");
                                self.field(true, "LexicalDeclaration");
                                self.lexical_value(&l.lexical_declaration);
                            }
                        }
                        self.close();
                    }
                }
                self.field(false, "Update");
                self.opt_expr(&n.update);
                self.field(false, "Test");
                self.opt_expr(&n.test);
                self.field(false, "Body");
                self.stmt(&n.body);
            }
            Statement::FunctionDeclaration(n) => {
                self.open("FunctionDeclaration");
                self.field(true, "Function");
                self.function(&n.function);
            }
            Statement::IfStatement(n) => {
                self.open("IfStatement");
                self.field(true, "If");
                self.idx(n.if_);
                self.field(false, "Test");
                self.expr(&n.test);
                self.field(false, "Consequent");
                self.stmt(&n.consequent);
                self.field(false, "Alternate");
                self.opt_stmt(&n.alternate);
            }
            Statement::LabelledStatement(n) => {
                self.open("LabelledStatement");
                self.field(true, "Label");
                self.ident_ptr(&n.label);
                self.field(false, "Colon");
                self.idx(n.colon);
                self.field(false, "Statement");
                self.stmt(&n.statement);
            }
            Statement::LexicalDeclaration(n) => {
                self.lexical_value(n);
                return;
            }
            Statement::ReturnStatement(n) => {
                self.open("ReturnStatement");
                self.field(true, "Return");
                self.idx(n.return_);
                self.field(false, "Argument");
                self.opt_expr(&n.argument);
            }
            Statement::SwitchStatement(n) => {
                self.open("SwitchStatement");
                self.field(true, "Switch");
                self.idx(n.switch);
                self.field(false, "Discriminant");
                self.expr(&n.discriminant);
                self.field(false, "Default");
                let s = format!("int({})", n.default);
                self.w(&s);
                self.field(false, "Body");
                self.w("[");
                for (i, c) in n.body.iter().enumerate() {
                    if i > 0 {
                        self.w(" ");
                    }
                    self.ptr(None);
                    self.open("CaseStatement");
                    self.field(true, "Case");
                    self.idx(c.case);
                    self.field(false, "Test");
                    self.opt_expr(&c.test);
                    self.field(false, "Consequent");
                    self.stmts(&c.consequent);
                    self.close();
                }
                self.w("]");
                self.field(false, "RightBrace");
                self.idx(n.right_brace);
            }
            Statement::ThrowStatement(n) => {
                self.open("ThrowStatement");
                self.field(true, "Throw");
                self.idx(n.throw);
                self.field(false, "Argument");
                self.expr(&n.argument);
            }
            Statement::TryStatement(n) => {
                self.open("TryStatement");
                self.field(true, "Try");
                self.idx(n.try_);
                self.field(false, "Body");
                self.block(&n.body);
                self.field(false, "Catch");
                match &n.catch {
                    None => self.nil(),
                    Some(c) => {
                        self.ptr(None);
                        self.open("CatchStatement");
                        self.field(true, "Catch");
                        self.idx(c.catch);
                        self.field(false, "Parameter");
                        self.opt_target(&c.parameter);
                        self.field(false, "Body");
                        self.block(&c.body);
                        self.close();
                    }
                }
                self.field(false, "Finally");
                self.opt_block(&n.finally);
            }
            Statement::VariableStatement(n) => {
                self.open("VariableStatement");
                self.field(true, "Var");
                self.idx(n.var);
                self.field(false, "List");
                self.bindings(&n.list);
            }
            Statement::WhileStatement(n) => {
                self.open("WhileStatement");
                self.field(true, "While");
                self.idx(n.while_);
                self.field(false, "Test");
                self.expr(&n.test);
                self.field(false, "Body");
                self.stmt(&n.body);
            }
            Statement::WithStatement(n) => {
                self.open("WithStatement");
                self.field(true, "With");
                self.idx(n.with);
                self.field(false, "Object");
                self.expr(&n.object);
                self.field(false, "Body");
                self.stmt(&n.body);
            }
            Statement::BlockStatement(_) => unreachable!(),
        }
        self.close();
    }
}

pub fn node_type(n: Node<'_>) -> &'static str {
    match n {
        Node::Program(_) => "Program",
        Node::ArrayLiteral(_) => "ArrayLiteral",
        Node::ArrayPattern(_) => "ArrayPattern",
        Node::ArrowFunctionLiteral(_) => "ArrowFunctionLiteral",
        Node::AssignExpression(_) => "AssignExpression",
        Node::AwaitExpression(_) => "AwaitExpression",
        Node::BadExpression(_) => "BadExpression",
        Node::BinaryExpression(_) => "BinaryExpression",
        Node::Binding(_) => "Binding",
        Node::BooleanLiteral(_) => "BooleanLiteral",
        Node::BracketExpression(_) => "BracketExpression",
        Node::CallExpression(_) => "CallExpression",
        Node::ClassLiteral(_) => "ClassLiteral",
        Node::ConditionalExpression(_) => "ConditionalExpression",
        Node::DotExpression(_) => "DotExpression",
        Node::ExpressionBody(_) => "ExpressionBody",
        Node::FunctionLiteral(_) => "FunctionLiteral",
        Node::Identifier(_) => "Identifier",
        Node::MetaProperty(_) => "MetaProperty",
        Node::NewExpression(_) => "NewExpression",
        Node::NullLiteral(_) => "NullLiteral",
        Node::NumberLiteral(_) => "NumberLiteral",
        Node::ObjectLiteral(_) => "ObjectLiteral",
        Node::ObjectPattern(_) => "ObjectPattern",
        Node::Optional(_) => "Optional",
        Node::OptionalChain(_) => "OptionalChain",
        Node::ParameterList(_) => "ParameterList",
        Node::PrivateDotExpression(_) => "PrivateDotExpression",
        Node::PrivateIdentifier(_) => "PrivateIdentifier",
        Node::PropertyKeyed(_) => "PropertyKeyed",
        Node::PropertyShort(_) => "PropertyShort",
        Node::RegExpLiteral(_) => "RegExpLiteral",
        Node::SequenceExpression(_) => "SequenceExpression",
        Node::SpreadElement(_) => "SpreadElement",
        Node::StringLiteral(_) => "StringLiteral",
        Node::SuperExpression(_) => "SuperExpression",
        Node::TemplateElement(_) => "TemplateElement",
        Node::TemplateLiteral(_) => "TemplateLiteral",
        Node::ThisExpression(_) => "ThisExpression",
        Node::UnaryExpression(_) => "UnaryExpression",
        Node::YieldExpression(_) => "YieldExpression",
        Node::BadStatement(_) => "BadStatement",
        Node::BlockStatement(_) => "BlockStatement",
        Node::BranchStatement(_) => "BranchStatement",
        Node::CaseStatement(_) => "CaseStatement",
        Node::CatchStatement(_) => "CatchStatement",
        Node::ClassDeclaration(_) => "ClassDeclaration",
        Node::DebuggerStatement(_) => "DebuggerStatement",
        Node::DoWhileStatement(_) => "DoWhileStatement",
        Node::EmptyStatement(_) => "EmptyStatement",
        Node::ExpressionStatement(_) => "ExpressionStatement",
        Node::ForInStatement(_) => "ForInStatement",
        Node::ForOfStatement(_) => "ForOfStatement",
        Node::ForStatement(_) => "ForStatement",
        Node::FunctionDeclaration(_) => "FunctionDeclaration",
        Node::IfStatement(_) => "IfStatement",
        Node::LabelledStatement(_) => "LabelledStatement",
        Node::LexicalDeclaration(_) => "LexicalDeclaration",
        Node::ReturnStatement(_) => "ReturnStatement",
        Node::SwitchStatement(_) => "SwitchStatement",
        Node::ThrowStatement(_) => "ThrowStatement",
        Node::TryStatement(_) => "TryStatement",
        Node::VariableStatement(_) => "VariableStatement",
        Node::WhileStatement(_) => "WhileStatement",
        Node::WithStatement(_) => "WithStatement",
        Node::VariableDeclaration(_) => "VariableDeclaration",
        Node::FieldDefinition(_) => "FieldDefinition",
        Node::MethodDefinition(_) => "MethodDefinition",
        Node::ClassStaticBlock(_) => "ClassStaticBlock",
        Node::ForLoopInitializerExpression(_) => "ForLoopInitializerExpression",
        Node::ForLoopInitializerVarDeclList(_) => "ForLoopInitializerVarDeclList",
        Node::ForLoopInitializerLexicalDecl(_) => "ForLoopInitializerLexicalDecl",
        Node::ForIntoVar(_) => "ForIntoVar",
        Node::ForDeclaration(_) => "ForDeclaration",
        Node::ForIntoExpression(_) => "ForIntoExpression",
        Node::File(_) => "*file.File",
        Node::BigInt(_) => "*big.Int",
    }
}
