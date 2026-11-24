use crate::array::ARRAY_OBJECT_CLASS;
use crate::builtin_bigint::string_to_big_int;
use crate::builtin_math::{go_mod, pow};
use crate::builtin_symbol::SYM_UNSCOPABLES;
use crate::compiler::{EvalContext, MASK_CONST, MASK_DELETABLE, MASK_STRICT, MASK_TYP, MASK_VAR};
use crate::compiler::{Program, ResolvedPrivateName, VarType};
use crate::compiler_expr::FuncType;
use crate::deps::big;
use crate::func::YieldMarker;
use crate::func::{BaseJsFuncObject, FuncKind};
use crate::heap::{FxHashMap, ObjRef, StashRef, SymRef, Tracer};
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, EmptyIter, Flag, FunctionCall, IterNextFunc,
    ObjClass, ObjExt, PrivateElements, PrivateEnv, PrivateEnvType, PrivateId, PropertyDescriptor,
    enumerate_recursive, instance_of_operator, iterate_enumerable_properties, private_env_key,
    to_flag,
};
use crate::object_template::TemplatedObject;
use crate::regexp::RegexpPattern;
use crate::runtime::{
    Exception, InterruptedError, IteratorRecord, JsResult, Runtime, StackFrame, StackOverflowError,
    Throw, create_data_property_or_throw,
};
use crate::string::{
    JsString, STRING_BIG_INT, STRING_BOOLEAN, STRING_NUMBER, STRING_OBJECT_C, STRING_STRING,
    STRING_SYMBOL, STRING_UNDEFINED, StringBuilder, ascii_string, new_string_value,
    string_value_from_raw,
};
use crate::unistring;
use crate::value::{ERR_ACCESS_BEFORE_INIT, ERR_ASSIGN_TO_CONST, Unresolved, nil_safe};
use crate::value::{
    ERR_MIX_BIG_INT_TYPE, Value, ValueError, ValueProperty, float_to_value, func_name,
    int_to_value, new_big_int, property_set, to_int32, to_numeric, to_uint32,
};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

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
    pub names: NamesMap,
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
        let names = self.names.get_or_insert_with(Default::default);
        if !names.contains_key(name) {
            let mut idx = names.len() as u32 | MASK_VAR;
            if deletable {
                idx |= MASK_DELETABLE;
            }
            Arc::make_mut(names).insert(name.clone(), idx);
            self.values.push(Value::Undefined);
        }
    }

    pub fn create_lex_binding(&mut self, name: &unistring::String, is_const: bool) {
        let names = self.names.get_or_insert_with(Default::default);
        if !names.contains_key(name) {
            let mut idx = names.len() as u32;
            if is_const {
                idx |= MASK_CONST | MASK_STRICT;
            }
            Arc::make_mut(names).insert(name.clone(), idx);
            self.values.push(Value::Empty);
        }
    }

    pub fn delete_binding(&mut self, name: &unistring::String) {
        if let Some(n) = &mut self.names {
            Arc::make_mut(n).remove(name);
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
    pub obj: Value,
}

impl IterStackItem {
    fn trace(&self, t: &mut Tracer) {
        t.value(&self.val);
        t.value(&self.obj);
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
    StackVar {
        n: unistring::String,
        idx: usize,
    },
    StackLex {
        n: unistring::String,
        idx: usize,
    },
    StackConst {
        n: unistring::String,
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
        this: Option<Box<Value>>,
        strict: bool,
        name_converted: bool,
    },
    ObjStr {
        base: ObjRef,
        name: unistring::String,
        this: Option<Box<Value>>,
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
            Ref::StackVar { .. }
            | Ref::StackLex { .. }
            | Ref::StackConst { .. }
            | Ref::StackThis { .. }
            | Ref::Unresolved { .. } => {}
        }
    }

    pub fn root(&self, rt: &mut Runtime) {
        match self {
            Ref::Obj {
                base, name, this, ..
            } => {
                if *base != rt.global_object {
                    rt.handles.push(*base);
                }
                if let Value::Object(o) = name {
                    rt.handles.push(*o);
                }
                if let Some(Value::Object(o)) = this.as_deref() {
                    rt.handles.push(*o);
                }
            }
            Ref::ObjStr { base, this, .. } => {
                if *base != rt.global_object {
                    rt.handles.push(*base);
                }
                if let Some(Value::Object(o)) = this.as_deref() {
                    rt.handles.push(*o);
                }
            }
            Ref::PrivateRes { base, .. } | Ref::PrivateId { base, .. } => rt.handles.push(*base),
            _ => {}
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
            Ref::StackVar { idx, .. } => Ok(nil_safe(Some(rt.vm.stack[*idx].clone()))),
            Ref::StackLex { idx, .. } | Ref::StackConst { idx, .. } => {
                let v = rt.vm.stack[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                Ok(v)
            }
            Ref::StackThis { idx } => {
                let v = rt.vm.stack[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_SUPER_NOT_CALLED.to_string()));
                }
                Ok(v)
            }
            Ref::StashThis { stash, idx } => {
                let v = rt.heap.stash(*stash).values[*idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_SUPER_NOT_CALLED.to_string()));
                }
                Ok(v)
            }
            Ref::Obj { .. } => {
                let key = self.get_key(rt)?;
                let Ref::Obj { base, this, .. } = self else {
                    unreachable!()
                };
                let (base, this) = (*base, this.clone());
                Ok(nil_safe(base.get(rt, &key, this.as_deref())?))
            }
            Ref::ObjStr {
                base,
                name,
                this,
                binding,
                ..
            } => {
                if let Some(v) = base.get_str(rt, name, this.as_deref())?
                    && !v.is_empty()
                {
                    return Ok(v);
                }
                if *binding {
                    return rt.throw_reference_error(name);
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
                    Some(&id.typ),
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
            Ref::StashConst { strict_const, .. } | Ref::StackConst { strict_const, .. } => {
                if *strict_const {
                    return Err(Throw::TypeError(ERR_ASSIGN_TO_CONST.to_string()));
                }
                Ok(())
            }
            Ref::StackVar { idx, .. } => {
                rt.vm.stack[*idx] = v;
                Ok(())
            }
            Ref::StackLex { idx, .. } => {
                let p = &mut rt.vm.stack[*idx];
                if p.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                *p = v;
                Ok(())
            }
            Ref::StackThis { idx } => {
                let p = &mut rt.vm.stack[*idx];
                if !p.is_empty() {
                    return Err(Throw::ReferenceError(ERR_SUPER_CALLED_TWICE.to_string()));
                }
                *p = v;
                Ok(())
            }
            Ref::StashThis { stash, idx } => {
                let p = &mut rt.heap.stash_mut(*stash).values[*idx];
                if !p.is_empty() {
                    return Err(Throw::ReferenceError(ERR_SUPER_CALLED_TWICE.to_string()));
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
                if *strict && *binding && !base.has_own_property_str(rt, name)? {
                    return Err(Throw::ReferenceError(format!("{name} is not defined")));
                }
                match this {
                    Some(this) => base.set_str(rt, name, v, this, *strict)?,
                    None => base.set_own_str(rt, name, v, *strict)?,
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
                    Some(&id.typ),
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
            Ref::StackLex { idx, .. } | Ref::StackConst { idx, .. } => {
                rt.vm.stack[*idx] = v;
                Ok(())
            }
            Ref::ObjStr {
                base,
                name,
                this,
                strict,
                ..
            } => {
                let (base, name, this, strict) = (*base, name.clone(), this.clone(), *strict);
                match this {
                    Some(this) => base.set_str(rt, &name, v, &this, strict)?,
                    None => base.set_own_str(rt, &name, v, strict)?,
                };
                Ok(())
            }
            Ref::PrivateRes { .. } | Ref::PrivateId { .. } => panic!("not supported"),
            _ => self.set(rt, v),
        }
    }

    pub fn refname(&mut self, rt: &mut Runtime) -> JsResult<unistring::String> {
        Ok(match self {
            Ref::Stash { n, .. }
            | Ref::StashLex { n, .. }
            | Ref::StashConst { n, .. }
            | Ref::StackVar { n, .. }
            | Ref::StackLex { n, .. }
            | Ref::StackConst { n, .. } => n.clone(),
            Ref::StackThis { .. } | Ref::StashThis { .. } => {
                unistring::String::new(crate::compiler::THIS_BINDING_NAME)
            }
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

    pub fn get_key(&mut self, rt: &mut Runtime) -> JsResult<Value> {
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
    pub tagged_template_ids: FxHashMap<ObjRef, usize>,
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
            tagged_template_ids: FxHashMap::default(),
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

    #[inline(always)]
    pub fn stack_expand(&mut self, idx: isize) {
        if idx < 0 || (idx as usize) < self.stack.len() {
            return;
        }
        self.stack_grow(idx as usize);
    }

    #[cold]
    fn stack_grow(&mut self, idx: usize) {
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

    #[inline(always)]
    pub fn push(&mut self, v: Value) {
        self.stack_expand(self.sp);
        self.stack[self.sp as usize].set(v);
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

    #[inline]
    pub fn push_ctx_unchecked(&mut self) {
        let ctx = Context {
            prg: self.prg.clone(),
            stash: self.stash,
            priv_env: self.priv_env.clone(),
            new_target: self.new_target.clone(),
            result: self.result.clone(),
            pc: self.pc,
            sb: self.sb,
            args: self.args,
        };
        self.call_stack.push(ctx);
    }

    #[inline]
    pub fn push_ctx_take_prg(&mut self) {
        let ctx = Context {
            prg: self.prg.take(),
            stash: self.stash,
            priv_env: self.priv_env.clone(),
            new_target: self.new_target.clone(),
            result: self.result.clone(),
            pc: self.pc,
            sb: self.sb,
            args: self.args,
        };
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
        self.stack.truncate(sp);
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

    #[inline]
    pub fn vm_push_ctx(&mut self) -> JsResult<()> {
        if self.vm.call_stack.len() > self.vm.max_call_stack_size {
            return Err(self.vm_stack_overflow());
        }
        self.vm.push_ctx_unchecked();
        Ok(())
    }

    #[inline]
    pub fn vm_push_ctx_for_call(&mut self) -> JsResult<()> {
        if self.vm.call_stack.len() > self.vm.max_call_stack_size {
            return Err(self.vm_stack_overflow());
        }
        self.vm.push_ctx_take_prg();
        Ok(())
    }

    #[cold]
    fn vm_stack_overflow(&mut self) -> Throw {
        let stack = self.vm_capture_stack(0);
        Throw::StackOverflow(Box::new(StackOverflowError {
            exception: Exception {
                val: Value::Empty,
                stack,
                val_string: None,
            },
        }))
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
            if self.handles.len() > handles_base {
                self.handles.truncate(handles_base);
            }
            if self.heap.gc_requested {
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

    pub fn vm_restore_stacks(
        &mut self,
        iter_len: u32,
        ref_len: u32,
    ) -> JsResult<Option<Box<Exception>>> {
        let mut ex = None;
        while self.vm.iter_stack.len() > iter_len as usize {
            let item = self.vm.iter_stack.pop();
            if let Some(item) = item
                && let Some(mut iter) = item.iter
            {
                iter.root(self);
                let r = self.vm_try_result(|rt| iter.return_iter(rt))?;
                if let Some(e) = r
                    && ex.is_none()
                {
                    ex = Some(e);
                }
            }
        }
        self.vm.ref_stack.truncate(ref_len as usize);
        Ok(ex)
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
        let roots_mark = self.extra_roots.len();
        if let Some(e) = &ex {
            self.extra_roots.push(e.val.clone());
        }
        loop {
            let Some(tf) = self.vm.try_stack.last_mut() else {
                break;
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
            if let Err(t) = self.vm_restore_stacks(iter_len, ref_len) {
                self.extra_roots.truncate(roots_mark);
                return Err(t);
            }

            if catch_pos == TRY_PANIC_MARKER {
                break;
            }
            if catch_pos >= 0 {
                let v = ex.as_ref().map(|e| e.val.clone()).unwrap_or_default();
                self.vm.push(v);
                self.vm.pc = catch_pos as isize;
                if let Some(tf) = self.vm.try_stack.last_mut() {
                    tf.catch_pos = -1;
                }
                self.extra_roots.truncate(roots_mark);
                return Ok(());
            }
            if finally_pos >= 0 {
                if let Some(tf) = self.vm.try_stack.last_mut() {
                    tf.exception = ex.take();
                    tf.finally_pos = -1;
                    tf.finally_ret = -1;
                }
                self.vm.pc = finally_pos as isize;
                self.extra_roots.truncate(roots_mark);
                return Ok(());
            }
        }
        self.extra_roots.truncate(roots_mark);
        match ex {
            None => Err(arg.unwrap_or(Throw::Value(Value::Undefined))),
            Some(e) => Err(Throw::Exception(e)),
        }
    }

    pub fn vm_try(
        &mut self,
        f: impl FnOnce(&mut Runtime) -> JsResult<()>,
    ) -> Option<Box<Exception>> {
        match self.vm_try_result(f) {
            Ok(r) => r,
            Err(t) => {
                self.pending_uncatchable = Some(t);
                None
            }
        }
    }

    pub fn vm_try_result(
        &mut self,
        f: impl FnOnce(&mut Runtime) -> JsResult<()>,
    ) -> JsResult<Option<Box<Exception>>> {
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        let out = match f(self) {
            Ok(()) => Ok(None),
            Err(t) => match self.vm_handle_throw(t) {
                Ok(()) => Ok(None),
                Err(Throw::Exception(e)) => Ok(Some(e)),
                Err(other) => Err(other),
            },
        };
        self.vm.pop_try_frame();
        out
    }

    pub fn vm_run_try(&mut self) -> JsResult<Option<Box<Exception>>> {
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        let r = self.vm_run_try_loop();
        self.vm.pop_try_frame();
        r
    }

    pub fn vm_run_try_loop(&mut self) -> JsResult<Option<Box<Exception>>> {
        let base = self.handles.len();
        loop {
            if self.handles.len() > base {
                self.handles.truncate(base);
            }
            if let Some(ex) = self.vm_run_try_inner()? {
                return Ok(Some(ex));
            }
            if self.vm.halted() {
                return Ok(None);
            }
        }
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

    pub fn set_max_call_stack_size(&mut self, size: usize) {
        self.vm.max_call_stack_size = size;
    }
    #[inline(always)]
    pub fn exec(&mut self, ins: &Instruction) -> JsResult<()> {
        match ins {
            Instruction::Placeholder => panic!("invalid instruction"),
            Instruction::LoadVal(v) => {
                self.vm.push(v.clone());
                self.vm.pc += 1;
            }
            Instruction::LoadUndef => {
                self.vm.push(Value::Undefined);
                self.vm.pc += 1;
            }
            Instruction::LoadNil => {
                self.vm.push(Value::Empty);
                self.vm.pc += 1;
            }
            Instruction::SaveResult => {
                self.vm.sp -= 1;
                self.vm.result = self.vm.stack[self.vm.sp as usize].clone();
                self.vm.pc += 1;
            }
            Instruction::LoadResult => {
                let v = self.vm.result.clone();
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::ClearResult => {
                self.vm.result = Value::Undefined;
                self.vm.pc += 1;
            }
            Instruction::LoadGlobalObject => {
                self.vm.push(Value::Object(self.global_object));
                self.vm.pc += 1;
            }
            Instruction::LoadStack(l) => self.vm_load_stack(*l),
            Instruction::LoadStack1(l) => self.vm_load_stack1(*l),
            Instruction::LoadStackLex(l) => return self.vm_load_stack_lex(*l),
            Instruction::LoadStack1Lex(l) => return self.vm_load_stack1_lex(*l),
            Instruction::LoadCallee => {
                let v = self.vm.stack[self.vm.sb as usize - 1].clone();
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::StoreStack(s) => self.vm_store_stack(*s),
            Instruction::StoreStack1(s) => self.vm_store_stack1(*s),
            Instruction::StoreStackLex(s) => self.vm_store_stack_lex(*s)?,
            Instruction::StoreStack1Lex(s) => self.vm_store_stack1_lex(*s)?,
            Instruction::InitStack(s) => self.vm_init_stack(*s),
            Instruction::InitStackP(s) => {
                self.vm_init_stack(*s);
                self.vm.sp -= 1;
            }
            Instruction::InitStack1(s) => self.vm_init_stack1(*s),
            Instruction::InitStack1P(s) => {
                self.vm_init_stack1(*s);
                self.vm.sp -= 1;
            }
            Instruction::StoreStackP(s) => {
                self.vm_store_stack(*s);
                self.vm.sp -= 1;
            }
            Instruction::StoreStack1P(s) => {
                self.vm_store_stack1(*s);
                self.vm.sp -= 1;
            }
            Instruction::StoreStackLexP(s) => {
                self.vm_store_stack_lex(*s)?;
                self.vm.sp -= 1;
            }
            Instruction::StoreStack1LexP(s) => {
                self.vm_store_stack1_lex(*s)?;
                self.vm.sp -= 1;
            }
            Instruction::ToNumber => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                self.vm.stack[p] = self.vm_to_numeric(&v)?;
                self.vm.pc += 1;
            }
            Instruction::Add => return self.exec_add(),
            Instruction::Sub => {
                let sp = self.vm.sp as usize;
                if let (Value::Int(l), Value::Int(r)) =
                    (&self.vm.stack[sp - 2], &self.vm.stack[sp - 1])
                {
                    let v = int_to_value(l.wrapping_sub(*r));
                    self.vm.stack[sp - 2].set(v);
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                    return Ok(());
                }
                return self.exec_numeric_binary(op_sub);
            }
            Instruction::Mul => return self.exec_numeric_binary(op_mul),
            Instruction::Exp => return self.exec_numeric_binary(op_exp),
            Instruction::Div => return self.exec_numeric_binary(op_div),
            Instruction::Mod => return self.exec_numeric_binary(op_mod),
            Instruction::Neg => return self.exec_neg(),
            Instruction::Plus => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                self.vm.stack[p] = v.to_number(self)?;
                self.vm.pc += 1;
            }
            Instruction::Inc => return self.exec_inc_dec(true),
            Instruction::Dec => return self.exec_inc_dec(false),
            Instruction::And => {
                return self
                    .exec_numeric_binary(|l, r| op_bitwise(l, r, big::Int::and, |a, b| a & b));
            }
            Instruction::Or => {
                return self
                    .exec_numeric_binary(|l, r| op_bitwise(l, r, big::Int::or, |a, b| a | b));
            }
            Instruction::Xor => {
                return self
                    .exec_numeric_binary(|l, r| op_bitwise(l, r, big::Int::xor, |a, b| a ^ b));
            }
            Instruction::Bnot => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                let n = self.vm_to_numeric(&v)?;
                self.vm.stack[p].set(op_bnot(&n)?);
                self.vm.pc += 1;
            }
            Instruction::Sal => return self.exec_numeric_binary(|l, r| op_shift(l, r, false)),
            Instruction::Sar => return self.exec_numeric_binary(|l, r| op_shift(l, r, true)),
            Instruction::Shr => return self.exec_numeric_binary(op_shr),
            Instruction::Jump(j) => self.vm.pc += *j as isize,
            Instruction::ToPropertyKey => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                self.vm.stack[p] = v.to_property_key(self)?;
                self.vm.pc += 1;
            }
            Instruction::ToString => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                self.vm.stack[p] = Value::String(v.to_string(self)?);
                self.vm.pc += 1;
            }
            Instruction::GetElemRef => return self.exec_get_elem_ref(false, false),
            Instruction::GetElemRefRecv => return self.exec_get_elem_ref(true, false),
            Instruction::GetElemRefStrict => return self.exec_get_elem_ref(false, true),
            Instruction::GetElemRefRecvStrict => return self.exec_get_elem_ref(true, true),
            Instruction::SetElem => return self.exec_set_elem(false),
            Instruction::SetElem1 => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 3].clone();
                let obj = self.vm_value_to_object(&v)?;
                let prop_name = self.vm.stack[sp - 2].clone();
                let val = self.vm.stack[sp - 1].clone();
                obj.set_own(self, &prop_name, val, true)?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::SetElem1Named => {
                let sp = self.vm.sp as usize;
                let receiver = self.vm.stack[sp - 3].clone();
                let base = self.vm_value_to_object(&receiver)?;
                let prop_name = self.vm.stack[sp - 2].clone();
                let val = self.vm.stack[sp - 1].clone();
                let f = self.to_object(&val)?;
                let name = Value::String(func_name(self, "", &prop_name));
                f.define_own_property_str(
                    self,
                    &STR_NAME,
                    PropertyDescriptor {
                        value: Some(name),
                        configurable: Flag::True,
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                base.set(self, &prop_name, val, &receiver, true)?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::DefineMethod(d) => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 3].clone();
                let obj = self.to_object(&o)?;
                let prop_name = self.vm.stack[sp - 2].clone();
                let m = self.vm.stack[sp - 1].clone();
                let method = self.to_object(&m)?;
                let name = Value::String(func_name(self, "", &prop_name));
                method.define_own_property_str(
                    self,
                    &STR_NAME,
                    PropertyDescriptor {
                        value: Some(name),
                        configurable: Flag::True,
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                obj.define_own_property(
                    self,
                    &prop_name,
                    PropertyDescriptor {
                        value: Some(Value::Object(method)),
                        writable: Flag::True,
                        configurable: Flag::True,
                        enumerable: to_flag(d.enumerable),
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::SetElemP => return self.exec_set_elem(true),
            Instruction::SetElemStrict => return self.exec_set_elem_strict(false),
            Instruction::SetElemRecv => return self.exec_set_elem_recv(false, false),
            Instruction::SetElemRecvStrict => return self.exec_set_elem_recv(true, false),
            Instruction::SetElemStrictP => return self.exec_set_elem_strict(true),
            Instruction::SetElemRecvP => return self.exec_set_elem_recv(false, true),
            Instruction::SetElemRecvStrictP => return self.exec_set_elem_recv(true, true),
            Instruction::DeleteElem => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 2].clone();
                let obj = self.vm_value_to_object(&v)?;
                let k = self.vm.stack[sp - 1].clone();
                let prop_name = k.to_property_key(self)?;
                let r = obj.delete(self, &prop_name, false)?;
                self.vm.stack[sp - 2].set(Value::Bool(r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DeleteElemStrict => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 2].clone();
                let obj = self.vm_value_to_object(&v)?;
                let k = self.vm.stack[sp - 1].clone();
                let prop_name = k.to_property_key(self)?;
                obj.delete(self, &prop_name, true)?;
                self.vm.stack[sp - 2].set(Value::Bool(true));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DeleteProp(name) => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                let obj = self.vm_value_to_object(&v)?;
                let r = obj.delete_str(self, name, false)?;
                self.vm.stack[p].set(Value::Bool(r));
                self.vm.pc += 1;
            }
            Instruction::DeletePropStrict(name) => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                let obj = self.vm_value_to_object(&v)?;
                obj.delete_str(self, name, true)?;
                self.vm.stack[p].set(Value::Bool(true));
                self.vm.pc += 1;
            }
            Instruction::GetPropRef(name) => return self.exec_get_prop_ref(name, false, false),
            Instruction::GetPropRefRecv(name) => return self.exec_get_prop_ref(name, true, false),
            Instruction::GetPropRefStrict(name) => {
                return self.exec_get_prop_ref(name, false, true);
            }
            Instruction::GetPropRefRecvStrict(name) => {
                return self.exec_get_prop_ref(name, true, true);
            }
            Instruction::SetProp(name) => {
                let sp = self.vm.sp as usize;
                let val = self.vm.stack[sp - 1].clone();
                let v = self.vm.stack[sp - 2].clone();
                let obj = self.vm_value_to_object(&v)?;
                obj.set_own_str(self, name, val.clone(), false)?;
                self.vm.stack[sp - 2].set(val);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::SetPropP(name) => {
                let sp = self.vm.sp as usize;
                let val = self.vm.stack[sp - 1].clone();
                let v = self.vm.stack[sp - 2].clone();
                let obj = self.vm_value_to_object(&v)?;
                obj.set_own_str(self, name, val, false)?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::SetPropStrict(name) => return self.exec_set_prop_strict(name, false),
            Instruction::SetPropRecv(name) => return self.exec_set_prop_recv(name, false, false),
            Instruction::SetPropRecvStrict(name) => {
                return self.exec_set_prop_recv(name, true, false);
            }
            Instruction::SetPropRecvP(name) => return self.exec_set_prop_recv(name, false, true),
            Instruction::SetPropRecvStrictP(name) => {
                return self.exec_set_prop_recv(name, true, true);
            }
            Instruction::SetPropStrictP(name) => return self.exec_set_prop_strict(name, true),
            Instruction::PutProp(name) => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 2].clone();
                let obj = self.to_object(&o)?;
                let v = self.vm.stack[sp - 1].clone();
                obj._put_prop(self, name, v, true, true, true);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DefinePropKeyed(name) => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 2].clone();
                let obj = self.to_object(&o)?;
                let v = self.vm.stack[sp - 1].clone();
                obj.define_own_property_str(
                    self,
                    name,
                    PropertyDescriptor {
                        value: Some(v),
                        writable: Flag::True,
                        configurable: Flag::True,
                        enumerable: Flag::True,
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DefineProp => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 3].clone();
                let obj = self.to_object(&o)?;
                let k = self.vm.stack[sp - 2].clone();
                let v = self.vm.stack[sp - 1].clone();
                obj.define_own_property(
                    self,
                    &k,
                    PropertyDescriptor {
                        value: Some(v),
                        writable: Flag::True,
                        configurable: Flag::True,
                        enumerable: Flag::True,
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::DefineMethodKeyed(d) => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 2].clone();
                let obj = self.to_object(&o)?;
                let m = self.vm.stack[sp - 1].clone();
                let method = self.to_object(&m)?;
                obj.define_own_property_str(
                    self,
                    &d.key,
                    PropertyDescriptor {
                        value: Some(Value::Object(method)),
                        writable: Flag::True,
                        configurable: Flag::True,
                        enumerable: to_flag(d.enumerable),
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::SetProto => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 2].clone();
                let p = self.vm.stack[sp - 1].clone();
                self.set_object_proto(&o, &p)?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DefineGetterKeyed(d) => return self.exec_define_accessor_keyed(d, true),
            Instruction::DefineSetterKeyed(d) => return self.exec_define_accessor_keyed(d, false),
            Instruction::DefineGetter(d) => return self.exec_define_accessor(d.enumerable, true),
            Instruction::DefineSetter(_) => return self.exec_define_accessor(true, false),
            Instruction::GetProp(name) => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot read property '{name}' of undefined"
                    ))));
                };
                let r = obj.get_str(self, name, Some(&v))?;
                self.vm.stack[p].set(nil_safe(r));
                self.vm.pc += 1;
            }
            Instruction::GetPropRecv(name) => {
                let sp = self.vm.sp as usize;
                let recv = self.vm.stack[sp - 2].clone();
                let v = self.vm.stack[sp - 1].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot read property '{name}' of undefined"
                    ))));
                };
                let r = obj.get_str(self, name, Some(&recv))?;
                self.vm.stack[sp - 2].set(nil_safe(r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::GetPropRecvCallee(name) => {
                let sp = self.vm.sp as usize;
                let recv = self.vm.stack[sp - 2].clone();
                let v = self.vm.stack[sp - 1].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot read property '{name}' of undefined"
                    ))));
                };
                let prop = match obj.get_str(self, name, Some(&recv))? {
                    Some(p) if !p.is_empty() => p,
                    _ => member_unresolved(name.clone()),
                };
                self.vm.stack[sp - 1].set(prop);
                self.vm.pc += 1;
            }
            Instruction::GetPropCallee(name) => {
                let p = self.vm.sp as usize - 1;
                let v = self.vm.stack[p].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot read property '{name}' of undefined or null"
                    ))));
                };
                let prop = match obj.get_str(self, name, Some(&v))? {
                    Some(p) if !p.is_empty() => p,
                    _ => member_unresolved(name.clone()),
                };
                self.vm.push(prop);
                self.vm.pc += 1;
            }
            Instruction::GetElem => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 2].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(self.cannot_read_property_of_undefined(sp - 1));
                };
                let k = self.vm.stack[sp - 1].clone();
                let prop_name = k.to_property_key(self)?;
                let r = obj.get(self, &prop_name, Some(&v))?;
                self.vm.stack[sp - 2].set(nil_safe(r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::GetElemRecv => {
                let sp = self.vm.sp as usize;
                let recv = self.vm.stack[sp - 3].clone();
                let v = self.vm.stack[sp - 1].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(self.cannot_read_property_of_undefined(sp - 2));
                };
                let k = self.vm.stack[sp - 2].clone();
                let prop_name = k.to_property_key(self)?;
                let r = obj.get(self, &prop_name, Some(&recv))?;
                self.vm.stack[sp - 3].set(nil_safe(r));
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::GetKey => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 2].clone();
                let obj = self.vm_base_object(&v)?;
                let prop_name = self.vm.stack[sp - 1].clone();
                let Some(obj) = obj else {
                    return Err(self.cannot_read_property_of_undefined(sp - 1));
                };
                let r = obj.get(self, &prop_name, Some(&v))?;
                self.vm.stack[sp - 2].set(nil_safe(r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::GetElemCallee => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 2].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(self.cannot_read_property_of_undefined(sp - 1));
                };
                let k = self.vm.stack[sp - 1].clone();
                let prop_name = k.to_property_key(self)?;
                let prop = match obj.get(self, &prop_name, Some(&v))? {
                    Some(p) if !p.is_empty() => p,
                    _ => {
                        let n = prop_name.string(self)?;
                        member_unresolved(n)
                    }
                };
                self.vm.stack[sp - 1].set(prop);
                self.vm.pc += 1;
            }
            Instruction::GetElemRecvCallee => {
                let sp = self.vm.sp as usize;
                let recv = self.vm.stack[sp - 3].clone();
                let v = self.vm.stack[sp - 2].clone();
                let Some(obj) = self.vm_base_object(&v)? else {
                    return Err(self.cannot_read_property_of_undefined(sp - 1));
                };
                let k = self.vm.stack[sp - 1].clone();
                let prop_name = k.to_property_key(self)?;
                let prop = match obj.get(self, &prop_name, Some(&recv))? {
                    Some(p) if !p.is_empty() => p,
                    _ => {
                        let n = prop_name.string(self)?;
                        member_unresolved(n)
                    }
                };
                self.vm.stack[sp - 2].set(prop);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::Dup => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::DupN(d) => {
                let v = self.vm.stack[self.vm.sp as usize - 1 - *d as usize].clone();
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::RdupN(d) => {
                let sp = self.vm.sp as usize;
                self.vm.stack[sp - 1 - *d as usize] = self.vm.stack[sp - 1].clone();
                self.vm.pc += 1;
            }
            Instruction::DupLast(d) => {
                let d = *d as usize;
                let sp = self.vm.sp as usize;
                let e = sp + d;
                self.vm.stack_expand(e as isize);
                for i in 0..d {
                    self.vm.stack[sp + i] = self.vm.stack[sp - d + i].clone();
                }
                self.vm.sp = e as isize;
                self.vm.pc += 1;
            }
            Instruction::NewObject => {
                let o = self.new_object();
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::NewArray(l) => {
                let o = self.new_array_values(Vec::with_capacity(*l as usize));
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::PushArrayItem => return self.exec_push_array_item(),
            Instruction::PushArraySpread => return self.exec_push_array_spread(),
            Instruction::PushSpread => return self.exec_push_spread(),
            Instruction::NewArrayFromIter => return self.exec_new_array_from_iter(),
            Instruction::NewRegexp(n) => {
                let o = self.new_reg_expp_value(n)?;
                self.vm.push(o);
                self.vm.pc += 1;
            }
            Instruction::StoreStash(s) | Instruction::InitStash(s) => self.vm_init_local(*s),
            Instruction::StoreStashP(s) | Instruction::InitStashP(s) => {
                self.vm_init_local(*s);
                self.vm.sp -= 1;
            }
            Instruction::StoreStashLex(s) => self.vm_set_local_lex(*s)?,
            Instruction::StoreStashLexP(s) => {
                self.vm_set_local_lex(*s)?;
                self.vm.sp -= 1;
            }
            Instruction::InitGlobalP(name) => {
                self.vm.sp -= 1;
                let v = self.vm.stack[self.vm.sp as usize].clone();
                let gs = self.global.stash;
                stash_init_by_name(self, gs, name, v)?;
                self.vm.pc += 1;
            }
            Instruction::InitGlobal(name) => {
                let v = self.vm.stack[self.vm.sp as usize].clone();
                let gs = self.global.stash;
                stash_init_by_name(self, gs, name, v)?;
                self.vm.pc += 1;
            }
            Instruction::ResolveVar1(name) => return self.exec_resolve_var1(name, false),
            Instruction::ResolveVar1Strict(name) => return self.exec_resolve_var1(name, true),
            Instruction::DeleteVar(name) => return self.exec_delete_var(name),
            Instruction::DeleteGlobal(name) => {
                let g = self.global_object;
                let ret = if g.has_property_str(self, name)? {
                    g.delete_str(self, name, false)?
                } else {
                    true
                };
                self.vm.push(Value::Bool(ret));
                self.vm.pc += 1;
            }
            Instruction::SetGlobal(name) => {
                let v = self.vm.peek();
                self.set_global(name, v, false)?;
                self.vm.pc += 1;
            }
            Instruction::SetGlobalStrict(name) => {
                let v = self.vm.peek();
                self.set_global(name, v, true)?;
                self.vm.pc += 1;
            }
            Instruction::LoadStash(g) => {
                let (s, idx) = self.vm_stash_slot(*g);
                let v = self.heap.stash(s).values[idx].clone();
                self.vm
                    .push(if v.is_empty() { Value::Undefined } else { v });
                self.vm.pc += 1;
            }
            Instruction::LoadStashLex(g) => {
                let (s, idx) = self.vm_stash_slot(*g);
                let v = self.heap.stash(s).values[idx].clone();
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::LoadMixed(g) => return self.exec_load_mixed(g, false),
            Instruction::LoadMixedLex(g) => return self.exec_load_mixed(g, true),
            Instruction::LoadMixedStack(g) => return self.exec_load_mixed_stack(g, 0),
            Instruction::LoadMixedStack1(g) => return self.exec_load_mixed_stack(g, 1),
            Instruction::LoadMixedStackLex(g) => return self.exec_load_mixed_stack(g, 2),
            Instruction::LoadMixedStack1Lex(g) => return self.exec_load_mixed_stack(g, 3),
            Instruction::ResolveMixed(r) => return self.exec_resolve_mixed(r),
            Instruction::ResolveMixedStack(r) => return self.exec_resolve_mixed_stack(r, false),
            Instruction::ResolveMixedStack1(r) => return self.exec_resolve_mixed_stack(r, true),
            Instruction::GetValue => {
                let l = self.vm.ref_stack.len() - 1;
                self.vm_ref_convert_key(l)?;
                let mut r = self.vm.ref_stack.pop().expect("ref");
                r.root(self);
                let v = r.get(self);
                self.vm.ref_stack.push(r);
                self.vm.push(v?);
                self.vm.pc += 1;
            }
            Instruction::PutValue => {
                let mut r = self.vm_pop_ref()?;
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                r.set(self, v)?;
                self.vm.pc += 1;
            }
            Instruction::PopRef => {
                self.vm.ref_stack.pop();
                self.vm.pc += 1;
            }
            Instruction::PutValueP => {
                let mut r = self.vm_pop_ref()?;
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                r.set(self, v)?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::InitValueP => {
                let mut r = self.vm_pop_ref()?;
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                r.init(self, v)?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::LoadDynamic(name) => {
                let mut val = None;
                let mut s = self.vm.stash;
                while let Some(st) = s {
                    if let Some(v) = stash_get_by_name(self, st, name)? {
                        val = Some(v);
                        break;
                    }
                    s = self.heap.stash(st).outer;
                }
                let val = match val {
                    Some(v) => v,
                    None => {
                        let g = self.global_object;
                        match g.get_str(self, name, None)? {
                            Some(v) if !v.is_empty() => v,
                            _ => return self.throw_reference_error(name),
                        }
                    }
                };
                self.vm.push(val);
                self.vm.pc += 1;
            }
            Instruction::LoadDynamicRef(name) => {
                let mut val = None;
                let mut s = self.vm.stash;
                while let Some(st) = s {
                    if let Some(v) = stash_get_by_name(self, st, name)? {
                        val = Some(v);
                        break;
                    }
                    s = self.heap.stash(st).outer;
                }
                let val = match val {
                    Some(v) => v,
                    None => {
                        let g = self.global_object;
                        match g.get_str(self, name, None)? {
                            Some(v) if !v.is_empty() => v,
                            _ => value_unresolved(name.clone()),
                        }
                    }
                };
                self.vm.push(val);
                self.vm.pc += 1;
            }
            Instruction::LoadDynamicCallee(name) => {
                let mut val = None;
                let mut callee = None;
                let mut s = self.vm.stash;
                while let Some(st) = s {
                    if let Some(v) = stash_get_by_name(self, st, name)? {
                        callee = self.heap.stash(st).obj;
                        val = Some(v);
                        break;
                    }
                    s = self.heap.stash(st).outer;
                }
                let val = match val {
                    Some(v) => v,
                    None => {
                        let g = self.global_object;
                        match g.get_str(self, name, None)? {
                            Some(v) if !v.is_empty() => v,
                            _ => value_unresolved(name.clone()),
                        }
                    }
                };
                match callee {
                    Some(c) => self.vm.push(Value::Object(c)),
                    None => self.vm.push(Value::Undefined),
                }
                self.vm.push(val);
                self.vm.pc += 1;
            }
            Instruction::Pop => {
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::CallEval(n) => return self.vm_call_eval(*n as usize, false),
            Instruction::CallEvalStrict(n) => return self.vm_call_eval(*n as usize, true),
            Instruction::CallEvalVariadic => {
                let n = self.vm_count_variadic_args() - 2;
                return self.vm_call_eval(n, false);
            }
            Instruction::CallEvalVariadicStrict => {
                let n = self.vm_count_variadic_args() - 2;
                return self.vm_call_eval(n, true);
            }
            Instruction::BoxThis => {
                let sb = self.vm.sb as usize;
                let v = self.vm.stack[sb].clone();
                self.vm.stack[sb] = if matches!(v, Value::Undefined | Value::Null) {
                    Value::Object(self.global_object)
                } else {
                    Value::Object(v.to_object(self)?)
                };
                self.vm.pc += 1;
            }
            Instruction::StartVariadic => {
                self.vm.push(Value::Symbol(VARIADIC_MARKER));
                self.vm.pc += 1;
            }
            Instruction::CallVariadic => {
                let n = self.vm_count_variadic_args() - 2;
                return self.exec_call(n);
            }
            Instruction::EndVariadic => {
                self.vm.sp -= 1;
                let sp = self.vm.sp as usize;
                self.vm.stack[sp - 1] = self.vm.stack[sp].clone();
                self.vm.pc += 1;
            }
            Instruction::Call(n) => return self.exec_call(*n as usize),
            Instruction::EnterBlock(e) => self.exec_enter_block(e),
            Instruction::EnterCatchBlock(e) => self.exec_enter_catch_block(e),
            Instruction::LeaveBlock(l) => {
                if l.pop_stash
                    && let Some(s) = self.vm.stash
                {
                    self.vm.stash = self.heap.stash(s).outer;
                }
                if l.stack_size > 0 {
                    self.vm.sp -= l.stack_size as isize;
                }
                self.vm.pc += 1;
            }
            Instruction::EnterFunc(e) => self.exec_enter_func(e),
            Instruction::EnterFunc1(e) => self.exec_enter_func1(e),
            Instruction::EnterFuncBody(e) => self.exec_enter_func_body(e),
            Instruction::Ret => self.exec_ret(),
            Instruction::Cret(c) => {
                let (s, idx) = self.vm_stash_slot(*c);
                let v = self.heap.stash(s).values[idx].clone();
                let sb = self.vm.sb as usize;
                self.vm.stack[sb].set(v);
                self.exec_ret();
            }
            Instruction::EnterFuncStashless(e) => self.exec_enter_func_stashless(e),
            Instruction::NewFunc(n) => {
                let o = self.new_func(&n.name, n.length, n.strict);
                self.vm_new_func_init(o, n);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::NewAsyncFunc(n) => {
                let o = self.new_async_func(&n.name, n.length, n.strict);
                self.vm_new_func_init(o, n);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::NewGeneratorFunc(n) => {
                let o = self.new_generator_func(&n.name, n.length, n.strict);
                self.vm_new_func_init(o, n);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::NewMethod(n) => {
                let o = self.new_method(&n.new_func.name, n.new_func.length, n.new_func.strict);
                return self.exec_new_method(o, n);
            }
            Instruction::NewAsyncMethod(n) => {
                let o =
                    self.new_async_method(&n.new_func.name, n.new_func.length, n.new_func.strict);
                return self.exec_new_method(o, n);
            }
            Instruction::NewGeneratorMethod(n) => {
                let o = self.new_generator_method(
                    &n.new_func.name,
                    n.new_func.length,
                    n.new_func.strict,
                );
                return self.exec_new_method(o, n);
            }
            Instruction::NewArrowFunc(n) => {
                let o = self.new_arrow_func(&n.name, n.length, n.strict);
                return self.exec_new_arrow_func(o, n);
            }
            Instruction::NewAsyncArrowFunc(n) => {
                let o = self.new_async_arrow_func(&n.name, n.length, n.strict);
                return self.exec_new_arrow_func(o, n);
            }
            Instruction::BindVars(d) => return self.exec_bind_vars(d),
            Instruction::BindGlobal(b) => return self.exec_bind_global(b),
            Instruction::JneP(j) => {
                self.vm.sp -= 1;
                if !self.vm.stack[self.vm.sp as usize].to_boolean() {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.pc += 1;
                }
            }
            Instruction::JeqP(j) => {
                self.vm.sp -= 1;
                if self.vm.stack[self.vm.sp as usize].to_boolean() {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.pc += 1;
                }
            }
            Instruction::Jeq(j) => {
                if self.vm.stack[self.vm.sp as usize - 1].to_boolean() {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                }
            }
            Instruction::Jne(j) => {
                if !self.vm.stack[self.vm.sp as usize - 1].to_boolean() {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                }
            }
            Instruction::Jdef(j) => {
                if !matches!(self.vm.stack[self.vm.sp as usize - 1], Value::Undefined) {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                }
            }
            Instruction::JdefP(j) => {
                if !matches!(self.vm.stack[self.vm.sp as usize - 1], Value::Undefined) {
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.pc += 1;
                }
                self.vm.sp -= 1;
            }
            Instruction::Jopt(j) => {
                let p = self.vm.sp as usize - 1;
                match self.vm.stack[p] {
                    Value::Null => {
                        self.vm.stack[p].set(Value::Undefined);
                        self.vm.pc += *j as isize;
                    }
                    Value::Undefined => self.vm.pc += *j as isize,
                    _ => self.vm.pc += 1,
                }
            }
            Instruction::Joptc(j) => {
                let p = self.vm.sp as usize - 1;
                let nullish = match &self.vm.stack[p] {
                    Value::Null | Value::Undefined => true,
                    Value::Unresolved(u) => u.member,
                    _ => false,
                };
                if nullish {
                    self.vm.sp -= 1;
                    let p = self.vm.sp as usize - 1;
                    self.vm.stack[p].set(Value::Undefined);
                    self.vm.pc += *j as isize;
                } else {
                    self.vm.pc += 1;
                }
            }
            Instruction::Jcoalesc(j) => {
                if matches!(
                    self.vm.stack[self.vm.sp as usize - 1],
                    Value::Undefined | Value::Null
                ) {
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                } else {
                    self.vm.pc += *j as isize;
                }
            }
            Instruction::JcoalescP(j) => {
                self.vm.sp -= 1;
                if matches!(
                    self.vm.stack[self.vm.sp as usize],
                    Value::Undefined | Value::Null
                ) {
                    self.vm.pc += 1;
                } else {
                    self.vm.pc += *j as isize;
                }
            }
            Instruction::Not => {
                let p = self.vm.sp as usize - 1;
                self.vm.stack[p] = Value::Bool(!self.vm.stack[p].to_boolean());
                self.vm.pc += 1;
            }
            Instruction::OpLt => return self.exec_cmp(false, |r| r == Some(true)),
            Instruction::OpLte => return self.exec_cmp(true, |r| r == Some(false)),
            Instruction::OpGt => return self.exec_cmp(true, |r| r == Some(true)),
            Instruction::OpGte => return self.exec_cmp(false, |r| r == Some(false)),
            Instruction::OpEq => {
                let sp = self.vm.sp as usize;
                let l = self.vm.stack[sp - 2].clone();
                let r = self.vm.stack[sp - 1].clone();
                self.vm.stack[sp - 2] = Value::Bool(l.equals(self, &r)?);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::OpNeq => {
                let sp = self.vm.sp as usize;
                let l = self.vm.stack[sp - 2].clone();
                let r = self.vm.stack[sp - 1].clone();
                self.vm.stack[sp - 2] = Value::Bool(!l.equals(self, &r)?);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::OpStrictEq => {
                let sp = self.vm.sp as usize;
                let r = self.vm.stack[sp - 2].strict_equals(&self.vm.stack[sp - 1]);
                self.vm.stack[sp - 2].set(Value::Bool(r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::OpStrictNeq => {
                let sp = self.vm.sp as usize;
                let r = self.vm.stack[sp - 2].strict_equals(&self.vm.stack[sp - 1]);
                self.vm.stack[sp - 2].set(Value::Bool(!r));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::OpInstanceof => {
                let sp = self.vm.sp as usize;
                let left = self.vm.stack[sp - 2].clone();
                let r = self.vm.stack[sp - 1].clone();
                let right = self.to_object(&r)?;
                let res = instance_of_operator(self, &left, right)?;
                self.vm.stack[sp - 2].set(Value::Bool(res));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::OpIn => {
                let sp = self.vm.sp as usize;
                let left = self.vm.stack[sp - 2].clone();
                let r = self.vm.stack[sp - 1].clone();
                let right = self.to_object(&r)?;
                let res = right.has_property(self, &left)?;
                self.vm.stack[sp - 2].set(Value::Bool(res));
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::Try(t) => {
                let catch_pos = if t.catch_offset > 0 {
                    self.vm.pc as i32 + t.catch_offset
                } else {
                    -1
                };
                let finally_pos = if t.finally_offset > 0 {
                    self.vm.pc as i32 + t.finally_offset
                } else {
                    -1
                };
                self.vm.push_try_frame(catch_pos, finally_pos);
                self.vm.pc += 1;
            }
            Instruction::LeaveTry => {
                let pc = self.vm.pc;
                let tf = self.vm.try_stack.last_mut().expect("try frame");
                if tf.finally_pos >= 0 {
                    tf.finally_ret = (pc + 1) as i32;
                    let fp = tf.finally_pos;
                    tf.finally_pos = -1;
                    tf.catch_pos = -1;
                    self.vm.pc = fp as isize;
                } else {
                    self.vm.pop_try_frame();
                    self.vm.pc += 1;
                }
            }
            Instruction::EnterFinally => {
                let tf = self.vm.try_stack.last_mut().expect("try frame");
                tf.finally_pos = -1;
                self.vm.pc += 1;
            }
            Instruction::LeaveFinally => {
                let tf = self.vm.try_stack.last_mut().expect("try frame");
                let ex = tf.exception.take();
                let ret = tf.finally_ret;
                self.vm.pop_try_frame();
                if let Some(ex) = ex {
                    return Err(Throw::Exception(ex));
                }
                if ret != -1 {
                    self.vm.pc = ret as isize;
                } else {
                    self.vm.pc += 1;
                }
            }
            Instruction::Throw => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let mut ex = Exception::new(v);
                if let Value::Object(o) = &ex.val
                    && let ObjExt::Error(e) = &self.obj(*o).ext
                    && !e.stack.is_empty()
                {
                    ex.stack = e.stack.clone();
                }
                if ex.stack.is_empty() {
                    ex.stack = self.vm_capture_stack(0);
                }
                return Err(Throw::Exception(Box::new(ex)));
            }
            Instruction::NewVariadic => {
                let n = self.vm_count_variadic_args() - 1;
                return self.exec_new(n);
            }
            Instruction::New(n) => return self.exec_new(*n as usize),
            Instruction::SuperCall(s) => return self.exec_super_call(*s as usize),
            Instruction::SuperCallVariadic => {
                let n = self.vm_count_variadic_args();
                return self.exec_super_call(n);
            }
            Instruction::LoadNewTarget => {
                let t = self.vm.new_target.clone();
                self.vm
                    .push(if t.is_empty() { Value::Undefined } else { t });
                self.vm.pc += 1;
            }
            Instruction::Typeof => {
                let p = self.vm.sp as usize - 1;
                let r = match &self.vm.stack[p] {
                    Value::Undefined => STRING_UNDEFINED.clone(),
                    Value::Unresolved(u) if !u.member => STRING_UNDEFINED.clone(),
                    Value::Null => STRING_OBJECT_C.clone(),
                    Value::Object(o) => o.type_of(self),
                    Value::Bool(_) => STRING_BOOLEAN.clone(),
                    Value::String(_) => STRING_STRING.clone(),
                    Value::Int(_) | Value::Float(_) => STRING_NUMBER.clone(),
                    Value::BigInt(_) => STRING_BIG_INT.clone(),
                    Value::Symbol(_) => STRING_SYMBOL.clone(),
                    v => {
                        return Err(Throw::TypeError(format!(
                            "Compiler bug: unknown type: {}",
                            go_type_of_value(v)
                        )));
                    }
                };
                self.vm.stack[p].set(Value::String(r));
                self.vm.pc += 1;
            }
            Instruction::CreateArgsMapped(n) => {
                let v = self.create_args_mapped(*n as usize);
                self.vm.push(Value::Object(v));
                self.vm.pc += 1;
            }
            Instruction::CreateArgsUnmapped(n) => {
                let v = self.create_args_unmapped(*n as usize);
                self.vm.push(Value::Object(v));
                self.vm.pc += 1;
            }
            Instruction::EnterWith => {
                self.vm_new_stash();
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let o = v.to_object(self)?;
                if let Some(s) = self.vm.stash {
                    self.heap.stash_mut(s).obj = Some(o);
                }
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::LeaveWith => {
                if let Some(s) = self.vm.stash {
                    self.vm.stash = self.heap.stash(s).outer;
                }
                self.vm.pc += 1;
            }
            Instruction::Enumerate => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                if matches!(v, Value::Undefined | Value::Null) {
                    self.vm.iter_stack.push(IterStackItem {
                        f: Some(Box::new(EmptyIter)),
                        ..IterStackItem::default()
                    });
                } else {
                    let o = self.vm_value_to_object(&v)?;
                    let f = enumerate_recursive(self, o)?;
                    self.vm.iter_stack.push(IterStackItem {
                        f: Some(f),
                        obj: Value::Object(o),
                        ..IterStackItem::default()
                    });
                }
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::EnumNext(jmp) => {
                let l = self.vm.iter_stack.len() - 1;
                let mut f = self.vm.iter_stack[l].f.take();
                let item = match &mut f {
                    Some(f) => f.next(self),
                    None => Ok(None),
                };
                if l < self.vm.iter_stack.len() {
                    self.vm.iter_stack[l].f = f;
                }
                match item? {
                    Some(item) => {
                        self.vm.iter_stack[l].val = item.name;
                        self.vm.pc += 1;
                    }
                    None => {
                        self.vm.iter_stack[l].f = None;
                        self.vm.pc += *jmp as isize;
                    }
                }
            }
            Instruction::EnumGet => {
                let v = self.vm.iter_stack[self.vm.iter_stack.len() - 1].val.clone();
                self.vm.push(v);
                self.vm.pc += 1;
            }
            Instruction::EnumPop => {
                self.vm.iter_stack.pop();
                self.vm.pc += 1;
            }
            Instruction::EnumPopClose => {
                let item = self.vm.iter_stack.pop();
                if let Some(item) = item
                    && let Some(mut iter) = item.iter
                {
                    if let Some(i) = iter.iterator {
                        self.handles.push(i);
                    }
                    iter.return_iter(self)?;
                }
                self.vm.pc += 1;
            }
            Instruction::IterateP => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let iter = self.vm_get_iterator(&v)?;
                self.vm.iter_stack.push(IterStackItem {
                    iter: Some(iter),
                    ..IterStackItem::default()
                });
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::Iterate => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let iter = self.vm_get_iterator(&v)?;
                self.vm.iter_stack.push(IterStackItem {
                    iter: Some(iter),
                    ..IterStackItem::default()
                });
                self.vm.pc += 1;
            }
            Instruction::IterNext(jmp) => {
                let l = self.vm.iter_stack.len() - 1;
                let (value, ex) = self.vm_iter_step(l)?;
                match ex {
                    None => match value {
                        None => self.vm.pc += *jmp as isize,
                        Some(v) => {
                            self.vm.iter_stack[l].val = v;
                            self.vm.pc += 1;
                        }
                    },
                    Some(ex) => {
                        self.vm.iter_stack.pop();
                        return Err(Throw::Value(ex.val));
                    }
                }
            }
            Instruction::IterGetNextOrUndef => {
                let l = self.vm.iter_stack.len() - 1;
                let mut value = None;
                let has_iterator = self.vm.iter_stack[l]
                    .iter
                    .as_ref()
                    .is_some_and(|i| i.iterator.is_some());
                if has_iterator {
                    let (v, ex) = self.vm_iter_step(l)?;
                    if let Some(ex) = ex {
                        self.vm.iter_stack.pop();
                        return Err(Throw::Value(ex.val));
                    }
                    value = v;
                }
                self.vm.push(nil_safe(value));
                self.vm.pc += 1;
            }
            Instruction::CopyStash => {
                let old = self.vm.stash.expect("stash");
                let (outer, values, names) = {
                    let st = self.heap.stash(old);
                    (st.outer, st.values.clone(), st.names.clone())
                };
                let s = self.heap.alloc_stash(Stash {
                    outer,
                    values,
                    names,
                    ..Stash::default()
                });
                self.vm.stash_allocs += 1;
                self.vm.stash = Some(s);
                self.vm.pc += 1;
            }
            Instruction::ThrowAssignToConst => {
                return Err(Throw::TypeError(ERR_ASSIGN_TO_CONST.to_string()));
            }
            Instruction::CopySpread => {
                let sp = self.vm.sp as usize;
                let t = self.vm.stack[sp - 2].clone();
                let s = self.vm.stack[sp - 1].clone();
                self.copy_data_properties(&t, &s)?;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::CopyRest => {
                let o = self.new_object();
                self.vm.push(Value::Object(o));
                let sp = self.vm.sp as usize;
                let s = self.vm.stack[sp - 2].clone();
                self.copy_data_properties(&Value::Object(o), &s)?;
                self.vm.pc += 1;
            }
            Instruction::CreateDestructSrc => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                self.check_object_coercible(&v)?;
                let o = self.new_destruct_keyed_source(v);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::CheckObjectCoercible => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                self.check_object_coercible(&v)?;
                self.vm.pc += 1;
            }
            Instruction::CreateArgsRestStack(n) => {
                let n = *n;
                let delta = self.vm.args - n;
                let mut values = Vec::new();
                if delta > 0 {
                    let start = (self.vm.sb + n + 1) as usize;
                    values.extend_from_slice(&self.vm.stack[start..start + delta as usize]);
                }
                let o = self.new_array_values(values);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::CreateArgsRestStash => {
                let s = self.vm.stash.expect("stash");
                let values = std::mem::take(&mut self.heap.stash_mut(s).extra_args);
                let o = self.new_array_values(values);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::ConcatStrings(n) => self.exec_concat_strings(*n as usize),
            Instruction::GetTaggedTmplObject(c) => {
                let o = self.exec_get_tagged_tmpl_object(c);
                self.vm.push(Value::Object(o));
                self.vm.pc += 1;
            }
            Instruction::LoadSuper => {
                let callee = self.vm.stack[self.vm.sb as usize - 1].clone();
                let home_object = self.get_home_object(&callee)?;
                match home_object.proto(self)? {
                    Some(p) => self.vm.push(Value::Object(p)),
                    None => self.vm.push(Value::Undefined),
                }
                self.vm.pc += 1;
            }
            Instruction::NewClass(c) => {
                let op = self.global.object_prototype;
                let fp = self.get_function_prototype();
                let (proto, cls) = self.vm_new_class_create(c, op, fp, false);
                let sp = self.vm.sp;
                self.vm.stack_expand(sp + 1);
                self.vm.stack[sp as usize].set(Value::Object(proto));
                self.vm.stack[sp as usize + 1].set(Value::Object(cls));
                self.vm.sp = sp + 2;
                self.vm.pc += 1;
            }
            Instruction::NewDerivedClass(c) => return self.exec_new_derived_class(c),
            Instruction::NewStaticFieldInit(c) => {
                let fp = self.get_function_prototype();
                let f = self.new_class_func(&unistring::String::default(), 0, Some(fp), false);
                if c.num_private_fields > 0 || c.num_private_methods > 0 {
                    self.vm_create_private_type(f, c.num_private_fields, c.num_private_methods);
                }
                let stash = self.vm.stash;
                let d = BaseJsFuncObject::data_mut(self, f);
                d.init_fields = c.init_fields.clone();
                d.stash = stash;
                self.vm.push(Value::Object(f));
                self.vm.pc += 1;
            }
            Instruction::LoadThisStash(l) => {
                let (s, idx) = self.vm_stash_slot(*l);
                let v = self.heap.stash(s).values[idx].clone();
                return self.vm_load_this(v);
            }
            Instruction::LoadThisStack => {
                let v = self.vm.stack[self.vm.sb as usize].clone();
                return self.vm_load_this(v);
            }
            Instruction::GetThisDynamic => {
                let this_name = STR_THIS_BINDING.clone();
                let mut s = self.vm.stash;
                while let Some(st) = s {
                    if self.heap.stash(st).obj.is_none()
                        && let Some(v) = stash_get_by_name(self, st, &this_name)?
                    {
                        self.vm.push(v);
                        self.vm.pc += 1;
                        return Ok(());
                    }
                    s = self.heap.stash(st).outer;
                }
                self.vm.push(Value::Object(self.global_object));
                self.vm.pc += 1;
            }
            Instruction::ThrowConst(e) => return Err(Throw::from(e.clone())),
            Instruction::ResolveThisStack => {
                let idx = self.vm.sb as usize;
                self.vm.ref_stack.push(Ref::StackThis { idx });
                self.vm.pc += 1;
            }
            Instruction::ResolveThisStash(r) => {
                let (stash, idx) = self.vm_stash_slot(*r);
                self.vm.ref_stack.push(Ref::StashThis { stash, idx });
                self.vm.pc += 1;
            }
            Instruction::ResolveThisDynamic => {
                let this_name = STR_THIS_BINDING.clone();
                let mut s = self.vm.stash;
                while let Some(st) = s {
                    let stash = self.heap.stash(st);
                    if stash.obj.is_none()
                        && let Some(idx) = stash.names.as_ref().and_then(|n| n.get(&this_name))
                    {
                        let idx = (*idx & !MASK_TYP) as usize;
                        self.vm.ref_stack.push(Ref::StashThis { stash: st, idx });
                        self.vm.pc += 1;
                        return Ok(());
                    }
                    s = stash.outer;
                }
                let e = self.get_reference_error();
                return Err(Throw::Value(self.new_error(
                    e,
                    "Compiler bug: 'this' reference is not found in resolveThisDynamic",
                )));
            }
            Instruction::DefineComputedKey(offset) => {
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - *offset as usize].clone();
                let obj = self.to_object(&o)?;
                if self.is_class_func(obj) {
                    let k = self.vm.stack[sp - 1].clone();
                    let key = k.to_property_key(self)?;
                    BaseJsFuncObject::data_mut(self, obj)
                        .computed_keys
                        .push(key);
                    self.vm.sp -= 1;
                    self.vm.pc += 1;
                    return Ok(());
                }
                let s = Value::Object(obj).go_string(self)?;
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Compiler bug: unexpected target for defineComputedKey: {s}"
                ))));
            }
            Instruction::LoadComputedKey(idx) => {
                let o = self.vm.stack[self.vm.sb as usize - 1].clone();
                let obj = self.to_object(&o)?;
                if self.is_class_func(obj) {
                    let v = BaseJsFuncObject::data(self, obj).computed_keys[*idx as usize].clone();
                    self.vm.push(v);
                    self.vm.pc += 1;
                    return Ok(());
                }
                let s = Value::Object(obj).go_string(self)?;
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Compiler bug: unexpected target for loadComputedKey: {s}"
                ))));
            }
            Instruction::InitStaticElements(i) => return self.exec_init_static_elements(i),
            Instruction::DefinePrivateMethod(d) => {
                let methods_owner = self.vm_private_methods_owner(d)?;
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 1].clone();
                BaseJsFuncObject::data_mut(self, methods_owner).private_methods[d.idx as usize] = v;
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::DefinePrivateGetter(d) => {
                return self.exec_define_private_accessor(d, true);
            }
            Instruction::DefinePrivateSetter(d) => {
                return self.exec_define_private_accessor(d, false);
            }
            Instruction::DefinePrivateProp(d) => {
                let callee = self.vm.stack[self.vm.sb as usize - 1].clone();
                let f = self.to_object(&callee)?;
                let typ = BaseJsFuncObject::data(self, f).private_env_type.clone();
                let sp = self.vm.sp as usize;
                let o = self.vm.stack[sp - 2].clone();
                let obj = self.to_object(&o)?;
                let v = self.vm.stack[sp - 1].clone();
                let typ = typ.expect("private env type");
                obj.get_private_env(self, &typ, false)?;
                let key = private_env_key(&typ);
                if let Some(m) = &mut self.obj_mut(obj).base.private_elements
                    && let Some(e) = m.get_mut(&key)
                {
                    e.fields[d.idx as usize] = v;
                }
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::GetPrivatePropRes(g) => {
                let p = self.vm.sp as usize - 1;
                let base = self.vm.stack[p].clone();
                self.vm.stack[p] = get_private_prop_res(self, g, &base)?;
                self.vm.pc += 1;
            }
            Instruction::GetPrivatePropId(g) => {
                let p = self.vm.sp as usize - 1;
                let base = self.vm.stack[p].clone();
                self.vm.stack[p] =
                    self.vm_get_private_prop(&base, &g.name, Some(&g.typ), g.idx, g.is_method)?;
                self.vm.pc += 1;
            }
            Instruction::GetPrivatePropIdCallee(g) => {
                let base = self.vm.stack[self.vm.sp as usize - 1].clone();
                let mut prop =
                    self.vm_get_private_prop(&base, &g.name, Some(&g.typ), g.idx, g.is_method)?;
                if prop.is_empty() {
                    prop = member_unresolved(g.string());
                }
                self.vm.push(prop);
                self.vm.pc += 1;
            }
            Instruction::GetPrivatePropResCallee(g) => {
                let base = self.vm.stack[self.vm.sp as usize - 1].clone();
                let mut prop = get_private_prop_res(self, g, &base)?;
                if prop.is_empty() {
                    prop = member_unresolved(g.string());
                }
                self.vm.push(prop);
                self.vm.pc += 1;
            }
            Instruction::SetPrivatePropRes(p) => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 1].clone();
                let base = self.vm.stack[sp - 2].clone();
                set_private_prop_res(self, p, &base, v.clone())?;
                self.vm.stack[sp - 2].set(v);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::SetPrivatePropResP(p) => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 1].clone();
                let base = self.vm.stack[sp - 2].clone();
                set_private_prop_res(self, p, &base, v)?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::SetPrivatePropId(p) => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 1].clone();
                let base = self.vm.stack[sp - 2].clone();
                self.vm_set_private_prop(
                    &base,
                    &p.name,
                    Some(&p.typ),
                    p.idx,
                    p.is_method,
                    v.clone(),
                )?;
                self.vm.stack[sp - 2].set(v);
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::SetPrivatePropIdP(p) => {
                let sp = self.vm.sp as usize;
                let v = self.vm.stack[sp - 1].clone();
                let base = self.vm.stack[sp - 2].clone();
                self.vm_set_private_prop(&base, &p.name, Some(&p.typ), p.idx, p.is_method, v)?;
                self.vm.sp -= 2;
                self.vm.pc += 1;
            }
            Instruction::PopPrivateEnv => {
                self.vm.priv_env = self.vm.priv_env.as_ref().and_then(|e| e.outer.clone());
                self.vm.pc += 1;
            }
            Instruction::PrivateInRes(i) => {
                let typ = self.vm_get_private_type(i.level, i.is_static);
                return self.exec_private_in(typ.as_ref(), i.idx, i.is_method);
            }
            Instruction::PrivateInId(i) => {
                let typ = Some(i.typ.clone());
                return self.exec_private_in(typ.as_ref(), i.idx, i.is_method);
            }
            Instruction::GetPrivateRefRes(r) => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let base = self.vm_value_to_object(&v)?;
                self.vm.ref_stack.push(Ref::PrivateRes {
                    base,
                    name: r.clone(),
                });
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::GetPrivateRefId(r) => {
                let v = self.vm.stack[self.vm.sp as usize - 1].clone();
                let base = self.vm_value_to_object(&v)?;
                self.vm.ref_stack.push(Ref::PrivateId {
                    base,
                    id: Rc::new((**r).clone()),
                });
                self.vm.sp -= 1;
                self.vm.pc += 1;
            }
            Instruction::YieldMarker(y) => {
                self.vm.pc = -self.vm.pc;
                self.vm.push(Value::YieldMarker(*y));
            }
        }
        Ok(())
    }

    pub fn vm_to_numeric(&mut self, v: &Value) -> JsResult<Value> {
        match v {
            Value::Int(_) | Value::BigInt(_) => Ok(v.clone()),
            Value::Float(f) => Ok(float_to_value(*f)),
            Value::Object(o) => {
                let p = o.to_primitive_number(self)?;
                if let Value::BigInt(_) = p {
                    return Ok(p);
                }
                p.to_number(self)
            }
            _ => v.to_number(self),
        }
    }

    pub fn vm_value_to_object(&mut self, v: &Value) -> JsResult<ObjRef> {
        if let Value::Object(o) = v {
            return Ok(*o);
        }
        let o = v.to_object(self)?;
        self.handles.push(o);
        Ok(o)
    }

    pub fn vm_base_object(&mut self, v: &Value) -> JsResult<Option<ObjRef>> {
        let r = v.base_object(self)?;
        if let (Value::Symbol(_), Some(o)) = (v, r) {
            self.handles.push(o);
        }
        Ok(r)
    }

    fn cannot_read_property_of_undefined(&mut self, idx: usize) -> Throw {
        let k = self.vm.stack[idx].clone();
        match k.go_string(self) {
            Ok(s) => Throw::Value(
                self.new_type_error(&format!("Cannot read property '{s}' of undefined")),
            ),
            Err(e) => e,
        }
    }

    #[inline]
    pub fn vm_to_callee(&mut self, v: &Value) -> JsResult<ObjRef> {
        match v {
            Value::Object(o) => Ok(*o),
            Value::Unresolved(u) if !u.member => self.throw_reference_error(&u.name),
            Value::Unresolved(u) => Err(Throw::Value(
                self.new_type_error(&format!("Object has no member '{}'", u.name)),
            )),
            _ => {
                let s = v.to_string(self)?.to_rust_string();
                Err(Throw::Value(
                    self.new_type_error(&format!("Value is not an object: {s}")),
                ))
            }
        }
    }

    #[inline]
    fn vm_load_stack(&mut self, l: isize) {
        let v = if l > 0 {
            let v = &self.vm.stack[(self.vm.sb + self.vm.args + l) as usize];
            if v.is_empty() {
                Value::Undefined
            } else {
                v.clone()
            }
        } else {
            self.vm.stack[self.vm.sb as usize].clone()
        };
        self.vm.push(v);
        self.vm.pc += 1;
    }

    #[inline]
    fn vm_load_stack1(&mut self, l: isize) {
        let v = if l > 0 {
            let v = &self.vm.stack[(self.vm.sb + l) as usize];
            if v.is_empty() {
                Value::Undefined
            } else {
                v.clone()
            }
        } else {
            self.vm.stack[self.vm.sb as usize].clone()
        };
        self.vm.push(v);
        self.vm.pc += 1;
    }

    #[inline]
    fn vm_load_stack_lex(&mut self, l: isize) -> JsResult<()> {
        let p = if l <= 0 {
            let arg = -l;
            if arg > self.vm.args {
                self.vm.push(Value::Undefined);
                self.vm.pc += 1;
                return Ok(());
            }
            (self.vm.sb + arg) as usize
        } else {
            (self.vm.sb + self.vm.args + l) as usize
        };
        let v = self.vm.stack[p].clone();
        if v.is_empty() {
            return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
        }
        self.vm.push(v);
        self.vm.pc += 1;
        Ok(())
    }

    #[inline]
    fn vm_load_stack1_lex(&mut self, l: isize) -> JsResult<()> {
        let v = self.vm.stack[(self.vm.sb + l) as usize].clone();
        if v.is_empty() {
            return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
        }
        self.vm.push(v);
        self.vm.pc += 1;
        Ok(())
    }

    #[inline]
    fn vm_store_stack(&mut self, s: isize) {
        if s > 0 {
            let v = self.vm.stack[self.vm.sp as usize - 1].clone();
            self.vm.stack[(self.vm.sb + self.vm.args + s) as usize].set(v);
        } else {
            panic!("Illegal stack var index");
        }
        self.vm.pc += 1;
    }

    #[inline]
    fn vm_store_stack1(&mut self, s: isize) {
        if s > 0 {
            let v = self.vm.stack[self.vm.sp as usize - 1].clone();
            self.vm.stack[(self.vm.sb + s) as usize].set(v);
        } else {
            panic!("Illegal stack var index");
        }
        self.vm.pc += 1;
    }

    fn vm_store_stack_lex(&mut self, s: isize) -> JsResult<()> {
        let p = if s < 0 {
            (self.vm.sb - s) as usize
        } else {
            (self.vm.sb + self.vm.args + s) as usize
        };
        if !self.vm.stack[p].is_empty() {
            self.vm.stack[p] = self.vm.stack[self.vm.sp as usize - 1].clone();
        } else {
            return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn vm_store_stack1_lex(&mut self, s: isize) -> JsResult<()> {
        if s <= 0 {
            panic!("Illegal stack var index");
        }
        let p = (self.vm.sb + s) as usize;
        if !self.vm.stack[p].is_empty() {
            self.vm.stack[p] = self.vm.stack[self.vm.sp as usize - 1].clone();
        } else {
            return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn vm_init_stack(&mut self, s: isize) {
        let v = self.vm.stack[self.vm.sp as usize - 1].clone();
        if s <= 0 {
            self.vm.stack[(self.vm.sb - s) as usize].set(v);
        } else {
            self.vm.stack[(self.vm.sb + self.vm.args + s) as usize].set(v);
        }
        self.vm.pc += 1;
    }

    fn vm_init_stack1(&mut self, s: isize) {
        if s <= 0 {
            panic!("Illegal stack var index");
        }
        let v = self.vm.stack[self.vm.sp as usize - 1].clone();
        self.vm.stack[(self.vm.sb + s) as usize].set(v);
        self.vm.pc += 1;
    }

    pub fn vm_stash_slot(&self, s: u32) -> (StashRef, usize) {
        let level = s >> 24;
        let idx = (s & 0x00FF_FFFF) as usize;
        let mut stash = self.vm.stash.expect("stash");
        for _ in 0..level {
            stash = self.heap.stash(stash).outer.expect("outer stash");
        }
        (stash, idx)
    }

    fn vm_set_local_lex(&mut self, s: u32) -> JsResult<()> {
        let v = self.vm.stack[self.vm.sp as usize - 1].clone();
        let (st, idx) = self.vm_stash_slot(s);
        let p = &mut self.heap.stash_mut(st).values[idx];
        if p.is_empty() {
            return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
        }
        *p = v;
        self.vm.pc += 1;
        Ok(())
    }

    fn vm_init_local(&mut self, s: u32) {
        let v = self.vm.stack[self.vm.sp as usize - 1].clone();
        let (st, idx) = self.vm_stash_slot(s);
        self.heap.stash_mut(st).init_by_idx(idx as u32, v);
        self.vm.pc += 1;
    }

    fn exec_add(&mut self) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        if let (Value::Int(l), Value::Int(r)) = (&self.vm.stack[sp - 2], &self.vm.stack[sp - 1]) {
            let v = int_to_value(l.wrapping_add(*r));
            self.vm.stack[sp - 2].set(v);
            self.vm.sp -= 1;
            self.vm.pc += 1;
            return Ok(());
        }
        let mut right = self.vm.stack[sp - 1].clone();
        let mut left = self.vm.stack[sp - 2].clone();
        if let Value::Object(o) = left {
            left = o.to_primitive(self)?;
            self.vm.stack[sp - 2].set(left.clone());
        }
        if let Value::Object(o) = right {
            right = o.to_primitive(self)?;
        }
        let ret = match (&left, &right) {
            (Value::String(_), _) | (_, Value::String(_)) => {
                let ls = match &left {
                    Value::String(s) => s.clone(),
                    _ => left.to_string(self)?,
                };
                let rs = match &right {
                    Value::String(s) => s.clone(),
                    _ => right.to_string(self)?,
                };
                Value::String(ls.concat(&rs))
            }
            (Value::Int(l), Value::Int(r)) => int_to_value(l.wrapping_add(*r)),
            (Value::Int(_), Value::BigInt(_)) => return Err(Throw::from(mix_big_int())),
            (Value::Int(l), _) => {
                let rf = right.to_float(self)?;
                float_to_value(*l as f64 + rf)
            }
            (Value::BigInt(l), Value::BigInt(r)) => new_big_int(l.add(r)),
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                return Err(Throw::from(mix_big_int()));
            }
            _ => {
                let lf = left.to_float(self)?;
                let rf = right.to_float(self)?;
                float_to_value(lf + rf)
            }
        };
        self.vm.stack[sp - 2].set(ret);
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_numeric_binary(&mut self, op: BinaryOp) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let l = self.vm.stack[sp - 2].clone();
        let left = self.vm_to_numeric(&l)?;
        let r = self.vm.stack[sp - 1].clone();
        let right = self.vm_to_numeric(&r)?;
        let res = op(&left, &right)?;
        self.vm.stack[sp - 2].set(res);
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_neg(&mut self) -> JsResult<()> {
        let p = self.vm.sp as usize - 1;
        let operand = self.vm.stack[p].clone();
        let result = match self.vm_to_numeric(&operand)? {
            Value::BigInt(n) => new_big_int(n.neg()),
            Value::Int(n) => {
                if n == 0 {
                    Value::Float(-0.0)
                } else {
                    Value::Int(n.wrapping_neg())
                }
            }
            _ => {
                let mut f = operand.to_float(self)?;
                if !f.is_nan() {
                    f = -f;
                }
                Value::Float(f)
            }
        };
        self.vm.stack[p].set(result);
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_inc_dec(&mut self, inc: bool) -> JsResult<()> {
        let p = self.vm.sp as usize - 1;
        let v = self.vm.stack[p].clone();
        let r = match &v {
            Value::BigInt(n) => {
                let one = big::Int::new_int(1);
                new_big_int(if inc { n.add(&one) } else { n.sub(&one) })
            }
            Value::Int(n) => int_to_value(if inc {
                n.wrapping_add(1)
            } else {
                n.wrapping_sub(1)
            }),
            _ => {
                let f = v.to_float(self)?;
                Value::Float(if inc { f + 1.0 } else { f - 1.0 })
            }
        };
        self.vm.stack[p].set(r);
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_cmp(&mut self, swap: bool, f: fn(Option<bool>) -> bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        if let (Value::Int(l), Value::Int(r)) = (&self.vm.stack[sp - 2], &self.vm.stack[sp - 1]) {
            let res = if swap { Some(r < l) } else { Some(l < r) };
            self.vm.stack[sp - 2].set(Value::Bool(f(res)));
            self.vm.sp -= 1;
            self.vm.pc += 1;
            return Ok(());
        }
        let left = match self.vm.stack[sp - 2].clone() {
            Value::Object(o) => {
                let p = o.to_primitive_number(self)?;
                self.vm.stack[sp - 2].set(p.clone());
                p
            }
            v => v,
        };
        let right = match self.vm.stack[sp - 1].clone() {
            Value::Object(o) => o.to_primitive_number(self)?,
            v => v,
        };
        let r = if swap {
            cmp(&right, &left)?
        } else {
            cmp(&left, &right)?
        };
        self.vm.stack[sp - 2].set(Value::Bool(f(r)));
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_get_elem_ref(&mut self, recv: bool, strict: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        if recv {
            let v = self.vm.stack[sp - 1].clone();
            let base = self.vm_value_to_object(&v)?;
            let name = self.vm.stack[sp - 2].clone();
            let this = self.vm.stack[sp - 3].clone();
            self.vm.ref_stack.push(Ref::Obj {
                base,
                name,
                this: if this.is_empty() {
                    None
                } else {
                    Some(Box::new(this))
                },
                strict,
                name_converted: false,
            });
            self.vm.sp -= 3;
        } else {
            let v = self.vm.stack[sp - 2].clone();
            let base = self.vm_value_to_object(&v)?;
            let name = self.vm.stack[sp - 1].clone();
            self.vm.ref_stack.push(Ref::Obj {
                base,
                name,
                this: None,
                strict,
                name_converted: false,
            });
            self.vm.sp -= 2;
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_set_elem(&mut self, pop: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let v = self.vm.stack[sp - 3].clone();
        let obj = self.vm_value_to_object(&v)?;
        let k = self.vm.stack[sp - 2].clone();
        let prop_name = k.to_property_key(self)?;
        let val = self.vm.stack[sp - 1].clone();
        obj.set_own(self, &prop_name, val.clone(), false)?;
        if pop {
            self.vm.sp -= 3;
        } else {
            self.vm.sp -= 2;
            self.vm.stack[sp - 3].set(val);
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_set_elem_strict(&mut self, pop: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let k = self.vm.stack[sp - 2].clone();
        let prop_name = k.to_property_key(self)?;
        let receiver = self.vm.stack[sp - 3].clone();
        let val = self.vm.stack[sp - 1].clone();
        if let Value::Object(r) = &receiver {
            r.set_own(self, &prop_name, val.clone(), true)?;
        } else {
            let base = self.vm_value_to_object(&receiver)?;
            base.set(self, &prop_name, val.clone(), &receiver, true)?;
        }
        if pop {
            self.vm.sp -= 3;
        } else {
            self.vm.sp -= 2;
            self.vm.stack[sp - 3].set(val);
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_set_elem_recv(&mut self, strict: bool, pop: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let receiver = self.vm.stack[sp - 4].clone();
        let k = self.vm.stack[sp - 3].clone();
        let prop_name = k.to_property_key(self)?;
        let o = self.vm.stack[sp - 2].clone();
        let val = self.vm.stack[sp - 1].clone();
        let obj = self.vm_value_to_object(&o)?;
        obj.set(self, &prop_name, val.clone(), &receiver, strict)?;
        if pop {
            self.vm.sp -= 4;
        } else {
            self.vm.sp -= 3;
            self.vm.stack[sp - 4].set(val);
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_get_prop_ref(
        &mut self,
        name: &unistring::String,
        recv: bool,
        strict: bool,
    ) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let this = if recv {
            let t = self.vm.stack[sp - 2].clone();
            if t.is_empty() {
                None
            } else {
                Some(Box::new(t))
            }
        } else {
            None
        };
        let v = self.vm.stack[sp - 1].clone();
        let base = self.vm_value_to_object(&v)?;
        self.vm.ref_stack.push(Ref::ObjStr {
            base,
            name: name.clone(),
            this,
            strict,
            binding: false,
        });
        self.vm.sp -= if recv { 2 } else { 1 };
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_set_prop_strict(&mut self, name: &unistring::String, pop: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let receiver = self.vm.stack[sp - 2].clone();
        let val = self.vm.stack[sp - 1].clone();
        if let Value::Object(r) = &receiver {
            r.set_own_str(self, name, val.clone(), true)?;
        } else {
            let base = self.vm_value_to_object(&receiver)?;
            base.set_str(self, name, val.clone(), &receiver, true)?;
        }
        if pop {
            self.vm.sp -= 2;
        } else {
            self.vm.stack[sp - 2].set(val);
            self.vm.sp -= 1;
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_set_prop_recv(
        &mut self,
        name: &unistring::String,
        strict: bool,
        pop: bool,
    ) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let receiver = self.vm.stack[sp - 3].clone();
        let o = self.vm.stack[sp - 2].clone();
        let val = self.vm.stack[sp - 1].clone();
        let obj = self.vm_value_to_object(&o)?;
        obj.set_str(self, name, val.clone(), &receiver, strict)?;
        if pop {
            self.vm.sp -= 3;
        } else {
            self.vm.stack[sp - 3].set(val);
            self.vm.sp -= 2;
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_define_accessor_keyed(&mut self, d: &DefineKeyed, getter: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let o = self.vm.stack[sp - 2].clone();
        let obj = self.to_object(&o)?;
        let val = self.vm.stack[sp - 1].clone();
        let method = self.to_object(&val)?;
        let prefix = if getter { "get " } else { "set " };
        let name = ascii_string(prefix).concat(&string_value_from_raw(&d.key));
        method.define_own_property_str(
            self,
            &STR_NAME,
            PropertyDescriptor {
                value: Some(Value::String(name)),
                configurable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        )?;
        let mut descr = PropertyDescriptor {
            configurable: Flag::True,
            enumerable: to_flag(d.enumerable),
            ..PropertyDescriptor::default()
        };
        if getter {
            descr.getter = Some(val);
        } else {
            descr.setter = Some(val);
        }
        obj.define_own_property_str(self, &d.key, descr, true)?;
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_define_accessor(&mut self, enumerable: bool, getter: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let o = self.vm.stack[sp - 3].clone();
        let obj = self.to_object(&o)?;
        let prop_name = self.vm.stack[sp - 2].clone();
        let val = self.vm.stack[sp - 1].clone();
        let method = self.to_object(&val)?;
        let prefix = if getter { "get " } else { "set " };
        let name = func_name(self, prefix, &prop_name);
        method.define_own_property_str(
            self,
            &STR_NAME,
            PropertyDescriptor {
                value: Some(Value::String(name)),
                configurable: Flag::True,
                ..PropertyDescriptor::default()
            },
            true,
        )?;
        let mut descr = PropertyDescriptor {
            configurable: Flag::True,
            enumerable: to_flag(enumerable),
            ..PropertyDescriptor::default()
        };
        if getter {
            descr.getter = Some(val);
        } else {
            descr.setter = Some(val);
        }
        obj.define_own_property(self, &prop_name, descr, true)?;
        self.vm.sp -= 2;
        self.vm.pc += 1;
        Ok(())
    }

    fn array_data_mut(&mut self, o: ObjRef) -> &mut crate::array::ArrayObject {
        match &mut self.obj_mut(o).ext {
            ObjExt::Array(a) => a,
            _ => panic!("not an array object"),
        }
    }

    fn invalid_array_length_error(&mut self) -> Throw {
        let e = self.get_range_error();
        Throw::Value(self.new_error(e, "Invalid array length"))
    }

    fn exec_push_array_item(&mut self) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let arr = self.vm.stack[sp - 2].as_object().expect("array");
        if self.array_data_mut(arr).length < u32::MAX {
            self.array_data_mut(arr).length += 1;
        } else {
            return Err(self.invalid_array_length_error());
        }
        let val = self.vm.stack[sp - 1].clone();
        let a = self.array_data_mut(arr);
        let not_nil = !val.is_empty();
        a.values.push(val);
        if not_nil {
            a.obj_count += 1;
        }
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_push_array_spread(&mut self) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let arr = self.vm.stack[sp - 2].as_object().expect("array");
        let v = self.vm.stack[sp - 1].clone();
        let mut iter = self.vm_get_iterator(&v)?;
        iter.iterate(self, &mut |rt, val| {
            if rt.array_data_mut(arr).length < u32::MAX {
                rt.array_data_mut(arr).length += 1;
            } else {
                return Err(rt.invalid_array_length_error());
            }
            let a = rt.array_data_mut(arr);
            a.values.push(val);
            a.obj_count += 1;
            Ok(())
        })?;
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_push_spread(&mut self) -> JsResult<()> {
        self.vm.sp -= 1;
        let obj = self.vm.stack[self.vm.sp as usize].clone();
        let mut iter = self.vm_get_iterator(&obj)?;
        iter.iterate(self, &mut |rt, val| {
            rt.vm.push(val);
            Ok(())
        })?;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_new_array_from_iter(&mut self) -> JsResult<()> {
        let item = self.vm.iter_stack.pop().expect("iter stack");
        let mut values = Vec::new();
        if let Some(mut iter) = item.iter
            && iter.iterator.is_some()
        {
            iter.root(self);
            let mark = self.extra_roots.len();
            let r = iter.iterate(self, &mut |rt, val| {
                rt.extra_roots.push(val.clone());
                values.push(val);
                Ok(())
            });
            self.extra_roots.truncate(mark);
            r?;
        }
        let o = self.new_array_values(values);
        self.vm.push(Value::Object(o));
        self.vm.pc += 1;
        Ok(())
    }

    pub fn vm_get_iterator(&mut self, v: &Value) -> JsResult<IteratorRecord> {
        let iter = self.get_iterator(v, None)?;
        iter.root(self);
        Ok(iter)
    }

    fn vm_iter_step(&mut self, l: usize) -> JsResult<(Option<Value>, Option<Box<Exception>>)> {
        let mut iter = self.vm.iter_stack[l].iter.expect("iterator record");
        let mut value = None;
        let ex = self.vm_try_result(|rt| {
            value = iter.step(rt)?;
            Ok(())
        })?;
        if let Some(it) = self.vm.iter_stack.get_mut(l) {
            it.iter = Some(iter);
        }
        Ok((value, ex))
    }

    fn vm_ref_convert_key(&mut self, i: usize) -> JsResult<()> {
        if let Ref::Obj {
            name,
            name_converted: false,
            ..
        } = &self.vm.ref_stack[i]
        {
            let n = name.clone();
            let k = n.to_property_key(self)?;
            if let Some(Ref::Obj {
                name,
                name_converted,
                ..
            }) = self.vm.ref_stack.get_mut(i)
            {
                *name = k;
                *name_converted = true;
            }
        }
        Ok(())
    }

    fn vm_pop_ref(&mut self) -> JsResult<Ref> {
        let l = self.vm.ref_stack.len() - 1;
        self.vm_ref_convert_key(l)?;
        let r = self.vm.ref_stack.pop().expect("ref");
        r.root(self);
        Ok(r)
    }

    fn exec_load_mixed(&mut self, g: &LoadMixed, lex: bool) -> JsResult<()> {
        let level = g.idx >> 24;
        let idx = (g.idx & 0x00FF_FFFF) as usize;
        let mut stash = self.vm.stash;
        for _ in 0..level {
            let st = stash.expect("stash");
            if let Some(v) = stash_get_by_name(self, st, &g.name)? {
                if g.callee {
                    match self.heap.stash(st).obj {
                        Some(o) => self.vm.push(Value::Object(o)),
                        None => self.vm.push(Value::Undefined),
                    }
                }
                self.vm.push(v);
                self.vm.pc += 1;
                return Ok(());
            }
            stash = self.heap.stash(st).outer;
        }
        if g.callee {
            self.vm.push(Value::Undefined);
        }
        if let Some(st) = stash {
            let v = self.heap.stash(st).values[idx].clone();
            if lex {
                if v.is_empty() {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                self.vm.push(v);
            } else {
                self.vm
                    .push(if v.is_empty() { Value::Undefined } else { v });
            }
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_load_mixed_stack(&mut self, g: &LoadMixedStack, kind: u8) -> JsResult<()> {
        let mut stash = self.vm.stash;
        for _ in 0..g.level {
            let st = stash.expect("stash");
            if let Some(v) = stash_get_by_name(self, st, &g.name)? {
                if g.callee {
                    match self.heap.stash(st).obj {
                        Some(o) => self.vm.push(Value::Object(o)),
                        None => self.vm.push(Value::Undefined),
                    }
                }
                self.vm.push(v);
                self.vm.pc += 1;
                return Ok(());
            }
            stash = self.heap.stash(st).outer;
        }
        if g.callee {
            self.vm.push(Value::Undefined);
        }
        match kind {
            0 => self.vm_load_stack(g.idx),
            1 => self.vm_load_stack1(g.idx),
            2 => return self.vm_load_stack_lex(g.idx),
            _ => return self.vm_load_stack1_lex(g.idx),
        }
        Ok(())
    }

    fn exec_resolve_mixed(&mut self, r: &ResolveMixed) -> JsResult<()> {
        let level = r.idx >> 24;
        let idx = (r.idx & 0x00FF_FFFF) as usize;
        let mut stash = self.vm.stash;
        for _ in 0..level {
            let st = stash.expect("stash");
            if let Some(rf) = stash_get_ref_by_name(self, st, &r.name, r.strict)? {
                self.vm.ref_stack.push(rf);
                self.vm.pc += 1;
                return Ok(());
            }
            stash = self.heap.stash(st).outer;
        }
        let rf = match stash {
            Some(st) => new_stash_ref(r.typ, r.name.clone(), st, idx),
            None => Ref::Unresolved {
                name: r.name.clone(),
            },
        };
        self.vm.ref_stack.push(rf);
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_resolve_mixed_stack(&mut self, r: &ResolveMixedStack, one: bool) -> JsResult<()> {
        let mut stash = self.vm.stash;
        for _ in 0..r.level {
            let st = stash.expect("stash");
            if let Some(rf) = stash_get_ref_by_name(self, st, &r.name, r.strict)? {
                self.vm.ref_stack.push(rf);
                self.vm.pc += 1;
                return Ok(());
            }
            stash = self.heap.stash(st).outer;
        }
        let idx = if one {
            self.vm.sb + r.idx
        } else if r.idx > 0 {
            self.vm.sb + self.vm.args + r.idx
        } else {
            self.vm.sb - r.idx
        };
        let rf = new_stack_ref(r.typ, r.name.clone(), idx as usize);
        self.vm.ref_stack.push(rf);
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_resolve_var1(&mut self, name: &unistring::String, strict: bool) -> JsResult<()> {
        let mut s = self.vm.stash;
        while let Some(st) = s {
            if let Some(rf) = stash_get_ref_by_name(self, st, name, strict)? {
                self.vm.ref_stack.push(rf);
                self.vm.pc += 1;
                return Ok(());
            }
            s = self.heap.stash(st).outer;
        }
        let g = self.global_object;
        let rf = if !strict {
            Ref::ObjStr {
                base: g,
                name: name.clone(),
                this: None,
                strict: false,
                binding: true,
            }
        } else if g.has_property_str(self, name)? {
            Ref::ObjStr {
                base: g,
                name: name.clone(),
                this: None,
                strict: true,
                binding: true,
            }
        } else {
            Ref::Unresolved { name: name.clone() }
        };
        self.vm.ref_stack.push(rf);
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_delete_var(&mut self, name: &unistring::String) -> JsResult<()> {
        let mut ret = true;
        let mut found = false;
        let mut s = self.vm.stash;
        while let Some(st) = s {
            if let Some(obj) = self.heap.stash(st).obj {
                if stash_obj_has(self, obj, name)? {
                    ret = obj.delete_str(self, name, false)?;
                    found = true;
                    break;
                }
            } else if let Some(idx) = self
                .heap
                .stash(st)
                .names
                .as_ref()
                .and_then(|n| n.get(name))
                .copied()
            {
                if idx & (MASK_VAR | MASK_DELETABLE) == MASK_VAR | MASK_DELETABLE {
                    self.heap.stash_mut(st).delete_binding(name);
                } else {
                    ret = false;
                }
                found = true;
                break;
            }
            s = self.heap.stash(st).outer;
        }
        if !found {
            let g = self.global_object;
            if g.has_property_str(self, name)? {
                ret = g.delete_str(self, name, false)?;
            }
        }
        self.vm.push(Value::Bool(ret));
        self.vm.pc += 1;
        Ok(())
    }

    pub fn vm_call_eval(&mut self, n: usize, strict: bool) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let callee = self.vm.stack[sp - n - 1].clone();
        let c = self.to_object(&callee)?;
        if Some(c) == self.global.eval {
            if n > 0 {
                let src_val = self.vm.stack[sp - n].clone();
                if let Value::String(src) = &src_val {
                    let ret = self.eval(src, true, strict)?;
                    self.vm.stack[sp - n - 2].set(ret);
                } else {
                    self.vm.stack[sp - n - 2].set(src_val);
                }
            } else {
                self.vm.stack[sp - n - 2].set(Value::Undefined);
            }
            self.vm.sp -= n as isize + 1;
            self.vm.pc += 1;
            Ok(())
        } else {
            self.exec_call(n)
        }
    }

    pub fn vm_count_variadic_args(&self) -> usize {
        let mut count = 0;
        let mut i = self.vm.sp - 1;
        while i >= 0 {
            if let Value::Symbol(s) = &self.vm.stack[i as usize]
                && *s == VARIADIC_MARKER
            {
                return count;
            }
            count += 1;
            i -= 1;
        }
        panic!("Variadic marker was not found. Compiler bug.");
    }

    #[inline]
    fn exec_call(&mut self, n: usize) -> JsResult<()> {
        let v = self.vm.stack[self.vm.sp as usize - n - 1].clone();
        let obj = self.vm_to_callee(&v)?;
        obj.vm_call(self, n)
    }

    fn vm_clear_stack_region(&mut self, from: usize, to: usize, v: Value) {
        for slot in &mut self.vm.stack[from..to] {
            *slot = v.clone();
        }
    }

    fn exec_enter_block(&mut self, e: &EnterBlock) {
        if e.stash_size > 0 {
            self.vm_new_stash();
            let s = self.vm.stash.expect("stash");
            let st = self.heap.stash_mut(s);
            st.values = vec![Value::Empty; e.stash_size as usize];
            if e.names.as_ref().is_some_and(|n| !n.is_empty()) {
                st.names = e.names.clone();
            }
        }
        let ss = e.stack_size as isize;
        let sp = self.vm.sp;
        self.vm.stack_expand(sp + ss - 1);
        self.vm_clear_stack_region(sp as usize, (sp + ss) as usize, Value::Empty);
        self.vm.sp += ss;
        self.vm.pc += 1;
    }

    fn exec_enter_catch_block(&mut self, e: &EnterBlock) {
        self.vm_new_stash();
        let s = self.vm.stash.expect("stash");
        self.vm.sp -= 1;
        let ex = self.vm.stack[self.vm.sp as usize].clone();
        let st = self.heap.stash_mut(s);
        st.values = vec![Value::Empty; e.stash_size as usize];
        if e.names.as_ref().is_some_and(|n| !n.is_empty()) {
            st.names = e.names.clone();
        }
        st.values[0] = ex;
        let ss = e.stack_size as isize;
        let sp = self.vm.sp;
        self.vm.stack_expand(sp + ss - 1);
        self.vm_clear_stack_region(sp as usize, (sp + ss) as usize, Value::Empty);
        self.vm.sp += ss;
        self.vm.pc += 1;
    }

    fn vm_func_names(names: &NamesMap) -> NamesMap {
        if names.as_ref().is_some_and(|n| !n.is_empty()) {
            names.clone()
        } else {
            None
        }
    }

    fn exec_enter_func(&mut self, e: &EnterFunc) {
        let mut sp = self.vm.sp;
        self.vm.sb = sp - self.vm.args - 1;
        self.vm_new_stash();
        let stash = self.vm.stash.expect("stash");
        let args = self.vm.args as usize;
        let num_args = e.num_args as usize;
        let mut values = vec![Value::Empty; e.stash_size as usize];
        let mut extra_args = Vec::new();
        let mut ss = e.stack_size as isize;
        let mut ea = 0isize;
        if e.args_to_stash {
            let spu = sp as usize;
            let offset = args as isize - num_args as isize;
            let n = values.len().min(args);
            values[..n].clone_from_slice(&self.vm.stack[spu - args..spu - args + n]);
            if offset > 0 {
                extra_args = self.vm.stack[spu - offset as usize..spu].to_vec();
            } else {
                for v in &mut values[args..num_args] {
                    *v = Value::Undefined;
                }
            }
            sp -= args as isize;
        } else {
            let d = num_args as isize - args as isize;
            if d > 0 {
                ss += d;
                ea = d;
                self.vm.args = num_args as isize;
            }
        }
        let st = self.heap.stash_mut(stash);
        st.func_type = e.func_type;
        st.values = values;
        st.names = Self::vm_func_names(&e.names);
        if !extra_args.is_empty() {
            st.extra_args = extra_args;
        }
        self.vm.stack_expand(sp + ss - 1);
        if ea > 0 {
            let end = (self.vm.sp + ea) as usize;
            self.vm_clear_stack_region(sp as usize, end, Value::Undefined);
        }
        self.vm_clear_stack_region((sp + ea) as usize, (sp + ss) as usize, Value::Empty);
        self.vm.sp = sp + ss;
        self.vm.pc += 1;
    }

    fn exec_enter_func1(&mut self, e: &EnterFunc1) {
        let sp = self.vm.sp as usize;
        self.vm.sb = self.vm.sp - self.vm.args - 1;
        self.vm_new_stash();
        let stash = self.vm.stash.expect("stash");
        let args = self.vm.args as usize;
        let mut values = vec![Value::Empty; e.stash_size as usize];
        let mut extra_args = Vec::new();
        let offset = args as isize - e.args_to_copy as isize;
        if offset > 0 {
            let src = &self.vm.stack[sp - args..sp - offset as usize];
            let n = values.len().min(src.len());
            values[..n].clone_from_slice(&src[..n]);
            let offset2 = args as isize - e.num_args as isize;
            if offset2 > 0 {
                extra_args = self.vm.stack[sp - offset2 as usize..sp].to_vec();
            }
        } else {
            let n = values.len().min(args);
            values[..n].clone_from_slice(&self.vm.stack[sp - args..sp - args + n]);
            if e.args_to_copy as usize > args {
                for v in &mut values[args..e.args_to_copy as usize] {
                    *v = Value::Undefined;
                }
            }
        }
        let st = self.heap.stash_mut(stash);
        st.func_type = e.func_type;
        st.values = values;
        st.names = Self::vm_func_names(&e.names);
        if !extra_args.is_empty() {
            st.extra_args = extra_args;
        }
        self.vm.pc += 1;
    }

    fn exec_enter_func_body(&mut self, e: &EnterFuncBody) {
        if e.enter_block.stash_size > 0 || e.extensible {
            self.vm_new_stash();
            let stash = self.vm.stash.expect("stash");
            let st = self.heap.stash_mut(stash);
            st.func_type = e.func_type;
            st.values = vec![Value::Empty; e.enter_block.stash_size as usize];
            st.names = Self::vm_func_names(&e.enter_block.names);
        }
        let mut sp = self.vm.sp;
        if e.adjust_stack {
            sp -= self.vm.args;
        }
        let nsp = sp + e.enter_block.stack_size as isize;
        if e.enter_block.stack_size > 0 {
            self.vm.stack_expand(nsp - 1);
            self.vm_clear_stack_region(sp as usize, nsp as usize, Value::Empty);
        }
        self.vm.sp = nsp;
        self.vm.pc += 1;
    }

    #[inline]
    fn exec_ret(&mut self) {
        let sb = self.vm.sb as usize;
        self.vm.stack[sb - 1] = self.vm.stack[self.vm.sp as usize - 1].clone();
        self.vm.sp = sb as isize;
        self.vm.pop_ctx();
        self.vm.pc += 1;
    }

    fn exec_enter_func_stashless(&mut self, e: &EnterFuncStashless) {
        let sp = self.vm.sp;
        self.vm.sb = sp - self.vm.args - 1;
        let d = e.args as isize - self.vm.args;
        if d > 0 {
            let ss = sp + e.stack_size as isize + d;
            self.vm.stack_expand(ss);
            self.vm_clear_stack_region(sp as usize, (sp + d) as usize, Value::Undefined);
            self.vm_clear_stack_region((sp + d) as usize, ss as usize, Value::Empty);
            self.vm.args = e.args as isize;
            self.vm.sp = ss;
        } else if e.stack_size > 0 {
            let ss = sp + e.stack_size as isize;
            self.vm.stack_expand(ss);
            self.vm_clear_stack_region(sp as usize, ss as usize, Value::Empty);
            self.vm.sp = ss;
        }
        self.vm.pc += 1;
    }

    fn vm_new_func_init(&mut self, o: ObjRef, n: &NewFunc) {
        let stash = self.vm.stash;
        let priv_env = self.vm.priv_env.clone();
        let d = BaseJsFuncObject::data_mut(self, o);
        d.prg = Some(n.prg.clone());
        d.stash = stash;
        d.priv_env = priv_env;
        d.src = n.source.clone();
    }

    fn exec_new_method(&mut self, o: ObjRef, n: &NewMethod) -> JsResult<()> {
        self.vm_new_func_init(o, &n.new_func);
        if n.home_obj_offset > 0 {
            let v = self.vm.stack[self.vm.sp as usize - n.home_obj_offset as usize].clone();
            let h = self.to_object(&v)?;
            BaseJsFuncObject::data_mut(self, o).home_object = Some(h);
        }
        self.vm.push(Value::Object(o));
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_new_arrow_func(&mut self, o: ObjRef, n: &NewFunc) -> JsResult<()> {
        self.vm_new_func_init(o, n);
        if self.vm.sb > 0 {
            let v = self.vm.stack[self.vm.sb as usize - 1].clone();
            let fo = self.get_func_object(&v)?;
            BaseJsFuncObject::data_mut(self, o).func_obj = fo;
        }
        self.vm.push(Value::Object(o));
        self.vm.pc += 1;
        Ok(())
    }

    pub fn js_func_kind(&self, o: ObjRef) -> Option<FuncKind> {
        match &self.obj(o).ext {
            ObjExt::JsFunc(f) => Some(f.kind),
            _ => None,
        }
    }

    pub fn is_class_func(&self, o: ObjRef) -> bool {
        self.js_func_kind(o) == Some(FuncKind::Class)
    }

    pub fn get_func_object(&mut self, v: &Value) -> JsResult<Option<ObjRef>> {
        if let Value::Object(o) = v {
            if self.js_func_kind(*o) == Some(FuncKind::Arrow) {
                return Ok(BaseJsFuncObject::data(self, *o).func_obj);
            }
            return Ok(Some(*o));
        }
        if v.is_undefined() {
            return Ok(None);
        }
        Err(Throw::TypeError("Value is not an Object".to_string()))
    }

    pub fn get_home_object(&mut self, v: &Value) -> JsResult<ObjRef> {
        if let Value::Object(o) = v {
            match self.js_func_kind(*o) {
                Some(FuncKind::Method | FuncKind::GeneratorMethod | FuncKind::AsyncMethod) => {
                    if let Some(h) = BaseJsFuncObject::data(self, *o).home_object {
                        return Ok(h);
                    }
                }
                Some(FuncKind::Class) => {
                    let p = nil_safe(BaseObject::get_str(self, *o, &STR_PROTOTYPE, None)?);
                    return self.to_object(&p);
                }
                Some(FuncKind::Arrow | FuncKind::AsyncArrow) => {
                    if let Some(f) = BaseJsFuncObject::data(self, *o).func_obj {
                        return self.get_home_object(&Value::Object(f));
                    }
                }
                _ => {}
            }
        }
        Err(Throw::TypeError(format!(
            "Compiler bug: getHomeObject() on the wrong value: {}",
            go_type_of_value(v)
        )))
    }

    pub fn vm_already_declared(&mut self, name: &unistring::String) -> Value {
        let e = self.get_syntax_error();
        self.new_error(e, &format!("Identifier '{name}' has already been declared"))
    }

    fn global_stash_has(&self, name: &unistring::String) -> bool {
        self.heap
            .stash(self.global.stash)
            .names
            .as_ref()
            .is_some_and(|n| n.contains_key(name))
    }

    pub fn vm_check_bind_vars_global(&mut self, names: &[unistring::String]) -> JsResult<()> {
        let o = self.global_object;
        if std::ptr::eq(self.obj(o).class, &BASE_OBJECT_CLASS) {
            if self.obj(o).base.extensible {
                for name in names {
                    if self.global_stash_has(name) {
                        return Err(Throw::Value(self.vm_already_declared(name)));
                    }
                }
            } else {
                for name in names {
                    if !BaseObject::has_own_property_str(self, o, name)? {
                        return Err(Throw::Value(self.new_type_error(&format!(
                            "Cannot define global variable '{name}', global object is not extensible"
                        ))));
                    }
                    if self.global_stash_has(name) {
                        return Err(Throw::Value(self.vm_already_declared(name)));
                    }
                }
            }
        } else {
            for name in names {
                if !o.has_own_property_str(self, name)? && !o.is_extensible(self)? {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot define global variable '{name}', global object is not extensible"
                    ))));
                }
                if self.global_stash_has(name) {
                    return Err(Throw::Value(self.vm_already_declared(name)));
                }
            }
        }
        Ok(())
    }

    pub fn vm_create_global_var_bindings(
        &mut self,
        names: &[unistring::String],
        d: bool,
    ) -> JsResult<()> {
        let o = self.global_object;
        if let ObjExt::Template(_) = &self.obj(o).ext {
            for name in names {
                if !TemplatedObject::has_own_property_str(self, o, name)?
                    && self.obj(o).base.extensible
                {
                    BaseObject::_put_prop(self, o, name, Value::Undefined, true, true, d);
                }
            }
        } else {
            let cf = to_flag(d);
            for name in names {
                if !o.has_own_property_str(self, name)? && o.is_extensible(self)? {
                    o.define_own_property_str(
                        self,
                        name,
                        PropertyDescriptor {
                            value: Some(Value::Undefined),
                            writable: Flag::True,
                            enumerable: Flag::True,
                            configurable: cf,
                            ..PropertyDescriptor::default()
                        },
                        true,
                    )?;
                    o.set_own_str(self, name, Value::Undefined, false)?;
                }
            }
        }
        Ok(())
    }

    pub fn vm_create_global_func_bindings(
        &mut self,
        names: &[unistring::String],
        d: bool,
    ) -> JsResult<()> {
        let o = self.global_object;
        let b = self.vm.sp as usize - names.len();
        let shortcut = matches!(&self.obj(o).ext, ObjExt::Template(_));
        for (i, name) in names.iter().enumerate() {
            let prop = o.get_own_prop_str(self, name)?;
            let value = self.vm.stack[b + i].clone();
            if shortcut && prop.is_none() && self.obj(o).base.extensible {
                BaseObject::_put_prop(self, o, name, value, true, true, d);
            } else {
                let mut desc = PropertyDescriptor {
                    value: Some(value.clone()),
                    ..PropertyDescriptor::default()
                };
                let non_configurable = matches!(&prop, Some(Value::Property(p)) if !p.configurable);
                if !non_configurable {
                    desc.writable = Flag::True;
                    desc.enumerable = Flag::True;
                    desc.configurable = to_flag(d);
                }
                if shortcut {
                    TemplatedObject::define_own_property_str(self, o, name, desc, true)?;
                } else {
                    o.define_own_property_str(self, name, desc, true)?;
                    o.set_own_str(self, name, value, false)?;
                }
            }
        }
        self.vm.sp = b as isize;
        Ok(())
    }

    pub fn vm_check_bind_funcs_global(&mut self, names: &[unistring::String]) -> JsResult<()> {
        let o = self.global_object;
        for name in names {
            if self.global_stash_has(name) {
                return Err(Throw::Value(self.vm_already_declared(name)));
            }
            let prop = o.get_own_prop_str(self, name)?;
            let allowed = match &prop {
                None => o.is_extensible(self)?,
                Some(Value::Property(p)) => {
                    p.configurable
                        || p.getter_func.is_none()
                            && p.setter_func.is_none()
                            && p.writable
                            && p.enumerable
                }
                Some(_) => true,
            };
            if !allowed {
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Cannot redefine global function '{name}'"
                ))));
            }
        }
        Ok(())
    }

    pub fn vm_check_bind_lex_global(&mut self, names: &[unistring::String]) -> JsResult<()> {
        let o = self.global_object;
        for name in names {
            let fail = if self.global_stash_has(name) {
                true
            } else {
                matches!(o.get_own_prop_str(self, name)?, Some(Value::Property(p)) if !p.configurable)
            };
            if fail {
                return Err(Throw::Value(self.vm_already_declared(name)));
            }
        }
        Ok(())
    }

    fn exec_bind_vars(&mut self, d: &BindVars) -> JsResult<()> {
        let mut target = None;
        for name in &d.names {
            let mut s = self.vm.stash;
            while let Some(st) = s {
                let stash = self.heap.stash(st);
                if let Some(idx) = stash.names.as_ref().and_then(|n| n.get(name))
                    && idx & MASK_VAR == 0
                {
                    return Err(Throw::Value(self.vm_already_declared(name)));
                }
                if stash.is_variable() {
                    target = Some(st);
                    break;
                }
                s = stash.outer;
            }
        }
        let target = target.or(self.vm.stash).expect("stash");
        for name in &d.names {
            self.heap
                .stash_mut(target)
                .create_binding(name, d.deletable);
        }
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_bind_global(&mut self, b: &BindGlobal) -> JsResult<()> {
        self.vm_check_bind_funcs_global(&b.funcs)?;
        self.vm_check_bind_lex_global(&b.lets)?;
        self.vm_check_bind_lex_global(&b.consts)?;
        self.vm_check_bind_vars_global(&b.vars)?;
        let gs = self.global.stash;
        for name in &b.lets {
            self.heap.stash_mut(gs).create_lex_binding(name, false);
        }
        for name in &b.consts {
            self.heap.stash_mut(gs).create_lex_binding(name, true);
        }
        self.vm_create_global_func_bindings(&b.funcs, b.deletable)?;
        self.vm_create_global_var_bindings(&b.vars, b.deletable)?;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_new(&mut self, n: usize) -> JsResult<()> {
        let sp0 = self.vm.sp as usize - n;
        let obj = self.vm.stack[sp0 - 1].clone();
        let ctor = self.to_constructor(&obj)?;
        let mut buf = self.vm.take_arg_buf();
        buf.extend_from_slice(&self.vm.stack[sp0..sp0 + n]);
        let r = ctor.construct(self, &buf, None);
        self.vm.return_arg_buf(buf);
        let r = r?;
        self.vm.stack[sp0 - 1].set(Value::Object(r));
        self.vm.sp = sp0 as isize;
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_super_call(&mut self, s: usize) -> JsResult<()> {
        let mut this_ref = self.vm.ref_stack.pop().expect("this ref");
        let callee = self.vm.stack[self.vm.sb as usize - 1].clone();
        let obj = self.to_object(&callee)?;
        let cls = match self.js_func_kind(obj) {
            Some(FuncKind::Class) => Some(obj),
            Some(FuncKind::Arrow) => BaseJsFuncObject::data(self, obj)
                .func_obj
                .filter(|f| self.is_class_func(*f)),
            _ => None,
        };
        let Some(cls) = cls else {
            return Err(Throw::Value(
                self.new_type_error("wrong callee type for super()"),
            ));
        };
        let sp0 = self.vm.sp as usize - s;
        let nt = self.vm.new_target.clone();
        let new_target = self.to_object(&nt)?;
        let mut buf = self.vm.take_arg_buf();
        buf.extend_from_slice(&self.vm.stack[sp0..sp0 + s]);
        let v = class_func_create_instance(self, cls, &buf, new_target);
        self.vm.return_arg_buf(buf);
        let v = v?;
        this_ref.set(self, Value::Object(v))?;
        self.vm.sp = sp0 as isize;
        class_func_init_fields(self, cls, v)?;
        self.vm.push(Value::Object(v));
        self.vm.pc += 1;
        Ok(())
    }

    fn exec_concat_strings(&mut self, n: usize) {
        let sp = self.vm.sp as usize;
        let mut b = StringBuilder::new();
        for i in sp - n..sp {
            match &self.vm.stack[i] {
                Value::String(s) => b.write_string(s),
                v => panic!("unknown string type: {v:?}"),
            }
        }
        self.vm.sp -= n as isize - 1;
        let p = self.vm.sp as usize - 1;
        self.vm.stack[p].set(Value::String(b.string()));
        self.vm.pc += 1;
    }

    fn exec_get_tagged_tmpl_object(&mut self, c: &GetTaggedTmplObject) -> ObjRef {
        let cooked = self.new_array_object();
        let cooked_values: Vec<Value> = c
            .cooked
            .iter()
            .map(|p| Value::Property(Box::new(p.clone())))
            .collect();
        set_array_values(self.array_data_mut(cooked), cooked_values);
        let raw = self.new_array_object();
        let raw_values: Vec<Value> = c
            .raw
            .iter()
            .map(|p| Value::Property(Box::new(p.clone())))
            .collect();
        set_array_values(self.array_data_mut(raw), raw_values);
        {
            let a = self.array_data_mut(cooked);
            a.prop_value_count = c.cooked.len();
            a.length_prop.writable = false;
        }
        {
            let a = self.array_data_mut(raw);
            a.prop_value_count = c.raw.len();
            a.length_prop.writable = false;
        }
        let _ = raw.prevent_extensions(self, true);
        self.obj_mut(raw).class = &TAGGED_TEMPLATE_ARRAY_CLASS;
        self.vm
            .tagged_template_ids
            .insert(raw, c.raw.as_ptr() as usize);
        BaseObject::_put_prop(
            self,
            cooked,
            &STR_RAW,
            Value::Object(raw),
            false,
            false,
            false,
        );
        let _ = cooked.prevent_extensions(self, true);
        self.obj_mut(cooked).class = &TAGGED_TEMPLATE_ARRAY_CLASS;
        self.vm
            .tagged_template_ids
            .insert(cooked, c.cooked.as_ptr() as usize);
        cooked
    }

    pub fn vm_new_class_create(
        &mut self,
        c: &NewClass,
        proto_parent: Option<ObjRef>,
        ctor_parent: ObjRef,
        derived: bool,
    ) -> (ObjRef, ObjRef) {
        let proto = self.new_base_object(proto_parent, CLASS_OBJECT);
        let f = self.new_class_func(&c.name, c.length, Some(ctor_parent), derived);
        BaseObject::_put_prop(
            self,
            f,
            &STR_PROTOTYPE,
            Value::Object(proto),
            false,
            false,
            false,
        );
        BaseObject::_put_prop(
            self,
            proto,
            &STR_CONSTRUCTOR,
            Value::Object(f),
            true,
            false,
            true,
        );
        let stash = self.vm.stash;
        {
            let d = BaseJsFuncObject::data_mut(self, f);
            d.prg = c.ctor.clone();
            d.stash = stash;
            d.src = c.source.clone();
            d.init_fields = c.init_fields.clone();
        }
        if c.has_private_env {
            self.vm.priv_env = Some(Rc::new(PrivateEnv {
                outer: self.vm.priv_env.clone(),
                ..PrivateEnv::default()
            }));
            self.vm_create_private_type(f, c.num_private_fields, c.num_private_methods);
            let typ = BaseJsFuncObject::data(self, f).private_env_type.clone();
            self.vm_fill_private_names_map(typ.as_ref(), &c.private_fields, &c.private_methods);
            if let Some(pe) = &self.vm.priv_env {
                *pe.instance_type.borrow_mut() = typ;
            }
        }
        let pe = self.vm.priv_env.clone();
        BaseJsFuncObject::data_mut(self, f).priv_env = pe;
        (proto, f)
    }

    fn exec_new_derived_class(&mut self, c: &NewClass) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let o = self.vm.stack[sp - 1].clone();
        let proto_parent;
        let super_class;
        if !o.is_null() {
            let sc = match &o {
                Value::Object(sc) if sc.assert_constructor(self).is_some() => *sc,
                _ => {
                    return Err(Throw::Value(self.new_type_error(
                        "Class extends value is not a constructor or null",
                    )));
                }
            };
            let v = sc.get_str(self, &STR_PROTOTYPE, None)?;
            match v {
                Some(Value::Null) => proto_parent = None,
                Some(Value::Object(p)) => proto_parent = Some(p),
                _ => {
                    return Err(Throw::Value(self.new_type_error(
                        "Class extends value does not have valid prototype property",
                    )));
                }
            }
            super_class = sc;
        } else {
            proto_parent = None;
            super_class = self.get_function_prototype();
        }
        let (proto, cls) = self.vm_new_class_create(c, proto_parent, super_class, true);
        self.vm.stack[sp - 1].set(Value::Object(proto));
        self.vm.push(Value::Object(cls));
        self.vm.pc += 1;
        Ok(())
    }

    pub fn vm_create_private_type(&mut self, f: ObjRef, num_fields: u32, num_methods: u32) {
        let typ = Arc::new(PrivateEnvType {
            num_fields,
            num_methods,
        });
        let d = BaseJsFuncObject::data_mut(self, f);
        d.private_env_type = Some(typ);
        d.private_methods = vec![Value::Empty; num_methods as usize];
    }

    pub fn vm_fill_private_names_map(
        &mut self,
        typ: Option<&Arc<PrivateEnvType>>,
        private_fields: &[unistring::String],
        private_methods: &[unistring::String],
    ) {
        if private_fields.is_empty() && private_methods.is_empty() {
            return;
        }
        let Some(pe) = &self.vm.priv_env else {
            return;
        };
        let Some(typ) = typ else {
            return;
        };
        let mut names = pe.names.borrow_mut();
        for (idx, field) in private_fields.iter().enumerate() {
            names.insert(
                field.clone(),
                Rc::new(PrivateId {
                    typ: typ.clone(),
                    name: unistring::String::default(),
                    idx: idx as u32,
                    is_method: false,
                }),
            );
        }
        for (idx, method) in private_methods.iter().enumerate() {
            names.insert(
                method.clone(),
                Rc::new(PrivateId {
                    typ: typ.clone(),
                    name: unistring::String::default(),
                    idx: idx as u32,
                    is_method: true,
                }),
            );
        }
    }

    fn exec_init_static_elements(&mut self, i: &InitStaticElements) -> JsResult<()> {
        let sp = self.vm.sp as usize;
        let cls = self.vm.stack[sp - 1].clone();
        let si = self.vm.stack[sp - 3].clone();
        let static_init = self.to_object(&si)?;
        self.vm.sp -= 2;
        if self.is_class_func(static_init) {
            BaseObject::_put_prop(
                self,
                static_init,
                &STR_PROTOTYPE,
                cls.clone(),
                true,
                true,
                true,
            );
            let pe = self.vm.priv_env.clone();
            BaseJsFuncObject::data_mut(self, static_init).priv_env = pe;
            let typ = BaseJsFuncObject::data(self, static_init)
                .private_env_type
                .clone();
            if let Some(typ) = typ {
                if let Some(pe) = &self.vm.priv_env {
                    *pe.static_type.borrow_mut() = Some(typ.clone());
                }
                self.vm_fill_private_names_map(Some(&typ), &i.private_fields, &i.private_methods);
            }
            let c = self.to_object(&cls)?;
            class_func_init_fields(self, static_init, c)?;
            let p = self.vm.sp as usize - 1;
            self.vm.stack[p].set(cls);
            self.vm.pc += 1;
            return Ok(());
        }
        let s = Value::Object(static_init).go_string(self)?;
        Err(Throw::Value(self.new_type_error(&format!(
            "Compiler bug: unexpected target for initStaticElements: {s}"
        ))))
    }

    fn vm_private_methods_owner(&mut self, d: &DefinePrivateMethod) -> JsResult<ObjRef> {
        let v = self.vm.stack[self.vm.sp as usize - d.target_offset as usize].clone();
        let obj = self.to_object(&v)?;
        if self.is_class_func(obj) {
            return Ok(obj);
        }
        let t = self.go_type_name(obj);
        Err(Throw::Value(self.new_type_error(&format!(
            "Compiler bug: wrong target type for definePrivateMethod: {t}"
        ))))
    }

    fn exec_define_private_accessor(
        &mut self,
        d: &DefinePrivateMethod,
        getter: bool,
    ) -> JsResult<()> {
        let owner = self.vm_private_methods_owner(d)?;
        let val = self.vm.stack[self.vm.sp as usize - 1].clone();
        let method = self.to_object(&val)?;
        let idx = d.idx as usize;
        let already = {
            let fd = BaseJsFuncObject::data_mut(self, owner);
            let slot = &mut fd.private_methods[idx];
            if !matches!(slot, Value::Property(_)) {
                *slot = Value::Property(Box::new(ValueProperty {
                    accessor: true,
                    ..ValueProperty::default()
                }));
            }
            let Value::Property(p) = slot else {
                unreachable!()
            };
            if getter {
                if p.getter_func.is_some() {
                    true
                } else {
                    p.getter_func = Some(method);
                    false
                }
            } else if p.setter_func.is_some() {
                true
            } else {
                p.setter_func = Some(method);
                false
            }
        };
        if already {
            let msg = if getter {
                "Private getter has already been declared"
            } else {
                "Private setter has already been declared"
            };
            return Err(Throw::Value(self.new_type_error(msg)));
        }
        self.vm.sp -= 1;
        self.vm.pc += 1;
        Ok(())
    }

    fn private_elements(&self, o: ObjRef, typ: &Arc<PrivateEnvType>) -> Option<&PrivateElements> {
        self.obj(o)
            .base
            .private_elements
            .as_ref()
            .and_then(|m| m.get(&private_env_key(typ)))
    }

    fn private_elements_mut(
        &mut self,
        o: ObjRef,
        typ: &Arc<PrivateEnvType>,
    ) -> Option<&mut PrivateElements> {
        self.obj_mut(o)
            .base
            .private_elements
            .as_mut()
            .and_then(|m| m.get_mut(&private_env_key(typ)))
    }

    fn exec_private_in(
        &mut self,
        typ: Option<&Arc<PrivateEnvType>>,
        idx: u32,
        is_method: bool,
    ) -> JsResult<()> {
        let p = self.vm.sp as usize - 1;
        let v = self.vm.stack[p].clone();
        let obj = self.to_object(&v)?;
        let mut has = false;
        if let Some(typ) = typ
            && obj.get_private_env(self, typ, false)?
            && let Some(pe) = self.private_elements(obj, typ)
        {
            let slot = if is_method {
                pe.methods.get(idx as usize)
            } else {
                pe.fields.get(idx as usize)
            };
            has = slot.is_some_and(|v| !v.is_empty());
        }
        self.vm.stack[p].set(Value::Bool(has));
        self.vm.pc += 1;
        Ok(())
    }

    pub fn vm_get_private_type(&self, level: u8, is_static: bool) -> Option<Arc<PrivateEnvType>> {
        let mut e = self.vm.priv_env.as_ref();
        for _ in 0..level {
            e = e.and_then(|x| x.outer.as_ref());
        }
        let e = e?;
        if is_static {
            e.static_type.borrow().clone()
        } else {
            e.instance_type.borrow().clone()
        }
    }

    pub fn vm_get_private_prop(
        &mut self,
        base: &Value,
        name: &unistring::String,
        typ: Option<&Arc<PrivateEnvType>>,
        idx: u32,
        is_method: bool,
    ) -> JsResult<Value> {
        let obj = self.to_object(base)?;
        let mut penv = None;
        if let Some(typ) = typ
            && obj.get_private_env(self, typ, false)?
        {
            penv = self.private_elements(obj, typ).map(|pe| {
                if is_method {
                    pe.methods.get(idx as usize).cloned().unwrap_or_default()
                } else {
                    pe.fields.get(idx as usize).cloned().unwrap_or(Value::Empty)
                }
            });
        }
        let mut v = match penv {
            Some(v) => {
                if !is_method && v.is_empty() {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Private member #{name} is accessed before it is initialized"
                    ))));
                }
                v
            }
            None => {
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Cannot read private member #{name} from an object whose class did not declare it"
                ))));
            }
        };
        if let Value::Property(prop) = &v {
            if prop.getter_func.is_none() {
                return Err(Throw::Value(self.new_type_error(&format!(
                    "'#{name}' was defined without a getter"
                ))));
            }
            let prop = prop.clone();
            v = prop.get(self, &Value::Object(obj))?;
        }
        Ok(v)
    }

    pub fn vm_set_private_prop(
        &mut self,
        base: &Value,
        name: &unistring::String,
        typ: Option<&Arc<PrivateEnvType>>,
        idx: u32,
        is_method: bool,
        val: Value,
    ) -> JsResult<()> {
        let obj = self.to_object(base)?;
        let exists = match typ {
            Some(typ) => {
                obj.get_private_env(self, typ, false)? && self.private_elements(obj, typ).is_some()
            }
            None => false,
        };
        if !exists {
            return Err(Throw::Value(self.new_type_error(&format!(
                "Cannot write private member #{name} from an object whose class did not declare it"
            ))));
        }
        let typ = typ.expect("private type");
        if is_method {
            let v = self
                .private_elements(obj, typ)
                .and_then(|pe| pe.methods.get(idx as usize).cloned())
                .unwrap_or(Value::Empty);
            if let Value::Property(prop) = v {
                if let Some(setter) = prop.setter_func {
                    property_set(self, setter, base, val)?;
                } else {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Cannot assign to read only property '#{name}'"
                    ))));
                }
            } else {
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Private method '#{name}' is not writable"
                ))));
            }
        } else {
            let initialised = self
                .private_elements(obj, typ)
                .and_then(|pe| pe.fields.get(idx as usize))
                .is_some_and(|v| !v.is_empty());
            if !initialised {
                return Err(Throw::Value(self.new_type_error(&format!(
                    "Private member #{name} is accessed before it is initialized"
                ))));
            }
            if let Some(pe) = self.private_elements_mut(obj, typ) {
                pe.fields[idx as usize] = val;
            }
        }
        Ok(())
    }

    fn vm_load_this(&mut self, v: Value) -> JsResult<()> {
        if !v.is_empty() {
            self.vm.push(v);
        } else {
            let e = self.get_reference_error();
            return Err(Throw::Value(self.new_error(e, ERR_SUPER_NOT_CALLED)));
        }
        self.vm.pc += 1;
        Ok(())
    }

    pub fn copy_data_properties(&mut self, target: &Value, source: &Value) -> JsResult<()> {
        let target_obj = self.to_object(target)?;
        if matches!(source, Value::Null | Value::Undefined) {
            return Ok(());
        }
        let source_obj = self.vm_value_to_object(source)?;
        let mut it = iterate_enumerable_properties(self, source_obj)?;
        while let Some(item) = it.next(self)? {
            create_data_property_or_throw(
                self,
                target_obj,
                &item.name,
                item.value.unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    pub fn vm_suspend(
        &mut self,
        ectx: &mut ExecCtx,
        try_stack_len: u32,
        iter_stack_len: u32,
        ref_stack_len: u32,
    ) {
        self.vm.save_ctx(&mut ectx.context);
        let sb = self.vm.sb as usize;
        let sp = self.vm.sp as usize;
        ectx.stack.clear();
        ectx.stack.extend_from_slice(&self.vm.stack[sb - 1..sp]);
        if self.vm.try_stack.len() > try_stack_len as usize {
            ectx.try_stack.clear();
            ectx.try_stack
                .extend(self.vm.try_stack.drain(try_stack_len as usize..));
            let sp = (sb - 1) as i32;
            for tf in &mut ectx.try_stack {
                tf.iter_len -= iter_stack_len;
                tf.ref_len -= ref_stack_len;
                tf.sp -= sp;
            }
        }
        if self.vm.iter_stack.len() > iter_stack_len as usize {
            ectx.iter_stack.clear();
            ectx.iter_stack
                .extend(self.vm.iter_stack.drain(iter_stack_len as usize..));
        }
        if self.vm.ref_stack.len() > ref_stack_len as usize {
            ectx.ref_stack.clear();
            ectx.ref_stack
                .extend(self.vm.ref_stack.drain(ref_stack_len as usize..));
        }
    }

    pub fn vm_resume(&mut self, ctx: &mut ExecCtx) {
        self.vm.restore_ctx(&ctx.context);
        let sp = self.vm.sp;
        self.vm.sb = sp + 1;
        let n = ctx.stack.len() as isize;
        self.vm.stack_expand(sp + n);
        for (i, v) in ctx.stack.iter().enumerate() {
            self.vm.stack[sp as usize + i].set(v.clone());
        }
        self.vm.sp += n;
        let call_stack_len = self.vm.call_stack.len() as u32;
        let iter_len = self.vm.iter_stack.len() as u32;
        let ref_len = self.vm.ref_stack.len() as u32;
        for tf in &mut ctx.try_stack {
            tf.call_stack_len = call_stack_len;
            tf.iter_len += iter_len;
            tf.ref_len += ref_len;
            tf.sp += sp as i32;
        }
        self.vm.try_stack.append(&mut ctx.try_stack);
        self.vm.iter_stack.append(&mut ctx.iter_stack);
        self.vm.ref_stack.append(&mut ctx.ref_stack);
    }

    fn new_reg_expp_value(&mut self, n: &NewRegexp) -> JsResult<Value> {
        let _ = n;
        todo!("newRegExpp: the RegExp object is not ported yet")
    }
}

impl Vm {
    pub fn push_ctx(&mut self) -> JsResult<()> {
        if self.call_stack.len() > self.max_call_stack_size {
            return Err(Throw::StackOverflow(Box::new(StackOverflowError {
                exception: Exception::new(Value::Empty),
            })));
        }
        self.push_ctx_unchecked();
        Ok(())
    }
}

impl Program {
    pub fn from_code(code: Vec<Instruction>, src: Option<Arc<crate::file::File>>) -> Program {
        Program {
            code,
            src,
            ..Program::default()
        }
    }
}

impl IteratorRecord {
    pub fn root(&self, rt: &mut Runtime) {
        if let Some(i) = self.iterator {
            rt.handles.push(i);
        }
        if let Some(n) = self.next {
            rt.handles.push(n.obj);
        }
    }
}

pub const VARIADIC_MARKER: SymRef = SymRef(u32::MAX);

pub(crate) const ERR_SUPER_NOT_CALLED: &str =
    "Must call super constructor in derived class before accessing 'this'";
pub(crate) const ERR_SUPER_CALLED_TWICE: &str = "Super constructor may only be called once";

pub(crate) static STR_NAME: LazyLock<unistring::String> =
    LazyLock::new(|| unistring::String::new("name"));
pub(crate) static STR_PROTOTYPE: LazyLock<unistring::String> =
    LazyLock::new(|| unistring::String::new("prototype"));
pub(crate) static STR_CONSTRUCTOR: LazyLock<unistring::String> =
    LazyLock::new(|| unistring::String::new("constructor"));
pub(crate) static STR_RAW: LazyLock<unistring::String> =
    LazyLock::new(|| unistring::String::new("raw"));
pub(crate) static STR_THIS_BINDING: LazyLock<unistring::String> =
    LazyLock::new(|| unistring::String::new(crate::compiler::THIS_BINDING_NAME));

pub(crate) static TAGGED_TEMPLATE_ARRAY_CLASS: LazyLock<ObjClass> = LazyLock::new(|| ObjClass {
    equal: tagged_template_array_equal,
    ..ARRAY_OBJECT_CLASS
});

fn tagged_template_array_equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
    if std::ptr::eq(rt.obj(other).class, &*TAGGED_TEMPLATE_ARRAY_CLASS) {
        return match (
            rt.vm.tagged_template_ids.get(&o),
            rt.vm.tagged_template_ids.get(&other),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
    }
    false
}

fn set_array_values(a: &mut crate::array::ArrayObject, values: Vec<Value>) {
    a.length = values.len() as u32;
    a.obj_count = values.len();
    a.values = values;
}

pub(crate) fn member_unresolved(name: unistring::String) -> Value {
    Value::Unresolved(Box::new(Unresolved { name, member: true }))
}

pub(crate) fn value_unresolved(name: unistring::String) -> Value {
    Value::Unresolved(Box::new(Unresolved {
        name,
        member: false,
    }))
}

pub(crate) fn go_type_of_value(v: &Value) -> &'static str {
    match v {
        Value::Undefined => "goja.valueUndefined",
        Value::Null => "goja.valueNull",
        Value::Bool(_) => "goja.valueBool",
        Value::Int(_) => "goja.valueInt",
        Value::Float(_) => "goja.valueFloat",
        Value::String(JsString::Ascii(_)) => "goja.asciiString",
        Value::String(JsString::Unicode(_)) => "goja.unicodeString",
        Value::String(JsString::Imported(_)) => "*goja.importedString",
        Value::BigInt(_) => "*goja.valueBigInt",
        Value::Symbol(_) => "*goja.Symbol",
        Value::Object(_) => "*goja.Object",
        Value::Empty => "<nil>",
        Value::Property(_) => "*goja.valueProperty",
        Value::Unresolved(u) => {
            if u.member {
                "goja.memberUnresolved"
            } else {
                "goja.valueUnresolved"
            }
        }
        Value::YieldMarker(_) => "*goja.yieldMarker",
    }
}

fn new_stash_ref(typ: VarType, n: unistring::String, stash: StashRef, idx: usize) -> Ref {
    match typ {
        VarType::Var => Ref::Stash { n, stash, idx },
        VarType::Let => Ref::StashLex { n, stash, idx },
        VarType::Const | VarType::StrictConst => Ref::StashConst {
            n,
            stash,
            idx,
            strict_const: typ == VarType::StrictConst,
        },
    }
}

fn new_stack_ref(typ: VarType, n: unistring::String, idx: usize) -> Ref {
    match typ {
        VarType::Var => Ref::StackVar { n, idx },
        VarType::Let => Ref::StackLex { n, idx },
        VarType::Const | VarType::StrictConst => Ref::StackConst {
            n,
            idx,
            strict_const: typ == VarType::StrictConst,
        },
    }
}

pub struct VmEvalContext {
    pub func_type: Option<FuncType>,
    pub priv_env: Option<Rc<PrivateEnv>>,
}

impl VmEvalContext {
    pub fn new(rt: &Runtime) -> VmEvalContext {
        let mut func_type = None;
        let mut s = rt.vm.stash;
        while let Some(st) = s {
            let stash = rt.heap.stash(st);
            if stash.func_type != FuncType::None && stash.func_type != FuncType::Arrow {
                func_type = Some(stash.func_type);
                break;
            }
            s = stash.outer;
        }
        VmEvalContext {
            func_type,
            priv_env: rt.vm.priv_env.clone(),
        }
    }
}

impl EvalContext for VmEvalContext {
    fn func_type(&self) -> Option<FuncType> {
        self.func_type
    }

    fn private_id(&self, name: &unistring::String) -> Option<PrivateId> {
        let mut e = self.priv_env.as_ref();
        while let Some(env) = e {
            if let Some(id) = env.names.borrow().get(name) {
                return Some((**id).clone());
            }
            e = env.outer.as_ref();
        }
        None
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
    rt.vm.stack[rt.vm.sp as usize].set(Value::Object(o));
    rt.vm.sp += 1;
    rt.vm.stack[rt.vm.sp as usize].set(this);
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
        if let Err(e) = rt.vm_push_ctx() {
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
        if let Err(e) = rt.vm_push_ctx() {
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
    match rt.vm_run_try_loop() {
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
    if need_pop {
        rt.vm.pop_ctx();
    }
    rt.vm.pop_try_frame();
    let r = rt.vm.pop();
    rt.root(&r);
    Ok((r, None))
}

pub fn js_func_vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
    rt.vm_push_ctx_for_call()?;
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
    rt.vm_push_ctx_for_call()?;
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
    rt.vm.stack[sp - n - 1].set(Value::Empty);
    rt.vm.stack[sp - n - 2].set(callee);
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
    let proto = new_target.get_str(rt, &STR_PROTOTYPE, None)?;
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
    let (pet, init_fields, stash, priv_env) = {
        let f = BaseJsFuncObject::data(rt, o);
        (
            f.private_env_type.clone(),
            f.init_fields.clone(),
            f.stash,
            f.priv_env.clone(),
        )
    };
    if let Some(pet) = &pet {
        instance.get_private_env(rt, pet, true)?;
        let pm = BaseJsFuncObject::data(rt, o).private_methods.clone();
        let key = crate::object::private_env_key(pet);
        if let Some(m) = &mut rt.obj_mut(instance).base.private_elements
            && let Some(e) = m.get_mut(&key)
        {
            e.methods = pm;
        }
    }
    if let Some(init_fields) = init_fields {
        rt.vm_push_ctx()?;
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
    rt: &mut Runtime,
    name: &ResolvedPrivateName,
    base: &Value,
) -> JsResult<Value> {
    let typ = rt.vm_get_private_type(name.level, name.is_static);
    rt.vm_get_private_prop(base, &name.name, typ.as_ref(), name.idx, name.is_method)
}

pub fn set_private_prop_res(
    rt: &mut Runtime,
    name: &ResolvedPrivateName,
    base: &Value,
    val: Value,
) -> JsResult<()> {
    let typ = rt.vm_get_private_type(name.level, name.is_static);
    rt.vm_set_private_prop(
        base,
        &name.name,
        typ.as_ref(),
        name.idx,
        name.is_method,
        val,
    )
}

pub fn generator_next(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    crate::func::generator_object_next(rt, o, v)
}

pub fn generator_return(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    crate::func::generator_object_return(rt, o, v)
}

pub fn generator_throw(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    crate::func::generator_object_throw(rt, o, v)
}
