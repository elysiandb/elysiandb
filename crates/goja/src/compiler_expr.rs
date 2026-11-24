use crate::ast;
use crate::builtin_regexp::compile_regexp;
use crate::compiler::{
    BindingId, Block, BlockType, CResult, Compiler, PrivateName, ProgId, ResolvedPrivateName,
    TargetRef, expression_go_type, go_bytes, private_id_string, property_go_type,
};
use crate::file::Idx;
use crate::func::YieldMarker;
use crate::object::PrivateId;
use crate::string::{PROTO, ascii_string, new_string_value, string_value_from_raw};
use crate::token::{self, Token};
use crate::unistring;
use crate::value::{Value, ValueError, ValueProperty, float_to_value, int_to_value, new_big_int};
use crate::vm::{
    DefineKeyed, DefineMethod, DefinePrivateMethod, DefinePrivateProp, EnterBlock, EnterFunc,
    EnterFunc1, EnterFuncBody, EnterFuncStashless, GetTaggedTmplObject, InitStaticElements,
    Instruction, NewClass, NewFunc, NewMethod, NewRegexp,
};
use std::cell::Cell;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum FuncType {
    #[default]
    None = 0,
    Regular = 1,
    Arrow = 2,
    Method = 3,
    ClsInit = 4,
    Ctor = 5,
    DerivedCtor = 6,
}

pub(crate) type Emitter<'a> = Box<dyn Fn(&mut Compiler) -> CResult<()> + 'a>;
pub(crate) type NamedEmitter<'a> =
    Box<dyn Fn(&mut Compiler, &unistring::String) -> CResult<()> + 'a>;
pub(crate) type Thunk<'b> = &'b dyn Fn(&mut Compiler) -> CResult<()>;
pub(crate) type AssignEmitter<'b, 'a> =
    &'b dyn Fn(&mut Compiler, CompiledExpr<'a>, CompiledExpr<'a>) -> CResult<()>;

static EMPTY_PARAMETER_LIST: ast::ParameterList = ast::ParameterList {
    opening: 0,
    list: Vec::new(),
    rest: None,
    closing: 0,
};

#[derive(Clone, Copy)]
pub(crate) enum FuncBody<'a> {
    Stmts(&'a [ast::Statement]),
    Expr(&'a ast::Expression),
}

pub(crate) struct CallExpr<'a> {
    pub(crate) args: Vec<CompiledExpr<'a>>,
    pub(crate) callee: CompiledExpr<'a>,
    pub(crate) is_variadic: bool,
}

pub(crate) struct TemplateLiteralExpr<'a> {
    pub(crate) tag: Option<CompiledExpr<'a>>,
    pub(crate) elements: &'a [ast::TemplateElement],
    pub(crate) expressions: Vec<CompiledExpr<'a>>,
}

pub(crate) struct FunctionLiteralExpr<'a> {
    pub(crate) name: Option<&'a ast::Identifier>,
    pub(crate) parameter_list: &'a ast::ParameterList,
    pub(crate) body: FuncBody<'a>,
    pub(crate) source: &'a [u8],
    pub(crate) declaration_list: &'a [ast::VariableDeclaration],
    pub(crate) lhs_name: unistring::String,
    pub(crate) strict: Option<&'a ast::StringLiteral>,
    pub(crate) home_obj_offset: u32,
    pub(crate) typ: FuncType,
    pub(crate) is_expr: bool,
    pub(crate) is_async: bool,
    pub(crate) is_generator: bool,
}

pub(crate) struct ClassLiteralExpr<'a> {
    pub(crate) name: Option<&'a ast::Identifier>,
    pub(crate) super_class: Option<CompiledExpr<'a>>,
    pub(crate) body: &'a [ast::ClassElement],
    pub(crate) lhs_name: unistring::String,
    pub(crate) source: &'a [u8],
}

pub(crate) enum ExprKind<'a> {
    Call(Box<CallExpr<'a>>),
    New(Box<CallExpr<'a>>),
    ObjectLiteral(&'a ast::ObjectLiteral),
    ArrayLiteral(&'a ast::ArrayLiteral),
    RegexpLiteral(&'a ast::RegExpLiteral),
    Literal(Value),
    TemplateLiteral(Box<TemplateLiteralExpr<'a>>),
    Assign(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>, Token),
    ObjectAssignmentPattern(&'a ast::ObjectPattern),
    ArrayAssignmentPattern(&'a ast::ArrayPattern),
    Identifier(unistring::String),
    Await(Box<CompiledExpr<'a>>),
    Yield(Option<Box<CompiledExpr<'a>>>, bool),
    FunctionLiteral(Box<FunctionLiteralExpr<'a>>),
    Bracket(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>),
    This,
    Super,
    NewTarget,
    Sequence(Vec<CompiledExpr<'a>>),
    Unary(Box<CompiledExpr<'a>>, Token, bool),
    Conditional(Box<(CompiledExpr<'a>, CompiledExpr<'a>, CompiledExpr<'a>)>),
    LogicalOr(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>),
    Coalesce(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>),
    LogicalAnd(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>),
    Binary(Box<(CompiledExpr<'a>, CompiledExpr<'a>)>, Token),
    EnumGet,
    SpreadCallArgument(Box<CompiledExpr<'a>>),
    OptionalChain(Box<CompiledExpr<'a>>),
    Optional(Box<CompiledExpr<'a>>),
    SuperDot(unistring::String),
    Dot(Box<CompiledExpr<'a>>, unistring::String),
    PrivateDot(Box<CompiledExpr<'a>>, unistring::String),
    SuperBracket(Box<CompiledExpr<'a>>),
    ClassLiteral(Box<ClassLiteralExpr<'a>>),
    PrivateIn(unistring::String, Box<CompiledExpr<'a>>),
    PatternInit(Emitter<'a>, Option<Box<CompiledExpr<'a>>>),
    EmitterExpr(Option<Emitter<'a>>, Option<NamedEmitter<'a>>),
}

pub(crate) struct CompiledExpr<'a> {
    pub(crate) offset: isize,
    pub(crate) kind: ExprKind<'a>,
}

fn ce(idx: Idx, kind: ExprKind<'_>) -> CompiledExpr<'_> {
    CompiledExpr {
        offset: idx as isize - 1,
        kind,
    }
}

fn not_valid_left<T>(c: &Compiler, offset: isize) -> CResult<T> {
    c.throw_syntax_error(offset, "Not a valid left-value expression")
}

impl<'a> CompiledExpr<'a> {
    pub(crate) fn add_src_map(&self, c: &mut Compiler) {
        if self.offset >= 0 {
            c.add_src_map(self.offset);
        }
    }

    pub(crate) fn constant(&self, c: &mut Compiler) -> CResult<bool> {
        match &self.kind {
            ExprKind::Literal(_) => Ok(true),
            ExprKind::Unary(operand, _, _) => operand.constant(c),
            ExprKind::Binary(lr, _) => Ok(lr.0.constant(c)? && lr.1.constant(c)?),
            ExprKind::LogicalOr(lr) => {
                if lr.0.constant(c)? {
                    return match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if v.to_boolean() {
                                Ok(true)
                            } else {
                                lr.1.constant(c)
                            }
                        }
                        Err(_) => Ok(true),
                    };
                }
                Ok(false)
            }
            ExprKind::Coalesce(lr) => {
                if lr.0.constant(c)? {
                    return match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if !v.is_null() && !v.is_undefined() {
                                Ok(true)
                            } else {
                                lr.1.constant(c)
                            }
                        }
                        Err(_) => Ok(true),
                    };
                }
                Ok(false)
            }
            ExprKind::LogicalAnd(lr) => {
                if lr.0.constant(c)? {
                    return match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if !v.to_boolean() {
                                Ok(true)
                            } else {
                                lr.1.constant(c)
                            }
                        }
                        Err(_) => Ok(true),
                    };
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn is_named_emitter(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::FunctionLiteral(_)
                | ExprKind::ClassLiteral(_)
                | ExprKind::PatternInit(..)
                | ExprKind::EmitterExpr(..)
        )
    }

    pub(crate) fn emit_named(&self, c: &mut Compiler, name: &unistring::String) -> CResult<()> {
        match &self.kind {
            ExprKind::FunctionLiteral(f) => f.emit_getter_named(c, self.offset, true, Some(name)),
            ExprKind::ClassLiteral(cl) => cl.emit_getter(c, self.offset, true, Some(name)),
            ExprKind::PatternInit(emit_src, def) => {
                emit_src(c)?;
                if let Some(def) = def {
                    let mark = c.code_len();
                    c.emit(Instruction::Placeholder);
                    c.emit_named_or_const(def, name)?;
                    let d = c.code_len() - mark;
                    c.set_code(mark, Instruction::Jdef(d as i32));
                }
                Ok(())
            }
            ExprKind::EmitterExpr(emitter, named) => match named {
                Some(n) => n(c, name),
                None => emitter.as_ref().map_or(Ok(()), |e| e(c)),
            },
            _ => self.emit_getter(c, true),
        }
    }

    pub(crate) fn emit_getter(&self, c: &mut Compiler, put_on_stack: bool) -> CResult<()> {
        match &self.kind {
            ExprKind::Literal(v) => {
                if put_on_stack {
                    c.emit_literal_value(v.clone());
                }
                Ok(())
            }
            ExprKind::Identifier(name) => self.identifier_emit_getter(c, name, put_on_stack),
            ExprKind::SuperDot(name) => {
                c.emit_load_this();
                c.emit(Instruction::LoadSuper);
                self.add_src_map(c);
                c.emit(Instruction::GetPropRecv(name.clone()));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::PrivateDot(left, name) => {
                left.emit_getter(c, true)?;
                self.add_src_map(c);
                let (rn, id) = c.resolve_private_name(name, self.offset)?;
                emit_private(
                    c,
                    rn,
                    id,
                    Instruction::GetPrivatePropRes,
                    Instruction::GetPrivatePropId,
                );
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::SuperBracket(member) => {
                c.emit_load_this();
                member.emit_getter(c, true)?;
                c.emit(Instruction::LoadSuper);
                self.add_src_map(c);
                c.emit(Instruction::GetElemRecv);
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Dot(left, name) => {
                left.emit_getter(c, true)?;
                self.add_src_map(c);
                c.emit(Instruction::GetProp(name.clone()));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Bracket(lm) => {
                lm.0.emit_getter(c, true)?;
                lm.1.emit_getter(c, true)?;
                self.add_src_map(c);
                c.emit(Instruction::GetElem);
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Assign(lr, op) => self.assign_emit_getter(c, &lr.0, &lr.1, *op, put_on_stack),
            ExprKind::TemplateLiteral(t) => t.emit_getter(c, put_on_stack),
            ExprKind::FunctionLiteral(f) => f.emit_getter_named(c, self.offset, put_on_stack, None),
            ExprKind::ClassLiteral(cl) => cl.emit_getter(c, self.offset, put_on_stack, None),
            ExprKind::This => {
                self.add_src_map(c);
                c.emit_load_this();
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Super => {
                if put_on_stack {
                    c.emit(Instruction::LoadSuper);
                }
                Ok(())
            }
            ExprKind::New(call) => {
                if call.is_variadic {
                    c.emit(Instruction::StartVariadic);
                }
                call.callee.emit_getter(c, true)?;
                for expr in &call.args {
                    expr.emit_getter(c, true)?;
                }
                self.add_src_map(c);
                if call.is_variadic {
                    c.emit(Instruction::NewVariadic);
                    c.emit(Instruction::EndVariadic);
                } else {
                    c.emit(Instruction::New(call.args.len() as u32));
                }
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::NewTarget => {
                let s = c.nearest_this(c.cur_scope());
                if s.is_none() || c.scopes[s.unwrap_or_default()].func_type == FuncType::None {
                    return c.throw_syntax_error(
                        self.offset,
                        "new.target expression is not allowed here",
                    );
                }
                if put_on_stack {
                    self.add_src_map(c);
                    c.emit(Instruction::LoadNewTarget);
                }
                Ok(())
            }
            ExprKind::Sequence(seq) => {
                if let Some((last, rest)) = seq.split_last() {
                    for e in rest {
                        e.emit_getter(c, false)?;
                    }
                    last.emit_getter(c, put_on_stack)?;
                }
                Ok(())
            }
            ExprKind::Unary(operand, op, postfix) => {
                self.unary_emit_getter(c, operand, *op, *postfix, put_on_stack)
            }
            ExprKind::Conditional(tca) => {
                tca.0.emit_getter(c, true)?;
                let j = c.code_len();
                c.emit(Instruction::Placeholder);
                tca.1.emit_getter(c, put_on_stack)?;
                let j1 = c.code_len();
                c.emit(Instruction::Placeholder);
                let d = c.code_len() - j;
                c.set_code(j, Instruction::JneP(d as i32));
                tca.2.emit_getter(c, put_on_stack)?;
                let d1 = c.code_len() - j1;
                c.set_code(j1, Instruction::Jump(d1 as i32));
                Ok(())
            }
            ExprKind::LogicalOr(lr) => {
                if lr.0.constant(c)? {
                    match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if !v.to_boolean() {
                                c.emit_expr(&lr.1, put_on_stack)?;
                            } else if put_on_stack {
                                c.emit_literal_value(v);
                            }
                        }
                        Err(ex) => c.emit_throw(&ex)?,
                    }
                    return Ok(());
                }
                c.emit_expr(&lr.0, true)?;
                let j = c.code_len();
                self.add_src_map(c);
                c.emit(Instruction::Placeholder);
                c.emit_expr(&lr.1, true)?;
                let d = c.code_len() - j;
                c.set_code(j, Instruction::Jeq(d as i32));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Coalesce(lr) => {
                if lr.0.constant(c)? {
                    match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if v.is_undefined() || v.is_null() {
                                c.emit_expr(&lr.1, put_on_stack)?;
                            } else if put_on_stack {
                                c.emit_literal_value(v);
                            }
                        }
                        Err(ex) => c.emit_throw(&ex)?,
                    }
                    return Ok(());
                }
                c.emit_expr(&lr.0, true)?;
                let j = c.code_len();
                self.add_src_map(c);
                c.emit(Instruction::Placeholder);
                c.emit_expr(&lr.1, true)?;
                let d = c.code_len() - j;
                c.set_code(j, Instruction::Jcoalesc(d as i32));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::LogicalAnd(lr) => {
                if lr.0.constant(c)? {
                    match c.eval_const(&lr.0)? {
                        Ok(v) => {
                            if !v.to_boolean() {
                                c.emit_literal_value(v);
                            } else {
                                c.emit_expr(&lr.1, put_on_stack)?;
                            }
                        }
                        Err(ex) => c.emit_throw(&ex)?,
                    }
                    return Ok(());
                }
                lr.0.emit_getter(c, true)?;
                let j = c.code_len();
                self.add_src_map(c);
                c.emit(Instruction::Placeholder);
                c.emit_expr(&lr.1, true)?;
                let d = c.code_len() - j;
                c.set_code(j, Instruction::Jne(d as i32));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Binary(lr, op) => {
                c.emit_expr(&lr.0, true)?;
                c.emit_expr(&lr.1, true)?;
                self.add_src_map(c);
                let ins = match *op {
                    token::LESS => Instruction::OpLt,
                    token::GREATER => Instruction::OpGt,
                    token::LESS_OR_EQUAL => Instruction::OpLte,
                    token::GREATER_OR_EQUAL => Instruction::OpGte,
                    token::EQUAL => Instruction::OpEq,
                    token::NOT_EQUAL => Instruction::OpNeq,
                    token::STRICT_EQUAL => Instruction::OpStrictEq,
                    token::STRICT_NOT_EQUAL => Instruction::OpStrictNeq,
                    token::PLUS => Instruction::Add,
                    token::MINUS => Instruction::Sub,
                    token::MULTIPLY => Instruction::Mul,
                    token::EXPONENT => Instruction::Exp,
                    token::SLASH => Instruction::Div,
                    token::REMAINDER => Instruction::Mod,
                    token::AND => Instruction::And,
                    token::OR => Instruction::Or,
                    token::EXCLUSIVE_OR => Instruction::Xor,
                    token::INSTANCEOF => Instruction::OpInstanceof,
                    token::IN => Instruction::OpIn,
                    token::SHIFT_LEFT => Instruction::Sal,
                    token::SHIFT_RIGHT => Instruction::Sar,
                    token::UNSIGNED_SHIFT_RIGHT => Instruction::Shr,
                    _ => {
                        c.assert(
                            false,
                            self.offset,
                            format!("Unknown operator: {}", op.string()),
                        )?;
                        unreachable!()
                    }
                };
                c.emit(ins);
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::PrivateIn(id, right) => {
                right.emit_getter(c, true)?;
                let (rn, pid) = c.resolve_private_name(id, self.offset)?;
                emit_private(
                    c,
                    rn,
                    pid,
                    Instruction::PrivateInRes,
                    Instruction::PrivateInId,
                );
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::ObjectLiteral(expr) => self.object_literal_emit_getter(c, expr, put_on_stack),
            ExprKind::ArrayLiteral(expr) => {
                self.add_src_map(c);
                let mut has_spread = false;
                let mark = c.code_len();
                c.emit(Instruction::Placeholder);
                for v in &expr.value {
                    match v {
                        Some(ast::Expression::SpreadElement(spread)) => {
                            has_spread = true;
                            c.compile_expression(&spread.expression)?
                                .emit_getter(c, true)?;
                            c.emit(Instruction::PushArraySpread);
                        }
                        Some(v) => {
                            let e = c.compile_expression(v)?;
                            c.emit_expr(&e, true)?;
                            c.emit(Instruction::PushArrayItem);
                        }
                        None => {
                            c.emit(Instruction::LoadNil);
                            c.emit(Instruction::PushArrayItem);
                        }
                    }
                }
                let obj_count = if has_spread {
                    0
                } else {
                    expr.value.len() as u32
                };
                c.set_code(mark, Instruction::NewArray(obj_count));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::RegexpLiteral(expr) => {
                if put_on_stack {
                    let pattern = match compile_regexp(&expr.pattern, &expr.flags) {
                        Ok(p) => p,
                        Err(err) => return c.throw_syntax_error(self.offset, err),
                    };
                    c.emit(Instruction::NewRegexp(Box::new(NewRegexp {
                        pattern: Arc::new(pattern),
                        src: new_string_value(&expr.pattern),
                    })));
                }
                Ok(())
            }
            ExprKind::Call(call) => self.call_emit_getter(c, call, put_on_stack),
            ExprKind::EnumGet => {
                c.emit(Instruction::EnumGet);
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::ObjectAssignmentPattern(_) | ExprKind::ArrayAssignmentPattern(_) => {
                if put_on_stack {
                    c.emit(Instruction::LoadUndef);
                }
                Ok(())
            }
            ExprKind::PatternInit(emit_src, def) => {
                if !put_on_stack {
                    return Ok(());
                }
                emit_src(c)?;
                if let Some(def) = def {
                    let mark = c.code_len();
                    c.emit(Instruction::Placeholder);
                    c.emit_expr(def, true)?;
                    let d = c.code_len() - mark;
                    c.set_code(mark, Instruction::Jdef(d as i32));
                }
                Ok(())
            }
            ExprKind::EmitterExpr(emitter, named) => {
                match emitter {
                    Some(e) => e(c)?,
                    None => {
                        if let Some(n) = named {
                            n(c, &unistring::String::default())?;
                        }
                    }
                }
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::SpreadCallArgument(expr) => {
                expr.emit_getter(c, put_on_stack)?;
                if put_on_stack {
                    c.emit(Instruction::PushSpread);
                }
                Ok(())
            }
            ExprKind::OptionalChain(expr) => {
                c.start_opt_chain();
                expr.emit_getter(c, true)?;
                c.end_opt_chain();
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Optional(expr) => {
                expr.emit_getter(c, put_on_stack)?;
                if put_on_stack {
                    let pc = c.code_len();
                    c.blk_mut().breaks.push(pc);
                    c.emit(Instruction::Placeholder);
                }
                Ok(())
            }
            ExprKind::Await(arg) => {
                arg.emit_getter(c, true)?;
                c.emit(Instruction::YieldMarker(YieldMarker::Await));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Yield(arg, delegate) => {
                match arg {
                    Some(a) => a.emit_getter(c, true)?,
                    None => c.emit(Instruction::LoadUndef),
                }
                let m = match (put_on_stack, *delegate) {
                    (true, true) => YieldMarker::YieldDelegateRes,
                    (true, false) => YieldMarker::YieldRes,
                    (false, true) => YieldMarker::YieldDelegate,
                    (false, false) => YieldMarker::Yield,
                };
                c.emit(Instruction::YieldMarker(m));
                Ok(())
            }
        }
    }

    fn identifier_emit_getter(
        &self,
        c: &mut Compiler,
        name: &unistring::String,
        put_on_stack: bool,
    ) -> CResult<()> {
        self.add_src_map(c);
        let s = c.cur_scope();
        let (b, no_dynamics) = c.lookup_name(s, name)?;
        if no_dynamics {
            c.assert(b.is_some(), self.offset, "No dynamics and not found")?;
            let b = b.unwrap_or_default();
            if put_on_stack {
                c.b_emit_get(b);
            } else {
                c.b_emit_get_p(b);
            }
        } else {
            match b {
                Some(b) => c.b_emit_get_var(b, false),
                None => c.emit(Instruction::LoadDynamic(name.clone())),
            }
            if !put_on_stack {
                c.emit(Instruction::Pop);
            }
        }
        Ok(())
    }

    fn identifier_emit_getter_or_ref(
        &self,
        c: &mut Compiler,
        name: &unistring::String,
    ) -> CResult<()> {
        self.add_src_map(c);
        let s = c.cur_scope();
        let (b, no_dynamics) = c.lookup_name(s, name)?;
        if no_dynamics {
            c.assert(b.is_some(), self.offset, "No dynamics and not found")?;
            c.b_emit_get(b.unwrap_or_default());
        } else {
            match b {
                Some(b) => c.b_emit_get_var(b, false),
                None => c.emit(Instruction::LoadDynamicRef(name.clone())),
            }
        }
        Ok(())
    }

    fn identifier_emit_getter_and_callee(
        &self,
        c: &mut Compiler,
        name: &unistring::String,
    ) -> CResult<()> {
        self.add_src_map(c);
        let s = c.cur_scope();
        let (b, no_dynamics) = c.lookup_name(s, name)?;
        if no_dynamics {
            c.assert(b.is_some(), self.offset, "No dynamics and not found")?;
            c.emit(Instruction::LoadUndef);
            c.b_emit_get(b.unwrap_or_default());
        } else {
            match b {
                Some(b) => c.b_emit_get_var(b, true),
                None => c.emit(Instruction::LoadDynamicCallee(name.clone())),
            }
        }
        Ok(())
    }

    fn identifier_emit_var_setter1(
        &self,
        c: &mut Compiler,
        name: &unistring::String,
        put_on_stack: bool,
        emit_right: &dyn Fn(&mut Compiler, bool) -> CResult<()>,
    ) -> CResult<()> {
        self.add_src_map(c);
        let s = c.cur_scope();
        let (b, no_dynamics) = c.lookup_name(s, name)?;
        if no_dynamics {
            if c.strict() {
                c.check_identifier_l_name(name, self.offset)?;
            }
            emit_right(c, false)?;
            match b {
                Some(b) => {
                    if put_on_stack {
                        c.b_emit_set(b);
                    } else {
                        c.b_emit_set_p(b);
                    }
                }
                None => {
                    if c.strict() {
                        c.emit(Instruction::SetGlobalStrict(name.clone()));
                    } else {
                        c.emit(Instruction::SetGlobal(name.clone()));
                    }
                    if !put_on_stack {
                        c.emit(Instruction::Pop);
                    }
                }
            }
        } else {
            c.emit_var_ref(name, self.offset, b)?;
            emit_right(c, true)?;
            if put_on_stack {
                c.emit(Instruction::PutValue);
            } else {
                c.emit(Instruction::PutValueP);
            }
        }
        Ok(())
    }

    pub(crate) fn emit_setter(
        &self,
        c: &mut Compiler,
        value_expr: &CompiledExpr<'_>,
        put_on_stack: bool,
    ) -> CResult<()> {
        match &self.kind {
            ExprKind::Identifier(name) => {
                self.identifier_emit_var_setter1(c, name, put_on_stack, &|c, _| {
                    c.emit_named_or_const(value_expr, name)
                })
            }
            ExprKind::SuperDot(name) => {
                c.emit_load_this();
                c.emit(Instruction::LoadSuper);
                value_expr.emit_getter(c, true)?;
                self.add_src_map(c);
                let strict = c.strict();
                c.emit(match (put_on_stack, strict) {
                    (true, true) => Instruction::SetPropRecvStrict(name.clone()),
                    (true, false) => Instruction::SetPropRecv(name.clone()),
                    (false, true) => Instruction::SetPropRecvStrictP(name.clone()),
                    (false, false) => Instruction::SetPropRecvP(name.clone()),
                });
                Ok(())
            }
            ExprKind::PrivateDot(left, name) => {
                let (rn, id) = c.resolve_private_name(name, self.offset)?;
                left.emit_getter(c, true)?;
                value_expr.emit_getter(c, true)?;
                self.add_src_map(c);
                if put_on_stack {
                    emit_private(
                        c,
                        rn,
                        id,
                        Instruction::SetPrivatePropRes,
                        Instruction::SetPrivatePropId,
                    );
                } else {
                    emit_private(
                        c,
                        rn,
                        id,
                        Instruction::SetPrivatePropResP,
                        Instruction::SetPrivatePropIdP,
                    );
                }
                Ok(())
            }
            ExprKind::SuperBracket(member) => {
                c.emit_load_this();
                member.emit_getter(c, true)?;
                c.emit(Instruction::LoadSuper);
                value_expr.emit_getter(c, true)?;
                self.add_src_map(c);
                let strict = c.strict();
                c.emit(match (put_on_stack, strict) {
                    (true, true) => Instruction::SetElemRecvStrict,
                    (true, false) => Instruction::SetElemRecv,
                    (false, true) => Instruction::SetElemRecvStrictP,
                    (false, false) => Instruction::SetElemRecvP,
                });
                Ok(())
            }
            ExprKind::Dot(left, name) => {
                left.emit_getter(c, true)?;
                value_expr.emit_getter(c, true)?;
                self.add_src_map(c);
                let strict = c.strict();
                c.emit(match (strict, put_on_stack) {
                    (true, true) => Instruction::SetPropStrict(name.clone()),
                    (true, false) => Instruction::SetPropStrictP(name.clone()),
                    (false, true) => Instruction::SetProp(name.clone()),
                    (false, false) => Instruction::SetPropP(name.clone()),
                });
                Ok(())
            }
            ExprKind::Bracket(lm) => {
                lm.0.emit_getter(c, true)?;
                lm.1.emit_getter(c, true)?;
                value_expr.emit_getter(c, true)?;
                self.add_src_map(c);
                let strict = c.strict();
                c.emit(match (strict, put_on_stack) {
                    (true, true) => Instruction::SetElemStrict,
                    (true, false) => Instruction::SetElemStrictP,
                    (false, true) => Instruction::SetElem,
                    (false, false) => Instruction::SetElemP,
                });
                Ok(())
            }
            ExprKind::ObjectAssignmentPattern(p) => {
                value_expr.emit_getter(c, true)?;
                c.emit_object_pattern(p, &|c, t, i| c.emit_pattern_assign(&t, &i), put_on_stack)
            }
            ExprKind::ArrayAssignmentPattern(p) => {
                value_expr.emit_getter(c, true)?;
                c.emit_array_pattern(p, &|c, t, i| c.emit_pattern_assign(&t, &i), put_on_stack)
            }
            _ => not_valid_left(c, self.offset),
        }
    }

    pub(crate) fn emit_ref(&self, c: &mut Compiler) -> CResult<()> {
        match &self.kind {
            ExprKind::Identifier(name) => {
                let s = c.cur_scope();
                let (b, _) = c.lookup_name(s, name)?;
                c.emit_var_ref(name, self.offset, b)
            }
            ExprKind::SuperDot(name) => {
                c.emit_load_this();
                c.emit(Instruction::LoadSuper);
                if c.strict() {
                    c.emit(Instruction::GetPropRefRecvStrict(name.clone()));
                } else {
                    c.emit(Instruction::GetPropRefRecv(name.clone()));
                }
                Ok(())
            }
            ExprKind::PrivateDot(left, name) => {
                left.emit_getter(c, true)?;
                let (rn, id) = c.resolve_private_name(name, self.offset)?;
                emit_private(
                    c,
                    rn,
                    id,
                    Instruction::GetPrivateRefRes,
                    Instruction::GetPrivateRefId,
                );
                Ok(())
            }
            ExprKind::SuperBracket(member) => {
                c.emit_load_this();
                member.emit_getter(c, true)?;
                c.emit(Instruction::LoadSuper);
                if c.strict() {
                    c.emit(Instruction::GetElemRefRecvStrict);
                } else {
                    c.emit(Instruction::GetElemRefRecv);
                }
                Ok(())
            }
            ExprKind::Dot(left, name) => {
                left.emit_getter(c, true)?;
                if c.strict() {
                    c.emit(Instruction::GetPropRefStrict(name.clone()));
                } else {
                    c.emit(Instruction::GetPropRef(name.clone()));
                }
                Ok(())
            }
            ExprKind::Bracket(lm) => {
                lm.0.emit_getter(c, true)?;
                lm.1.emit_getter(c, true)?;
                if c.strict() {
                    c.emit(Instruction::GetElemRefStrict);
                } else {
                    c.emit(Instruction::GetElemRef);
                }
                Ok(())
            }
            _ => c.assert(
                false,
                self.offset,
                "Cannot emit reference for this type of expression",
            ),
        }
    }

    pub(crate) fn emit_unary(
        &self,
        c: &mut Compiler,
        prepare: Option<Thunk<'_>>,
        body: Thunk<'_>,
        postfix: bool,
        put_on_stack: bool,
    ) -> CResult<()> {
        match &self.kind {
            ExprKind::Identifier(name) => {
                if put_on_stack {
                    self.identifier_emit_var_setter1(c, name, true, &|c, is_ref| {
                        c.emit(Instruction::LoadUndef);
                        if is_ref {
                            c.emit(Instruction::GetValue);
                        } else {
                            self.emit_getter(c, true)?;
                        }
                        if let Some(p) = prepare {
                            p(c)?;
                        }
                        if !postfix {
                            body(c)?;
                        }
                        c.emit(Instruction::RdupN(1));
                        if postfix {
                            body(c)?;
                        }
                        Ok(())
                    })?;
                    c.emit(Instruction::Pop);
                    Ok(())
                } else {
                    self.identifier_emit_var_setter1(c, name, false, &|c, is_ref| {
                        if is_ref {
                            c.emit(Instruction::GetValue);
                        } else {
                            self.emit_getter(c, true)?;
                        }
                        body(c)
                    })
                }
            }
            ExprKind::SuperDot(name) => {
                let set = |c: &mut Compiler, p: bool| {
                    let strict = c.strict();
                    c.emit(match (p, strict) {
                        (true, true) => Instruction::SetPropRecvStrictP(name.clone()),
                        (true, false) => Instruction::SetPropRecvP(name.clone()),
                        (false, true) => Instruction::SetPropRecvStrict(name.clone()),
                        (false, false) => Instruction::SetPropRecv(name.clone()),
                    });
                };
                if !put_on_stack {
                    c.emit_load_this();
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetPropRecv(name.clone()));
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                } else if !postfix {
                    c.emit_load_this();
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetPropRecv(name.clone()));
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    body(c)?;
                    self.add_src_map(c);
                    set(c, false);
                } else {
                    c.emit(Instruction::LoadUndef);
                    c.emit_load_this();
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetPropRecv(name.clone()));
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    c.emit(Instruction::RdupN(3));
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                }
                Ok(())
            }
            ExprKind::PrivateDot(left, name) => {
                let (rn, id) = c.resolve_private_name(name, self.offset)?;
                let get = |c: &mut Compiler| {
                    emit_private(
                        c,
                        rn.clone(),
                        id.clone(),
                        Instruction::GetPrivatePropRes,
                        Instruction::GetPrivatePropId,
                    )
                };
                if !put_on_stack {
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    get(c);
                    body(c)?;
                    self.add_src_map(c);
                    emit_private(
                        c,
                        rn,
                        id,
                        Instruction::SetPrivatePropResP,
                        Instruction::SetPrivatePropIdP,
                    );
                } else if !postfix {
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    get(c);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    body(c)?;
                    self.add_src_map(c);
                    emit_private(
                        c,
                        rn,
                        id,
                        Instruction::SetPrivatePropRes,
                        Instruction::SetPrivatePropId,
                    );
                } else {
                    c.emit(Instruction::LoadUndef);
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    get(c);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    c.emit(Instruction::RdupN(2));
                    body(c)?;
                    self.add_src_map(c);
                    emit_private(
                        c,
                        rn,
                        id,
                        Instruction::SetPrivatePropResP,
                        Instruction::SetPrivatePropIdP,
                    );
                }
                Ok(())
            }
            ExprKind::SuperBracket(member) => {
                let set = |c: &mut Compiler, p: bool| {
                    let strict = c.strict();
                    c.emit(match (p, strict) {
                        (true, true) => Instruction::SetElemRecvStrictP,
                        (true, false) => Instruction::SetElemRecvP,
                        (false, true) => Instruction::SetElemRecvStrict,
                        (false, false) => Instruction::SetElemRecv,
                    });
                };
                if !put_on_stack {
                    c.emit_load_this();
                    member.emit_getter(c, true)?;
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(3));
                    c.emit(Instruction::GetElemRecv);
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                } else if !postfix {
                    c.emit_load_this();
                    member.emit_getter(c, true)?;
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(3));
                    c.emit(Instruction::GetElemRecv);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    body(c)?;
                    self.add_src_map(c);
                    set(c, false);
                } else {
                    c.emit(Instruction::LoadUndef);
                    c.emit_load_this();
                    member.emit_getter(c, true)?;
                    c.emit(Instruction::LoadSuper);
                    c.emit(Instruction::DupLast(3));
                    c.emit(Instruction::GetElemRecv);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    c.emit(Instruction::RdupN(4));
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                }
                Ok(())
            }
            ExprKind::Dot(left, name) => {
                let set = |c: &mut Compiler, p: bool| {
                    let strict = c.strict();
                    c.emit(match (strict, p) {
                        (true, true) => Instruction::SetPropStrictP(name.clone()),
                        (true, false) => Instruction::SetPropStrict(name.clone()),
                        (false, true) => Instruction::SetPropP(name.clone()),
                        (false, false) => Instruction::SetProp(name.clone()),
                    });
                };
                if !put_on_stack {
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    c.emit(Instruction::GetProp(name.clone()));
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                } else if !postfix {
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    c.emit(Instruction::GetProp(name.clone()));
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    body(c)?;
                    self.add_src_map(c);
                    set(c, false);
                } else {
                    c.emit(Instruction::LoadUndef);
                    left.emit_getter(c, true)?;
                    c.emit(Instruction::Dup);
                    c.emit(Instruction::GetProp(name.clone()));
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    c.emit(Instruction::RdupN(2));
                    body(c)?;
                    self.add_src_map(c);
                    set(c, true);
                }
                Ok(())
            }
            ExprKind::Bracket(lm) => {
                let set = |c: &mut Compiler| {
                    if c.strict() {
                        c.emit(Instruction::SetElemStrict);
                    } else {
                        c.emit(Instruction::SetElem);
                    }
                };
                if !put_on_stack {
                    lm.0.emit_getter(c, true)?;
                    lm.1.emit_getter(c, true)?;
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetElem);
                    body(c)?;
                    self.add_src_map(c);
                    set(c);
                    c.emit(Instruction::Pop);
                } else if !postfix {
                    lm.0.emit_getter(c, true)?;
                    lm.1.emit_getter(c, true)?;
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetElem);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    body(c)?;
                    self.add_src_map(c);
                    set(c);
                } else {
                    c.emit(Instruction::LoadUndef);
                    lm.0.emit_getter(c, true)?;
                    lm.1.emit_getter(c, true)?;
                    c.emit(Instruction::DupLast(2));
                    c.emit(Instruction::GetElem);
                    if let Some(p) = prepare {
                        p(c)?;
                    }
                    c.emit(Instruction::RdupN(3));
                    body(c)?;
                    self.add_src_map(c);
                    set(c);
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            _ => not_valid_left(c, self.offset),
        }
    }

    pub(crate) fn emit_delete(&self, c: &mut Compiler, put_on_stack: bool) -> CResult<()> {
        match &self.kind {
            ExprKind::Identifier(name) => {
                if c.strict() {
                    return c.throw_syntax_error(
                        self.offset,
                        "Delete of an unqualified identifier in strict mode",
                    );
                }
                let s = c.cur_scope();
                let (b, no_dynamics) = c.lookup_name(s, name)?;
                if b.is_none() {
                    if no_dynamics {
                        c.emit(Instruction::DeleteGlobal(name.clone()));
                    } else {
                        c.emit(Instruction::DeleteVar(name.clone()));
                    }
                    if !put_on_stack {
                        c.emit(Instruction::Pop);
                    }
                    return Ok(());
                }
                if put_on_stack {
                    c.emit_literal_value(Value::Bool(false));
                }
                Ok(())
            }
            ExprKind::SuperDot(_) | ExprKind::SuperBracket(_) => {
                c.emit(Instruction::ThrowConst(ValueError::ReferenceError(
                    "Unsupported reference to 'super'".to_string(),
                )));
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::PrivateDot(..) => {
                c.throw_syntax_error(self.offset, "Private fields can not be deleted")
            }
            ExprKind::Dot(left, name) => {
                left.emit_getter(c, true)?;
                self.add_src_map(c);
                if c.strict() {
                    c.emit(Instruction::DeletePropStrict(name.clone()));
                } else {
                    c.emit(Instruction::DeleteProp(name.clone()));
                }
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Bracket(lm) => {
                lm.0.emit_getter(c, true)?;
                lm.1.emit_getter(c, true)?;
                self.add_src_map(c);
                if c.strict() {
                    c.emit(Instruction::DeleteElemStrict);
                } else {
                    c.emit(Instruction::DeleteElem);
                }
                if !put_on_stack {
                    c.emit(Instruction::Pop);
                }
                Ok(())
            }
            ExprKind::Call(_) => {
                self.emit_getter(c, false)?;
                if put_on_stack {
                    c.emit_literal_value(Value::Bool(true));
                }
                Ok(())
            }
            _ => {
                if put_on_stack {
                    self.add_src_map(c);
                    c.emit_literal_value(Value::Bool(true));
                }
                Ok(())
            }
        }
    }

    fn assign_emit_getter(
        &self,
        c: &mut Compiler,
        left: &CompiledExpr<'_>,
        right: &CompiledExpr<'_>,
        op: Token,
        put_on_stack: bool,
    ) -> CResult<()> {
        let arith = match op {
            token::ASSIGN => return left.emit_setter(c, right, put_on_stack),
            token::PLUS => Some(Instruction::Add),
            token::MINUS => Some(Instruction::Sub),
            token::MULTIPLY => Some(Instruction::Mul),
            token::EXPONENT => Some(Instruction::Exp),
            token::SLASH => Some(Instruction::Div),
            token::REMAINDER => Some(Instruction::Mod),
            token::OR => Some(Instruction::Or),
            token::AND => Some(Instruction::And),
            token::EXCLUSIVE_OR => Some(Instruction::Xor),
            token::SHIFT_LEFT => Some(Instruction::Sal),
            token::SHIFT_RIGHT => Some(Instruction::Sar),
            token::UNSIGNED_SHIFT_RIGHT => Some(Instruction::Shr),
            _ => None,
        };
        if let Some(ins) = arith {
            return left.emit_unary(
                c,
                None,
                &|c| {
                    right.emit_getter(c, true)?;
                    c.emit(ins.clone());
                    Ok(())
                },
                false,
                put_on_stack,
            );
        }
        match op {
            token::LOGICAL_AND | token::LOGICAL_OR | token::COALESCE => {
                left.emit_ref(c)?;
                c.emit(Instruction::GetValue);
                let mark = c.code_len();
                c.emit(Instruction::Placeholder);
                if let ExprKind::Identifier(name) = &left.kind {
                    c.emit_named_or_const(right, name)?;
                } else {
                    right.emit_getter(c, true)?;
                }
                if put_on_stack {
                    c.emit(Instruction::PutValue);
                } else {
                    c.emit(Instruction::PutValueP);
                }
                c.emit(Instruction::Jump(2));
                let offset = (c.code_len() - mark) as i32;
                let j = match (op, put_on_stack) {
                    (token::LOGICAL_AND, true) => Instruction::Jne(offset),
                    (token::LOGICAL_AND, false) => Instruction::JneP(offset),
                    (token::LOGICAL_OR, true) => Instruction::Jeq(offset),
                    (token::LOGICAL_OR, false) => Instruction::JeqP(offset),
                    (_, true) => Instruction::Jcoalesc(offset),
                    (_, false) => Instruction::JcoalescP(offset),
                };
                c.set_code(mark, j);
                c.emit(Instruction::PopRef);
                Ok(())
            }
            _ => c.assert(
                false,
                self.offset,
                format!("Unknown assign operator: {}", op.string()),
            ),
        }
    }

    fn unary_emit_getter(
        &self,
        c: &mut Compiler,
        operand: &CompiledExpr<'_>,
        op: Token,
        postfix: bool,
        put_on_stack: bool,
    ) -> CResult<()> {
        let to_number = |c: &mut Compiler| {
            self.add_src_map(c);
            c.emit(Instruction::ToNumber);
            Ok(())
        };
        match op {
            token::NOT => {
                operand.emit_getter(c, true)?;
                c.emit(Instruction::Not);
            }
            token::BITWISE_NOT => {
                operand.emit_getter(c, true)?;
                c.emit(Instruction::Bnot);
            }
            token::TYPEOF => {
                if let ExprKind::Identifier(name) = &operand.kind {
                    operand.identifier_emit_getter_or_ref(c, name)?;
                } else {
                    operand.emit_getter(c, true)?;
                }
                c.emit(Instruction::Typeof);
            }
            token::DELETE => return operand.emit_delete(c, put_on_stack),
            token::MINUS => {
                c.emit_expr(operand, true)?;
                c.emit(Instruction::Neg);
            }
            token::PLUS => {
                c.emit_expr(operand, true)?;
                c.emit(Instruction::Plus);
            }
            token::INCREMENT => {
                return operand.emit_unary(
                    c,
                    Some(&to_number),
                    &|c| {
                        c.emit(Instruction::Inc);
                        Ok(())
                    },
                    postfix,
                    put_on_stack,
                );
            }
            token::DECREMENT => {
                return operand.emit_unary(
                    c,
                    Some(&to_number),
                    &|c| {
                        c.emit(Instruction::Dec);
                        Ok(())
                    },
                    postfix,
                    put_on_stack,
                );
            }
            token::VOID => {
                c.emit_expr(operand, false)?;
                if put_on_stack {
                    c.emit(Instruction::LoadUndef);
                }
                return Ok(());
            }
            _ => {
                return c.assert(
                    false,
                    self.offset,
                    format!("Unknown unary operator: {}", op.string()),
                );
            }
        }
        if !put_on_stack {
            c.emit(Instruction::Pop);
        }
        Ok(())
    }

    fn object_literal_emit_getter(
        &self,
        c: &mut Compiler,
        expr: &'a ast::ObjectLiteral,
        put_on_stack: bool,
    ) -> CResult<()> {
        self.add_src_map(c);
        c.emit(Instruction::NewObject);
        let mut has_proto = false;
        for prop in &expr.value {
            match prop {
                ast::Property::PropertyKeyed(prop) => {
                    let (key, computed) = c.process_key(&prop.key)?;
                    let mut value_expr = c.compile_expression(&prop.value)?;
                    let mut ne = false;
                    if let ExprKind::FunctionLiteral(f) = &mut value_expr.kind {
                        if f.name.is_none() {
                            ne = true;
                        }
                        match prop.kind {
                            ast::PropertyKind::Method
                            | ast::PropertyKind::Get
                            | ast::PropertyKind::Set => {
                                f.typ = FuncType::Method;
                                f.home_obj_offset = if computed { 2 } else { 1 };
                            }
                            _ => {}
                        }
                    } else if value_expr.is_named_emitter() {
                        ne = true;
                    }
                    if computed {
                        c.emit(Instruction::ToPropertyKey);
                        c.emit_expr(&value_expr, true)?;
                        c.emit(match prop.kind {
                            ast::PropertyKind::Value => {
                                if ne {
                                    Instruction::SetElem1Named
                                } else {
                                    Instruction::SetElem1
                                }
                            }
                            ast::PropertyKind::Method => {
                                Instruction::DefineMethod(DefineMethod { enumerable: true })
                            }
                            ast::PropertyKind::Get => {
                                Instruction::DefineGetter(DefineMethod { enumerable: true })
                            }
                            ast::PropertyKind::Set => {
                                Instruction::DefineSetter(DefineMethod { enumerable: true })
                            }
                        });
                    } else {
                        let is_proto = key == PROTO && !prop.computed;
                        if is_proto {
                            if has_proto {
                                return c.throw_syntax_error(
                                    prop.key.idx0() as isize - 1,
                                    "Duplicate __proto__ fields are not allowed in object literals",
                                );
                            }
                            has_proto = true;
                        }
                        if ne && !is_proto {
                            value_expr.emit_named(c, &key)?;
                        } else {
                            c.emit_expr(&value_expr, true)?;
                        }
                        let keyed = |key: &unistring::String| {
                            Box::new(DefineKeyed {
                                key: key.clone(),
                                enumerable: true,
                            })
                        };
                        c.emit(match prop.kind {
                            ast::PropertyKind::Value => {
                                if is_proto {
                                    Instruction::SetProto
                                } else {
                                    Instruction::PutProp(key.clone())
                                }
                            }
                            ast::PropertyKind::Method => {
                                Instruction::DefineMethodKeyed(keyed(&key))
                            }
                            ast::PropertyKind::Get => Instruction::DefineGetterKeyed(keyed(&key)),
                            ast::PropertyKind::Set => Instruction::DefineSetterKeyed(keyed(&key)),
                        });
                    }
                }
                ast::Property::PropertyShort(prop) => {
                    let key = &prop.name.name;
                    if let Some(init) = &prop.initializer {
                        return c.throw_syntax_error(
                            init.idx0() as isize - 1,
                            "Invalid shorthand property initializer",
                        );
                    }
                    if c.strict() && *key == "let" {
                        return c.throw_syntax_error(
                            self.offset,
                            "'let' cannot be used as a shorthand property in strict mode",
                        );
                    }
                    c.compile_identifier_expression(&prop.name)?
                        .emit_getter(c, true)?;
                    c.emit(Instruction::PutProp(key.clone()));
                }
                ast::Property::SpreadElement(prop) => {
                    c.compile_expression(&prop.expression)?
                        .emit_getter(c, true)?;
                    c.emit(Instruction::CopySpread);
                }
            }
        }
        if !put_on_stack {
            c.emit(Instruction::Pop);
        }
        Ok(())
    }

    fn call_emit_getter(
        &self,
        c: &mut Compiler,
        call: &CallExpr<'_>,
        put_on_stack: bool,
    ) -> CResult<()> {
        if call.is_variadic {
            c.emit(Instruction::StartVariadic);
        }
        let callee_name = c.emit_callee(&call.callee)?;
        for expr in &call.args {
            expr.emit_getter(c, true)?;
        }
        self.add_src_map(c);
        if let ExprKind::Super = call.callee.kind {
            let s = c.cur_scope();
            let (b, eval) = c.lookup_this(s);
            c.assert(
                eval || b.is_some(),
                self.offset,
                "super call, but no 'this' binding",
            )?;
            if eval {
                c.emit(Instruction::ResolveThisDynamic);
            } else {
                c.mark_access_point(b.unwrap_or_default());
                c.emit(Instruction::ResolveThisStack);
            }
            if call.is_variadic {
                c.emit(Instruction::SuperCallVariadic);
            } else {
                c.emit(Instruction::SuperCall(call.args.len() as u32));
            }
        } else if callee_name == "eval" {
            let mut found_var = false;
            let mut sc = c.scope;
            while let Some(s) = sc {
                if !found_var && (c.scopes[s].variable || c.is_function(s)) {
                    found_var = true;
                    if !c.scopes[s].strict {
                        c.scopes[s].dynamic = true;
                    }
                }
                c.scopes[s].dyn_lookup = true;
                sc = c.scopes[s].outer;
            }
            if c.strict() {
                if call.is_variadic {
                    c.emit(Instruction::CallEvalVariadicStrict);
                } else {
                    c.emit(Instruction::CallEvalStrict(call.args.len() as u32));
                }
            } else if call.is_variadic {
                c.emit(Instruction::CallEvalVariadic);
            } else {
                c.emit(Instruction::CallEval(call.args.len() as u32));
            }
        } else if call.is_variadic {
            c.emit(Instruction::CallVariadic);
        } else {
            c.emit(Instruction::Call(call.args.len() as u32));
        }
        if call.is_variadic {
            c.emit(Instruction::EndVariadic);
        }
        if !put_on_stack {
            c.emit(Instruction::Pop);
        }
        Ok(())
    }
}

fn emit_private(
    c: &mut Compiler,
    rn: Option<ResolvedPrivateName>,
    id: Option<PrivateId>,
    res: fn(Box<ResolvedPrivateName>) -> Instruction,
    by_id: fn(Box<PrivateId>) -> Instruction,
) {
    match rn {
        Some(rn) => c.emit(res(Box::new(rn))),
        None => {
            if let Some(id) = id {
                c.emit(by_id(Box::new(id)));
            }
        }
    }
}

impl<'a> TemplateLiteralExpr<'a> {
    fn emit_getter(&self, c: &mut Compiler, put_on_stack: bool) -> CResult<()> {
        match &self.tag {
            None => {
                if self.elements.is_empty() {
                    c.emit_literal_string(ascii_string(""));
                } else {
                    let tail = &self.elements[self.elements.len() - 1].parsed;
                    if self.elements.len() == 1 {
                        c.emit_literal_string(string_value_from_raw(tail));
                    } else {
                        let mut string_count = 0;
                        let head = &self.elements[0].parsed;
                        if !head.is_empty() {
                            c.emit_literal_string(string_value_from_raw(head));
                            string_count += 1;
                        }
                        self.expressions[0].emit_getter(c, true)?;
                        c.emit(Instruction::ToString);
                        string_count += 1;
                        for i in 1..self.elements.len() - 1 {
                            let elt = &self.elements[i].parsed;
                            if !elt.is_empty() {
                                c.emit_literal_string(string_value_from_raw(elt));
                                string_count += 1;
                            }
                            self.expressions[i].emit_getter(c, true)?;
                            c.emit(Instruction::ToString);
                            string_count += 1;
                        }
                        if !tail.is_empty() {
                            c.emit_literal_string(string_value_from_raw(tail));
                            string_count += 1;
                        }
                        c.emit(Instruction::ConcatStrings(string_count));
                    }
                }
            }
            Some(tag) => {
                let mut cooked = Vec::with_capacity(self.elements.len());
                let mut raw = Vec::with_capacity(self.elements.len());
                for elt in self.elements {
                    raw.push(ValueProperty {
                        value: Value::String(new_string_value(&elt.literal)),
                        writable: false,
                        configurable: false,
                        enumerable: true,
                        accessor: false,
                        getter_func: None,
                        setter_func: None,
                    });
                    let cooked_val = if elt.valid {
                        Value::String(string_value_from_raw(&elt.parsed))
                    } else {
                        Value::Undefined
                    };
                    cooked.push(ValueProperty {
                        value: cooked_val,
                        writable: false,
                        configurable: false,
                        enumerable: true,
                        accessor: false,
                        getter_func: None,
                        setter_func: None,
                    });
                }
                c.emit_callee(tag)?;
                c.emit(Instruction::GetTaggedTmplObject(Box::new(
                    GetTaggedTmplObject { raw, cooked },
                )));
                for expr in &self.expressions {
                    expr.emit_getter(c, true)?;
                }
                c.emit(Instruction::Call(self.expressions.len() as u32 + 1));
            }
        }
        if !put_on_stack {
            c.emit(Instruction::Pop);
        }
        Ok(())
    }
}

fn is_pattern_target(t: &ast::BindingTarget) -> bool {
    matches!(
        t,
        ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_)
    )
}

impl<'a> FunctionLiteralExpr<'a> {
    fn body_list(&self) -> &'a [ast::Statement] {
        match self.body {
            FuncBody::Stmts(s) => s,
            FuncBody::Expr(_) => &[],
        }
    }

    pub(crate) fn compile(
        &self,
        c: &mut Compiler,
        offset: isize,
        lhs_override: Option<&unistring::String>,
    ) -> CResult<(Arc<Program>, ProgId, unistring::String, isize, bool)> {
        c.assert(
            self.typ != FuncType::None,
            offset,
            "compiledFunctionLiteral.typ is not set",
        )?;
        let saved_prg = c.p;
        let preamble_len: usize = 8;
        let new_p = c.new_prog(
            unistring::String::default(),
            vec![Instruction::Placeholder; preamble_len],
            vec![crate::compiler::SrcMapItem {
                pc: 0,
                src_pos: offset,
            }],
        );
        c.p = new_p;
        c.new_scope();
        let s = c.cur_scope();
        c.scopes[s].func_type = self.typ;
        let name = match self.name {
            Some(n) => n.name.clone(),
            None => lhs_override
                .cloned()
                .unwrap_or_else(|| self.lhs_name.clone()),
        };
        if !name.is_empty() {
            c.progs[new_p].func_name = name.clone();
        }
        let saved_block = c.block;
        c.push_block(Block::new(BlockType::Scope, None));
        let r = self.compile_body(c, offset, s, preamble_len);
        c.block = saved_block;
        let (length, strict, enter_func2_mark) = r?;
        if enter_func2_mark.is_some() {
            c.pop_scope();
        }
        c.pop_scope();
        c.p = saved_prg;
        let handle = c.progs[new_p].handle.clone();
        Ok((handle, new_p, name, length, strict))
    }

    fn compile_body(
        &self,
        c: &mut Compiler,
        offset: isize,
        s: usize,
        preamble_len: usize,
    ) -> CResult<(isize, bool, Option<usize>)> {
        let mut length: isize = 0;
        if !c.scopes[s].strict {
            c.scopes[s].strict = self.strict.is_some();
        }
        let mut has_patterns = false;
        let mut has_inits = false;
        let mut first_dup_idx: isize = -1;
        if self.parameter_list.rest.is_some() {
            has_patterns = true;
        }
        for item in &self.parameter_list.list {
            match &item.target {
                ast::BindingTarget::Identifier(tgt) => {
                    let off = tgt.idx as isize - 1;
                    let (b, unique) = c.compile_parameter_binding_identifier(&tgt.name, off)?;
                    if !unique {
                        first_dup_idx = off;
                    }
                    c.bindings[b].is_arg = true;
                }
                ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_) => {
                    let b = c.add_binding(s, item.idx0() as isize - 1)?;
                    c.bindings[b].is_arg = true;
                    has_patterns = true;
                }
                ast::BindingTarget::BadExpression(_) => {
                    return c.throw_syntax_error(
                        item.idx0() as isize - 1,
                        "Unsupported BindingElement type: *ast.Binding",
                    );
                }
            }
            if item.initializer.is_some() {
                has_inits = true;
            }
            if first_dup_idx >= 0
                && (has_patterns
                    || has_inits
                    || c.scopes[s].strict
                    || self.typ == FuncType::Arrow
                    || self.typ == FuncType::Method)
            {
                return c.throw_syntax_error(
                    first_dup_idx,
                    "Duplicate parameter name not allowed in this context",
                );
            }
            if (has_patterns || has_inits) && self.strict.is_some() {
                return c.throw_syntax_error(
                    self.strict.map_or(0, |st| st.idx) as isize - 1,
                    "Illegal 'use strict' directive in function with non-simple parameter list",
                );
            }
            if !has_inits {
                length += 1;
            }
        }

        let mut this_binding = None;
        if self.typ != FuncType::Arrow {
            this_binding = Some(c.create_this_binding(s)?);
        }

        if has_patterns {
            for item in &self.parameter_list.list {
                if is_pattern_target(&item.target) {
                    c.compile_parameter_pattern_binding(TargetRef::of_target(&item.target))?;
                }
            }
            if let Some(rest) = &self.parameter_list.rest {
                c.compile_parameter_pattern_binding(TargetRef::of_expr(rest))?;
            }
        }

        let params_count = self.parameter_list.list.len();
        c.scopes[s].num_args = params_count;
        let body = self.body_list();
        let funcs = c.extract_functions(body);
        let mut callee_binding: Option<BindingId> = None;

        let emit_args_rest_mark: Cell<isize> = Cell::new(-1);
        let mut first_forward_ref: isize = -1;
        let mut enter_func2_mark: Option<usize> = None;

        if has_patterns || has_inits {
            if self.is_expr
                && let Some(n) = self.name
            {
                let (b, created) = c.bind_name_lexical(s, &n.name, false, 0)?;
                if created {
                    c.bindings[b].is_const = true;
                    callee_binding = Some(b);
                }
            }
            for (i, item) in self.parameter_list.list.iter().enumerate() {
                if is_pattern_target(&item.target) {
                    let ffr = first_forward_ref;
                    let bi = c.scopes[s].bindings[i];
                    let init = c.compile_pattern_init_expr(
                        Box::new(move |c: &mut Compiler| {
                            if ffr == -1 {
                                c.b_emit_get(bi);
                            } else {
                                c.emit(Instruction::LoadStackLex(-(i as isize) - 1));
                            }
                            Ok(())
                        }),
                        item.initializer.as_ref(),
                        item.target.idx0(),
                    )?;
                    init.emit_getter(c, true)?;
                    c.emit_pattern(
                        PatternRef::of_target(&item.target),
                        &|c, target, init| c.emit_pattern_lexical_assign(&target, &init),
                        false,
                    )?;
                } else if let Some(initializer) = &item.initializer {
                    let mark_get = c.code_len();
                    c.emit(Instruction::Placeholder);
                    let mark = c.code_len();
                    c.emit(Instruction::Placeholder);
                    let e = c.compile_expression(initializer)?;
                    c.emit_expr(&e, true)?;
                    let bi = c.scopes[s].bindings[i];
                    if first_forward_ref == -1 && (c.is_dynamic(s) || c.b_use_count(bi) > 0) {
                        first_forward_ref = i as isize;
                    }
                    if first_forward_ref == -1 {
                        c.b_emit_get_at(bi, mark_get);
                    } else {
                        c.set_code(mark_get, Instruction::LoadStackLex(-(i as isize) - 1));
                    }
                    c.b_emit_init_p(bi);
                    let d = c.code_len() - mark;
                    c.set_code(mark, Instruction::JdefP(d as i32));
                } else {
                    let bi = c.scopes[s].bindings[i];
                    if first_forward_ref == -1 && c.b_use_count(bi) > 0 {
                        first_forward_ref = i as isize;
                    }
                    if first_forward_ref != -1 {
                        c.emit(Instruction::LoadStackLex(-(i as isize) - 1));
                        c.b_emit_init_p(bi);
                    }
                }
            }
            if let Some(rest) = &self.parameter_list.rest {
                let mark = &emit_args_rest_mark;
                let init = c.compile_emitter_expr(
                    Box::new(move |c: &mut Compiler| {
                        mark.set(c.code_len() as isize);
                        c.emit(Instruction::CreateArgsRestStack(params_count as isize));
                        Ok(())
                    }),
                    rest.idx0(),
                );
                c.emit_assign(rest, init, &|c, target, init| {
                    c.emit_pattern_lexical_assign(&target, &init)
                })?;
            }
            if first_forward_ref != -1 {
                let bs = c.scopes[s].bindings.clone();
                for b in bs {
                    c.bindings[b].in_stash = true;
                }
                c.scopes[s].args_in_stash = true;
                c.scopes[s].need_stash = true;
            }

            c.new_block_scope();
            let var_scope = c.cur_scope();
            c.scopes[var_scope].variable = true;
            enter_func2_mark = Some(c.code_len());
            c.emit(Instruction::Placeholder);
            c.compile_decl_list(self.declaration_list, false)?;
            c.create_function_bindings(&funcs)?;
            c.compile_lexical_declarations_func_body(body, callee_binding)?;
            let vbs = c.scopes[var_scope].bindings.clone();
            for b in vbs {
                if c.bindings[b].is_var {
                    let name = c.bindings[b].name.clone();
                    if let Some(&parent_binding) = c.scopes[s].bound_names.get(&name)
                        && Some(parent_binding) != callee_binding
                    {
                        c.b_emit_get(parent_binding);
                        c.b_emit_set_p(b);
                    }
                }
            }
        } else {
            let bs: Vec<BindingId> = c.scopes[s].bindings[..params_count].to_vec();
            for b in bs {
                c.bindings[b].is_var = true;
            }
            c.compile_decl_list(self.declaration_list, true)?;
            c.create_function_bindings(&funcs)?;
            c.compile_lexical_declarations(body, true)?;
            if self.is_expr
                && let Some(n) = self.name
            {
                let (b, created) = c.bind_name_lexical(s, &n.name, false, 0)?;
                if created {
                    c.bindings[b].is_const = true;
                    callee_binding = Some(b);
                }
            }
            if let Some(cb) = callee_binding {
                c.emit(Instruction::LoadCallee);
                c.b_emit_init_p(cb);
            }
        }

        c.compile_functions(&funcs)?;
        if self.is_generator {
            c.emit(Instruction::YieldMarker(YieldMarker::YieldEmpty));
        }
        match self.body {
            FuncBody::Stmts(list) => c.compile_statements(list, false)?,
            FuncBody::Expr(e) => c.compile_return_parts(0, Some(e))?,
        }

        let last_is_return = match self.body {
            FuncBody::Stmts(list) => {
                matches!(list.last(), Some(ast::Statement::ReturnStatement(_)))
            }
            FuncBody::Expr(_) => true,
        };
        if !last_is_return {
            if self.typ == FuncType::DerivedCtor {
                c.emit(Instruction::LoadUndef);
                c.mark_access_point(this_binding.unwrap_or_default());
                c.emit(Instruction::Ret);
            } else {
                c.emit(Instruction::LoadUndef);
                c.emit(Instruction::Ret);
            }
        }

        let mut delta: usize = 0;

        if c.is_dynamic(s) && !c.scopes[s].args_in_stash {
            c.move_args_to_stash(s);
        }

        if c.scopes[s].args_needed
            || c.is_dynamic(s) && self.typ != FuncType::Arrow && self.typ != FuncType::ClsInit
        {
            if self.typ == FuncType::ClsInit {
                return c.throw_syntax_error(
                    offset,
                    "'arguments' is not allowed in class field initializer or static initialization block",
                );
            }
            let (b, created) =
                c.bind_name_lexical(s, &unistring::String::new("arguments"), false, 0)?;
            if created || c.bindings[b].is_var {
                if !c.scopes[s].args_in_stash {
                    c.move_args_to_stash(s);
                }
                if c.scopes[s].strict {
                    c.bindings[b].is_const = true;
                } else {
                    let cur = c.cur_scope();
                    c.bindings[b].is_var = c.is_function(cur);
                }
                let mut pos = preamble_len - 2;
                delta += 2;
                if c.scopes[s].strict || has_patterns || has_inits {
                    c.set_code(pos, Instruction::CreateArgsUnmapped(params_count as u32));
                } else {
                    c.set_code(pos, Instruction::CreateArgsMapped(params_count as u32));
                }
                pos += 1;
                c.b_emit_init_p_at_scope(b, s, pos);
            }
        }

        if let Some(cb) = callee_binding {
            if !c.is_dynamic(s) && c.b_use_count(cb) == 0 {
                c.delete_binding(s, cb);
            } else {
                delta += 1;
                c.b_emit_init_p_at_scope(cb, s, preamble_len - delta);
                delta += 1;
                c.set_code(preamble_len - delta, Instruction::LoadCallee);
            }
        }

        let mut need_init_this = false;
        if let Some(tb) = this_binding {
            if !c.is_dynamic(s) && c.b_use_count(tb) == 0 {
                c.delete_binding(s, tb);
                this_binding = None;
            } else if c.bindings[tb].in_stash || c.is_dynamic(s) {
                delta += 1;
                c.b_emit_init_at_scope(tb, s, preamble_len - delta);
                need_init_this = true;
            }
        }

        let (stash_size, stack_size) = c.finalise_var_alloc(s, 0)?;

        if need_init_this
            && (!c.scopes[s].args_in_stash || first_forward_ref != -1 || stack_size > 0)
        {
            let pos = preamble_len - delta;
            let idx = match &c.progs[c.p].code[pos] {
                Instruction::InitStash(i) => *i,
                other => panic!(
                    "interface conversion: goja.instruction is {}, not goja.initStash",
                    other.go_type_name()
                ),
            };
            c.set_code(pos, Instruction::InitStashP(idx));
            delta += 1;
            c.set_code(preamble_len - delta, Instruction::LoadStack(0));
        }

        if !c.scopes[s].strict && this_binding.is_some() {
            delta += 1;
            c.set_code(preamble_len - delta, Instruction::BoxThis);
        }
        delta += 1;
        let delta = preamble_len - delta;
        let enter = if stash_size > 0 || c.scopes[s].args_in_stash {
            let ins = if first_forward_ref == -1 {
                let mut enter1 = EnterFunc {
                    names: None,
                    stash_size,
                    stack_size: stack_size as u32,
                    num_args: params_count as u32,
                    func_type: self.typ,
                    args_to_stash: c.scopes[s].args_in_stash,
                    extensible: c.scopes[s].dynamic,
                };
                if c.is_dynamic(s) {
                    enter1.names = c.make_names_map(s);
                }
                if let Some(m) = enter_func2_mark {
                    let mut ef2 = EnterFuncBody {
                        enter_block: EnterBlock::default(),
                        func_type: self.typ,
                        extensible: c.sc().dynamic,
                        adjust_stack: false,
                    };
                    c.update_enter_block(&mut ef2.enter_block);
                    c.set_code(m, Instruction::EnterFuncBody(Box::new(ef2)));
                }
                Instruction::EnterFunc(Box::new(enter1))
            } else {
                let mut enter1 = EnterFunc1 {
                    names: None,
                    stash_size,
                    num_args: params_count as u32,
                    args_to_copy: first_forward_ref as u32,
                    func_type: self.typ,
                    extensible: c.scopes[s].dynamic,
                };
                if c.is_dynamic(s) {
                    enter1.names = c.make_names_map(s);
                }
                if let Some(m) = enter_func2_mark {
                    let mut ef2 = EnterFuncBody {
                        enter_block: EnterBlock::default(),
                        func_type: self.typ,
                        extensible: c.sc().dynamic,
                        adjust_stack: true,
                    };
                    c.update_enter_block(&mut ef2.enter_block);
                    c.set_code(m, Instruction::EnterFuncBody(Box::new(ef2)));
                }
                Instruction::EnterFunc1(Box::new(enter1))
            };
            if emit_args_rest_mark.get() != -1 && c.scopes[s].args_in_stash {
                c.set_code(
                    emit_args_rest_mark.get() as usize,
                    Instruction::CreateArgsRestStash,
                );
            }
            ins
        } else {
            if let Some(m) = enter_func2_mark {
                let mut ef2 = EnterFuncBody {
                    enter_block: EnterBlock::default(),
                    func_type: self.typ,
                    extensible: c.sc().dynamic,
                    adjust_stack: false,
                };
                c.update_enter_block(&mut ef2.enter_block);
                c.set_code(m, Instruction::EnterFuncBody(Box::new(ef2)));
            }
            Instruction::EnterFuncStashless(EnterFuncStashless {
                stack_size: stack_size as u32,
                args: params_count as u32,
            })
        };
        c.set_code(delta, enter);
        let p = c.p;
        c.progs[p].src_map[0].pc = delta as isize;
        c.scope_trim_code(s, delta);
        Ok((length, c.scopes[s].strict, enter_func2_mark))
    }

    fn emit_getter_named(
        &self,
        c: &mut Compiler,
        offset: isize,
        put_on_stack: bool,
        lhs_override: Option<&unistring::String>,
    ) -> CResult<()> {
        let (p, _, name, length, strict) = self.compile(c, offset, lhs_override)?;
        let nf = NewFunc {
            prg: p,
            name,
            source: Arc::from(self.source),
            length,
            strict,
        };
        match self.typ {
            FuncType::Arrow => {
                if self.is_async {
                    c.emit(Instruction::NewAsyncArrowFunc(Box::new(nf)));
                } else {
                    c.emit(Instruction::NewArrowFunc(Box::new(nf)));
                }
            }
            FuncType::Method | FuncType::ClsInit => {
                let nm = Box::new(NewMethod {
                    new_func: nf,
                    home_obj_offset: self.home_obj_offset,
                });
                if self.is_async {
                    c.emit(Instruction::NewAsyncMethod(nm));
                } else if self.is_generator {
                    c.emit(Instruction::NewGeneratorMethod(nm));
                } else {
                    c.emit(Instruction::NewMethod(nm));
                }
            }
            FuncType::Regular => {
                if self.is_async {
                    c.emit(Instruction::NewAsyncFunc(Box::new(nf)));
                } else if self.is_generator {
                    c.emit(Instruction::NewGeneratorFunc(Box::new(nf)));
                } else {
                    c.emit(Instruction::NewFunc(Box::new(nf)));
                }
            }
            _ => {
                return c.throw_syntax_error(
                    offset,
                    format!("Unsupported func type: {}", self.typ as u8),
                );
            }
        }
        if !put_on_stack {
            c.emit(Instruction::Pop);
        }
        Ok(())
    }
}

use crate::compiler::Program;

struct ClsElement<'a> {
    key: unistring::String,
    private_name: Option<PrivateName>,
    initializer: Option<CompiledExpr<'a>>,
    body: Option<CompiledExpr<'a>>,
    computed: bool,
}

impl<'a> ClassLiteralExpr<'a> {
    fn process_class_key(
        &self,
        c: &mut Compiler,
        expr: &'a ast::Expression,
    ) -> CResult<(Option<PrivateName>, unistring::String, bool)> {
        if let ast::Expression::PrivateIdentifier(p) = expr {
            let pn = c.get_declared_private_id(&p.identifier.name)?;
            return Ok((Some(pn), private_id_string(&p.identifier.name), false));
        }
        let (key, computed) = c.process_key(expr)?;
        Ok((None, key, computed))
    }

    fn emit_getter(
        &self,
        c: &mut Compiler,
        offset: isize,
        put_on_stack: bool,
        lhs_override: Option<&unistring::String>,
    ) -> CResult<()> {
        let add_src_map = |c: &mut Compiler| {
            if offset >= 0 {
                c.add_src_map(offset);
            }
        };
        c.new_block_scope();
        let s = c.cur_scope();
        c.scopes[s].strict = true;

        let mark0 = c.code_len();
        c.emit(Instruction::EnterBlock(Box::default()));
        let ob = c.block;
        c.push_block(Block::new(BlockType::Scope, ob));
        let mut cls_binding = None;
        let cls_name = match self.name {
            Some(name) => {
                cls_binding =
                    Some(c.create_lexical_id_binding(&name.name, true, name.idx as isize - 1)?);
                name.name.clone()
            }
            None => lhs_override
                .cloned()
                .unwrap_or_else(|| self.lhs_name.clone()),
        };

        let mut ctor_method: Option<&'a ast::MethodDefinition> = None;
        let mut ctor_method_idx: Option<usize> = None;
        let mut statics_count = 0;
        let mut instance_fields_count = 0;
        let mut has_static_private_methods = false;
        c.class_scopes.push(crate::compiler::ClassScope {
            outer: c.class_scope,
            ..Default::default()
        });
        let cs = c.class_scopes.len() - 1;

        for (idx, elt) in self.body.iter().enumerate() {
            match elt {
                ast::ClassElement::ClassStaticBlock(elt) => {
                    if !elt.block.list.is_empty() {
                        statics_count += 1;
                    }
                }
                ast::ClassElement::FieldDefinition(elt) => {
                    if let ast::Expression::PrivateIdentifier(id) = &elt.key {
                        c.declare_private_id(
                            cs,
                            &id.identifier.name,
                            ast::PropertyKind::Value,
                            elt.static_,
                            elt.idx as isize - 1,
                        )?;
                    }
                    if elt.static_ {
                        statics_count += 1;
                    } else {
                        instance_fields_count += 1;
                    }
                }
                ast::ClassElement::MethodDefinition(elt) => {
                    if !elt.static_
                        && let ast::Expression::StringLiteral(id) = &elt.key
                        && !elt.computed
                        && id.value == "constructor"
                    {
                        if ctor_method.is_some() {
                            return c.throw_syntax_error(
                                id.idx as isize - 1,
                                "A class may only have one constructor",
                            );
                        }
                        ctor_method = Some(elt);
                        ctor_method_idx = Some(idx);
                        continue;
                    }
                    if let ast::Expression::PrivateIdentifier(id) = &elt.key {
                        c.declare_private_id(
                            cs,
                            &id.identifier.name,
                            elt.kind,
                            elt.static_,
                            elt.idx as isize - 1,
                        )?;
                        if elt.static_ {
                            has_static_private_methods = true;
                        }
                    }
                }
            }
        }

        let mut static_init_pc = None;
        if statics_count > 0 || has_static_private_methods {
            static_init_pc = Some(c.code_len());
            c.emit(Instruction::NewStaticFieldInit(Box::default()));
        }

        let new_class = NewClass {
            ctor: None,
            name: cls_name,
            source: Arc::from(self.source),
            init_fields: None,
            private_fields: Vec::new(),
            private_methods: Vec::new(),
            num_private_fields: 0,
            num_private_methods: 0,
            length: 0,
            has_private_env: false,
        };
        let derived;
        let new_class_pc;
        if let Some(super_class) = &self.super_class {
            derived = true;
            super_class.emit_getter(c, true)?;
            add_src_map(c);
            new_class_pc = c.code_len();
            c.emit(Instruction::NewDerivedClass(Box::new(new_class)));
        } else {
            derived = false;
            add_src_map(c);
            new_class_pc = c.code_len();
            c.emit(Instruction::NewClass(Box::new(new_class)));
        }

        c.class_scope = Some(cs);

        let with_new_class = |c: &mut Compiler, f: &mut dyn FnMut(&mut NewClass)| match &mut c.progs
            [c.p]
            .code[new_class_pc]
        {
            Instruction::NewClass(nc) | Instruction::NewDerivedClass(nc) => f(nc),
            _ => {}
        };

        if let Some(ctor) = ctor_method {
            let (p, length) = c.compile_ctor(&ctor.body, derived)?;
            with_new_class(c, &mut |nc| {
                nc.ctor = Some(p.clone());
                nc.length = length;
            });
        }

        let mut cur_is_prototype = false;
        let mut instance_fields: Vec<ClsElement<'a>> = Vec::with_capacity(instance_fields_count);
        let mut static_elements: Vec<ClsElement<'a>> = Vec::with_capacity(statics_count);

        for (idx, elt) in self.body.iter().enumerate() {
            if Some(idx) == ctor_method_idx {
                continue;
            }
            match elt {
                ast::ClassElement::ClassStaticBlock(elt) => {
                    if !elt.block.list.is_empty() {
                        let mut f = c.compile_function_literal_parts(
                            elt.static_,
                            None,
                            &EMPTY_PARAMETER_LIST,
                            &elt.block,
                            &elt.source,
                            &elt.declaration_list,
                            false,
                            false,
                            true,
                        )?;
                        if let ExprKind::FunctionLiteral(fl) = &mut f.kind {
                            fl.typ = FuncType::ClsInit;
                            fl.home_obj_offset = 1;
                        }
                        static_elements.push(ClsElement {
                            key: unistring::String::default(),
                            private_name: None,
                            initializer: None,
                            body: Some(f),
                            computed: false,
                        });
                    }
                }
                ast::ClassElement::FieldDefinition(elt) => {
                    let (private_name, key, computed) = self.process_class_key(c, &elt.key)?;
                    let mut el = ClsElement {
                        key: unistring::String::default(),
                        private_name: None,
                        initializer: None,
                        body: None,
                        computed,
                    };
                    if let Some(init) = &elt.initializer {
                        el.initializer = Some(c.compile_expression(init)?);
                    }
                    if computed {
                        let off = match (elt.static_, cur_is_prototype) {
                            (true, true) => 5,
                            (true, false) => 4,
                            (false, true) => 3,
                            (false, false) => 2,
                        };
                        c.emit(Instruction::DefineComputedKey(off));
                    } else {
                        el.private_name = private_name;
                        el.key = key;
                    }
                    if elt.static_ {
                        static_elements.push(el);
                    } else {
                        instance_fields.push(el);
                    }
                }
                ast::ClassElement::MethodDefinition(elt) => {
                    if elt.static_ {
                        if cur_is_prototype {
                            c.emit(Instruction::Pop);
                            cur_is_prototype = false;
                        }
                    } else if !cur_is_prototype {
                        c.emit(Instruction::DupN(1));
                        cur_is_prototype = true;
                    }
                    let (private_name, key, computed) = self.process_class_key(c, &elt.key)?;
                    let mut lit = c.compile_function_literal(&elt.body, true)?;
                    if let ExprKind::FunctionLiteral(fl) = &mut lit.kind {
                        fl.typ = FuncType::Method;
                        if computed {
                            c.emit(Instruction::ToPropertyKey);
                            fl.home_obj_offset = 2;
                        } else {
                            fl.home_obj_offset = 1;
                            fl.lhs_name = key.clone();
                        }
                    }
                    lit.emit_getter(c, true)?;
                    if let Some(pn) = private_name {
                        let target_offset = match (elt.static_, cur_is_prototype) {
                            (true, true) => 5,
                            (true, false) => 4,
                            (false, true) => 3,
                            (false, false) => 2,
                        };
                        let dpm = DefinePrivateMethod {
                            idx: pn.idx,
                            target_offset,
                        };
                        c.emit(match elt.kind {
                            ast::PropertyKind::Get => Instruction::DefinePrivateGetter(dpm),
                            ast::PropertyKind::Set => Instruction::DefinePrivateSetter(dpm),
                            _ => Instruction::DefinePrivateMethod(dpm),
                        });
                    } else if computed {
                        c.emit(match elt.kind {
                            ast::PropertyKind::Get => {
                                Instruction::DefineGetter(DefineMethod::default())
                            }
                            ast::PropertyKind::Set => {
                                Instruction::DefineSetter(DefineMethod::default())
                            }
                            _ => Instruction::DefineMethod(DefineMethod::default()),
                        });
                    } else {
                        let dk = Box::new(DefineKeyed {
                            key,
                            enumerable: false,
                        });
                        c.emit(match elt.kind {
                            ast::PropertyKind::Get => Instruction::DefineGetterKeyed(dk),
                            ast::PropertyKind::Set => Instruction::DefineSetterKeyed(dk),
                            _ => Instruction::DefineMethodKeyed(dk),
                        });
                    }
                }
            }
        }
        if cur_is_prototype {
            c.emit(Instruction::Pop);
        }

        if !instance_fields.is_empty() {
            let p = self.compile_fields_and_static_blocks(
                c,
                offset,
                &instance_fields,
                unistring::String::new("<instance_members_initializer>"),
            )?;
            with_new_class(c, &mut |nc| nc.init_fields = Some(p.clone()));
        }
        if let Some(pc) = static_init_pc
            && !static_elements.is_empty()
        {
            let p = self.compile_fields_and_static_blocks(
                c,
                offset,
                &static_elements,
                unistring::String::new("<static_initializer>"),
            )?;
            if let Instruction::NewStaticFieldInit(si) = &mut c.progs[c.p].code[pc] {
                si.init_fields = Some(p);
            }
        }

        let env = c.class_scopes[cs].instance_env.clone();
        let dyn_lookup = c.scopes[s].dyn_lookup;
        let has_private_env = !c.class_scopes[cs].private_names.is_empty();
        with_new_class(c, &mut |nc| {
            if dyn_lookup {
                nc.private_methods = env.methods.clone();
                nc.private_fields = env.fields.clone();
            }
            nc.num_private_methods = env.methods.len() as u32;
            nc.num_private_fields = env.fields.len() as u32;
            nc.has_private_env = has_private_env;
        });

        if cls_binding.is_some_and(|b| c.b_use_count(b) > 0) || c.scopes[s].dyn_lookup {
            if let Some(b) = cls_binding {
                c.b_move_to_stash(b);
                c.b_emit_init(b);
            }
        } else {
            if let Some(b) = cls_binding {
                c.delete_binding(s, b);
                cls_binding = None;
            }
            c.set_code(mark0, Instruction::Jump(1));
        }

        if statics_count > 0 || has_static_private_methods {
            let ise_pc = c.code_len();
            c.emit(Instruction::InitStaticElements(Box::default()));
            let env = c.class_scopes[cs].static_env.clone();
            if let Some(pc) = static_init_pc
                && let Instruction::NewStaticFieldInit(si) = &mut c.progs[c.p].code[pc]
            {
                si.num_private_fields = env.fields.len() as u32;
                si.num_private_methods = env.methods.len() as u32;
            }
            if c.scopes[s].dyn_lookup {
                c.set_code(
                    ise_pc,
                    Instruction::InitStaticElements(Box::new(InitStaticElements {
                        private_fields: env.fields,
                        private_methods: env.methods,
                    })),
                );
            }
        } else {
            c.emit(Instruction::EndVariadic);
        }

        if !put_on_stack {
            c.emit(Instruction::Pop);
        }

        if cls_binding.is_some() || c.scopes[s].dyn_lookup {
            c.leave_scope_block(mark0)?;
            let stack_size = match &c.progs[c.p].code[mark0] {
                Instruction::EnterBlock(e) => e.stack_size,
                _ => 0,
            };
            c.assert(
                stack_size == 0,
                offset,
                "enter.StackSize != 0 in compiledClassLiteral",
            )?;
        } else {
            let b = c.block.expect("compiler block");
            c.block = c.blocks[b].outer;
        }
        if !c.class_scopes[cs].private_names.is_empty() {
            c.emit(Instruction::PopPrivateEnv);
        }
        c.class_scope = c.class_scopes[cs].outer;
        c.pop_scope();
        Ok(())
    }

    fn compile_fields_and_static_blocks(
        &self,
        c: &mut Compiler,
        offset: isize,
        elements: &[ClsElement<'a>],
        func_name: unistring::String,
    ) -> CResult<Arc<Program>> {
        let saved_prg = c.p;
        let saved_block = c.block;
        c.push_block(Block::new(BlockType::Scope, None));
        c.p = c.new_prog(func_name, vec![Instruction::Placeholder; 2], Vec::new());
        let r = self.compile_fields_body(c, offset, elements);
        c.p = saved_prg;
        c.block = saved_block;
        r
    }

    fn compile_fields_body(
        &self,
        c: &mut Compiler,
        offset: isize,
        elements: &[ClsElement<'a>],
    ) -> CResult<Arc<Program>> {
        c.new_scope();
        let s = c.cur_scope();
        c.scopes[s].func_type = FuncType::ClsInit;
        let this_binding = c.create_this_binding(s)?;
        let mut val_idx: isize = 0;
        for elt in elements {
            if let Some(body) = &elt.body {
                c.emit(Instruction::Dup);
                body.emit_getter(c, true)?;
                body.add_src_map(c);
                c.emit(Instruction::Call(0));
                c.emit(Instruction::Pop);
            } else {
                if elt.computed {
                    c.emit(Instruction::LoadComputedKey(val_idx));
                    val_idx += 1;
                }
                match &elt.initializer {
                    Some(init) => {
                        if !elt.computed {
                            c.emit_named_or_const(init, &elt.key)?;
                        } else {
                            c.emit_expr(init, true)?;
                        }
                    }
                    None => c.emit(Instruction::LoadUndef),
                }
                if let Some(pn) = &elt.private_name {
                    c.emit(Instruction::DefinePrivateProp(DefinePrivateProp {
                        idx: pn.idx,
                    }));
                } else if elt.computed {
                    c.emit(Instruction::DefineProp);
                } else {
                    c.emit(Instruction::DefinePropKeyed(elt.key.clone()));
                }
            }
        }
        if c.is_dynamic(s) || c.b_use_count(this_binding) > 0 {
            if c.is_dynamic(s) || c.bindings[this_binding].in_stash {
                c.b_emit_init_at(this_binding, 1);
            }
        } else {
            c.delete_binding(s, this_binding);
        }
        let (stash_size, stack_size) = c.finalise_var_alloc(s, 0)?;
        c.assert(stack_size == 0, offset, "stackSize != 0 in initFields")?;
        if stash_size > 0 {
            c.assert(stash_size == 1, offset, "stashSize != 1 in initFields")?;
            let mut enter = EnterFunc {
                names: None,
                stash_size: 1,
                stack_size: 0,
                num_args: 0,
                func_type: FuncType::ClsInit,
                args_to_stash: false,
                extensible: false,
            };
            if c.scopes[s].dyn_lookup {
                enter.names = c.make_names_map(s);
            }
            c.set_code(0, Instruction::EnterFunc(Box::new(enter)));
            c.scope_trim_code(s, 0);
        } else {
            c.scope_trim_code(s, 2);
        }
        let res = c.progs[c.p].handle.clone();
        c.pop_scope();
        Ok(res)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PatternRef<'a> {
    Object(&'a ast::ObjectPattern),
    Array(&'a ast::ArrayPattern),
}

impl<'a> PatternRef<'a> {
    pub(crate) fn of_target(t: &'a ast::BindingTarget) -> PatternRef<'a> {
        match t {
            ast::BindingTarget::ObjectPattern(p) => PatternRef::Object(p),
            ast::BindingTarget::ArrayPattern(p) => PatternRef::Array(p),
            _ => unreachable!("binding target is not a pattern"),
        }
    }

    pub(crate) fn of_expr(e: &'a ast::Expression) -> Option<PatternRef<'a>> {
        match e {
            ast::Expression::ObjectPattern(p) => Some(PatternRef::Object(p)),
            ast::Expression::ArrayPattern(p) => Some(PatternRef::Array(p)),
            _ => None,
        }
    }
}

impl Compiler {
    pub(crate) fn compile_expression<'a>(
        &mut self,
        v: &'a ast::Expression,
    ) -> CResult<CompiledExpr<'a>> {
        use ast::Expression as E;
        match v {
            E::AssignExpression(v) => self.compile_assign_expression(v),
            E::NumberLiteral(v) => self.compile_number_literal(v),
            E::StringLiteral(v) => Ok(ce(
                v.idx,
                ExprKind::Literal(Value::String(string_value_from_raw(&v.value))),
            )),
            E::TemplateLiteral(v) => self.compile_template_literal(v),
            E::BooleanLiteral(v) => Ok(ce(v.idx, ExprKind::Literal(Value::Bool(v.value)))),
            E::NullLiteral(v) => Ok(ce(v.idx, ExprKind::Literal(Value::Null))),
            E::Identifier(v) => self.compile_identifier_expression(v),
            E::CallExpression(v) => self.compile_call_expression(v),
            E::ObjectLiteral(v) => Ok(ce(v.left_brace, ExprKind::ObjectLiteral(v))),
            E::ArrayLiteral(v) => Ok(ce(v.left_bracket, ExprKind::ArrayLiteral(v))),
            E::RegExpLiteral(v) => Ok(ce(v.idx, ExprKind::RegexpLiteral(v))),
            E::BinaryExpression(v) => self.compile_binary_expression(v),
            E::UnaryExpression(v) => {
                let operand = self.compile_expression(&v.operand)?;
                let idx0 = if v.postfix { v.operand.idx0() } else { v.idx };
                Ok(ce(
                    idx0,
                    ExprKind::Unary(Box::new(operand), v.operator, v.postfix),
                ))
            }
            E::ConditionalExpression(v) => {
                let test = self.compile_expression(&v.test)?;
                let consequent = self.compile_expression(&v.consequent)?;
                let alternate = self.compile_expression(&v.alternate)?;
                Ok(ce(
                    v.test.idx0(),
                    ExprKind::Conditional(Box::new((test, consequent, alternate))),
                ))
            }
            E::FunctionLiteral(v) => self.compile_function_literal(v, true),
            E::ArrowFunctionLiteral(v) => self.compile_arrow_function_literal(v),
            E::ClassLiteral(v) => self.compile_class_literal(v),
            E::DotExpression(v) => self.compile_dot_expression(v),
            E::PrivateDotExpression(v) => {
                let left = self.compile_expression(&v.left)?;
                Ok(ce(
                    v.identifier.identifier.idx,
                    ExprKind::PrivateDot(Box::new(left), v.identifier.identifier.name.clone()),
                ))
            }
            E::BracketExpression(v) => self.compile_bracket_expression(v),
            E::ThisExpression(v) => Ok(ce(v.idx, ExprKind::This)),
            E::SuperExpression(v) => {
                self.throw_syntax_error(v.idx as isize - 1, "'super' keyword unexpected here")
            }
            E::SequenceExpression(v) => {
                let mut s = Vec::with_capacity(v.sequence.len());
                for expr in &v.sequence {
                    s.push(self.compile_expression(expr)?);
                }
                let idx = if v.sequence.is_empty() {
                    0
                } else {
                    v.sequence[0].idx0()
                };
                Ok(ce(idx, ExprKind::Sequence(s)))
            }
            E::NewExpression(v) => {
                let (args, is_variadic) = self.compile_call_args(&v.argument_list)?;
                let callee = self.compile_expression(&v.callee)?;
                Ok(ce(
                    v.new,
                    ExprKind::New(Box::new(CallExpr {
                        args,
                        callee,
                        is_variadic,
                    })),
                ))
            }
            E::MetaProperty(v) => {
                if v.meta.name == "new" || v.property.name != "target" {
                    return Ok(ce(v.idx, ExprKind::NewTarget));
                }
                let mut m = b"Unsupported meta property: ".to_vec();
                m.extend_from_slice(&go_bytes(&v.meta.name));
                m.push(b'.');
                m.extend_from_slice(&go_bytes(&v.property.name));
                self.throw_syntax_error(v.idx as isize - 1, m)
            }
            E::ObjectPattern(v) => Ok(ce(v.left_brace, ExprKind::ObjectAssignmentPattern(v))),
            E::ArrayPattern(v) => Ok(ce(v.left_bracket, ExprKind::ArrayAssignmentPattern(v))),
            E::OptionalChain(v) => {
                let expr = self.compile_expression(&v.expression)?;
                Ok(ce(
                    v.expression.idx0(),
                    ExprKind::OptionalChain(Box::new(expr)),
                ))
            }
            E::Optional(v) => {
                let expr = self.compile_expression(&v.expression)?;
                Ok(ce(v.expression.idx0(), ExprKind::Optional(Box::new(expr))))
            }
            E::AwaitExpression(v) => {
                let arg = self.compile_expression(&v.argument)?;
                Ok(ce(v.await_, ExprKind::Await(Box::new(arg))))
            }
            E::YieldExpression(v) => {
                let arg = match &v.argument {
                    Some(a) => Some(Box::new(self.compile_expression(a)?)),
                    None => None,
                };
                Ok(ce(v.yield_, ExprKind::Yield(arg, v.delegate)))
            }
            _ => {
                self.assert(
                    false,
                    v.idx0() as isize - 1,
                    format!("Unknown expression type: {}", expression_go_type(v)),
                )?;
                unreachable!()
            }
        }
    }

    pub(crate) fn compile_expression_opt<'a>(
        &mut self,
        v: Option<&'a ast::Expression>,
    ) -> CResult<Option<CompiledExpr<'a>>> {
        match v {
            Some(v) => Ok(Some(self.compile_expression(v)?)),
            None => Ok(None),
        }
    }

    pub(crate) fn emit_var_ref(
        &mut self,
        name: &unistring::String,
        offset: isize,
        b: Option<BindingId>,
    ) -> CResult<()> {
        if self.strict() {
            self.check_identifier_l_name(name, offset)?;
        }
        match b {
            Some(b) => {
                let strict = self.strict();
                self.b_emit_resolve_var(b, strict);
            }
            None => {
                if self.strict() {
                    self.emit(Instruction::ResolveVar1Strict(name.clone()));
                } else {
                    self.emit(Instruction::ResolveVar1(name.clone()));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn check_super_base(&self, idx: Idx) -> CResult<()> {
        if let Some(s) = self.nearest_this(self.cur_scope()) {
            match self.scopes[s].func_type {
                FuncType::Method | FuncType::ClsInit | FuncType::Ctor | FuncType::DerivedCtor => {
                    return Ok(());
                }
                _ => {}
            }
        }
        self.throw_syntax_error(idx as isize - 1, "'super' keyword unexpected here")
    }

    fn compile_dot_expression<'a>(
        &mut self,
        v: &'a ast::DotExpression,
    ) -> CResult<CompiledExpr<'a>> {
        if let ast::Expression::SuperExpression(sup) = &v.left {
            self.check_super_base(sup.idx)?;
            return Ok(ce(
                v.identifier.idx,
                ExprKind::SuperDot(v.identifier.name.clone()),
            ));
        }
        let left = self.compile_expression(&v.left)?;
        Ok(ce(
            v.identifier.idx,
            ExprKind::Dot(Box::new(left), v.identifier.name.clone()),
        ))
    }

    pub(crate) fn check_constant_string(
        &mut self,
        expr: &CompiledExpr<'_>,
    ) -> CResult<Option<unistring::String>> {
        if expr.constant(self)?
            && let Ok(Value::String(s)) = self.eval_const(expr)?
        {
            return Ok(Some(s.string()));
        }
        Ok(None)
    }

    fn compile_bracket_expression<'a>(
        &mut self,
        v: &'a ast::BracketExpression,
    ) -> CResult<CompiledExpr<'a>> {
        if let ast::Expression::SuperExpression(sup) = &v.left {
            self.check_super_base(sup.idx)?;
            let member = self.compile_expression(&v.member)?;
            if let Some(name) = self.check_constant_string(&member)? {
                return Ok(ce(v.left_bracket, ExprKind::SuperDot(name)));
            }
            return Ok(ce(v.left_bracket, ExprKind::SuperBracket(Box::new(member))));
        }
        let left = self.compile_expression(&v.left)?;
        let member = self.compile_expression(&v.member)?;
        if let Some(name) = self.check_constant_string(&member)? {
            return Ok(ce(v.left_bracket, ExprKind::Dot(Box::new(left), name)));
        }
        Ok(ce(
            v.left_bracket,
            ExprKind::Bracket(Box::new((left, member))),
        ))
    }

    pub(crate) fn compile_parameter_binding_identifier(
        &mut self,
        name: &unistring::String,
        offset: isize,
    ) -> CResult<(BindingId, bool)> {
        if self.strict() {
            self.check_identifier_name(name, offset)?;
            self.check_identifier_l_name(name, offset)?;
        }
        let s = self.cur_scope();
        Ok(self.bind_name_shadow(s, name))
    }

    fn compile_parameter_pattern_id_binding(
        &mut self,
        name: &unistring::String,
        offset: isize,
    ) -> CResult<()> {
        let (_, unique) = self.compile_parameter_binding_identifier(name, offset)?;
        if !unique {
            return self.throw_syntax_error(
                offset,
                "Duplicate parameter name not allowed in this context",
            );
        }
        Ok(())
    }

    pub(crate) fn compile_parameter_pattern_binding(&mut self, item: TargetRef<'_>) -> CResult<()> {
        self.create_bindings(item, &mut |c, name, offset| {
            c.compile_parameter_pattern_id_binding(name, offset)
        })
    }

    pub(crate) fn compile_function_literal<'a>(
        &mut self,
        v: &'a ast::FunctionLiteral,
        is_expr: bool,
    ) -> CResult<CompiledExpr<'a>> {
        self.compile_function_literal_parts(
            v.function,
            v.name.as_deref(),
            &v.parameter_list,
            &v.body,
            &v.source,
            &v.declaration_list,
            v.async_,
            v.generator,
            is_expr,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_function_literal_parts<'a>(
        &mut self,
        function: Idx,
        name: Option<&'a ast::Identifier>,
        parameter_list: &'a ast::ParameterList,
        body: &'a ast::BlockStatement,
        source: &'a [u8],
        declaration_list: &'a [ast::VariableDeclaration],
        is_async: bool,
        is_generator: bool,
        is_expr: bool,
    ) -> CResult<CompiledExpr<'a>> {
        let strict_body = self.is_strict_statement(body);
        if let Some(n) = name
            && (self.strict() || strict_body.is_some())
        {
            self.check_identifier_name(&n.name, n.idx as isize - 1)?;
            self.check_identifier_l_name(&n.name, n.idx as isize - 1)?;
        }
        if is_async && is_generator {
            return self.throw_syntax_error(
                function as isize - 1,
                "Async generators are not supported yet",
            );
        }
        Ok(ce(
            function,
            ExprKind::FunctionLiteral(Box::new(FunctionLiteralExpr {
                name,
                parameter_list,
                body: FuncBody::Stmts(&body.list),
                source,
                declaration_list,
                lhs_name: unistring::String::default(),
                strict: strict_body,
                home_obj_offset: 0,
                typ: FuncType::Regular,
                is_expr,
                is_async,
                is_generator,
            })),
        ))
    }

    fn compile_class_literal<'a>(&mut self, v: &'a ast::ClassLiteral) -> CResult<CompiledExpr<'a>> {
        if let Some(n) = &v.name {
            self.check_identifier_l_name(&n.name, n.idx as isize - 1)?;
        }
        let super_class = self.compile_expression_opt(v.super_class.as_ref())?;
        Ok(ce(
            v.class,
            ExprKind::ClassLiteral(Box::new(ClassLiteralExpr {
                name: v.name.as_deref(),
                super_class,
                body: &v.body,
                lhs_name: unistring::String::default(),
                source: &v.source,
            })),
        ))
    }

    pub(crate) fn compile_class_literal_expr<'a>(
        &mut self,
        v: &'a ast::ClassLiteral,
    ) -> CResult<CompiledExpr<'a>> {
        self.compile_class_literal(v)
    }

    fn compile_ctor(
        &mut self,
        ctor: &ast::FunctionLiteral,
        derived: bool,
    ) -> CResult<(Arc<Program>, isize)> {
        let f = self.compile_function_literal(ctor, true)?;
        let offset = f.offset;
        let ExprKind::FunctionLiteral(mut fl) = f.kind else {
            unreachable!()
        };
        fl.typ = if derived {
            FuncType::DerivedCtor
        } else {
            FuncType::Ctor
        };
        let (p, _, _, length, _) = fl.compile(self, offset, None)?;
        Ok((p, length))
    }

    fn compile_arrow_function_literal<'a>(
        &mut self,
        v: &'a ast::ArrowFunctionLiteral,
    ) -> CResult<CompiledExpr<'a>> {
        let (strict_body, body) = match &v.body {
            ast::ConciseBody::BlockStatement(b) => {
                (self.is_strict_statement(b), FuncBody::Stmts(&b.list))
            }
            ast::ConciseBody::ExpressionBody(b) => (None, FuncBody::Expr(&b.expression)),
        };
        Ok(ce(
            v.start,
            ExprKind::FunctionLiteral(Box::new(FunctionLiteralExpr {
                name: None,
                parameter_list: &v.parameter_list,
                body,
                source: &v.source,
                declaration_list: &v.declaration_list,
                lhs_name: unistring::String::default(),
                strict: strict_body,
                home_obj_offset: 0,
                typ: FuncType::Arrow,
                is_expr: true,
                is_async: v.async_,
                is_generator: false,
            })),
        ))
    }

    pub(crate) fn emit_load_this(&mut self) {
        let s = self.cur_scope();
        let (b, eval) = self.lookup_this(s);
        match b {
            Some(b) => self.b_emit_get(b),
            None => {
                if eval {
                    self.emit(Instruction::GetThisDynamic);
                } else {
                    self.emit(Instruction::LoadGlobalObject);
                }
            }
        }
    }

    fn compile_call_args<'a>(
        &mut self,
        list: &'a [ast::Expression],
    ) -> CResult<(Vec<CompiledExpr<'a>>, bool)> {
        let mut args = Vec::with_capacity(list.len());
        let mut is_variadic = false;
        for arg_expr in list {
            if let ast::Expression::SpreadElement(spread) = arg_expr {
                args.push(self.compile_spread_call_argument(spread)?);
                is_variadic = true;
            } else {
                args.push(self.compile_expression(arg_expr)?);
            }
        }
        Ok((args, is_variadic))
    }

    pub(crate) fn emit_const(
        &mut self,
        expr: &CompiledExpr<'_>,
        put_on_stack: bool,
    ) -> CResult<()> {
        match self.eval_const(expr)? {
            Ok(v) => {
                if put_on_stack {
                    self.emit_literal_value(v);
                }
                Ok(())
            }
            Err(ex) => self.emit_throw(&ex),
        }
    }

    fn compile_binary_expression<'a>(
        &mut self,
        v: &'a ast::BinaryExpression,
    ) -> CResult<CompiledExpr<'a>> {
        let idx0 = v.left.idx0();
        match v.operator {
            token::LOGICAL_OR | token::COALESCE | token::LOGICAL_AND => {
                let left = self.compile_expression(&v.left)?;
                let right = self.compile_expression(&v.right)?;
                let lr = Box::new((left, right));
                let kind = match v.operator {
                    token::LOGICAL_OR => ExprKind::LogicalOr(lr),
                    token::COALESCE => ExprKind::Coalesce(lr),
                    _ => ExprKind::LogicalAnd(lr),
                };
                return Ok(ce(idx0, kind));
            }
            _ => {}
        }
        if let ast::Expression::PrivateIdentifier(id) = &v.left {
            let right = self.compile_expression(&v.right)?;
            return Ok(ce(
                id.identifier.idx,
                ExprKind::PrivateIn(id.identifier.name.clone(), Box::new(right)),
            ));
        }
        let left = self.compile_expression(&v.left)?;
        let right = self.compile_expression(&v.right)?;
        Ok(ce(
            idx0,
            ExprKind::Binary(Box::new((left, right)), v.operator),
        ))
    }

    pub(crate) fn process_key(
        &mut self,
        expr: &ast::Expression,
    ) -> CResult<(unistring::String, bool)> {
        let key_expr = self.compile_expression(expr)?;
        if key_expr.constant(self)?
            && let Ok(v) = self.eval_const(&key_expr)?
        {
            return Ok((v.prim_string(), false));
        }
        key_expr.emit_getter(self, true)?;
        Ok((unistring::String::default(), true))
    }

    pub(crate) fn emit_callee(&mut self, callee: &CompiledExpr<'_>) -> CResult<unistring::String> {
        let mut callee_name = unistring::String::default();
        match &callee.kind {
            ExprKind::Dot(left, name) => {
                left.emit_getter(self, true)?;
                self.emit(Instruction::GetPropCallee(name.clone()));
            }
            ExprKind::PrivateDot(left, name) => {
                left.emit_getter(self, true)?;
                let (rn, id) = self.resolve_private_name(name, callee.offset)?;
                emit_private(
                    self,
                    rn,
                    id,
                    Instruction::GetPrivatePropResCallee,
                    Instruction::GetPrivatePropIdCallee,
                );
            }
            ExprKind::SuperDot(name) => {
                self.emit_load_this();
                self.emit(Instruction::LoadSuper);
                self.emit(Instruction::GetPropRecvCallee(name.clone()));
            }
            ExprKind::Bracket(lm) => {
                lm.0.emit_getter(self, true)?;
                lm.1.emit_getter(self, true)?;
                self.emit(Instruction::GetElemCallee);
            }
            ExprKind::SuperBracket(member) => {
                self.emit_load_this();
                self.emit(Instruction::LoadSuper);
                member.emit_getter(self, true)?;
                self.emit(Instruction::GetElemRecvCallee);
            }
            ExprKind::Identifier(name) => {
                callee_name = name.clone();
                callee.identifier_emit_getter_and_callee(self, name)?;
            }
            ExprKind::OptionalChain(expr) => {
                self.start_opt_chain();
                self.emit_callee(expr)?;
                self.end_opt_chain();
            }
            ExprKind::Optional(expr) => {
                self.emit_callee(expr)?;
                let pc = self.code_len();
                self.blk_mut().conts.push(pc);
                self.emit(Instruction::Placeholder);
            }
            ExprKind::Super => {}
            _ => {
                self.emit(Instruction::LoadUndef);
                callee.emit_getter(self, true)?;
            }
        }
        Ok(callee_name)
    }

    fn compile_spread_call_argument<'a>(
        &mut self,
        spread: &'a ast::SpreadElement,
    ) -> CResult<CompiledExpr<'a>> {
        let expr = self.compile_expression(&spread.expression)?;
        Ok(ce(
            spread.expression.idx0(),
            ExprKind::SpreadCallArgument(Box::new(expr)),
        ))
    }

    fn compile_callee<'a>(&mut self, v: &'a ast::Expression) -> CResult<CompiledExpr<'a>> {
        if let ast::Expression::SuperExpression(sup) = v {
            if let Some(s) = self.nearest_this(self.cur_scope())
                && self.scopes[s].func_type == FuncType::DerivedCtor
            {
                return Ok(ce(sup.idx, ExprKind::Super));
            }
            return self
                .throw_syntax_error(v.idx0() as isize - 1, "'super' keyword unexpected here");
        }
        self.compile_expression(v)
    }

    fn compile_call_expression<'a>(
        &mut self,
        v: &'a ast::CallExpression,
    ) -> CResult<CompiledExpr<'a>> {
        let mut args = Vec::with_capacity(v.argument_list.len());
        let mut is_variadic = false;
        for arg_expr in &v.argument_list {
            if let ast::Expression::SpreadElement(spread) = arg_expr {
                args.push(self.compile_spread_call_argument(spread)?);
                is_variadic = true;
            } else {
                args.push(self.compile_expression(arg_expr)?);
            }
        }
        let callee = self.compile_callee(&v.callee)?;
        Ok(ce(
            v.left_parenthesis,
            ExprKind::Call(Box::new(CallExpr {
                args,
                callee,
                is_variadic,
            })),
        ))
    }

    pub(crate) fn compile_identifier_expression<'a>(
        &mut self,
        v: &'a ast::Identifier,
    ) -> CResult<CompiledExpr<'a>> {
        if self.strict() {
            self.check_identifier_name(&v.name, v.idx as isize - 1)?;
        }
        Ok(ce(v.idx, ExprKind::Identifier(v.name.clone())))
    }

    fn compile_number_literal<'a>(
        &mut self,
        v: &'a ast::NumberLiteral,
    ) -> CResult<CompiledExpr<'a>> {
        let lit = &v.literal;
        if self.strict() && lit.len() > 1 && lit[0] == b'0' && lit[1] <= b'7' && lit[1] >= b'0' {
            return self.throw_syntax_error(
                v.idx as isize - 1,
                "Octal literals are not allowed in strict mode",
            );
        }
        let val = match &v.value {
            ast::NumberValue::Int64(n) => int_to_value(*n),
            ast::NumberValue::Float64(f) => float_to_value(*f),
            ast::NumberValue::BigInt(b) => new_big_int((**b).clone()),
            ast::NumberValue::Int(_) => {
                self.assert(
                    false,
                    v.idx as isize - 1,
                    "Unsupported number literal type: int",
                )?;
                unreachable!()
            }
        };
        Ok(ce(v.idx, ExprKind::Literal(val)))
    }

    fn compile_template_literal<'a>(
        &mut self,
        v: &'a ast::TemplateLiteral,
    ) -> CResult<CompiledExpr<'a>> {
        let tag = self.compile_expression_opt(v.tag.as_ref())?;
        let mut expressions = Vec::with_capacity(v.expressions.len());
        for expr in &v.expressions {
            expressions.push(self.compile_expression(expr)?);
        }
        Ok(ce(
            v.open_quote,
            ExprKind::TemplateLiteral(Box::new(TemplateLiteralExpr {
                tag,
                elements: &v.elements,
                expressions,
            })),
        ))
    }

    fn compile_assign_expression<'a>(
        &mut self,
        v: &'a ast::AssignExpression,
    ) -> CResult<CompiledExpr<'a>> {
        let left = self.compile_expression(&v.left)?;
        let right = self.compile_expression(&v.right)?;
        Ok(ce(
            v.left.idx0(),
            ExprKind::Assign(Box::new((left, right)), v.operator),
        ))
    }

    pub(crate) fn emit_expr(&mut self, expr: &CompiledExpr<'_>, put_on_stack: bool) -> CResult<()> {
        if expr.constant(self)? {
            self.emit_const(expr, put_on_stack)
        } else {
            expr.emit_getter(self, put_on_stack)
        }
    }

    pub(crate) fn emit_named(
        &mut self,
        expr: &CompiledExpr<'_>,
        name: &unistring::String,
    ) -> CResult<()> {
        if expr.is_named_emitter() {
            expr.emit_named(self, name)
        } else {
            expr.emit_getter(self, true)
        }
    }

    pub(crate) fn emit_named_or_const(
        &mut self,
        expr: &CompiledExpr<'_>,
        name: &unistring::String,
    ) -> CResult<()> {
        if expr.constant(self)? {
            self.emit_const(expr, true)
        } else {
            self.emit_named(expr, name)
        }
    }

    pub(crate) fn emit_pattern<'a>(
        &mut self,
        pattern: PatternRef<'a>,
        emitter: AssignEmitter<'_, 'a>,
        put_on_stack: bool,
    ) -> CResult<()> {
        match pattern {
            PatternRef::Object(p) => self.emit_object_pattern(p, emitter, put_on_stack),
            PatternRef::Array(p) => self.emit_array_pattern(p, emitter, put_on_stack),
        }
    }

    pub(crate) fn emit_assign<'a>(
        &mut self,
        target: &'a ast::Expression,
        init: CompiledExpr<'a>,
        emit_assign_simple: AssignEmitter<'_, 'a>,
    ) -> CResult<()> {
        if let Some(pattern) = PatternRef::of_expr(target) {
            init.emit_getter(self, true)?;
            self.emit_pattern(pattern, emit_assign_simple, false)
        } else {
            let t = self.compile_expression(target)?;
            emit_assign_simple(self, t, init)
        }
    }

    pub(crate) fn emit_object_pattern<'a>(
        &mut self,
        pattern: &'a ast::ObjectPattern,
        emit_assign: AssignEmitter<'_, 'a>,
        put_on_stack: bool,
    ) -> CResult<()> {
        if pattern.rest.is_some() {
            self.emit(Instruction::CreateDestructSrc);
        } else {
            self.emit(Instruction::CheckObjectCoercible);
        }
        for prop in &pattern.properties {
            match prop {
                ast::Property::PropertyShort(prop) => {
                    self.emit(Instruction::Dup);
                    let target = self.compile_identifier_expression(&prop.name)?;
                    let name = &prop.name.name;
                    let init = self.compile_pattern_init_expr(
                        Box::new(move |c: &mut Compiler| {
                            c.emit(Instruction::GetProp(name.clone()));
                            Ok(())
                        }),
                        prop.initializer.as_ref(),
                        prop.name.idx,
                    )?;
                    emit_assign(self, target, init)?;
                }
                ast::Property::PropertyKeyed(prop) => {
                    self.emit(Instruction::Dup);
                    self.compile_expression(&prop.key)?
                        .emit_getter(self, true)?;
                    self.emit(Instruction::ToPropertyKey);
                    let (target, initializer) = match &prop.value {
                        ast::Expression::AssignExpression(e) => (&e.left, Some(&e.right)),
                        v => (v, None),
                    };
                    let init = self.compile_pattern_init_expr(
                        Box::new(|c: &mut Compiler| {
                            c.emit(Instruction::GetKey);
                            Ok(())
                        }),
                        initializer,
                        prop.key.idx0(),
                    )?;
                    self.emit_assign(target, init, emit_assign)?;
                }
                ast::Property::SpreadElement(s) => {
                    return self.throw_syntax_error(
                        s.expression.idx0() as isize - 1,
                        format!(
                            "Unsupported AssignmentProperty type: {}",
                            property_go_type(prop)
                        ),
                    );
                }
            }
        }
        if let Some(rest) = &pattern.rest {
            let target = self.compile_expression(rest)?;
            let init = self.compile_emitter_expr(
                Box::new(|c: &mut Compiler| {
                    c.emit(Instruction::CopyRest);
                    Ok(())
                }),
                rest.idx0(),
            );
            emit_assign(self, target, init)?;
            self.emit(Instruction::Pop);
        }
        if !put_on_stack {
            self.emit(Instruction::Pop);
        }
        Ok(())
    }

    pub(crate) fn emit_array_pattern<'a>(
        &mut self,
        pattern: &'a ast::ArrayPattern,
        emit_assign: AssignEmitter<'_, 'a>,
        put_on_stack: bool,
    ) -> CResult<()> {
        self.emit(Instruction::Iterate);
        for elt in &pattern.elements {
            match elt {
                None => {
                    self.emit(Instruction::IterGetNextOrUndef);
                    self.emit(Instruction::Pop);
                }
                Some(ast::Expression::AssignExpression(elt)) => {
                    let init = self.compile_pattern_init_expr(
                        Box::new(|c: &mut Compiler| {
                            c.emit(Instruction::IterGetNextOrUndef);
                            Ok(())
                        }),
                        Some(&elt.right),
                        elt.left.idx0(),
                    )?;
                    self.emit_assign(&elt.left, init, emit_assign)?;
                }
                Some(elt) => {
                    let init = self.compile_emitter_expr(
                        Box::new(|c: &mut Compiler| {
                            c.emit(Instruction::IterGetNextOrUndef);
                            Ok(())
                        }),
                        elt.idx0(),
                    );
                    self.emit_assign(elt, init, emit_assign)?;
                }
            }
        }
        if let Some(rest) = &pattern.rest {
            let init = self.compile_emitter_expr(
                Box::new(|c: &mut Compiler| {
                    c.emit(Instruction::NewArrayFromIter);
                    Ok(())
                }),
                rest.idx0(),
            );
            self.emit_assign(rest, init, emit_assign)?;
        } else {
            self.emit(Instruction::EnumPopClose);
        }
        if !put_on_stack {
            self.emit(Instruction::Pop);
        }
        Ok(())
    }

    pub(crate) fn compile_pattern_init_expr<'a>(
        &mut self,
        emit_src: Emitter<'a>,
        def: Option<&'a ast::Expression>,
        idx: Idx,
    ) -> CResult<CompiledExpr<'a>> {
        let def = match def {
            Some(d) => Some(Box::new(self.compile_expression(d)?)),
            None => None,
        };
        Ok(ce(idx, ExprKind::PatternInit(emit_src, def)))
    }

    pub(crate) fn compile_emitter_expr<'a>(
        &mut self,
        emitter: Emitter<'a>,
        idx: Idx,
    ) -> CompiledExpr<'a> {
        ce(idx, ExprKind::EmitterExpr(Some(emitter), None))
    }

    pub(crate) fn start_opt_chain(&mut self) {
        let ob = self.block;
        self.push_block(Block::new(BlockType::OptChain, ob));
    }

    pub(crate) fn end_opt_chain(&mut self) {
        let lbl = self.code_len();
        let b = self.block.expect("compiler block");
        let breaks = self.blocks[b].breaks.clone();
        for item in breaks {
            self.set_code(item, Instruction::Jopt((lbl - item) as i32));
        }
        let conts = self.blocks[b].conts.clone();
        for item in conts {
            self.set_code(item, Instruction::Joptc((lbl - item) as i32));
        }
        self.block = self.blocks[b].outer;
    }
}
