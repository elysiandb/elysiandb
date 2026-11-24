use crate::builtin_bigint::string_to_big_int;
use crate::builtin_math::{go_mod, pow};
use crate::compiler::{Program, ResolvedPrivateName, VarType};
use crate::compiler_expr::FuncType;
use crate::deps::big;
use crate::func::YieldMarker;
use crate::object::PrivateId;
use crate::regexp::RegexpPattern;
use crate::string::{JsString, ascii_string, new_string_value};
use crate::unistring;
use crate::value::{
    ERR_MIX_BIG_INT_TYPE, Value, ValueError, ValueProperty, float_to_value, int_to_value,
    new_big_int, to_int32, to_numeric, to_uint32,
};
use std::collections::HashMap;
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
        _ => v.to_string().go_string(),
    }
}

pub(crate) fn op_add(left: &Value, right: &Value) -> Result<Value, ValueError> {
    let is_left_string = matches!(left, Value::String(_));
    let is_right_string = matches!(right, Value::String(_));
    if is_left_string || is_right_string {
        let l = left.to_string();
        let r = right.to_string();
        return Ok(Value::String(l.concat(&r)));
    }
    Ok(match left {
        Value::Int(l) => match right {
            Value::Int(r) => int_to_value(l.wrapping_add(*r)),
            Value::BigInt(_) => return Err(mix_big_int()),
            _ => float_to_value(*l as f64 + right.to_float()),
        },
        Value::BigInt(l) => match right {
            Value::BigInt(r) => new_big_int(l.add(r)),
            _ => return Err(mix_big_int()),
        },
        _ => {
            if let Value::BigInt(_) = right {
                return Err(mix_big_int());
            }
            float_to_value(left.to_float() + right.to_float())
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
    Ok(float_to_value(left.to_float() - right.to_float()))
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
    Ok(float_to_value(left.to_float() * right.to_float()))
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
    let (left, right) = (left_value.to_float(), right_value.to_float());
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
    Ok(float_to_value(go_mod(left.to_float(), right.to_float())))
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
            let mut f = operand.to_float();
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
    let (nx, ny) = (px.to_float(), py.to_float());
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
            Instruction::OpEq => Some(|l, r| Ok(Value::Bool(l.equals(r)))),
            Instruction::OpNeq => Some(|l, r| Ok(Value::Bool(!l.equals(r)))),
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
            Instruction::Plus => Some(|v| v.to_number()),
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
                    Some(Slot::Val(v)) => Some(Value::String(v.to_string())),
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
