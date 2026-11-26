use crate::deps::big;
use crate::file::{File, Idx};
use crate::token::Token;
use crate::unistring;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    Value,
    Get,
    Set,
    Method,
}

impl PropertyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PropertyKind::Value => "value",
            PropertyKind::Get => "get",
            PropertyKind::Set => "set",
            PropertyKind::Method => "method",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NumberValue {
    Int64(i64),
    Float64(f64),
    BigInt(Box<big::Int>),
    Int(i64),
}

impl fmt::Display for NumberValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumberValue::Int64(v) | NumberValue::Int(v) => write!(f, "{v}"),
            NumberValue::Float64(v) => {
                f.write_str(&gocompat::strconv::format_float(*v, b'g', -1, 64))
            }
            NumberValue::BigInt(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expression {
    ArrayLiteral(Box<ArrayLiteral>),
    ArrayPattern(Box<ArrayPattern>),
    ArrowFunctionLiteral(Box<ArrowFunctionLiteral>),
    AssignExpression(Box<AssignExpression>),
    AwaitExpression(Box<AwaitExpression>),
    BadExpression(Box<BadExpression>),
    BinaryExpression(Box<BinaryExpression>),
    BooleanLiteral(Box<BooleanLiteral>),
    BracketExpression(Box<BracketExpression>),
    CallExpression(Box<CallExpression>),
    ClassLiteral(Box<ClassLiteral>),
    ConditionalExpression(Box<ConditionalExpression>),
    DotExpression(Box<DotExpression>),
    FunctionLiteral(Box<FunctionLiteral>),
    Identifier(Box<Identifier>),
    MetaProperty(Box<MetaProperty>),
    NewExpression(Box<NewExpression>),
    NullLiteral(Box<NullLiteral>),
    NumberLiteral(Box<NumberLiteral>),
    ObjectLiteral(Box<ObjectLiteral>),
    ObjectPattern(Box<ObjectPattern>),
    Optional(Box<Optional>),
    OptionalChain(Box<OptionalChain>),
    PrivateDotExpression(Box<PrivateDotExpression>),
    PrivateIdentifier(Box<PrivateIdentifier>),
    RegExpLiteral(Box<RegExpLiteral>),
    SequenceExpression(Box<SequenceExpression>),
    SpreadElement(Box<SpreadElement>),
    StringLiteral(Box<StringLiteral>),
    SuperExpression(Box<SuperExpression>),
    TemplateLiteral(Box<TemplateLiteral>),
    ThisExpression(Box<ThisExpression>),
    UnaryExpression(Box<UnaryExpression>),
    YieldExpression(Box<YieldExpression>),
}

#[derive(Clone, Debug)]
pub enum BindingTarget {
    Identifier(Box<Identifier>),
    ArrayPattern(Box<ArrayPattern>),
    ObjectPattern(Box<ObjectPattern>),
    BadExpression(Box<BadExpression>),
}

#[derive(Clone, Debug)]
pub enum Property {
    PropertyShort(Box<PropertyShort>),
    PropertyKeyed(Box<PropertyKeyed>),
    SpreadElement(Box<SpreadElement>),
}

#[derive(Clone, Debug)]
pub enum ConciseBody {
    BlockStatement(Box<BlockStatement>),
    ExpressionBody(Box<ExpressionBody>),
}

#[derive(Clone, Debug)]
pub enum Statement {
    BadStatement(Box<BadStatement>),
    BlockStatement(Box<BlockStatement>),
    BranchStatement(Box<BranchStatement>),
    ClassDeclaration(Box<ClassDeclaration>),
    DebuggerStatement(Box<DebuggerStatement>),
    DoWhileStatement(Box<DoWhileStatement>),
    EmptyStatement(Box<EmptyStatement>),
    ExpressionStatement(Box<ExpressionStatement>),
    ForInStatement(Box<ForInStatement>),
    ForOfStatement(Box<ForOfStatement>),
    ForStatement(Box<ForStatement>),
    FunctionDeclaration(Box<FunctionDeclaration>),
    IfStatement(Box<IfStatement>),
    LabelledStatement(Box<LabelledStatement>),
    LexicalDeclaration(Box<LexicalDeclaration>),
    ReturnStatement(Box<ReturnStatement>),
    SwitchStatement(Box<SwitchStatement>),
    ThrowStatement(Box<ThrowStatement>),
    TryStatement(Box<TryStatement>),
    VariableStatement(Box<VariableStatement>),
    WhileStatement(Box<WhileStatement>),
    WithStatement(Box<WithStatement>),
}

#[derive(Clone, Debug)]
pub enum ClassElement {
    FieldDefinition(Box<FieldDefinition>),
    MethodDefinition(Box<MethodDefinition>),
    ClassStaticBlock(Box<ClassStaticBlock>),
}

#[derive(Clone, Debug)]
pub enum ForLoopInitializer {
    ForLoopInitializerExpression(Box<ForLoopInitializerExpression>),
    ForLoopInitializerVarDeclList(Box<ForLoopInitializerVarDeclList>),
    ForLoopInitializerLexicalDecl(Box<ForLoopInitializerLexicalDecl>),
}

#[derive(Clone, Debug)]
pub enum ForInto {
    ForIntoVar(Box<ForIntoVar>),
    ForDeclaration(Box<ForDeclaration>),
    ForIntoExpression(Box<ForIntoExpression>),
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub target: BindingTarget,
    pub initializer: Option<Expression>,
}

#[derive(Clone, Debug)]
pub struct YieldExpression {
    pub yield_: Idx,
    pub argument: Option<Expression>,
    pub delegate: bool,
}

#[derive(Clone, Debug)]
pub struct AwaitExpression {
    pub await_: Idx,
    pub argument: Expression,
}

#[derive(Clone, Debug)]
pub struct ArrayLiteral {
    pub left_bracket: Idx,
    pub right_bracket: Idx,
    pub value: Vec<Option<Expression>>,
}

#[derive(Clone, Debug)]
pub struct ArrayPattern {
    pub left_bracket: Idx,
    pub right_bracket: Idx,
    pub elements: Vec<Option<Expression>>,
    pub rest: Option<Expression>,
}

#[derive(Clone, Debug)]
pub struct AssignExpression {
    pub operator: Token,
    pub left: Expression,
    pub right: Expression,
}

#[derive(Clone, Debug)]
pub struct BadExpression {
    pub from: Idx,
    pub to: Idx,
}

#[derive(Clone, Debug)]
pub struct BinaryExpression {
    pub operator: Token,
    pub left: Expression,
    pub right: Expression,
    pub comparison: bool,
}

#[derive(Clone, Debug)]
pub struct BooleanLiteral {
    pub idx: Idx,
    pub literal: Vec<u8>,
    pub value: bool,
}

#[derive(Clone, Debug)]
pub struct BracketExpression {
    pub left: Expression,
    pub member: Expression,
    pub left_bracket: Idx,
    pub right_bracket: Idx,
}

#[derive(Clone, Debug)]
pub struct CallExpression {
    pub callee: Expression,
    pub left_parenthesis: Idx,
    pub argument_list: Vec<Expression>,
    pub right_parenthesis: Idx,
}

#[derive(Clone, Debug)]
pub struct ConditionalExpression {
    pub test: Expression,
    pub consequent: Expression,
    pub alternate: Expression,
}

#[derive(Clone, Debug)]
pub struct DotExpression {
    pub left: Expression,
    pub identifier: Identifier,
}

#[derive(Clone, Debug)]
pub struct PrivateDotExpression {
    pub left: Expression,
    pub identifier: PrivateIdentifier,
}

#[derive(Clone, Debug)]
pub struct OptionalChain {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct Optional {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct FunctionLiteral {
    pub function: Idx,
    pub name: Option<Box<Identifier>>,
    pub parameter_list: Box<ParameterList>,
    pub body: Box<BlockStatement>,
    pub source: Vec<u8>,
    pub declaration_list: Vec<VariableDeclaration>,
    pub async_: bool,
    pub generator: bool,
}

#[derive(Clone, Debug)]
pub struct ClassLiteral {
    pub class: Idx,
    pub right_brace: Idx,
    pub name: Option<Box<Identifier>>,
    pub super_class: Option<Expression>,
    pub body: Vec<ClassElement>,
    pub source: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ExpressionBody {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct ArrowFunctionLiteral {
    pub start: Idx,
    pub parameter_list: Box<ParameterList>,
    pub body: ConciseBody,
    pub source: Vec<u8>,
    pub declaration_list: Vec<VariableDeclaration>,
    pub async_: bool,
}

#[derive(Clone, Debug)]
pub struct Identifier {
    pub name: unistring::String,
    pub idx: Idx,
}

#[derive(Clone, Debug)]
pub struct PrivateIdentifier {
    pub identifier: Identifier,
}

#[derive(Clone, Debug)]
pub struct NewExpression {
    pub new: Idx,
    pub callee: Expression,
    pub left_parenthesis: Idx,
    pub argument_list: Vec<Expression>,
    pub right_parenthesis: Idx,
}

#[derive(Clone, Debug)]
pub struct NullLiteral {
    pub idx: Idx,
    pub literal: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct NumberLiteral {
    pub idx: Idx,
    pub literal: Vec<u8>,
    pub value: NumberValue,
}

#[derive(Clone, Debug)]
pub struct ObjectLiteral {
    pub left_brace: Idx,
    pub right_brace: Idx,
    pub value: Vec<Property>,
}

#[derive(Clone, Debug)]
pub struct ObjectPattern {
    pub left_brace: Idx,
    pub right_brace: Idx,
    pub properties: Vec<Property>,
    pub rest: Option<Expression>,
}

#[derive(Clone, Debug, Default)]
pub struct ParameterList {
    pub opening: Idx,
    pub list: Vec<Arc<Binding>>,
    pub rest: Option<Expression>,
    pub closing: Idx,
}

#[derive(Clone, Debug)]
pub struct PropertyShort {
    pub name: Identifier,
    pub initializer: Option<Expression>,
}

#[derive(Clone, Debug)]
pub struct PropertyKeyed {
    pub key: Expression,
    pub kind: PropertyKind,
    pub value: Expression,
    pub computed: bool,
}

#[derive(Clone, Debug)]
pub struct SpreadElement {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct RegExpLiteral {
    pub idx: Idx,
    pub literal: Vec<u8>,
    pub pattern: Vec<u8>,
    pub flags: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct SequenceExpression {
    pub sequence: Vec<Expression>,
}

#[derive(Clone, Debug)]
pub struct StringLiteral {
    pub idx: Idx,
    pub literal: Vec<u8>,
    pub value: unistring::String,
}

#[derive(Clone, Debug)]
pub struct TemplateElement {
    pub idx: Idx,
    pub literal: Vec<u8>,
    pub parsed: unistring::String,
    pub valid: bool,
}

#[derive(Clone, Debug)]
pub struct TemplateLiteral {
    pub open_quote: Idx,
    pub close_quote: Idx,
    pub tag: Option<Expression>,
    pub elements: Vec<TemplateElement>,
    pub expressions: Vec<Expression>,
}

#[derive(Clone, Debug)]
pub struct ThisExpression {
    pub idx: Idx,
}

#[derive(Clone, Debug)]
pub struct SuperExpression {
    pub idx: Idx,
}

#[derive(Clone, Debug)]
pub struct UnaryExpression {
    pub operator: Token,
    pub idx: Idx,
    pub operand: Expression,
    pub postfix: bool,
}

#[derive(Clone, Debug)]
pub struct MetaProperty {
    pub meta: Box<Identifier>,
    pub property: Box<Identifier>,
    pub idx: Idx,
}

#[derive(Clone, Debug)]
pub struct BadStatement {
    pub from: Idx,
    pub to: Idx,
}

#[derive(Clone, Debug)]
pub struct BlockStatement {
    pub left_brace: Idx,
    pub list: Vec<Statement>,
    pub right_brace: Idx,
}

#[derive(Clone, Debug)]
pub struct BranchStatement {
    pub idx: Idx,
    pub token: Token,
    pub label: Option<Box<Identifier>>,
}

#[derive(Clone, Debug)]
pub struct CaseStatement {
    pub case: Idx,
    pub test: Option<Expression>,
    pub consequent: Vec<Statement>,
}

#[derive(Clone, Debug)]
pub struct CatchStatement {
    pub catch: Idx,
    pub parameter: Option<BindingTarget>,
    pub body: Box<BlockStatement>,
}

#[derive(Clone, Debug)]
pub struct DebuggerStatement {
    pub debugger: Idx,
}

#[derive(Clone, Debug)]
pub struct DoWhileStatement {
    pub do_: Idx,
    pub test: Expression,
    pub body: Statement,
    pub right_parenthesis: Idx,
}

#[derive(Clone, Debug)]
pub struct EmptyStatement {
    pub semicolon: Idx,
}

#[derive(Clone, Debug)]
pub struct ExpressionStatement {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct ForInStatement {
    pub for_: Idx,
    pub into: ForInto,
    pub source: Expression,
    pub body: Statement,
}

#[derive(Clone, Debug)]
pub struct ForOfStatement {
    pub for_: Idx,
    pub into: ForInto,
    pub source: Expression,
    pub body: Statement,
}

#[derive(Clone, Debug)]
pub struct ForStatement {
    pub for_: Idx,
    pub initializer: Option<ForLoopInitializer>,
    pub update: Option<Expression>,
    pub test: Option<Expression>,
    pub body: Statement,
}

#[derive(Clone, Debug)]
pub struct IfStatement {
    pub if_: Idx,
    pub test: Expression,
    pub consequent: Statement,
    pub alternate: Option<Statement>,
}

#[derive(Clone, Debug)]
pub struct LabelledStatement {
    pub label: Box<Identifier>,
    pub colon: Idx,
    pub statement: Statement,
}

#[derive(Clone, Debug)]
pub struct ReturnStatement {
    pub return_: Idx,
    pub argument: Option<Expression>,
}

#[derive(Clone, Debug)]
pub struct SwitchStatement {
    pub switch: Idx,
    pub discriminant: Expression,
    pub default: i64,
    pub body: Vec<CaseStatement>,
    pub right_brace: Idx,
}

#[derive(Clone, Debug)]
pub struct ThrowStatement {
    pub throw: Idx,
    pub argument: Expression,
}

#[derive(Clone, Debug)]
pub struct TryStatement {
    pub try_: Idx,
    pub body: Box<BlockStatement>,
    pub catch: Option<Box<CatchStatement>>,
    pub finally: Option<Box<BlockStatement>>,
}

#[derive(Clone, Debug)]
pub struct VariableStatement {
    pub var: Idx,
    pub list: Vec<Arc<Binding>>,
}

#[derive(Clone, Debug)]
pub struct LexicalDeclaration {
    pub idx: Idx,
    pub token: Token,
    pub list: Vec<Arc<Binding>>,
}

#[derive(Clone, Debug)]
pub struct WhileStatement {
    pub while_: Idx,
    pub test: Expression,
    pub body: Statement,
}

#[derive(Clone, Debug)]
pub struct WithStatement {
    pub with: Idx,
    pub object: Expression,
    pub body: Statement,
}

#[derive(Clone, Debug)]
pub struct FunctionDeclaration {
    pub function: Box<FunctionLiteral>,
}

#[derive(Clone, Debug)]
pub struct ClassDeclaration {
    pub class: Box<ClassLiteral>,
}

#[derive(Clone, Debug)]
pub struct VariableDeclaration {
    pub var: Idx,
    pub list: Vec<Arc<Binding>>,
}

#[derive(Clone, Debug)]
pub struct FieldDefinition {
    pub idx: Idx,
    pub key: Expression,
    pub initializer: Option<Expression>,
    pub computed: bool,
    pub static_: bool,
}

#[derive(Clone, Debug)]
pub struct MethodDefinition {
    pub idx: Idx,
    pub key: Expression,
    pub kind: PropertyKind,
    pub body: Box<FunctionLiteral>,
    pub computed: bool,
    pub static_: bool,
}

#[derive(Clone, Debug)]
pub struct ClassStaticBlock {
    pub static_: Idx,
    pub block: Box<BlockStatement>,
    pub source: Vec<u8>,
    pub declaration_list: Vec<VariableDeclaration>,
}

#[derive(Clone, Debug)]
pub struct ForLoopInitializerExpression {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct ForLoopInitializerVarDeclList {
    pub var: Idx,
    pub list: Vec<Arc<Binding>>,
}

#[derive(Clone, Debug)]
pub struct ForLoopInitializerLexicalDecl {
    pub lexical_declaration: LexicalDeclaration,
}

#[derive(Clone, Debug)]
pub struct ForIntoVar {
    pub binding: Arc<Binding>,
}

#[derive(Clone, Debug)]
pub struct ForDeclaration {
    pub idx: Idx,
    pub is_const: bool,
    pub target: BindingTarget,
}

#[derive(Clone, Debug)]
pub struct ForIntoExpression {
    pub expression: Expression,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub body: Vec<Statement>,
    pub declaration_list: Vec<VariableDeclaration>,
    pub file: Option<Arc<File>>,
}

fn idx_len(idx: Idx, len: usize) -> Idx {
    idx + len as Idx
}

impl Expression {
    pub fn idx0(&self) -> Idx {
        match self {
            Expression::ArrayLiteral(n) => n.left_bracket,
            Expression::ArrayPattern(n) => n.left_bracket,
            Expression::ArrowFunctionLiteral(n) => n.start,
            Expression::AssignExpression(n) => n.left.idx0(),
            Expression::AwaitExpression(n) => n.await_,
            Expression::BadExpression(n) => n.from,
            Expression::BinaryExpression(n) => n.left.idx0(),
            Expression::BooleanLiteral(n) => n.idx,
            Expression::BracketExpression(n) => n.left.idx0(),
            Expression::CallExpression(n) => n.callee.idx0(),
            Expression::ClassLiteral(n) => n.class,
            Expression::ConditionalExpression(n) => n.test.idx0(),
            Expression::DotExpression(n) => n.left.idx0(),
            Expression::FunctionLiteral(n) => n.idx0(),
            Expression::Identifier(n) => n.idx0(),
            Expression::MetaProperty(n) => n.idx,
            Expression::NewExpression(n) => n.new,
            Expression::NullLiteral(n) => n.idx,
            Expression::NumberLiteral(n) => n.idx,
            Expression::ObjectLiteral(n) => n.left_brace,
            Expression::ObjectPattern(n) => n.left_brace,
            Expression::Optional(n) => n.expression.idx0(),
            Expression::OptionalChain(n) => n.expression.idx0(),
            Expression::PrivateDotExpression(n) => n.left.idx0(),
            Expression::PrivateIdentifier(n) => n.identifier.idx0(),
            Expression::RegExpLiteral(n) => n.idx,
            Expression::SequenceExpression(n) => n.sequence[0].idx0(),
            Expression::SpreadElement(n) => n.expression.idx0(),
            Expression::StringLiteral(n) => n.idx,
            Expression::SuperExpression(n) => n.idx,
            Expression::TemplateLiteral(n) => n.open_quote,
            Expression::ThisExpression(n) => n.idx,
            Expression::UnaryExpression(n) => {
                if n.postfix {
                    n.operand.idx0()
                } else {
                    n.idx
                }
            }
            Expression::YieldExpression(n) => n.yield_,
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            Expression::ArrayLiteral(n) => n.right_bracket + 1,
            Expression::ArrayPattern(n) => n.right_bracket + 1,
            Expression::ArrowFunctionLiteral(n) => n.body.idx1(),
            Expression::AssignExpression(n) => n.right.idx1(),
            Expression::AwaitExpression(n) => n.argument.idx1(),
            Expression::BadExpression(n) => n.to,
            Expression::BinaryExpression(n) => n.right.idx1(),
            Expression::BooleanLiteral(n) => idx_len(n.idx, n.literal.len()),
            Expression::BracketExpression(n) => n.right_bracket + 1,
            Expression::CallExpression(n) => n.right_parenthesis + 1,
            Expression::ClassLiteral(n) => n.right_brace + 1,
            Expression::ConditionalExpression(n) => n.alternate.idx1(),
            Expression::DotExpression(n) => n.identifier.idx1(),
            Expression::FunctionLiteral(n) => n.idx1(),
            Expression::Identifier(n) => n.idx1(),
            Expression::MetaProperty(n) => n.property.idx1(),
            Expression::NewExpression(n) => {
                if !n.argument_list.is_empty() {
                    n.right_parenthesis + 1
                } else {
                    n.callee.idx1()
                }
            }
            Expression::NullLiteral(n) => n.idx + 4,
            Expression::NumberLiteral(n) => idx_len(n.idx, n.literal.len()),
            Expression::ObjectLiteral(n) => n.right_brace + 1,
            Expression::ObjectPattern(n) => n.right_brace + 1,
            Expression::Optional(n) => n.expression.idx1(),
            Expression::OptionalChain(n) => n.expression.idx1(),
            Expression::PrivateDotExpression(n) => n.identifier.idx1(),
            Expression::PrivateIdentifier(n) => n.identifier.idx1(),
            Expression::RegExpLiteral(n) => idx_len(n.idx, n.literal.len()),
            Expression::SequenceExpression(n) => n.sequence[n.sequence.len() - 1].idx1(),
            Expression::SpreadElement(n) => n.expression.idx1(),
            Expression::StringLiteral(n) => idx_len(n.idx, n.literal.len()),
            Expression::SuperExpression(n) => n.idx + 5,
            Expression::TemplateLiteral(n) => n.close_quote + 1,
            Expression::ThisExpression(n) => n.idx + 4,
            Expression::UnaryExpression(n) => {
                if n.postfix {
                    n.operand.idx1() + 2
                } else {
                    n.operand.idx1()
                }
            }
            Expression::YieldExpression(n) => match &n.argument {
                Some(a) => a.idx1(),
                None => n.yield_ + 5,
            },
        }
    }
}

impl Identifier {
    pub fn idx0(&self) -> Idx {
        self.idx
    }

    pub fn idx1(&self) -> Idx {
        idx_len(self.idx, self.name.len())
    }
}

impl PrivateIdentifier {
    pub fn idx0(&self) -> Idx {
        self.identifier.idx0()
    }

    pub fn idx1(&self) -> Idx {
        self.identifier.idx1()
    }
}

impl FunctionLiteral {
    pub fn idx0(&self) -> Idx {
        self.function
    }

    pub fn idx1(&self) -> Idx {
        self.body.idx1()
    }
}

impl ClassLiteral {
    pub fn idx0(&self) -> Idx {
        self.class
    }

    pub fn idx1(&self) -> Idx {
        self.right_brace + 1
    }
}

impl ParameterList {
    pub fn idx0(&self) -> Idx {
        self.opening
    }

    pub fn idx1(&self) -> Idx {
        self.closing + 1
    }
}

impl TemplateElement {
    pub fn idx0(&self) -> Idx {
        self.idx
    }

    pub fn idx1(&self) -> Idx {
        idx_len(self.idx, self.literal.len())
    }
}

impl BindingTarget {
    pub fn idx0(&self) -> Idx {
        match self {
            BindingTarget::Identifier(n) => n.idx0(),
            BindingTarget::ArrayPattern(n) => n.left_bracket,
            BindingTarget::ObjectPattern(n) => n.left_brace,
            BindingTarget::BadExpression(n) => n.from,
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            BindingTarget::Identifier(n) => n.idx1(),
            BindingTarget::ArrayPattern(n) => n.right_bracket + 1,
            BindingTarget::ObjectPattern(n) => n.right_brace + 1,
            BindingTarget::BadExpression(n) => n.to,
        }
    }

    pub fn into_expression(self) -> Expression {
        match self {
            BindingTarget::Identifier(n) => Expression::Identifier(n),
            BindingTarget::ArrayPattern(n) => Expression::ArrayPattern(n),
            BindingTarget::ObjectPattern(n) => Expression::ObjectPattern(n),
            BindingTarget::BadExpression(n) => Expression::BadExpression(n),
        }
    }
}

impl From<BindingTarget> for Expression {
    fn from(t: BindingTarget) -> Expression {
        t.into_expression()
    }
}

impl Binding {
    pub fn idx0(&self) -> Idx {
        self.target.idx0()
    }

    pub fn idx1(&self) -> Idx {
        match &self.initializer {
            Some(i) => i.idx1(),
            None => self.target.idx1(),
        }
    }
}

impl Property {
    pub fn idx0(&self) -> Idx {
        match self {
            Property::PropertyShort(n) => n.name.idx,
            Property::PropertyKeyed(n) => n.key.idx0(),
            Property::SpreadElement(n) => n.expression.idx0(),
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            Property::PropertyShort(n) => match &n.initializer {
                Some(i) => i.idx1(),
                None => n.name.idx1(),
            },
            Property::PropertyKeyed(n) => n.value.idx1(),
            Property::SpreadElement(n) => n.expression.idx1(),
        }
    }
}

impl ConciseBody {
    pub fn idx0(&self) -> Idx {
        match self {
            ConciseBody::BlockStatement(n) => n.left_brace,
            ConciseBody::ExpressionBody(n) => n.expression.idx0(),
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            ConciseBody::BlockStatement(n) => n.idx1(),
            ConciseBody::ExpressionBody(n) => n.expression.idx1(),
        }
    }
}

impl BlockStatement {
    pub fn idx0(&self) -> Idx {
        self.left_brace
    }

    pub fn idx1(&self) -> Idx {
        self.right_brace + 1
    }
}

impl CaseStatement {
    pub fn idx0(&self) -> Idx {
        self.case
    }

    pub fn idx1(&self) -> Idx {
        self.consequent[self.consequent.len() - 1].idx1()
    }
}

impl CatchStatement {
    pub fn idx0(&self) -> Idx {
        self.catch
    }

    pub fn idx1(&self) -> Idx {
        self.body.idx1()
    }
}

impl LexicalDeclaration {
    pub fn idx0(&self) -> Idx {
        self.idx
    }

    pub fn idx1(&self) -> Idx {
        self.list[self.list.len() - 1].idx1()
    }
}

impl Statement {
    pub fn idx0(&self) -> Idx {
        match self {
            Statement::BadStatement(n) => n.from,
            Statement::BlockStatement(n) => n.left_brace,
            Statement::BranchStatement(n) => n.idx,
            Statement::ClassDeclaration(n) => n.class.idx0(),
            Statement::DebuggerStatement(n) => n.debugger,
            Statement::DoWhileStatement(n) => n.do_,
            Statement::EmptyStatement(n) => n.semicolon,
            Statement::ExpressionStatement(n) => n.expression.idx0(),
            Statement::ForInStatement(n) => n.for_,
            Statement::ForOfStatement(n) => n.for_,
            Statement::ForStatement(n) => n.for_,
            Statement::FunctionDeclaration(n) => n.function.idx0(),
            Statement::IfStatement(n) => n.if_,
            Statement::LabelledStatement(n) => n.label.idx0(),
            Statement::LexicalDeclaration(n) => n.idx,
            Statement::ReturnStatement(n) => n.return_,
            Statement::SwitchStatement(n) => n.switch,
            Statement::ThrowStatement(n) => n.throw,
            Statement::TryStatement(n) => n.try_,
            Statement::VariableStatement(n) => n.var,
            Statement::WhileStatement(n) => n.while_,
            Statement::WithStatement(n) => n.with,
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            Statement::BadStatement(n) => n.to,
            Statement::BlockStatement(n) => n.idx1(),
            Statement::BranchStatement(n) => match &n.label {
                None => idx_len(n.idx, n.token.string().len()),
                Some(l) => l.idx1(),
            },
            Statement::ClassDeclaration(n) => n.class.idx1(),
            Statement::DebuggerStatement(n) => n.debugger + 8,
            Statement::DoWhileStatement(n) => n.right_parenthesis + 1,
            Statement::EmptyStatement(n) => n.semicolon + 1,
            Statement::ExpressionStatement(n) => n.expression.idx1(),
            Statement::ForInStatement(n) => n.body.idx1(),
            Statement::ForOfStatement(n) => n.body.idx1(),
            Statement::ForStatement(n) => n.body.idx1(),
            Statement::FunctionDeclaration(n) => n.function.idx1(),
            Statement::IfStatement(n) => match &n.alternate {
                Some(a) => a.idx1(),
                None => n.consequent.idx1(),
            },
            Statement::LabelledStatement(n) => n.statement.idx1(),
            Statement::LexicalDeclaration(n) => n.idx1(),
            Statement::ReturnStatement(n) => match &n.argument {
                Some(a) => a.idx1(),
                None => n.return_ + 6,
            },
            Statement::SwitchStatement(n) => n.right_brace + 1,
            Statement::ThrowStatement(n) => n.argument.idx1(),
            Statement::TryStatement(n) => {
                if let Some(f) = &n.finally {
                    return f.idx1();
                }
                if let Some(c) = &n.catch {
                    return c.idx1();
                }
                n.body.idx1()
            }
            Statement::VariableStatement(n) => n.list[n.list.len() - 1].idx1(),
            Statement::WhileStatement(n) => n.body.idx1(),
            Statement::WithStatement(n) => n.body.idx1(),
        }
    }
}

impl VariableDeclaration {
    pub fn idx0(&self) -> Idx {
        self.var
    }

    pub fn idx1(&self) -> Idx {
        match self.list.last() {
            Some(b) => b.idx1(),
            None => self.var + 3,
        }
    }
}

impl ClassElement {
    pub fn idx0(&self) -> Idx {
        match self {
            ClassElement::FieldDefinition(n) => n.idx,
            ClassElement::MethodDefinition(n) => n.idx,
            ClassElement::ClassStaticBlock(n) => n.static_,
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            ClassElement::FieldDefinition(n) => match &n.initializer {
                Some(i) => i.idx1(),
                None => n.key.idx1(),
            },
            ClassElement::MethodDefinition(n) => n.body.idx1(),
            ClassElement::ClassStaticBlock(n) => n.block.idx1(),
        }
    }
}

impl ForLoopInitializer {
    pub fn idx0(&self) -> Idx {
        match self {
            ForLoopInitializer::ForLoopInitializerExpression(n) => n.expression.idx0(),
            ForLoopInitializer::ForLoopInitializerVarDeclList(n) => n.list[0].idx0(),
            ForLoopInitializer::ForLoopInitializerLexicalDecl(n) => n.lexical_declaration.idx0(),
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            ForLoopInitializer::ForLoopInitializerExpression(n) => n.expression.idx1(),
            ForLoopInitializer::ForLoopInitializerVarDeclList(n) => n.list[n.list.len() - 1].idx1(),
            ForLoopInitializer::ForLoopInitializerLexicalDecl(n) => n.lexical_declaration.idx1(),
        }
    }
}

impl ForInto {
    pub fn idx0(&self) -> Idx {
        match self {
            ForInto::ForIntoVar(n) => n.binding.idx0(),
            ForInto::ForDeclaration(n) => n.idx,
            ForInto::ForIntoExpression(n) => n.expression.idx0(),
        }
    }

    pub fn idx1(&self) -> Idx {
        match self {
            ForInto::ForIntoVar(n) => n.binding.idx1(),
            ForInto::ForDeclaration(n) => n.target.idx1(),
            ForInto::ForIntoExpression(n) => n.expression.idx1(),
        }
    }
}

impl Program {
    pub fn idx0(&self) -> Idx {
        self.body[0].idx0()
    }

    pub fn idx1(&self) -> Idx {
        self.body[self.body.len() - 1].idx1()
    }
}
