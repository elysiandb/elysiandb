use super::node::*;
use crate::deps::big;
use crate::file::File;

#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Program(&'a Program),
    ArrayLiteral(&'a ArrayLiteral),
    ArrayPattern(&'a ArrayPattern),
    ArrowFunctionLiteral(&'a ArrowFunctionLiteral),
    AssignExpression(&'a AssignExpression),
    AwaitExpression(&'a AwaitExpression),
    BadExpression(&'a BadExpression),
    BinaryExpression(&'a BinaryExpression),
    Binding(&'a Binding),
    BooleanLiteral(&'a BooleanLiteral),
    BracketExpression(&'a BracketExpression),
    CallExpression(&'a CallExpression),
    ClassLiteral(&'a ClassLiteral),
    ConditionalExpression(&'a ConditionalExpression),
    DotExpression(&'a DotExpression),
    ExpressionBody(&'a ExpressionBody),
    FunctionLiteral(&'a FunctionLiteral),
    Identifier(&'a Identifier),
    MetaProperty(&'a MetaProperty),
    NewExpression(&'a NewExpression),
    NullLiteral(&'a NullLiteral),
    NumberLiteral(&'a NumberLiteral),
    ObjectLiteral(&'a ObjectLiteral),
    ObjectPattern(&'a ObjectPattern),
    Optional(&'a Optional),
    OptionalChain(&'a OptionalChain),
    ParameterList(&'a ParameterList),
    PrivateDotExpression(&'a PrivateDotExpression),
    PrivateIdentifier(&'a PrivateIdentifier),
    PropertyKeyed(&'a PropertyKeyed),
    PropertyShort(&'a PropertyShort),
    RegExpLiteral(&'a RegExpLiteral),
    SequenceExpression(&'a SequenceExpression),
    SpreadElement(&'a SpreadElement),
    StringLiteral(&'a StringLiteral),
    SuperExpression(&'a SuperExpression),
    TemplateElement(&'a TemplateElement),
    TemplateLiteral(&'a TemplateLiteral),
    ThisExpression(&'a ThisExpression),
    UnaryExpression(&'a UnaryExpression),
    YieldExpression(&'a YieldExpression),
    BadStatement(&'a BadStatement),
    BlockStatement(&'a BlockStatement),
    BranchStatement(&'a BranchStatement),
    CaseStatement(&'a CaseStatement),
    CatchStatement(&'a CatchStatement),
    ClassDeclaration(&'a ClassDeclaration),
    DebuggerStatement(&'a DebuggerStatement),
    DoWhileStatement(&'a DoWhileStatement),
    EmptyStatement(&'a EmptyStatement),
    ExpressionStatement(&'a ExpressionStatement),
    ForInStatement(&'a ForInStatement),
    ForOfStatement(&'a ForOfStatement),
    ForStatement(&'a ForStatement),
    FunctionDeclaration(&'a FunctionDeclaration),
    IfStatement(&'a IfStatement),
    LabelledStatement(&'a LabelledStatement),
    LexicalDeclaration(&'a LexicalDeclaration),
    ReturnStatement(&'a ReturnStatement),
    SwitchStatement(&'a SwitchStatement),
    ThrowStatement(&'a ThrowStatement),
    TryStatement(&'a TryStatement),
    VariableStatement(&'a VariableStatement),
    WhileStatement(&'a WhileStatement),
    WithStatement(&'a WithStatement),
    VariableDeclaration(&'a VariableDeclaration),
    FieldDefinition(&'a FieldDefinition),
    MethodDefinition(&'a MethodDefinition),
    ClassStaticBlock(&'a ClassStaticBlock),
    ForLoopInitializerExpression(&'a ForLoopInitializerExpression),
    ForLoopInitializerVarDeclList(&'a ForLoopInitializerVarDeclList),
    ForLoopInitializerLexicalDecl(&'a ForLoopInitializerLexicalDecl),
    ForIntoVar(&'a ForIntoVar),
    ForDeclaration(&'a ForDeclaration),
    ForIntoExpression(&'a ForIntoExpression),
    File(&'a File),
    BigInt(&'a big::Int),
}

impl<'a> From<&'a Expression> for Node<'a> {
    fn from(e: &'a Expression) -> Node<'a> {
        match e {
            Expression::ArrayLiteral(n) => Node::ArrayLiteral(n),
            Expression::ArrayPattern(n) => Node::ArrayPattern(n),
            Expression::ArrowFunctionLiteral(n) => Node::ArrowFunctionLiteral(n),
            Expression::AssignExpression(n) => Node::AssignExpression(n),
            Expression::AwaitExpression(n) => Node::AwaitExpression(n),
            Expression::BadExpression(n) => Node::BadExpression(n),
            Expression::BinaryExpression(n) => Node::BinaryExpression(n),
            Expression::BooleanLiteral(n) => Node::BooleanLiteral(n),
            Expression::BracketExpression(n) => Node::BracketExpression(n),
            Expression::CallExpression(n) => Node::CallExpression(n),
            Expression::ClassLiteral(n) => Node::ClassLiteral(n),
            Expression::ConditionalExpression(n) => Node::ConditionalExpression(n),
            Expression::DotExpression(n) => Node::DotExpression(n),
            Expression::FunctionLiteral(n) => Node::FunctionLiteral(n),
            Expression::Identifier(n) => Node::Identifier(n),
            Expression::MetaProperty(n) => Node::MetaProperty(n),
            Expression::NewExpression(n) => Node::NewExpression(n),
            Expression::NullLiteral(n) => Node::NullLiteral(n),
            Expression::NumberLiteral(n) => Node::NumberLiteral(n),
            Expression::ObjectLiteral(n) => Node::ObjectLiteral(n),
            Expression::ObjectPattern(n) => Node::ObjectPattern(n),
            Expression::Optional(n) => Node::Optional(n),
            Expression::OptionalChain(n) => Node::OptionalChain(n),
            Expression::PrivateDotExpression(n) => Node::PrivateDotExpression(n),
            Expression::PrivateIdentifier(n) => Node::PrivateIdentifier(n),
            Expression::RegExpLiteral(n) => Node::RegExpLiteral(n),
            Expression::SequenceExpression(n) => Node::SequenceExpression(n),
            Expression::SpreadElement(n) => Node::SpreadElement(n),
            Expression::StringLiteral(n) => Node::StringLiteral(n),
            Expression::SuperExpression(n) => Node::SuperExpression(n),
            Expression::TemplateLiteral(n) => Node::TemplateLiteral(n),
            Expression::ThisExpression(n) => Node::ThisExpression(n),
            Expression::UnaryExpression(n) => Node::UnaryExpression(n),
            Expression::YieldExpression(n) => Node::YieldExpression(n),
        }
    }
}

impl<'a> From<&'a Statement> for Node<'a> {
    fn from(s: &'a Statement) -> Node<'a> {
        match s {
            Statement::BadStatement(n) => Node::BadStatement(n),
            Statement::BlockStatement(n) => Node::BlockStatement(n),
            Statement::BranchStatement(n) => Node::BranchStatement(n),
            Statement::ClassDeclaration(n) => Node::ClassDeclaration(n),
            Statement::DebuggerStatement(n) => Node::DebuggerStatement(n),
            Statement::DoWhileStatement(n) => Node::DoWhileStatement(n),
            Statement::EmptyStatement(n) => Node::EmptyStatement(n),
            Statement::ExpressionStatement(n) => Node::ExpressionStatement(n),
            Statement::ForInStatement(n) => Node::ForInStatement(n),
            Statement::ForOfStatement(n) => Node::ForOfStatement(n),
            Statement::ForStatement(n) => Node::ForStatement(n),
            Statement::FunctionDeclaration(n) => Node::FunctionDeclaration(n),
            Statement::IfStatement(n) => Node::IfStatement(n),
            Statement::LabelledStatement(n) => Node::LabelledStatement(n),
            Statement::LexicalDeclaration(n) => Node::LexicalDeclaration(n),
            Statement::ReturnStatement(n) => Node::ReturnStatement(n),
            Statement::SwitchStatement(n) => Node::SwitchStatement(n),
            Statement::ThrowStatement(n) => Node::ThrowStatement(n),
            Statement::TryStatement(n) => Node::TryStatement(n),
            Statement::VariableStatement(n) => Node::VariableStatement(n),
            Statement::WhileStatement(n) => Node::WhileStatement(n),
            Statement::WithStatement(n) => Node::WithStatement(n),
        }
    }
}

impl<'a> From<&'a BindingTarget> for Node<'a> {
    fn from(t: &'a BindingTarget) -> Node<'a> {
        match t {
            BindingTarget::Identifier(n) => Node::Identifier(n),
            BindingTarget::ArrayPattern(n) => Node::ArrayPattern(n),
            BindingTarget::ObjectPattern(n) => Node::ObjectPattern(n),
            BindingTarget::BadExpression(n) => Node::BadExpression(n),
        }
    }
}

impl<'a> From<&'a Property> for Node<'a> {
    fn from(p: &'a Property) -> Node<'a> {
        match p {
            Property::PropertyShort(n) => Node::PropertyShort(n),
            Property::PropertyKeyed(n) => Node::PropertyKeyed(n),
            Property::SpreadElement(n) => Node::SpreadElement(n),
        }
    }
}

impl<'a> From<&'a ConciseBody> for Node<'a> {
    fn from(b: &'a ConciseBody) -> Node<'a> {
        match b {
            ConciseBody::BlockStatement(n) => Node::BlockStatement(n),
            ConciseBody::ExpressionBody(n) => Node::ExpressionBody(n),
        }
    }
}

impl<'a> From<&'a ClassElement> for Node<'a> {
    fn from(e: &'a ClassElement) -> Node<'a> {
        match e {
            ClassElement::FieldDefinition(n) => Node::FieldDefinition(n),
            ClassElement::MethodDefinition(n) => Node::MethodDefinition(n),
            ClassElement::ClassStaticBlock(n) => Node::ClassStaticBlock(n),
        }
    }
}

impl<'a> From<&'a ForLoopInitializer> for Node<'a> {
    fn from(i: &'a ForLoopInitializer) -> Node<'a> {
        match i {
            ForLoopInitializer::ForLoopInitializerExpression(n) => {
                Node::ForLoopInitializerExpression(n)
            }
            ForLoopInitializer::ForLoopInitializerVarDeclList(n) => {
                Node::ForLoopInitializerVarDeclList(n)
            }
            ForLoopInitializer::ForLoopInitializerLexicalDecl(n) => {
                Node::ForLoopInitializerLexicalDecl(n)
            }
        }
    }
}

impl<'a> From<&'a ForInto> for Node<'a> {
    fn from(i: &'a ForInto) -> Node<'a> {
        match i {
            ForInto::ForIntoVar(n) => Node::ForIntoVar(n),
            ForInto::ForDeclaration(n) => Node::ForDeclaration(n),
            ForInto::ForIntoExpression(n) => Node::ForIntoExpression(n),
        }
    }
}

pub fn walk_nodes<'a, F>(node: Node<'a>, f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    if f(node) {
        walk_children(node, f);
    }
}

fn opt<'a, T, F>(v: &'a Option<T>, f: &mut F)
where
    &'a T: Into<Node<'a>>,
    F: FnMut(Node<'a>) -> bool,
{
    if let Some(v) = v {
        walk_nodes(v.into(), f);
    }
}

fn each<'a, T, F>(v: &'a [T], f: &mut F)
where
    &'a T: Into<Node<'a>>,
    F: FnMut(Node<'a>) -> bool,
{
    for item in v {
        walk_nodes(item.into(), f);
    }
}

fn bindings<'a, F>(list: &'a [std::sync::Arc<Binding>], f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    for b in list {
        walk_nodes(Node::Binding(b), f);
    }
}

fn declarations<'a, F>(list: &'a [VariableDeclaration], f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    for d in list {
        walk_nodes(Node::VariableDeclaration(d), f);
    }
}

fn expr<'a, F>(e: &'a Expression, f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    walk_nodes(e.into(), f);
}

fn stmt<'a, F>(s: &'a Statement, f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    walk_nodes(s.into(), f);
}

fn identifier<'a, F>(i: &'a Option<Box<Identifier>>, f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    if let Some(i) = i {
        walk_nodes(Node::Identifier(i), f);
    }
}

pub fn walk_children<'a, F>(node: Node<'a>, f: &mut F)
where
    F: FnMut(Node<'a>) -> bool,
{
    match node {
        Node::Program(n) => {
            each(&n.body, f);
            declarations(&n.declaration_list, f);
            if let Some(file) = &n.file {
                walk_nodes(Node::File(file), f);
            }
        }
        Node::ArrayLiteral(n) => {
            for v in n.value.iter().flatten() {
                expr(v, f);
            }
        }
        Node::ArrayPattern(n) => {
            for v in n.elements.iter().flatten() {
                expr(v, f);
            }
            opt(&n.rest, f);
        }
        Node::ArrowFunctionLiteral(n) => {
            walk_nodes(Node::ParameterList(&n.parameter_list), f);
            walk_nodes((&n.body).into(), f);
            declarations(&n.declaration_list, f);
        }
        Node::AssignExpression(n) => {
            expr(&n.left, f);
            expr(&n.right, f);
        }
        Node::AwaitExpression(n) => expr(&n.argument, f),
        Node::BinaryExpression(n) => {
            expr(&n.left, f);
            expr(&n.right, f);
        }
        Node::Binding(n) => {
            walk_nodes((&n.target).into(), f);
            opt(&n.initializer, f);
        }
        Node::BracketExpression(n) => {
            expr(&n.left, f);
            expr(&n.member, f);
        }
        Node::CallExpression(n) => {
            expr(&n.callee, f);
            each(&n.argument_list, f);
        }
        Node::ClassLiteral(n) => {
            identifier(&n.name, f);
            opt(&n.super_class, f);
            each(&n.body, f);
        }
        Node::ConditionalExpression(n) => {
            expr(&n.test, f);
            expr(&n.consequent, f);
            expr(&n.alternate, f);
        }
        Node::DotExpression(n) => expr(&n.left, f),
        Node::ExpressionBody(n) => expr(&n.expression, f),
        Node::FunctionLiteral(n) => {
            identifier(&n.name, f);
            walk_nodes(Node::ParameterList(&n.parameter_list), f);
            walk_nodes(Node::BlockStatement(&n.body), f);
            declarations(&n.declaration_list, f);
        }
        Node::MetaProperty(n) => {
            walk_nodes(Node::Identifier(&n.meta), f);
            walk_nodes(Node::Identifier(&n.property), f);
        }
        Node::NewExpression(n) => {
            expr(&n.callee, f);
            each(&n.argument_list, f);
        }
        Node::NumberLiteral(n) => {
            if let NumberValue::BigInt(b) = &n.value {
                walk_nodes(Node::BigInt(b), f);
            }
        }
        Node::ObjectLiteral(n) => each(&n.value, f),
        Node::ObjectPattern(n) => {
            each(&n.properties, f);
            opt(&n.rest, f);
        }
        Node::Optional(n) => expr(&n.expression, f),
        Node::OptionalChain(n) => expr(&n.expression, f),
        Node::ParameterList(n) => {
            bindings(&n.list, f);
            opt(&n.rest, f);
        }
        Node::PrivateDotExpression(n) => expr(&n.left, f),
        Node::PropertyKeyed(n) => {
            expr(&n.key, f);
            expr(&n.value, f);
        }
        Node::PropertyShort(n) => opt(&n.initializer, f),
        Node::SequenceExpression(n) => each(&n.sequence, f),
        Node::SpreadElement(n) => expr(&n.expression, f),
        Node::TemplateLiteral(n) => {
            opt(&n.tag, f);
            for e in &n.elements {
                walk_nodes(Node::TemplateElement(e), f);
            }
            each(&n.expressions, f);
        }
        Node::UnaryExpression(n) => expr(&n.operand, f),
        Node::YieldExpression(n) => opt(&n.argument, f),
        Node::BlockStatement(n) => each(&n.list, f),
        Node::BranchStatement(n) => identifier(&n.label, f),
        Node::CaseStatement(n) => {
            opt(&n.test, f);
            each(&n.consequent, f);
        }
        Node::CatchStatement(n) => {
            opt(&n.parameter, f);
            walk_nodes(Node::BlockStatement(&n.body), f);
        }
        Node::ClassDeclaration(n) => walk_nodes(Node::ClassLiteral(&n.class), f),
        Node::DoWhileStatement(n) => {
            expr(&n.test, f);
            stmt(&n.body, f);
        }
        Node::ExpressionStatement(n) => expr(&n.expression, f),
        Node::ForInStatement(n) => {
            walk_nodes((&n.into).into(), f);
            expr(&n.source, f);
            stmt(&n.body, f);
        }
        Node::ForOfStatement(n) => {
            walk_nodes((&n.into).into(), f);
            expr(&n.source, f);
            stmt(&n.body, f);
        }
        Node::ForStatement(n) => {
            opt(&n.initializer, f);
            opt(&n.update, f);
            opt(&n.test, f);
            stmt(&n.body, f);
        }
        Node::FunctionDeclaration(n) => walk_nodes(Node::FunctionLiteral(&n.function), f),
        Node::IfStatement(n) => {
            expr(&n.test, f);
            stmt(&n.consequent, f);
            opt(&n.alternate, f);
        }
        Node::LabelledStatement(n) => {
            walk_nodes(Node::Identifier(&n.label), f);
            stmt(&n.statement, f);
        }
        Node::LexicalDeclaration(n) => bindings(&n.list, f),
        Node::ReturnStatement(n) => opt(&n.argument, f),
        Node::SwitchStatement(n) => {
            expr(&n.discriminant, f);
            for c in &n.body {
                walk_nodes(Node::CaseStatement(c), f);
            }
        }
        Node::ThrowStatement(n) => expr(&n.argument, f),
        Node::TryStatement(n) => {
            walk_nodes(Node::BlockStatement(&n.body), f);
            if let Some(c) = &n.catch {
                walk_nodes(Node::CatchStatement(c), f);
            }
            if let Some(b) = &n.finally {
                walk_nodes(Node::BlockStatement(b), f);
            }
        }
        Node::VariableStatement(n) => bindings(&n.list, f),
        Node::WhileStatement(n) => {
            expr(&n.test, f);
            stmt(&n.body, f);
        }
        Node::WithStatement(n) => {
            expr(&n.object, f);
            stmt(&n.body, f);
        }
        Node::VariableDeclaration(n) => bindings(&n.list, f),
        Node::FieldDefinition(n) => {
            expr(&n.key, f);
            opt(&n.initializer, f);
        }
        Node::MethodDefinition(n) => {
            expr(&n.key, f);
            walk_nodes(Node::FunctionLiteral(&n.body), f);
        }
        Node::ClassStaticBlock(n) => {
            walk_nodes(Node::BlockStatement(&n.block), f);
            declarations(&n.declaration_list, f);
        }
        Node::ForLoopInitializerExpression(n) => expr(&n.expression, f),
        Node::ForLoopInitializerVarDeclList(n) => bindings(&n.list, f),
        Node::ForLoopInitializerLexicalDecl(n) => bindings(&n.lexical_declaration.list, f),
        Node::ForIntoVar(n) => walk_nodes(Node::Binding(&n.binding), f),
        Node::ForDeclaration(n) => walk_nodes((&n.target).into(), f),
        Node::ForIntoExpression(n) => expr(&n.expression, f),
        Node::BadExpression(_)
        | Node::BooleanLiteral(_)
        | Node::Identifier(_)
        | Node::NullLiteral(_)
        | Node::PrivateIdentifier(_)
        | Node::RegExpLiteral(_)
        | Node::StringLiteral(_)
        | Node::SuperExpression(_)
        | Node::TemplateElement(_)
        | Node::ThisExpression(_)
        | Node::BadStatement(_)
        | Node::DebuggerStatement(_)
        | Node::EmptyStatement(_)
        | Node::File(_)
        | Node::BigInt(_) => {}
    }
}
