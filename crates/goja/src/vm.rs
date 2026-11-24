use crate::builtin_bigint::string_to_big_int;
use crate::builtin_math::{go_mod, pow};
use crate::builtin_symbol::SYM_UNSCOPABLES;
use crate::compiler::{MASK_CONST, MASK_DELETABLE, MASK_STRICT, MASK_TYP, MASK_VAR};
use crate::compiler::{Program, ResolvedPrivateName, VarType};
use crate::compiler_expr::FuncType;
use crate::deps::big;
use crate::func::BaseJsFuncObject;
use crate::func::YieldMarker;
use crate::heap::{FxHashMap, ObjRef, StashRef, Tracer};
use crate::object::PrivateId;
use crate::object::{FunctionCall, IterNextFunc, ObjExt, PrivateEnv, PrivateEnvType};
use crate::regexp::RegexpPattern;
use crate::runtime::{
    Exception, InterruptedError, IteratorRecord, JsResult, Runtime, StackFrame, Throw,
};
use crate::string::{JsString, ascii_string, new_string_value};
use crate::unistring;
use crate::value::{ERR_ACCESS_BEFORE_INIT, ERR_ASSIGN_TO_CONST, nil_safe};
use crate::value::{
    ERR_MIX_BIG_INT_TYPE, Value, ValueError, ValueProperty, float_to_value, int_to_value,
    new_big_int, to_int32, to_numeric, to_uint32,
};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub type NamesMap = Option<Arc<HashMap<unistring::String, u32>>>;

#[derive(Clone, Debug, Default)]
pub struct DefineMethod {
    pub enumerable: bool,
}

#[derive(Clone, Debug)]
pub struct DefineKeyed {
    pub key: unistring::String,
    pub enumerable: bool,
}

#[derive(Clone, Debug)]
pub struct NewRegexp {
    pub pattern: Arc<RegexpPattern>,
    pub src: JsString,
}

#[derive(Clone, Debug)]
pub struct LoadMixed {
    pub name: unistring::String,
    pub idx: u32,
    pub callee: bool,
}

#[derive(Clone, Debug)]
pub struct LoadMixedStack {
    pub name: unistring::String,
    pub idx: isize,
    pub level: u8,
    pub callee: bool,
}

#[derive(Clone, Debug)]
pub struct ResolveMixed {
    pub name: unistring::String,
    pub idx: u32,
    pub typ: VarType,
    pub strict: bool,
}

#[derive(Clone, Debug)]
pub struct ResolveMixedStack {
    pub name: unistring::String,
    pub idx: isize,
    pub typ: VarType,
    pub level: u8,
    pub strict: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EnterBlock {
    pub names: NamesMap,
    pub stash_size: u32,
    pub stack_size: u32,
}

#[derive(Clone, Debug, Default)]
pub struct LeaveBlock {
    pub stack_size: u32,
    pub pop_stash: bool,
}

#[derive(Clone, Debug)]
pub struct EnterFunc {
    pub names: NamesMap,
    pub stash_size: u32,
    pub stack_size: u32,
    pub num_args: u32,
    pub func_type: FuncType,
    pub args_to_stash: bool,
    pub extensible: bool,
}

#[derive(Clone, Debug)]
pub struct EnterFunc1 {
    pub names: NamesMap,
    pub stash_size: u32,
    pub num_args: u32,
    pub args_to_copy: u32,
    pub func_type: FuncType,
    pub extensible: bool,
}

#[derive(Clone, Debug)]
pub struct EnterFuncBody {
    pub enter_block: EnterBlock,
    pub func_type: FuncType,
    pub extensible: bool,
    pub adjust_stack: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EnterFuncStashless {
    pub stack_size: u32,
    pub args: u32,
}

#[derive(Clone, Debug)]
pub struct NewFunc {
    pub prg: Arc<Program>,
    pub name: unistring::String,
    pub source: Arc<[u8]>,
    pub length: isize,
    pub strict: bool,
}

#[derive(Clone, Debug)]
pub struct NewMethod {
    pub new_func: NewFunc,
    pub home_obj_offset: u32,
}

#[derive(Clone, Debug)]
pub struct BindVars {
    pub names: Vec<unistring::String>,
    pub deletable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BindGlobal {
    pub vars: Vec<unistring::String>,
    pub funcs: Vec<unistring::String>,
    pub lets: Vec<unistring::String>,
    pub consts: Vec<unistring::String>,
    pub deletable: bool,
}

#[derive(Clone, Debug)]
pub struct GetTaggedTmplObject {
    pub raw: Vec<ValueProperty>,
    pub cooked: Vec<ValueProperty>,
}

#[derive(Clone, Debug)]
pub struct NewClass {
    pub ctor: Option<Arc<Program>>,
    pub name: unistring::String,
    pub source: Arc<[u8]>,
    pub init_fields: Option<Arc<Program>>,
    pub private_fields: Vec<unistring::String>,
    pub private_methods: Vec<unistring::String>,
    pub num_private_fields: u32,
    pub num_private_methods: u32,
    pub length: isize,
    pub has_private_env: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NewStaticFieldInit {
    pub init_fields: Option<Arc<Program>>,
    pub num_private_fields: u32,
    pub num_private_methods: u32,
}

#[derive(Clone, Debug, Default)]
pub struct InitStaticElements {
    pub private_fields: Vec<unistring::String>,
    pub private_methods: Vec<unistring::String>,
}

#[derive(Clone, Debug, Default)]
pub struct DefinePrivateMethod {
    pub idx: isize,
    pub target_offset: isize,
}

#[derive(Clone, Debug, Default)]
pub struct DefinePrivateProp {
    pub idx: isize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Try {
    pub catch_offset: i32,
    pub finally_offset: i32,
}

#[derive(Clone, Debug)]
pub enum Instruction {
    Placeholder,
    LoadVal(Value),
    LoadUndef,
    LoadNil,
    SaveResult,
    LoadResult,
    ClearResult,
    LoadGlobalObject,
    LoadStack(isize),
    LoadStack1(isize),
    LoadStackLex(isize),
    LoadStack1Lex(isize),
    LoadCallee,
    StoreStack(isize),
    StoreStack1(isize),
    StoreStackLex(isize),
    StoreStack1Lex(isize),
    InitStack(isize),
    InitStackP(isize),
    InitStack1(isize),
    InitStack1P(isize),
    StoreStackP(isize),
    StoreStack1P(isize),
    StoreStackLexP(isize),
    StoreStack1LexP(isize),
    ToNumber,
    Add,
    Sub,
    Mul,
    Exp,
    Div,
    Mod,
    Neg,
    Plus,
    Inc,
    Dec,
    And,
    Or,
    Xor,
    Bnot,
    Sal,
    Sar,
    Shr,
    Jump(i32),
    ToPropertyKey,
    ToString,
    GetElemRef,
    GetElemRefRecv,
    GetElemRefStrict,
    GetElemRefRecvStrict,
    SetElem,
    SetElem1,
    SetElem1Named,
    DefineMethod(DefineMethod),
    SetElemP,
    SetElemStrict,
    SetElemRecv,
    SetElemRecvStrict,
    SetElemStrictP,
    SetElemRecvP,
    SetElemRecvStrictP,
    DeleteElem,
    DeleteElemStrict,
    DeleteProp(unistring::String),
    DeletePropStrict(unistring::String),
    GetPropRef(unistring::String),
    GetPropRefRecv(unistring::String),
    GetPropRefStrict(unistring::String),
    GetPropRefRecvStrict(unistring::String),
    SetProp(unistring::String),
    SetPropP(unistring::String),
    SetPropStrict(unistring::String),
    SetPropRecv(unistring::String),
    SetPropRecvStrict(unistring::String),
    SetPropRecvP(unistring::String),
    SetPropRecvStrictP(unistring::String),
    SetPropStrictP(unistring::String),
    PutProp(unistring::String),
    DefinePropKeyed(unistring::String),
    DefineProp,
    DefineMethodKeyed(Box<DefineKeyed>),
    SetProto,
    DefineGetterKeyed(Box<DefineKeyed>),
    DefineSetterKeyed(Box<DefineKeyed>),
    DefineGetter(DefineMethod),
    DefineSetter(DefineMethod),
    GetProp(unistring::String),
    GetPropRecv(unistring::String),
    GetPropRecvCallee(unistring::String),
    GetPropCallee(unistring::String),
    GetElem,
    GetElemRecv,
    GetKey,
    GetElemCallee,
    GetElemRecvCallee,
    Dup,
    DupN(u32),
    RdupN(u32),
    DupLast(u32),
    NewObject,
    NewArray(u32),
    PushArrayItem,
    PushArraySpread,
    PushSpread,
    NewArrayFromIter,
    NewRegexp(Box<NewRegexp>),
    StoreStash(u32),
    StoreStashP(u32),
    StoreStashLex(u32),
    StoreStashLexP(u32),
    InitStash(u32),
    InitStashP(u32),
    InitGlobalP(unistring::String),
    InitGlobal(unistring::String),
    ResolveVar1(unistring::String),
    DeleteVar(unistring::String),
    DeleteGlobal(unistring::String),
    ResolveVar1Strict(unistring::String),
    SetGlobal(unistring::String),
    SetGlobalStrict(unistring::String),
    LoadStash(u32),
    LoadStashLex(u32),
    LoadMixed(Box<LoadMixed>),
    LoadMixedLex(Box<LoadMixed>),
    LoadMixedStack(Box<LoadMixedStack>),
    LoadMixedStack1(Box<LoadMixedStack>),
    LoadMixedStackLex(Box<LoadMixedStack>),
    LoadMixedStack1Lex(Box<LoadMixedStack>),
    ResolveMixed(Box<ResolveMixed>),
    ResolveMixedStack(Box<ResolveMixedStack>),
    ResolveMixedStack1(Box<ResolveMixedStack>),
    GetValue,
    PutValue,
    PopRef,
    PutValueP,
    InitValueP,
    LoadDynamic(unistring::String),
    LoadDynamicRef(unistring::String),
    LoadDynamicCallee(unistring::String),
    Pop,
    CallEval(u32),
    CallEvalStrict(u32),
    CallEvalVariadic,
    CallEvalVariadicStrict,
    BoxThis,
    StartVariadic,
    CallVariadic,
    EndVariadic,
    Call(u32),
    EnterBlock(Box<EnterBlock>),
    EnterCatchBlock(Box<EnterBlock>),
    LeaveBlock(LeaveBlock),
    EnterFunc(Box<EnterFunc>),
    EnterFunc1(Box<EnterFunc1>),
    EnterFuncBody(Box<EnterFuncBody>),
    Ret,
    Cret(u32),
    EnterFuncStashless(EnterFuncStashless),
    NewFunc(Box<NewFunc>),
    NewAsyncFunc(Box<NewFunc>),
    NewGeneratorFunc(Box<NewFunc>),
    NewMethod(Box<NewMethod>),
    NewAsyncMethod(Box<NewMethod>),
    NewGeneratorMethod(Box<NewMethod>),
    NewArrowFunc(Box<NewFunc>),
    NewAsyncArrowFunc(Box<NewFunc>),
    BindVars(Box<BindVars>),
    BindGlobal(Box<BindGlobal>),
    JneP(i32),
    JeqP(i32),
    Jeq(i32),
    Jne(i32),
    Jdef(i32),
    JdefP(i32),
    Jopt(i32),
    Joptc(i32),
    Jcoalesc(i32),
    JcoalescP(i32),
    Not,
    OpLt,
    OpLte,
    OpGt,
    OpGte,
    OpEq,
    OpNeq,
    OpStrictEq,
    OpStrictNeq,
    OpInstanceof,
    OpIn,
    Try(Try),
    LeaveTry,
    EnterFinally,
    LeaveFinally,
    Throw,
    NewVariadic,
    New(u32),
    SuperCall(u32),
    SuperCallVariadic,
    LoadNewTarget,
    Typeof,
    CreateArgsMapped(u32),
    CreateArgsUnmapped(u32),
    EnterWith,
    LeaveWith,
    Enumerate,
    EnumNext(i32),
    EnumGet,
    EnumPop,
    EnumPopClose,
    IterateP,
    Iterate,
    IterNext(i32),
    IterGetNextOrUndef,
    CopyStash,
    ThrowAssignToConst,
    CopySpread,
    CopyRest,
    CreateDestructSrc,
    CheckObjectCoercible,
    CreateArgsRestStack(isize),
    CreateArgsRestStash,
    ConcatStrings(isize),
    GetTaggedTmplObject(Box<GetTaggedTmplObject>),
    LoadSuper,
    NewClass(Box<NewClass>),
    NewDerivedClass(Box<NewClass>),
    NewStaticFieldInit(Box<NewStaticFieldInit>),
    LoadThisStash(u32),
    LoadThisStack,
    GetThisDynamic,
    ThrowConst(ValueError),
    ResolveThisStack,
    ResolveThisStash(u32),
    ResolveThisDynamic,
    DefineComputedKey(isize),
    LoadComputedKey(isize),
    InitStaticElements(Box<InitStaticElements>),
    DefinePrivateMethod(DefinePrivateMethod),
    DefinePrivateGetter(DefinePrivateMethod),
    DefinePrivateSetter(DefinePrivateMethod),
    DefinePrivateProp(DefinePrivateProp),
    GetPrivatePropRes(Box<ResolvedPrivateName>),
    GetPrivatePropId(Box<PrivateId>),
    GetPrivatePropIdCallee(Box<PrivateId>),
    GetPrivatePropResCallee(Box<ResolvedPrivateName>),
    SetPrivatePropRes(Box<ResolvedPrivateName>),
    SetPrivatePropResP(Box<ResolvedPrivateName>),
    SetPrivatePropId(Box<PrivateId>),
    SetPrivatePropIdP(Box<PrivateId>),
    PopPrivateEnv,
    PrivateInRes(Box<ResolvedPrivateName>),
    PrivateInId(Box<PrivateId>),
    GetPrivateRefRes(Box<ResolvedPrivateName>),
    GetPrivateRefId(Box<PrivateId>),
    YieldMarker(YieldMarker),
}

impl Instruction {
    pub fn go_type_name(&self) -> &'static str {
        match self {
            Instruction::Placeholder => "<nil>",
            Instruction::LoadVal(_) => "goja.loadVal",
            Instruction::LoadUndef => "goja._loadUndef",
            Instruction::LoadNil => "goja._loadNil",
            Instruction::SaveResult => "goja._saveResult",
            Instruction::LoadResult => "goja._loadResult",
            Instruction::ClearResult => "goja._clearResult",
            Instruction::LoadGlobalObject => "goja._loadGlobalObject",
            Instruction::LoadStack(_) => "goja.loadStack",
            Instruction::LoadStack1(_) => "goja.loadStack1",
            Instruction::LoadStackLex(_) => "goja.loadStackLex",
            Instruction::LoadStack1Lex(_) => "goja.loadStack1Lex",
            Instruction::LoadCallee => "goja._loadCallee",
            Instruction::StoreStack(_) => "goja.storeStack",
            Instruction::StoreStack1(_) => "goja.storeStack1",
            Instruction::StoreStackLex(_) => "goja.storeStackLex",
            Instruction::StoreStack1Lex(_) => "goja.storeStack1Lex",
            Instruction::InitStack(_) => "goja.initStack",
            Instruction::InitStackP(_) => "goja.initStackP",
            Instruction::InitStack1(_) => "goja.initStack1",
            Instruction::InitStack1P(_) => "goja.initStack1P",
            Instruction::StoreStackP(_) => "goja.storeStackP",
            Instruction::StoreStack1P(_) => "goja.storeStack1P",
            Instruction::StoreStackLexP(_) => "goja.storeStackLexP",
            Instruction::StoreStack1LexP(_) => "goja.storeStack1LexP",
            Instruction::ToNumber => "goja._toNumber",
            Instruction::Add => "goja._add",
            Instruction::Sub => "goja._sub",
            Instruction::Mul => "goja._mul",
            Instruction::Exp => "goja._exp",
            Instruction::Div => "goja._div",
            Instruction::Mod => "goja._mod",
            Instruction::Neg => "goja._neg",
            Instruction::Plus => "goja._plus",
            Instruction::Inc => "goja._inc",
            Instruction::Dec => "goja._dec",
            Instruction::And => "goja._and",
            Instruction::Or => "goja._or",
            Instruction::Xor => "goja._xor",
            Instruction::Bnot => "goja._bnot",
            Instruction::Sal => "goja._sal",
            Instruction::Sar => "goja._sar",
            Instruction::Shr => "goja._shr",
            Instruction::Jump(_) => "goja.jump",
            Instruction::ToPropertyKey => "goja._toPropertyKey",
            Instruction::ToString => "goja._toString",
            Instruction::GetElemRef => "goja._getElemRef",
            Instruction::GetElemRefRecv => "goja._getElemRefRecv",
            Instruction::GetElemRefStrict => "goja._getElemRefStrict",
            Instruction::GetElemRefRecvStrict => "goja._getElemRefRecvStrict",
            Instruction::SetElem => "goja._setElem",
            Instruction::SetElem1 => "goja._setElem1",
            Instruction::SetElem1Named => "goja._setElem1Named",
            Instruction::DefineMethod(_) => "*goja.defineMethod",
            Instruction::SetElemP => "goja._setElemP",
            Instruction::SetElemStrict => "goja._setElemStrict",
            Instruction::SetElemRecv => "goja._setElemRecv",
            Instruction::SetElemRecvStrict => "goja._setElemRecvStrict",
            Instruction::SetElemStrictP => "goja._setElemStrictP",
            Instruction::SetElemRecvP => "goja._setElemRecvP",
            Instruction::SetElemRecvStrictP => "goja._setElemRecvStrictP",
            Instruction::DeleteElem => "goja._deleteElem",
            Instruction::DeleteElemStrict => "goja._deleteElemStrict",
            Instruction::DeleteProp(_) => "goja.deleteProp",
            Instruction::DeletePropStrict(_) => "goja.deletePropStrict",
            Instruction::GetPropRef(_) => "goja.getPropRef",
            Instruction::GetPropRefRecv(_) => "goja.getPropRefRecv",
            Instruction::GetPropRefStrict(_) => "goja.getPropRefStrict",
            Instruction::GetPropRefRecvStrict(_) => "goja.getPropRefRecvStrict",
            Instruction::SetProp(_) => "goja.setProp",
            Instruction::SetPropP(_) => "goja.setPropP",
            Instruction::SetPropStrict(_) => "goja.setPropStrict",
            Instruction::SetPropRecv(_) => "goja.setPropRecv",
            Instruction::SetPropRecvStrict(_) => "goja.setPropRecvStrict",
            Instruction::SetPropRecvP(_) => "goja.setPropRecvP",
            Instruction::SetPropRecvStrictP(_) => "goja.setPropRecvStrictP",
            Instruction::SetPropStrictP(_) => "goja.setPropStrictP",
            Instruction::PutProp(_) => "goja.putProp",
            Instruction::DefinePropKeyed(_) => "goja.definePropKeyed",
            Instruction::DefineProp => "goja.defineProp",
            Instruction::DefineMethodKeyed(_) => "*goja.defineMethodKeyed",
            Instruction::SetProto => "goja._setProto",
            Instruction::DefineGetterKeyed(_) => "*goja.defineGetterKeyed",
            Instruction::DefineSetterKeyed(_) => "*goja.defineSetterKeyed",
            Instruction::DefineGetter(_) => "*goja.defineGetter",
            Instruction::DefineSetter(_) => "*goja.defineSetter",
            Instruction::GetProp(_) => "goja.getProp",
            Instruction::GetPropRecv(_) => "goja.getPropRecv",
            Instruction::GetPropRecvCallee(_) => "goja.getPropRecvCallee",
            Instruction::GetPropCallee(_) => "goja.getPropCallee",
            Instruction::GetElem => "goja._getElem",
            Instruction::GetElemRecv => "goja._getElemRecv",
            Instruction::GetKey => "goja._getKey",
            Instruction::GetElemCallee => "goja._getElemCallee",
            Instruction::GetElemRecvCallee => "goja._getElemRecvCallee",
            Instruction::Dup => "goja._dup",
            Instruction::DupN(_) => "goja.dupN",
            Instruction::RdupN(_) => "goja.rdupN",
            Instruction::DupLast(_) => "goja.dupLast",
            Instruction::NewObject => "goja._newObject",
            Instruction::NewArray(_) => "goja.newArray",
            Instruction::PushArrayItem => "goja._pushArrayItem",
            Instruction::PushArraySpread => "goja._pushArraySpread",
            Instruction::PushSpread => "goja._pushSpread",
            Instruction::NewArrayFromIter => "goja._newArrayFromIter",
            Instruction::NewRegexp(_) => "*goja.newRegexp",
            Instruction::StoreStash(_) => "goja.storeStash",
            Instruction::StoreStashP(_) => "goja.storeStashP",
            Instruction::StoreStashLex(_) => "goja.storeStashLex",
            Instruction::StoreStashLexP(_) => "goja.storeStashLexP",
            Instruction::InitStash(_) => "goja.initStash",
            Instruction::InitStashP(_) => "goja.initStashP",
            Instruction::InitGlobalP(_) => "goja.initGlobalP",
            Instruction::InitGlobal(_) => "goja.initGlobal",
            Instruction::ResolveVar1(_) => "goja.resolveVar1",
            Instruction::DeleteVar(_) => "goja.deleteVar",
            Instruction::DeleteGlobal(_) => "goja.deleteGlobal",
            Instruction::ResolveVar1Strict(_) => "goja.resolveVar1Strict",
            Instruction::SetGlobal(_) => "goja.setGlobal",
            Instruction::SetGlobalStrict(_) => "goja.setGlobalStrict",
            Instruction::LoadStash(_) => "goja.loadStash",
            Instruction::LoadStashLex(_) => "goja.loadStashLex",
            Instruction::LoadMixed(_) => "*goja.loadMixed",
            Instruction::LoadMixedLex(_) => "*goja.loadMixedLex",
            Instruction::LoadMixedStack(_) => "*goja.loadMixedStack",
            Instruction::LoadMixedStack1(_) => "*goja.loadMixedStack1",
            Instruction::LoadMixedStackLex(_) => "*goja.loadMixedStackLex",
            Instruction::LoadMixedStack1Lex(_) => "*goja.loadMixedStack1Lex",
            Instruction::ResolveMixed(_) => "*goja.resolveMixed",
            Instruction::ResolveMixedStack(_) => "*goja.resolveMixedStack",
            Instruction::ResolveMixedStack1(_) => "*goja.resolveMixedStack1",
            Instruction::GetValue => "goja._getValue",
            Instruction::PutValue => "goja._putValue",
            Instruction::PopRef => "goja._popRef",
            Instruction::PutValueP => "goja._putValueP",
            Instruction::InitValueP => "goja._initValueP",
            Instruction::LoadDynamic(_) => "goja.loadDynamic",
            Instruction::LoadDynamicRef(_) => "goja.loadDynamicRef",
            Instruction::LoadDynamicCallee(_) => "goja.loadDynamicCallee",
            Instruction::Pop => "goja._pop",
            Instruction::CallEval(_) => "goja.callEval",
            Instruction::CallEvalStrict(_) => "goja.callEvalStrict",
            Instruction::CallEvalVariadic => "goja._callEvalVariadic",
            Instruction::CallEvalVariadicStrict => "goja._callEvalVariadicStrict",
            Instruction::BoxThis => "goja._boxThis",
            Instruction::StartVariadic => "goja._startVariadic",
            Instruction::CallVariadic => "goja._callVariadic",
            Instruction::EndVariadic => "goja._endVariadic",
            Instruction::Call(_) => "goja.call",
            Instruction::EnterBlock(_) => "*goja.enterBlock",
            Instruction::EnterCatchBlock(_) => "*goja.enterCatchBlock",
            Instruction::LeaveBlock(_) => "*goja.leaveBlock",
            Instruction::EnterFunc(_) => "*goja.enterFunc",
            Instruction::EnterFunc1(_) => "*goja.enterFunc1",
            Instruction::EnterFuncBody(_) => "*goja.enterFuncBody",
            Instruction::Ret => "goja._ret",
            Instruction::Cret(_) => "goja.cret",
            Instruction::EnterFuncStashless(_) => "*goja.enterFuncStashless",
            Instruction::NewFunc(_) => "*goja.newFunc",
            Instruction::NewAsyncFunc(_) => "*goja.newAsyncFunc",
            Instruction::NewGeneratorFunc(_) => "*goja.newGeneratorFunc",
            Instruction::NewMethod(_) => "*goja.newMethod",
            Instruction::NewAsyncMethod(_) => "*goja.newAsyncMethod",
            Instruction::NewGeneratorMethod(_) => "*goja.newGeneratorMethod",
            Instruction::NewArrowFunc(_) => "*goja.newArrowFunc",
            Instruction::NewAsyncArrowFunc(_) => "*goja.newAsyncArrowFunc",
            Instruction::BindVars(_) => "*goja.bindVars",
            Instruction::BindGlobal(_) => "*goja.bindGlobal",
            Instruction::JneP(_) => "goja.jneP",
            Instruction::JeqP(_) => "goja.jeqP",
            Instruction::Jeq(_) => "goja.jeq",
            Instruction::Jne(_) => "goja.jne",
            Instruction::Jdef(_) => "goja.jdef",
            Instruction::JdefP(_) => "goja.jdefP",
            Instruction::Jopt(_) => "goja.jopt",
            Instruction::Joptc(_) => "goja.joptc",
            Instruction::Jcoalesc(_) => "goja.jcoalesc",
            Instruction::JcoalescP(_) => "goja.jcoalescP",
            Instruction::Not => "goja._not",
            Instruction::OpLt => "goja._op_lt",
            Instruction::OpLte => "goja._op_lte",
            Instruction::OpGt => "goja._op_gt",
            Instruction::OpGte => "goja._op_gte",
            Instruction::OpEq => "goja._op_eq",
            Instruction::OpNeq => "goja._op_neq",
            Instruction::OpStrictEq => "goja._op_strict_eq",
            Instruction::OpStrictNeq => "goja._op_strict_neq",
            Instruction::OpInstanceof => "goja._op_instanceof",
            Instruction::OpIn => "goja._op_in",
            Instruction::Try(_) => "goja.try",
            Instruction::LeaveTry => "goja.leaveTry",
            Instruction::EnterFinally => "goja.enterFinally",
            Instruction::LeaveFinally => "goja.leaveFinally",
            Instruction::Throw => "goja._throw",
            Instruction::NewVariadic => "goja._newVariadic",
            Instruction::New(_) => "goja._new",
            Instruction::SuperCall(_) => "goja.superCall",
            Instruction::SuperCallVariadic => "goja._superCallVariadic",
            Instruction::LoadNewTarget => "goja._loadNewTarget",
            Instruction::Typeof => "goja._typeof",
            Instruction::CreateArgsMapped(_) => "goja.createArgsMapped",
            Instruction::CreateArgsUnmapped(_) => "goja.createArgsUnmapped",
            Instruction::EnterWith => "goja._enterWith",
            Instruction::LeaveWith => "goja._leaveWith",
            Instruction::Enumerate => "goja._enumerate",
            Instruction::EnumNext(_) => "goja.enumNext",
            Instruction::EnumGet => "goja._enumGet",
            Instruction::EnumPop => "goja._enumPop",
            Instruction::EnumPopClose => "goja._enumPopClose",
            Instruction::IterateP => "goja._iterateP",
            Instruction::Iterate => "goja._iterate",
            Instruction::IterNext(_) => "goja.iterNext",
            Instruction::IterGetNextOrUndef => "goja.iterGetNextOrUndef",
            Instruction::CopyStash => "goja.copyStash",
            Instruction::ThrowAssignToConst => "goja._throwAssignToConst",
            Instruction::CopySpread => "goja._copySpread",
            Instruction::CopyRest => "goja._copyRest",
            Instruction::CreateDestructSrc => "goja._createDestructSrc",
            Instruction::CheckObjectCoercible => "goja._checkObjectCoercible",
            Instruction::CreateArgsRestStack(_) => "goja.createArgsRestStack",
            Instruction::CreateArgsRestStash => "goja._createArgsRestStash",
            Instruction::ConcatStrings(_) => "goja.concatStrings",
            Instruction::GetTaggedTmplObject(_) => "*goja.getTaggedTmplObject",
            Instruction::LoadSuper => "goja._loadSuper",
            Instruction::NewClass(_) => "*goja.newClass",
            Instruction::NewDerivedClass(_) => "*goja.newDerivedClass",
            Instruction::NewStaticFieldInit(_) => "*goja.newStaticFieldInit",
            Instruction::LoadThisStash(_) => "goja.loadThisStash",
            Instruction::LoadThisStack => "goja.loadThisStack",
            Instruction::GetThisDynamic => "goja.getThisDynamic",
            Instruction::ThrowConst(_) => "goja.throwConst",
            Instruction::ResolveThisStack => "goja.resolveThisStack",
            Instruction::ResolveThisStash(_) => "goja.resolveThisStash",
            Instruction::ResolveThisDynamic => "goja.resolveThisDynamic",
            Instruction::DefineComputedKey(_) => "goja.defineComputedKey",
            Instruction::LoadComputedKey(_) => "goja.loadComputedKey",
            Instruction::InitStaticElements(_) => "*goja.initStaticElements",
            Instruction::DefinePrivateMethod(_) => "*goja.definePrivateMethod",
            Instruction::DefinePrivateGetter(_) => "*goja.definePrivateGetter",
            Instruction::DefinePrivateSetter(_) => "*goja.definePrivateSetter",
            Instruction::DefinePrivateProp(_) => "*goja.definePrivateProp",
            Instruction::GetPrivatePropRes(_) => "*goja.getPrivatePropRes",
            Instruction::GetPrivatePropId(_) => "*goja.getPrivatePropId",
            Instruction::GetPrivatePropIdCallee(_) => "*goja.getPrivatePropIdCallee",
            Instruction::GetPrivatePropResCallee(_) => "*goja.getPrivatePropResCallee",
            Instruction::SetPrivatePropRes(_) => "*goja.setPrivatePropRes",
            Instruction::SetPrivatePropResP(_) => "*goja.setPrivatePropResP",
            Instruction::SetPrivatePropId(_) => "*goja.setPrivatePropId",
            Instruction::SetPrivatePropIdP(_) => "*goja.setPrivatePropIdP",
            Instruction::PopPrivateEnv => "goja.popPrivateEnv",
            Instruction::PrivateInRes(_) => "*goja.privateInRes",
            Instruction::PrivateInId(_) => "*goja.privateInId",
            Instruction::GetPrivateRefRes(_) => "*goja.getPrivateRefRes",
            Instruction::GetPrivateRefId(_) => "*goja.getPrivateRefId",
            Instruction::YieldMarker(_) => "*goja.yieldMarker",
        }
    }
}

fn mix_big_int() -> ValueError {
    ValueError::TypeError(ERR_MIX_BIG_INT_TYPE.to_string())
}

fn range_error(msg: &str) -> ValueError {
    ValueError::RangeError(msg.to_string())
}

pub(crate) fn value_go_string(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.go_string(),
        _ => v.prim_to_string().go_string(),
    }
}

pub(crate) fn op_add(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let is_left_string = matches!(left, Value::String(_));
    let is_right_string = matches!(right, Value::String(_));
    if is_left_string || is_right_string {
        let l = left.prim_to_string();
        let r = right.prim_to_string();
        return Ok(Value::String(l.concat(&r)));
    }
    Ok(match left {
        Value::Int(l) => match right {
            Value::Int(r) => int_to_value(l.wrapping_add(*r)),
            Value::BigInt(_) => return Err(mix_big_int()),
            _ => float_to_value(*l as f64 + right.prim_to_float()),
        },
        Value::BigInt(l) => match right {
            Value::BigInt(r) => new_big_int(l.add(r)),
            _ => return Err(mix_big_int()),
        },
        _ => {
            if let Value::BigInt(_) = right {
                return Err(mix_big_int());
            }
            float_to_value(left.prim_to_float() + right.prim_to_float())
        }
    })
}

pub(crate) fn op_sub(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    match (&left, &right) {
        (Value::Int(l), Value::Int(r)) => return Ok(int_to_value(l.wrapping_sub(*r))),
        (Value::Int(_), Value::BigInt(_)) | (Value::Float(_), Value::BigInt(_)) => {
            return Err(mix_big_int());
        }
        (Value::BigInt(l), Value::BigInt(r)) => return Ok(new_big_int(l.sub(r))),
        (Value::BigInt(_), _) => return Err(mix_big_int()),
        _ => {}
    }
    Ok(float_to_value(left.prim_to_float() - right.prim_to_float()))
}

pub(crate) fn op_mul(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    match (&left, &right) {
        (Value::Int(l), Value::Int(r)) => {
            let (l, r) = (*l, *r);
            if l == 0 && r == -1 || l == -1 && r == 0 {
                return Ok(Value::Float(-0.0));
            }
            let res = l.wrapping_mul(r);
            if l == 0 || r == 0 || res.wrapping_div(l) == r {
                return Ok(int_to_value(res));
            }
        }
        (Value::Int(_), Value::BigInt(_)) | (Value::Float(_), Value::BigInt(_)) => {
            return Err(mix_big_int());
        }
        (Value::BigInt(l), Value::BigInt(r)) => return Ok(new_big_int(l.mul(r))),
        (Value::BigInt(_), _) => return Err(mix_big_int()),
        _ => {}
    }
    Ok(float_to_value(left.prim_to_float() * right.prim_to_float()))
}

pub(crate) fn op_exp(x: &Value, y: &Value) -> Result<Value, ValueError> {
    let x = to_numeric(x)?;
    let y = to_numeric(y)?;
    if let Value::BigInt(x) = &x {
        if let Value::BigInt(y) = &y {
            if y.cmp(&big::Int::new()) < 0 {
                return Err(range_error("exponent must be positive"));
            }
            return Ok(new_big_int(x.exp(y, None).unwrap_or_default()));
        }
        return Err(mix_big_int());
    } else if let Value::BigInt(_) = &y {
        return Err(mix_big_int());
    }
    Ok(pow(&x, &y))
}

pub(crate) fn op_div(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let left_value = to_numeric(left)?;
    let right_value = to_numeric(right)?;
    if let Value::BigInt(l) = &left_value {
        if let Value::BigInt(r) = &right_value {
            if r.cmp(&big::Int::new()) == 0 {
                return Err(range_error("Division by zero"));
            }
            if l.cmp_abs(r) < 0 {
                return Ok(new_big_int(big::Int::new()));
            }
            return Ok(new_big_int(l.quo_rem(r).0));
        }
        return Err(mix_big_int());
    } else if let Value::BigInt(_) = &right_value {
        return Err(mix_big_int());
    }
    let (left, right) = (left_value.prim_to_float(), right_value.prim_to_float());
    if left.is_nan() || right.is_nan() {
        return Ok(Value::Float(f64::NAN));
    }
    if left.is_infinite() && right.is_infinite() {
        return Ok(Value::Float(f64::NAN));
    }
    if left == 0.0 && right == 0.0 {
        return Ok(Value::Float(f64::NAN));
    }
    let same_sign = left.is_sign_negative() == right.is_sign_negative();
    if left.is_infinite() {
        return Ok(Value::Float(if same_sign {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }));
    }
    if right.is_infinite() {
        return Ok(if same_sign {
            Value::Int(0)
        } else {
            Value::Float(-0.0)
        });
    }
    if right == 0.0 {
        return Ok(Value::Float(if same_sign {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }));
    }
    Ok(float_to_value(left / right))
}

pub(crate) fn op_mod(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    match (&left, &right) {
        (Value::Int(l), Value::Int(r)) => {
            if *r == 0 {
                return Ok(Value::Float(f64::NAN));
            }
            let m = l.wrapping_rem(*r);
            if m == 0 && *l < 0 {
                return Ok(Value::Float(-0.0));
            }
            return Ok(int_to_value(m));
        }
        (Value::Int(_), Value::BigInt(_)) | (Value::Float(_), Value::BigInt(_)) => {
            return Err(mix_big_int());
        }
        (Value::BigInt(l), Value::BigInt(r)) => {
            let zero = big::Int::new();
            if r.cmp(&zero) == 0 {
                return Err(range_error("Division by zero"));
            }
            if l.cmp(&zero) < 0 {
                let v = l.abs().modulo(r);
                return Ok(new_big_int(v.neg()));
            }
            return Ok(new_big_int(l.modulo(r)));
        }
        (Value::BigInt(_), _) => return Err(mix_big_int()),
        _ => {}
    }
    Ok(float_to_value(go_mod(
        left.prim_to_float(),
        right.prim_to_float(),
    )))
}

pub(crate) fn op_neg(operand: &Value) -> Result<Value, ValueError> {
    Ok(match to_numeric(operand)? {
        Value::BigInt(n) => new_big_int(n.neg()),
        Value::Int(n) => {
            if n == 0 {
                Value::Float(-0.0)
            } else {
                Value::Int(n.wrapping_neg())
            }
        }
        _ => {
            let mut f = operand.prim_to_float();
            if !f.is_nan() {
                f = -f;
            }
            Value::Float(f)
        }
    })
}

pub(crate) fn op_bitwise(
    left: &Value,
    right: &Value,
    big_op: fn(&big::Int, &big::Int) -> big::Int,
    int_op: fn(i32, i32) -> i32,
) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    if let Value::BigInt(l) = &left {
        if let Value::BigInt(r) = &right {
            return Ok(new_big_int(big_op(l, r)));
        }
        return Err(mix_big_int());
    } else if let Value::BigInt(_) = &right {
        return Err(mix_big_int());
    }
    Ok(int_to_value(i64::from(int_op(
        to_int32(&left)?,
        to_int32(&right)?,
    ))))
}

pub(crate) fn op_bnot(v: &Value) -> Result<Value, ValueError> {
    Ok(match to_numeric(v)? {
        Value::BigInt(n) => new_big_int(n.not()),
        n => int_to_value(i64::from(!to_int32(&n)?)),
    })
}

pub(crate) fn op_shift(
    left: &Value,
    right: &Value,
    arithmetic_right: bool,
) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    if let Value::BigInt(l) = &left {
        if let Value::BigInt(r) = &right {
            let n = r.uint64();
            let shift_right = (r.sign() < 0) != arithmetic_right;
            return Ok(new_big_int(if shift_right { l.rsh(n) } else { l.lsh(n) }));
        }
        return Err(mix_big_int());
    } else if let Value::BigInt(_) = &right {
        return Err(mix_big_int());
    }
    let l = to_int32(&left)?;
    let s = to_uint32(&right)? & 0x1F;
    Ok(int_to_value(i64::from(if arithmetic_right {
        l >> s
    } else {
        l.wrapping_shl(s)
    })))
}

pub(crate) fn op_shr(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    if matches!(left, Value::BigInt(_)) || matches!(right, Value::BigInt(_)) {
        return Err(ValueError::TypeError(
            "BigInts have no unsigned right shift, use >> instead".to_string(),
        ));
    }
    Ok(int_to_value(i64::from(
        to_uint32(&left)? >> (to_uint32(&right)? & 0x1F),
    )))
}

fn big_cmp_float(x: &big::Int, f: f64, x_on_left: bool) -> Option<bool> {
    if f.is_nan() {
        return None;
    }
    let c = big::cmp_float64(x, f);
    Some(if x_on_left { c < 0 } else { c > 0 })
}

pub(crate) fn cmp(px: &Value, py: &Value) -> Result<Option<bool>, ValueError> {
    match (px, py) {
        (Value::String(xs), Value::String(ys)) => return Ok(Some(xs.compare_to(ys) < 0)),
        (Value::BigInt(x), Value::String(ys)) => {
            return Ok(string_to_big_int(&ys.to_trimmed_utf8()).map(|ny| x.cmp(&ny) < 0));
        }
        (Value::String(xs), Value::BigInt(y)) => {
            return Ok(string_to_big_int(&xs.to_trimmed_utf8()).map(|nx| nx.cmp(y) < 0));
        }
        _ => {}
    }
    let px = to_numeric(px)?;
    let py = to_numeric(py)?;
    match (&px, &py) {
        (Value::Int(x), Value::Int(y)) => return Ok(Some(x < y)),
        (Value::Int(x), Value::BigInt(y)) => return Ok(Some(big::Int::new_int(*x).cmp(y) < 0)),
        (Value::Float(x), Value::BigInt(y)) => {
            if *x == f64::NEG_INFINITY {
                return Ok(Some(true));
            }
            return Ok(big_cmp_float(y, *x, false));
        }
        (Value::BigInt(x), Value::Int(y)) => return Ok(Some(x.cmp(&big::Int::new_int(*y)) < 0)),
        (Value::BigInt(x), Value::Float(y)) => {
            if *y == f64::INFINITY {
                return Ok(Some(true));
            }
            return Ok(big_cmp_float(x, *y, true));
        }
        (Value::BigInt(x), Value::BigInt(y)) => return Ok(Some(x.cmp(y) < 0)),
        _ => {}
    }
    let (nx, ny) = (px.prim_to_float(), py.prim_to_float());
    if nx.is_nan() || ny.is_nan() {
        return Ok(None);
    }
    Ok(Some(nx < ny))
}

pub(crate) fn type_of(v: &Value) -> JsString {
    ascii_string(match v {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Bool(_) => "boolean",
        Value::String(_) => "string",
        Value::Int(_) | Value::Float(_) => "number",
        Value::BigInt(_) => "bigint",
        Value::Symbol(_) => "symbol",
        Value::Object(_) => "object",
        _ => "undefined",
    })
}

fn not_an_object(v: &Value) -> ValueError {
    let mut msg = b"Value is not an object: ".to_vec();
    msg.extend_from_slice(&value_go_string(v));
    ValueError::TypeError(String::from_utf8_lossy(&msg).into_owned())
}

#[derive(Clone, Debug)]
pub(crate) struct ConstException {
    pub name: String,
    pub message: Option<Value>,
}

pub(crate) enum ConstThrow {
    Exception(ConstException),
    Unsupported(&'static str),
}

enum Slot {
    Val(Value),
    ErrorCtor(&'static str),
    Error(ConstException),
}

fn exception_of(e: ValueError) -> ConstThrow {
    let (name, msg) = match e {
        ValueError::TypeError(m) => ("TypeError", m),
        ValueError::RangeError(m) => ("RangeError", m),
        ValueError::ReferenceError(m) => ("ReferenceError", m),
        ValueError::SyntaxError(m) => ("SyntaxError", m),
    };
    ConstThrow::Exception(ConstException {
        name: name.to_string(),
        message: Some(Value::String(new_string_value(msg.as_bytes()))),
    })
}

type BinaryOp = fn(&Value, &Value) -> Result<Value, ValueError>;
type UnaryOp = fn(&Value) -> Result<Value, ValueError>;

pub(crate) fn eval_const_code(code: &[Instruction]) -> Result<Value, ConstThrow> {
    let mut stack: Vec<Slot> = Vec::new();
    let mut pc = 0usize;
    fn val(s: Option<Slot>) -> Result<Value, ConstThrow> {
        match s {
            Some(Slot::Val(v)) => Ok(v),
            _ => Err(ConstThrow::Unsupported("non-primitive operand")),
        }
    }
    while pc < code.len() {
        let ins = &code[pc];
        let binary: Option<BinaryOp> = match ins {
            Instruction::Add => Some(op_add),
            Instruction::Sub => Some(op_sub),
            Instruction::Mul => Some(op_mul),
            Instruction::Exp => Some(op_exp),
            Instruction::Div => Some(op_div),
            Instruction::Mod => Some(op_mod),
            Instruction::And => Some(|l, r| op_bitwise(l, r, big::Int::and, |a, b| a & b)),
            Instruction::Or => Some(|l, r| op_bitwise(l, r, big::Int::or, |a, b| a | b)),
            Instruction::Xor => Some(|l, r| op_bitwise(l, r, big::Int::xor, |a, b| a ^ b)),
            Instruction::Sal => Some(|l, r| op_shift(l, r, false)),
            Instruction::Sar => Some(|l, r| op_shift(l, r, true)),
            Instruction::Shr => Some(op_shr),
            Instruction::OpLt => Some(|l, r| Ok(Value::Bool(cmp(l, r)? == Some(true)))),
            Instruction::OpLte => Some(|l, r| Ok(Value::Bool(cmp(r, l)? == Some(false)))),
            Instruction::OpGt => Some(|l, r| Ok(Value::Bool(cmp(r, l)? == Some(true)))),
            Instruction::OpGte => Some(|l, r| Ok(Value::Bool(cmp(l, r)? == Some(false)))),
            Instruction::OpEq => Some(|l, r| Ok(Value::Bool(l.prim_equals(r)))),
            Instruction::OpNeq => Some(|l, r| Ok(Value::Bool(!l.prim_equals(r)))),
            Instruction::OpStrictEq => Some(|l, r| Ok(Value::Bool(l.strict_equals(r)))),
            Instruction::OpStrictNeq => Some(|l, r| Ok(Value::Bool(!l.strict_equals(r)))),
            Instruction::OpInstanceof | Instruction::OpIn => Some(|_, r| Err(not_an_object(r))),
            _ => None,
        };
        if let Some(op) = binary {
            let right = val(stack.pop())?;
            let left = val(stack.pop())?;
            stack.push(Slot::Val(op(&left, &right).map_err(exception_of)?));
            pc += 1;
            continue;
        }
        let unary: Option<UnaryOp> = match ins {
            Instruction::Not => Some(|v| Ok(Value::Bool(!v.to_boolean()))),
            Instruction::Bnot => Some(op_bnot),
            Instruction::Typeof => Some(|v| Ok(Value::String(type_of(v)))),
            Instruction::Neg => Some(op_neg),
            Instruction::Plus => Some(|v| v.prim_to_number()),
            Instruction::ToNumber => Some(to_numeric),
            _ => None,
        };
        if let Some(op) = unary {
            let v = val(stack.pop())?;
            stack.push(Slot::Val(op(&v).map_err(exception_of)?));
            pc += 1;
            continue;
        }
        match ins {
            Instruction::LoadVal(v) => stack.push(Slot::Val(v.clone())),
            Instruction::LoadUndef => stack.push(Slot::Val(Value::Undefined)),
            Instruction::Pop => {
                stack.pop();
            }
            Instruction::LoadDynamic(name) => {
                if *name == "TypeError" {
                    stack.push(Slot::ErrorCtor("TypeError"));
                } else if *name == "RangeError" {
                    stack.push(Slot::ErrorCtor("RangeError"));
                } else {
                    return Err(exception_of(ValueError::ReferenceError(format!(
                        "{name} is not defined"
                    ))));
                }
            }
            Instruction::New(n) => {
                let n = *n as usize;
                if stack.len() < n + 1 {
                    return Err(ConstThrow::Unsupported("stack underflow"));
                }
                let args = stack.split_off(stack.len() - n);
                let ctor = stack.pop();
                let Some(Slot::ErrorCtor(name)) = ctor else {
                    return Err(ConstThrow::Unsupported("not a constructor"));
                };
                let message = match args.into_iter().next() {
                    Some(Slot::Val(Value::Undefined)) | None => None,
                    Some(Slot::Val(v)) => Some(Value::String(v.prim_to_string())),
                    Some(_) => return Err(ConstThrow::Unsupported("object argument")),
                };
                stack.push(Slot::Error(ConstException {
                    name: name.to_string(),
                    message,
                }));
            }
            Instruction::Throw => match stack.pop() {
                Some(Slot::Error(e)) => return Err(ConstThrow::Exception(e)),
                _ => return Err(ConstThrow::Unsupported("throw of a non-error value")),
            },
            _ => return Err(ConstThrow::Unsupported(ins.go_type_name())),
        }
        pc += 1;
    }
    val(stack.pop())
}

pub const TRY_PANIC_MARKER: i32 = -2;

#[derive(Clone, Default)]
pub struct Stash {
    pub values: Vec<Value>,
    pub extra_args: Vec<Value>,
    pub names: Option<FxHashMap<unistring::String, u32>>,
    pub obj: Option<ObjRef>,
    pub outer: Option<StashRef>,
    pub func_type: FuncType,
}

impl Stash {
    pub fn trace(&self, t: &mut Tracer) {
        t.values(&self.values);
        t.values(&self.extra_args);
        t.opt_obj(self.obj);
        if let Some(o) = self.outer {
            t.stash(o);
        }
    }

    pub fn is_variable(&self) -> bool {
        self.func_type != FuncType::None
    }

    pub fn create_binding(&mut self, name: &unistring::String, deletable: bool) {
        let names = self.names.get_or_insert_with(FxHashMap::default);
        if !names.contains_key(name) {
            let mut idx = names.len() as u32 | MASK_VAR;
            if deletable {
                idx |= MASK_DELETABLE;
            }
            names.insert(name.clone(), idx);
            self.values.push(Value::Undefined);
        }
    }

    pub fn create_lex_binding(&mut self, name: &unistring::String, is_const: bool) {
        let names = self.names.get_or_insert_with(FxHashMap::default);
        if !names.contains_key(name) {
            let mut idx = names.len() as u32;
            if is_const {
                idx |= MASK_CONST | MASK_STRICT;
            }
            names.insert(name.clone(), idx);
            self.values.push(Value::Empty);
        }
    }

    pub fn delete_binding(&mut self, name: &unistring::String) {
        if let Some(n) = &mut self.names {
            n.remove(name);
        }
    }

    pub fn init_by_idx(&mut self, idx: u32, v: Value) {
        if self.obj.is_some() {
            panic!("Attempt to init by idx into an object scope");
        }
        self.values[idx as usize] = v;
    }

    pub fn get_by_idx(&self, idx: u32) -> Value {
        self.values[idx as usize].clone()
    }
}

#[derive(Clone, Default)]
pub struct Context {
    pub prg: Option<Arc<Program>>,
    pub stash: Option<StashRef>,
    pub priv_env: Option<Rc<PrivateEnv>>,
    pub new_target: Value,
    pub result: Value,
    pub pc: isize,
    pub sb: isize,
    pub args: isize,
}

impl Context {
    pub fn trace(&self, t: &mut Tracer) {
        if let Some(s) = self.stash {
            t.stash(s);
        }
        t.value(&self.new_target);
        t.value(&self.result);
    }
}

#[derive(Default)]
pub struct TryFrame {
    pub exception: Option<Box<Exception>>,
    pub call_stack_len: u32,
    pub iter_len: u32,
    pub ref_len: u32,
    pub sp: i32,
    pub stash: Option<StashRef>,
    pub priv_env: Option<Rc<PrivateEnv>>,
    pub catch_pos: i32,
    pub finally_pos: i32,
    pub finally_ret: i32,
}

impl TryFrame {
    fn trace(&self, t: &mut Tracer) {
        if let Some(e) = &self.exception {
            t.value(&e.val);
        }
        if let Some(s) = self.stash {
            t.stash(s);
        }
    }
}

#[derive(Default)]
pub struct ExecCtx {
    pub context: Context,
    pub stack: Vec<Value>,
    pub try_stack: Vec<TryFrame>,
    pub iter_stack: Vec<IterStackItem>,
    pub ref_stack: Vec<Ref>,
}

impl ExecCtx {
    pub fn trace(&self, t: &mut Tracer) {
        self.context.trace(t);
        t.values(&self.stack);
        for f in &self.try_stack {
            f.trace(t);
        }
        for i in &self.iter_stack {
            i.trace(t);
        }
        for r in &self.ref_stack {
            r.trace(t);
        }
    }
}

#[derive(Default)]
pub struct IterStackItem {
    pub val: Value,
    pub f: Option<IterNextFunc>,
    pub iter: Option<IteratorRecord>,
}

impl IterStackItem {
    fn trace(&self, t: &mut Tracer) {
        t.value(&self.val);
        if let Some(f) = &self.f {
            f.trace(t);
        }
        if let Some(i) = &self.iter {
            i.trace(t);
        }
    }
}

#[derive(Clone)]
pub enum Ref {
    Stash {
        n: unistring::String,
        stash: StashRef,
        idx: usize,
    },
    StashLex {
        n: unistring::String,
        stash: StashRef,
        idx: usize,
    },
    StashConst {
        n: unistring::String,
        stash: StashRef,
        idx: usize,
        strict_const: bool,
    },
    StackThis {
        idx: usize,
    },
    StashThis {
        stash: StashRef,
        idx: usize,
    },
    Obj {
        base: ObjRef,
        name: Value,
        this: Option<Value>,
        strict: bool,
        name_converted: bool,
    },
    ObjStr {
        base: ObjRef,
        name: unistring::String,
        this: Option<Value>,
        strict: bool,
        binding: bool,
    },
    PrivateRes {
        base: ObjRef,
        name: Box<ResolvedPrivateName>,
    },
    PrivateId {
        base: ObjRef,
        id: Rc<PrivateId>,
    },
    Unresolved {
        name: unistring::String,
    },
}

impl Ref {
    pub fn trace(&self, t: &mut Tracer) {
        match self {
            Ref::Stash { stash, .. }
            | Ref::StashLex { stash, .. }
            | Ref::StashConst { stash, .. }
            | Ref::StashThis { stash, .. } => t.stash(*stash),
            Ref::Obj {
                base, name, this, ..
            } => {
                t.obj(*base);
                t.value(name);
                if let Some(v) = this {
                    t.value(v);
                }
            }
            Ref::ObjStr { base, this, .. } => {
                t.obj(*base);
                if let Some(v) = this {
                    t.value(v);
                }
            }
            Ref::PrivateRes { base, .. } | Ref::PrivateId { base, .. } => t.obj(*base),
            Ref::StackThis { .. } | Ref::Unresolved { .. } => {}
        }
    }

    pub fn get(&mut self, rt: &mut Runtime) -> JsResult<Value> {
        match self {
            Ref::Stash { stash, idx, .. } => {
                Ok(nil_safe(Some(rt.heap.stash(*stash).values[*idx].clone())))
            }
            Ref::StashLex { stash, idx, .. } | Ref::StashConst { stash, idx, .. } => {
                let v = rt.heap.stash(*stash).values[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                Ok(v)
            }
            Ref::StackThis { idx } => {
                let v = rt.vm.stack[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(
                        "Must call super constructor in derived class before accessing 'this'"
                            .to_string(),
                    ));
                }
                Ok(v)
            }
            Ref::StashThis { stash, idx } => {
                let v = rt.heap.stash(*stash).values[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(
                        "Must call super constructor in derived class before accessing 'this'"
                            .to_string(),
                    ));
                }
                Ok(v)
            }
            Ref::Obj { .. } => {
                let key = self.get_key(rt)?;
                let Ref::Obj { base, this, .. } = self else {
                    unreachable!()
                };
                let (base, this) = (*base, this.clone());
                Ok(nil_safe(base.get(rt, &key, this.as_ref())?))
            }
            Ref::ObjStr {
                base,
                name,
                this,
                binding,
                ..
            } => {
                let (base, name, this, binding) = (*base, name.clone(), this.clone(), *binding);
                if let Some(v) = base.get_str(rt, &name, this.as_ref())? {
                    return Ok(v);
                }
                if binding {
                    return rt.throw_reference_error(&name);
                }
                Ok(Value::Undefined)
            }
            Ref::PrivateRes { base, name } => {
                let (base, name) = (*base, name.clone());
                get_private_prop_res(rt, &name, &Value::Object(base))
            }
            Ref::PrivateId { base, id } => {
                let (base, id) = (*base, id.clone());
                rt.vm_get_private_prop(
                    &Value::Object(base),
                    &id.name,
                    &id.typ,
                    id.idx,
                    id.is_method,
                )
            }
            Ref::Unresolved { name } => {
                let name = name.clone();
                rt.throw_reference_error(&name)
            }
        }
    }

    pub fn set(&mut self, rt: &mut Runtime, v: Value) -> JsResult<()> {
        match self {
            Ref::Stash { stash, idx, .. } => {
                rt.heap.stash_mut(*stash).values[*idx] = v;
                Ok(())
            }
            Ref::StashLex { stash, idx, .. } => {
                let p = &mut rt.heap.stash_mut(*stash).values[*idx];
                if p.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                *p = v;
                Ok(())
            }
            Ref::StashConst { strict_const, .. } => {
                if *strict_const {
                    return Err(Throw::TypeError(ERR_ASSIGN_TO_CONST.to_string()));
                }
                Ok(())
            }
            Ref::StackThis { idx } => {
                let p = &mut rt.vm.stack[*idx];
                if !p.is_empty() {
                    return Err(Throw::ReferenceError(
                        "Super constructor may only be called once".to_string(),
                    ));
                }
                *p = v;
                Ok(())
            }
            Ref::StashThis { stash, idx } => {
                let p = &mut rt.heap.stash_mut(*stash).values[*idx];
                if !p.is_empty() {
                    return Err(Throw::ReferenceError(
                        "Super constructor may only be called once".to_string(),
                    ));
                }
                *p = v;
                Ok(())
            }
            Ref::Obj { .. } => {
                let key = self.get_key(rt)?;
                let Ref::Obj {
                    base, this, strict, ..
                } = self
                else {
                    unreachable!()
                };
                let (base, this, strict) = (*base, this.clone(), *strict);
                match this {
                    Some(this) => base.set(rt, &key, v, &this, strict)?,
                    None => base.set_own(rt, &key, v, strict)?,
                };
                Ok(())
            }
            Ref::ObjStr {
                base,
                name,
                this,
                strict,
                binding,
            } => {
                let (base, name, this, strict, binding) =
                    (*base, name.clone(), this.clone(), *strict, *binding);
                if strict && binding && !base.has_own_property_str(rt, &name)? {
                    return Err(Throw::ReferenceError(format!("{name} is not defined")));
                }
                match this {
                    Some(this) => base.set_str(rt, &name, v, &this, strict)?,
                    None => base.set_own_str(rt, &name, v, strict)?,
                };
                Ok(())
            }
            Ref::PrivateRes { base, name } => {
                let (base, name) = (*base, name.clone());
                set_private_prop_res(rt, &name, &Value::Object(base), v)
            }
            Ref::PrivateId { base, id } => {
                let (base, id) = (*base, id.clone());
                rt.vm_set_private_prop(
                    &Value::Object(base),
                    &id.name,
                    &id.typ,
                    id.idx,
                    id.is_method,
                    v,
                )
            }
            Ref::Unresolved { name } => {
                let name = name.clone();
                rt.throw_reference_error(&name)
            }
        }
    }

    pub fn init(&mut self, rt: &mut Runtime, v: Value) -> JsResult<()> {
        match self {
            Ref::StashLex { stash, idx, .. } | Ref::StashConst { stash, idx, .. } => {
                rt.heap.stash_mut(*stash).values[*idx] = v;
                Ok(())
            }
            Ref::Obj { .. } | Ref::ObjStr { .. } => self.set(rt, v),
            Ref::PrivateRes { .. } | Ref::PrivateId { .. } => panic!("not supported"),
            _ => self.set(rt, v),
        }
    }

    pub fn refname(&mut self, rt: &mut Runtime) -> JsResult<unistring::String> {
        Ok(match self {
            Ref::Stash { n, .. } | Ref::StashLex { n, .. } | Ref::StashConst { n, .. } => n.clone(),
            Ref::StackThis { .. } | Ref::StashThis { .. } => unistring::String::new("this"),
            Ref::Obj { .. } => {
                let k = self.get_key(rt)?;
                k.string(rt)?
            }
            Ref::ObjStr { name, .. } => name.clone(),
            Ref::PrivateRes { name, .. } => name.string(),
            Ref::PrivateId { id, .. } => id.string(),
            Ref::Unresolved { name } => name.clone(),
        })
    }

    fn get_key(&mut self, rt: &mut Runtime) -> JsResult<Value> {
        if let Ref::Obj {
            name,
            name_converted,
            ..
        } = self
        {
            if !*name_converted {
                let n = name.clone();
                let k = n.to_property_key(rt)?;
                if let Ref::Obj {
                    name,
                    name_converted,
                    ..
                } = self
                {
                    *name = k;
                    *name_converted = true;
                }
            }
            if let Ref::Obj { name, .. } = self {
                return Ok(name.clone());
            }
        }
        Ok(Value::Undefined)
    }
}

pub fn stash_obj_has(rt: &mut Runtime, obj: ObjRef, name: &unistring::String) -> JsResult<bool> {
    if obj.has_property_str(rt, name)? {
        if let Some(Value::Object(unscopables)) = obj.get_sym(rt, SYM_UNSCOPABLES, None)?
            && let Some(b) = unscopables.get_str(rt, name, None)?
        {
            return Ok(!b.to_boolean());
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn stash_init_by_name(
    rt: &mut Runtime,
    s: StashRef,
    name: &unistring::String,
    v: Value,
) -> JsResult<()> {
    let st = rt.heap.stash_mut(s);
    if let Some(idx) = st.names.as_ref().and_then(|n| n.get(name)).copied() {
        st.values[(idx & !MASK_TYP) as usize] = v;
        return Ok(());
    }
    Err(Throw::ReferenceError(format!("{name} is not defined")))
}

pub fn stash_get_by_name(
    rt: &mut Runtime,
    s: StashRef,
    name: &unistring::String,
) -> JsResult<Option<Value>> {
    if let Some(obj) = rt.heap.stash(s).obj {
        if stash_obj_has(rt, obj, name)? {
            return Ok(Some(nil_safe(obj.get_str(rt, name, None)?)));
        }
        return Ok(None);
    }
    let st = rt.heap.stash(s);
    if let Some(idx) = st.names.as_ref().and_then(|n| n.get(name)).copied() {
        let v = st.values[(idx & !MASK_TYP) as usize].clone();
        if v.is_empty() {
            if idx & MASK_VAR == 0 {
                return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
            }
            return Ok(Some(Value::Undefined));
        }
        return Ok(Some(v));
    }
    Ok(None)
}

pub fn stash_get_ref_by_name(
    rt: &mut Runtime,
    s: StashRef,
    name: &unistring::String,
    strict: bool,
) -> JsResult<Option<Ref>> {
    if let Some(obj) = rt.heap.stash(s).obj {
        if stash_obj_has(rt, obj, name)? {
            return Ok(Some(Ref::ObjStr {
                base: obj,
                name: name.clone(),
                this: None,
                strict,
                binding: true,
            }));
        }
        return Ok(None);
    }
    let st = rt.heap.stash(s);
    if let Some(idx) = st.names.as_ref().and_then(|n| n.get(name)).copied() {
        let i = (idx & !MASK_TYP) as usize;
        if idx & MASK_VAR == 0 {
            if idx & MASK_CONST == 0 {
                return Ok(Some(Ref::StashLex {
                    n: name.clone(),
                    stash: s,
                    idx: i,
                }));
            }
            return Ok(Some(Ref::StashConst {
                n: name.clone(),
                stash: s,
                idx: i,
                strict_const: strict || (idx & MASK_STRICT != 0),
            }));
        }
        return Ok(Some(Ref::Stash {
            n: name.clone(),
            stash: s,
            idx: i,
        }));
    }
    Ok(None)
}

pub struct Vm {
    pub prg: Option<Arc<Program>>,
    pub pc: isize,
    pub stack: Vec<Value>,
    pub sp: isize,
    pub sb: isize,
    pub args: isize,
    pub stash: Option<StashRef>,
    pub priv_env: Option<Rc<PrivateEnv>>,
    pub call_stack: Vec<Context>,
    pub iter_stack: Vec<IterStackItem>,
    pub ref_stack: Vec<Ref>,
    pub try_stack: Vec<TryFrame>,
    pub new_target: Value,
    pub result: Value,
    pub max_call_stack_size: usize,
    pub stash_allocs: usize,
    pub interrupt: Arc<crate::runtime::InterruptState>,
    pub arg_bufs: Vec<Vec<Value>>,
    pub cur_async_runner: Option<ObjRef>,
}

impl Vm {
    pub fn new(global_stash: StashRef, interrupt: Arc<crate::runtime::InterruptState>) -> Vm {
        Vm {
            prg: None,
            pc: 0,
            stack: Vec::new(),
            sp: 0,
            sb: -1,
            args: 0,
            stash: Some(global_stash),
            priv_env: None,
            call_stack: Vec::new(),
            iter_stack: Vec::new(),
            ref_stack: Vec::new(),
            try_stack: Vec::new(),
            new_target: Value::Empty,
            result: Value::Empty,
            max_call_stack_size: i32::MAX as usize,
            stash_allocs: 0,
            interrupt,
            arg_bufs: Vec::new(),
            cur_async_runner: None,
        }
    }

    pub fn init(&mut self) {
        self.sb = -1;
        self.max_call_stack_size = i32::MAX as usize;
    }

    pub fn trace(&self, t: &mut Tracer) {
        t.values(&self.stack);
        if let Some(s) = self.stash {
            t.stash(s);
        }
        for c in &self.call_stack {
            c.trace(t);
        }
        for i in &self.iter_stack {
            i.trace(t);
        }
        for r in &self.ref_stack {
            r.trace(t);
        }
        for f in &self.try_stack {
            f.trace(t);
        }
        t.value(&self.new_target);
        t.value(&self.result);
        for b in &self.arg_bufs {
            t.values(b);
        }
        t.opt_obj(self.cur_async_runner);
    }

    pub fn halted(&self) -> bool {
        let pc = self.pc;
        match &self.prg {
            Some(p) => pc < 0 || pc as usize >= p.code.len(),
            None => true,
        }
    }

    pub fn stack_expand(&mut self, idx: isize) {
        let idx = idx as usize;
        if idx < self.stack.len() {
            return;
        }
        let idx = idx + 1;
        if idx > self.stack.capacity() {
            let new_cap = if idx < 1024 {
                idx * 2
            } else {
                (idx + 1025) & !1023
            };
            self.stack.reserve_exact(new_cap - self.stack.len());
        }
        self.stack.resize(idx, Value::Empty);
    }

    #[inline]
    pub fn push(&mut self, v: Value) {
        self.stack_expand(self.sp);
        self.stack[self.sp as usize] = v;
        self.sp += 1;
    }

    #[inline]
    pub fn pop(&mut self) -> Value {
        self.sp -= 1;
        self.stack[self.sp as usize].clone()
    }

    #[inline]
    pub fn peek(&self) -> Value {
        self.stack[self.sp as usize - 1].clone()
    }

    pub fn save_ctx(&self, ctx: &mut Context) {
        ctx.prg = self.prg.clone();
        ctx.stash = self.stash;
        ctx.priv_env = self.priv_env.clone();
        ctx.new_target = self.new_target.clone();
        ctx.result = self.result.clone();
        ctx.pc = self.pc;
        ctx.sb = self.sb;
        ctx.args = self.args;
    }

    pub fn push_ctx_unchecked(&mut self) {
        let mut ctx = Context::default();
        self.save_ctx(&mut ctx);
        self.call_stack.push(ctx);
    }

    pub fn restore_ctx(&mut self, ctx: &Context) {
        self.prg = ctx.prg.clone();
        self.stash = ctx.stash;
        self.priv_env = ctx.priv_env.clone();
        self.new_target = ctx.new_target.clone();
        self.result = ctx.result.clone();
        self.pc = ctx.pc;
        self.sb = ctx.sb;
        self.args = ctx.args;
    }

    pub fn pop_ctx(&mut self) {
        if let Some(ctx) = self.call_stack.pop() {
            self.prg = ctx.prg;
            self.stash = ctx.stash;
            self.priv_env = ctx.priv_env;
            self.new_target = ctx.new_target;
            self.result = ctx.result;
            self.pc = ctx.pc;
            self.sb = ctx.sb;
            self.args = ctx.args;
        }
    }

    pub fn push_try_frame(&mut self, catch_pos: i32, finally_pos: i32) {
        self.try_stack.push(TryFrame {
            exception: None,
            call_stack_len: self.call_stack.len() as u32,
            iter_len: self.iter_stack.len() as u32,
            ref_len: self.ref_stack.len() as u32,
            sp: self.sp as i32,
            stash: self.stash,
            priv_env: self.priv_env.clone(),
            catch_pos,
            finally_pos,
            finally_ret: -1,
        });
    }

    pub fn pop_try_frame(&mut self) {
        self.try_stack.pop();
    }

    pub fn take_arg_buf(&mut self) -> Vec<Value> {
        self.arg_bufs.pop().unwrap_or_default()
    }

    pub fn return_arg_buf(&mut self, mut b: Vec<Value>) {
        b.clear();
        if self.arg_bufs.len() < 64 {
            self.arg_bufs.push(b);
        }
    }

    pub fn clear_stack(&mut self) {
        let sp = (self.sp.max(0) as usize).min(self.stack.len());
        for v in &mut self.stack[sp..] {
            *v = Value::Empty;
        }
    }

    pub fn capture_stack(
        &self,
        rt: &Runtime,
        mut stack: Vec<StackFrame>,
        ctx_offset: usize,
    ) -> Vec<StackFrame> {
        if self.prg.is_some() || self.sb > 0 {
            let func_name = match &self.prg {
                Some(p) => p.func_name.clone(),
                None => get_func_name(rt, &self.stack, self.sb),
            };
            stack.push(StackFrame {
                prg: self.prg.clone(),
                pc: self.pc,
                func_name,
            });
        }
        let mut i = self.call_stack.len() as isize - 1;
        while i > ctx_offset as isize - 1 {
            let frame = &self.call_stack[i as usize];
            if frame.prg.is_some() || frame.sb > 0 {
                let func_name = match &frame.prg {
                    Some(p) => p.func_name.clone(),
                    None => get_func_name(rt, &self.stack, frame.sb),
                };
                stack.push(StackFrame {
                    prg: frame.prg.clone(),
                    pc: frame.pc,
                    func_name,
                });
            }
            i -= 1;
        }
        stack
    }
}

pub fn get_func_name(rt: &Runtime, stack: &[Value], sb: isize) -> unistring::String {
    if sb > 0
        && let Some(Value::Object(f)) = stack.get(sb as usize - 1)
    {
        if rt.is_proxy(*f) {
            return unistring::String::new("proxy");
        }
        if let Some(v) = rt.obj(*f).base.values.get(&unistring::String::new("name")) {
            let v = match v {
                Value::Property(p) if p.getter_func.is_none() => p.value.clone(),
                Value::Property(_) => return unistring::String::default(),
                v => v.clone(),
            };
            return match v {
                Value::Empty | Value::Undefined => unistring::String::new("undefined"),
                v => v.prim_string(),
            };
        }
        if let ObjExt::Template(_) = &rt.obj(*f).ext {
            return unistring::String::default();
        }
        return unistring::String::new("undefined");
    }
    unistring::String::default()
}

impl Runtime {
    pub fn vm_new_stash(&mut self) {
        let outer = self.vm.stash;
        let s = self.heap.alloc_stash(Stash {
            outer,
            ..Stash::default()
        });
        self.vm.stash = Some(s);
        self.vm.stash_allocs += 1;
    }

    pub fn vm_push_ctx(&mut self) -> JsResult<()> {
        self.vm.push_ctx()
    }

    pub fn vm_capture_stack(&self, ctx_offset: usize) -> Vec<StackFrame> {
        self.vm.capture_stack(
            self,
            Vec::with_capacity(self.vm.call_stack.len() + 1),
            ctx_offset,
        )
    }

    pub fn vm_run(&mut self) -> JsResult<()> {
        let saved_in_native = self.in_native;
        self.in_native = false;
        let handles_base = self.handles.len();
        let r = self.vm_run_loop(handles_base);
        self.in_native = saved_in_native;
        r
    }

    fn vm_run_loop(&mut self, handles_base: usize) -> JsResult<()> {
        let mut prg = match &self.vm.prg {
            Some(p) => p.clone(),
            None => return Ok(()),
        };
        loop {
            if self.vm.interrupt.is_set() {
                break;
            }
            if self.heap.gc_requested {
                self.handles.truncate(handles_base);
                self.collect_garbage();
            }
            match &self.vm.prg {
                Some(p) if Arc::ptr_eq(p, &prg) => {}
                Some(p) => prg = p.clone(),
                None => return Ok(()),
            }
            let pc = self.vm.pc;
            if pc < 0 || pc as usize >= prg.code.len() {
                return Ok(());
            }
            self.exec(&prg.code[pc as usize])?;
        }
        let (iface, text) = match self.interrupt_state.take_value() {
            Some((v, t)) => (Some(v), t),
            None => (None, String::new()),
        };
        let stack = self.vm_capture_stack(0);
        Err(Throw::Interrupted(Box::new(InterruptedError {
            exception: Exception {
                val: Value::Empty,
                stack,
                val_string: None,
            },
            iface,
            text,
        })))
    }

    pub fn exec(&mut self, ins: &Instruction) -> JsResult<()> {
        let _ = ins;
        todo!("instruction dispatch")
    }

    pub fn vm_restore_stacks(&mut self, iter_len: u32, ref_len: u32) -> Option<Box<Exception>> {
        let mut ex = None;
        while self.vm.iter_stack.len() > iter_len as usize {
            let item = self.vm.iter_stack.pop();
            if let Some(mut item) = item
                && let Some(iter) = &mut item.iter
            {
                self.extra_roots.push(item.val.clone());
                let r = self.vm_try(|rt| iter.return_iter(rt));
                self.extra_roots.pop();
                if let Some(e) = r
                    && ex.is_none()
                {
                    ex = Some(e);
                }
            }
        }
        self.vm.ref_stack.truncate(ref_len as usize);
        ex
    }

    pub fn vm_exception_from_throw(&mut self, x: Throw) -> Exception {
        let mut ex = match x {
            Throw::Value(Value::Object(o)) => {
                let mut ex = Exception::new(Value::Object(o));
                if let ObjExt::Error(e) = &self.obj(o).ext {
                    ex.stack = e.stack.clone();
                }
                ex
            }
            Throw::Value(v) => Exception::new(v),
            Throw::Exception(e) => *e,
            Throw::TypeError(m) => Exception::new(self.new_type_error(&m)),
            Throw::ReferenceError(m) => {
                let e = self.get_reference_error();
                Exception::new(self.new_error(e, &m))
            }
            Throw::ReferenceErrorName(n) => Exception::new(self.new_reference_error(&n)),
            Throw::RangeError(m) => {
                let e = self.get_range_error();
                Exception::new(self.new_error(e, &m))
            }
            Throw::SyntaxError(m) => {
                let e = self.get_syntax_error();
                Exception::new(self.new_error(e, &m))
            }
            Throw::Interrupted(e) => e.exception,
            Throw::StackOverflow(e) => e.exception,
        };
        if ex.stack.is_empty() {
            ex.stack = self.vm_capture_stack(0);
        }
        ex
    }

    pub fn vm_handle_throw(&mut self, arg: Throw) -> Result<(), Throw> {
        let uncatchable = arg.is_uncatchable();
        let mut ex: Option<Box<Exception>> = None;
        let mut arg = Some(arg);
        if !uncatchable {
            let e =
                self.vm_exception_from_throw(arg.take().unwrap_or(Throw::Value(Value::Undefined)));
            ex = Some(Box::new(e));
        }
        if let Some(e) = &ex {
            self.extra_roots.push(e.val.clone());
        }
        let result = loop {
            let Some(tf) = self.vm.try_stack.last_mut() else {
                break None;
            };
            if tf.catch_pos == -1 && tf.finally_pos == -1
                || ex.is_none() && tf.catch_pos != TRY_PANIC_MARKER
            {
                tf.exception = None;
                self.vm.pop_try_frame();
                continue;
            }
            let call_stack_len = tf.call_stack_len as usize;
            let (sp, stash, priv_env, iter_len, ref_len, catch_pos, finally_pos) = (
                tf.sp,
                tf.stash,
                tf.priv_env.clone(),
                tf.iter_len,
                tf.ref_len,
                tf.catch_pos,
                tf.finally_pos,
            );
            if call_stack_len < self.vm.call_stack.len() {
                let ctx = &self.vm.call_stack[call_stack_len];
                self.vm.prg = ctx.prg.clone();
                self.vm.new_target = ctx.new_target.clone();
                self.vm.result = ctx.result.clone();
                self.vm.pc = ctx.pc;
                self.vm.sb = ctx.sb;
                self.vm.args = ctx.args;
                self.vm.call_stack.truncate(call_stack_len);
            }
            self.vm.sp = sp as isize;
            self.vm.stash = stash;
            self.vm.priv_env = priv_env;
            let _ = self.vm_restore_stacks(iter_len, ref_len);

            if catch_pos == TRY_PANIC_MARKER {
                break Some(());
            }
            if catch_pos >= 0 {
                let v = ex.as_ref().map(|e| e.val.clone()).unwrap_or_default();
                self.vm.push(v);
                self.vm.pc = catch_pos as isize;
                if let Some(tf) = self.vm.try_stack.last_mut() {
                    tf.catch_pos = -1;
                }
                if ex.is_some() {
                    self.extra_roots.pop();
                }
                return Ok(());
            }
            if finally_pos >= 0 {
                if let Some(tf) = self.vm.try_stack.last_mut() {
                    tf.exception = ex.take();
                    tf.finally_pos = -1;
                    tf.finally_ret = -1;
                }
                self.vm.pc = finally_pos as isize;
                self.extra_roots.pop();
                return Ok(());
            }
        };
        let _ = result;
        match ex {
            None => Err(arg.unwrap_or(Throw::Value(Value::Undefined))),
            Some(e) => {
                self.extra_roots.pop();
                Err(Throw::Exception(e))
            }
        }
    }

    pub fn vm_try(
        &mut self,
        f: impl FnOnce(&mut Runtime) -> JsResult<()>,
    ) -> Option<Box<Exception>> {
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        let r = f(self);
        let out = match r {
            Ok(()) => None,
            Err(t) => match self.vm_handle_throw(t) {
                Ok(()) => None,
                Err(Throw::Exception(e)) => Some(e),
                Err(other) => {
                    self.vm.pop_try_frame();
                    self.pending_uncatchable = Some(other);
                    return None;
                }
            },
        };
        self.vm.pop_try_frame();
        out
    }

    pub fn vm_try_result(
        &mut self,
        f: impl FnOnce(&mut Runtime) -> JsResult<()>,
    ) -> JsResult<Option<Box<Exception>>> {
        let r = self.vm_try(f);
        if let Some(t) = self.pending_uncatchable.take() {
            return Err(t);
        }
        Ok(r)
    }

    pub fn vm_run_try(&mut self) -> JsResult<Option<Box<Exception>>> {
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        let r = loop {
            match self.vm_run_try_inner() {
                Ok(Some(ex)) => break Ok(Some(ex)),
                Ok(None) => {
                    if self.vm.halted() {
                        break Ok(None);
                    }
                }
                Err(t) => break Err(t),
            }
        };
        self.vm.pop_try_frame();
        r
    }

    pub fn vm_run_try_inner(&mut self) -> JsResult<Option<Box<Exception>>> {
        match self.vm_run() {
            Ok(()) => Ok(None),
            Err(t) => match self.vm_handle_throw(t) {
                Ok(()) => Ok(None),
                Err(Throw::Exception(e)) => Ok(Some(e)),
                Err(other) => Err(other),
            },
        }
    }
}

impl Vm {
    pub fn push_ctx(&mut self) -> JsResult<()> {
        if self.call_stack.len() > self.max_call_stack_size {
            return Err(Throw::StackOverflow(Box::new(
                crate::runtime::StackOverflowError {
                    exception: Exception::new(Value::Empty),
                },
            )));
        }
        self.push_ctx_unchecked();
        Ok(())
    }
}

pub fn js_func_call(
    rt: &mut Runtime,
    o: ObjRef,
    call: FunctionCall<'_>,
    new_target: Value,
) -> JsResult<Value> {
    let this = if call.this.is_empty() {
        Value::Undefined
    } else {
        call.this.clone()
    };
    js_func_call_args(rt, o, call.arguments, new_target, this)
}

pub fn js_func_call_args(
    rt: &mut Runtime,
    o: ObjRef,
    args: &[Value],
    new_target: Value,
    this: Value,
) -> JsResult<Value> {
    let (res, ex) = js_func_call_ex(rt, o, args, new_target, this)?;
    match ex {
        Some(e) => Err(Throw::Exception(e)),
        None => Ok(res),
    }
}

pub fn js_func_call_ex(
    rt: &mut Runtime,
    o: ObjRef,
    args: &[Value],
    new_target: Value,
    this: Value,
) -> JsResult<(Value, Option<Box<Exception>>)> {
    let vm_sp = rt.vm.sp;
    rt.vm.stack_expand(vm_sp + args.len() as isize + 1);
    rt.vm.stack[rt.vm.sp as usize] = Value::Object(o);
    rt.vm.sp += 1;
    rt.vm.stack[rt.vm.sp as usize] = this;
    rt.vm.sp += 1;
    for arg in args {
        rt.vm.stack[rt.vm.sp as usize] = if arg.is_empty() {
            Value::Undefined
        } else {
            arg.clone()
        };
        rt.vm.sp += 1;
    }
    rt.vm.push_try_frame(TRY_PANIC_MARKER, -1);
    let mut need_pop = false;
    if rt.vm.prg.is_some() {
        if let Err(e) = rt.vm.push_ctx() {
            rt.vm.pop_try_frame();
            return Err(e);
        }
        rt.vm.call_stack.push(Context {
            pc: -2,
            ..Context::default()
        });
        need_pop = true;
    } else {
        rt.vm.pc = -2;
        if let Err(e) = rt.vm.push_ctx() {
            rt.vm.pop_try_frame();
            return Err(e);
        }
    }
    let f = BaseJsFuncObject::data(rt, o);
    let (prg, stash, priv_env) = (f.prg.clone(), f.stash, f.priv_env.clone());
    rt.vm.args = args.len() as isize;
    rt.vm.prg = prg;
    rt.vm.stash = stash;
    rt.vm.priv_env = priv_env;
    rt.vm.new_target = new_target;
    rt.vm.pc = 0;
    loop {
        match rt.vm_run_try_inner() {
            Ok(Some(ex)) => {
                rt.vm.pop_try_frame();
                return Ok((Value::Undefined, Some(ex)));
            }
            Ok(None) => {}
            Err(t) => {
                rt.vm.pop_try_frame();
                return Err(t);
            }
        }
        if rt.vm.halted() {
            break;
        }
    }
    if need_pop {
        rt.vm.pop_ctx();
    }
    rt.vm.pop_try_frame();
    let r = rt.vm.pop();
    rt.root(&r);
    Ok((r, None))
}

pub fn js_func_vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
    rt.vm.push_ctx()?;
    let f = BaseJsFuncObject::data(rt, o);
    let (prg, stash, priv_env) = (f.prg.clone(), f.stash, f.priv_env.clone());
    rt.vm.args = n as isize;
    rt.vm.prg = prg;
    rt.vm.stash = stash;
    rt.vm.priv_env = priv_env;
    rt.vm.pc = 0;
    let sp = rt.vm.sp as usize;
    rt.vm.stack.swap(sp - n - 1, sp - n - 2);
    Ok(())
}

pub fn arrow_func_vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
    rt.vm.push_ctx()?;
    let f = BaseJsFuncObject::data(rt, o);
    let (prg, stash, priv_env, nt) = (
        f.prg.clone(),
        f.stash,
        f.priv_env.clone(),
        f.new_target.clone(),
    );
    rt.vm.args = n as isize;
    rt.vm.prg = prg;
    rt.vm.stash = stash;
    rt.vm.priv_env = priv_env;
    rt.vm.pc = 0;
    let sp = rt.vm.sp as usize;
    let callee = rt.vm.stack[sp - n - 1].clone();
    rt.vm.stack[sp - n - 1] = Value::Empty;
    rt.vm.stack[sp - n - 2] = callee;
    rt.vm.new_target = nt;
    Ok(())
}

pub fn js_func_construct(
    rt: &mut Runtime,
    o: ObjRef,
    args: &[Value],
    new_target: Option<ObjRef>,
) -> JsResult<ObjRef> {
    let new_target = new_target.unwrap_or(o);
    let proto = new_target.get_str(rt, &"prototype".into(), None)?;
    let proto_obj = match proto {
        Some(Value::Object(p)) => Some(p),
        _ => rt.global.object_prototype,
    };
    let obj = rt.new_base_object(proto_obj, crate::object::CLASS_OBJECT);
    let ret = js_func_call(
        rt,
        o,
        FunctionCall::new(&Value::Object(obj), args),
        Value::Object(new_target),
    )?;
    if let Value::Object(r) = ret {
        return Ok(r);
    }
    Ok(obj)
}

pub fn class_func_construct(
    rt: &mut Runtime,
    o: ObjRef,
    args: &[Value],
    new_target: Option<ObjRef>,
) -> JsResult<ObjRef> {
    let new_target = new_target.unwrap_or(o);
    let (has_prg, derived) = {
        let f = BaseJsFuncObject::data(rt, o);
        (f.prg.is_some(), f.derived)
    };
    if !has_prg {
        let instance = class_func_create_instance(rt, o, args, new_target)?;
        class_func_init_fields(rt, o, instance)?;
        return Ok(instance);
    }
    let mut instance = None;
    let mut this_val = Value::Empty;
    if !derived {
        let i = class_func_create_instance(rt, o, args, new_target)?;
        class_func_init_fields(rt, o, i)?;
        this_val = Value::Object(i);
        instance = Some(i);
    }
    let ret = js_func_call_args(rt, o, args, Value::Object(new_target), this_val)?;
    if let Value::Object(r) = ret {
        return Ok(r);
    }
    if derived {
        if !ret.is_undefined() {
            return Err(Throw::Value(rt.new_type_error(
                "Derived constructors may only return object or undefined",
            )));
        }
        let v = rt.vm.stack[rt.vm.sp as usize + 1].clone();
        if !v.is_empty() {
            instance = Some(rt.to_object(&v)?);
        } else {
            let e = rt.get_reference_error();
            return Err(Throw::Value(rt.new_error(
                e,
                "Must call super constructor in derived class before returning from derived constructor",
            )));
        }
    }
    Ok(instance.unwrap_or(o))
}

pub fn class_func_create_instance(
    rt: &mut Runtime,
    o: ObjRef,
    args: &[Value],
    new_target: ObjRef,
) -> JsResult<ObjRef> {
    if BaseJsFuncObject::data(rt, o).derived {
        let proto = rt.obj(o).base.prototype;
        match proto.and_then(|p| p.assert_constructor(rt)) {
            Some(ctor) => ctor.construct(rt, args, Some(new_target)),
            None => Err(Throw::Value(
                rt.new_type_error("Super constructor is not a constructor"),
            )),
        }
    } else {
        crate::func::BaseFuncObject::create_instance(rt, o, Some(new_target))
    }
}

pub fn class_func_init_fields(rt: &mut Runtime, o: ObjRef, instance: ObjRef) -> JsResult<()> {
    let (pet, pm, init_fields, stash, priv_env) = {
        let f = BaseJsFuncObject::data(rt, o);
        (
            f.private_env_type.clone(),
            f.private_methods.clone(),
            f.init_fields.clone(),
            f.stash,
            f.priv_env.clone(),
        )
    };
    if let Some(pet) = &pet {
        instance.get_private_env(rt, pet, true)?;
        let key = crate::object::private_env_key(pet);
        if let Some(m) = &mut rt.obj_mut(instance).base.private_elements
            && let Some(e) = m.get_mut(&key)
        {
            e.methods = pm;
        }
    }
    if let Some(init_fields) = init_fields {
        rt.vm.push_ctx()?;
        rt.vm.prg = Some(init_fields);
        rt.vm.stash = stash;
        rt.vm.priv_env = priv_env;
        rt.vm.new_target = Value::Empty;
        rt.vm.push(Value::Object(o));
        rt.vm.sb = rt.vm.sp;
        rt.vm.push(Value::Object(instance));
        rt.vm.pc = 0;
        let ex = rt.vm_run_try()?;
        rt.vm.pop_ctx();
        if let Some(ex) = ex {
            return Err(Throw::Exception(ex));
        }
        rt.vm.sp -= 2;
    }
    Ok(())
}

pub fn get_private_prop_res(
    _rt: &mut Runtime,
    _name: &ResolvedPrivateName,
    _base: &Value,
) -> JsResult<Value> {
    todo!("getPrivatePropRes._get")
}

pub fn set_private_prop_res(
    _rt: &mut Runtime,
    _name: &ResolvedPrivateName,
    _base: &Value,
    _val: Value,
) -> JsResult<()> {
    todo!("setPrivatePropRes._set")
}

impl Runtime {
    pub fn vm_get_private_prop(
        &mut self,
        _base: &Value,
        _name: &unistring::String,
        _typ: &Arc<PrivateEnvType>,
        _idx: u32,
        _is_method: bool,
    ) -> JsResult<Value> {
        todo!("vm.getPrivateProp")
    }

    pub fn vm_set_private_prop(
        &mut self,
        _base: &Value,
        _name: &unistring::String,
        _typ: &Arc<PrivateEnvType>,
        _idx: u32,
        _is_method: bool,
        _val: Value,
    ) -> JsResult<()> {
        todo!("vm.setPrivateProp")
    }
}

pub fn generator_next(_rt: &mut Runtime, _o: ObjRef, _v: Value) -> JsResult<Value> {
    todo!("generatorObject.next")
}

pub fn generator_return(_rt: &mut Runtime, _o: ObjRef, _v: Value) -> JsResult<Value> {
    todo!("generatorObject._return")
}

pub fn generator_throw(_rt: &mut Runtime, _o: ObjRef, _v: Value) -> JsResult<Value> {
    todo!("generatorObject.throw")
}
