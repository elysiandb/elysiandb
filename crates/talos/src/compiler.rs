use crate::ast;
use crate::compiler_expr::{CompiledExpr, ExprKind, FuncType};
use crate::file::File;
use crate::object::PrivateId;
use crate::string::{JsString, ascii_string};
use crate::token;
use crate::unistring;
use crate::value::Value;
use crate::vm::{
    ConstException, ConstThrow, EnterBlock, Instruction, LeaveBlock, LoadMixed, LoadMixedStack,
    ResolveMixed, ResolveMixedStack, eval_const_code,
};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

pub(crate) type ScopeId = usize;
pub(crate) type BindingId = usize;
pub(crate) type BlockId = usize;
pub(crate) type ProgId = usize;
pub(crate) type ClassScopeId = usize;

pub(crate) type CResult<T> = Result<T, CompilerSyntaxError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockType {
    Loop,
    LoopEnum,
    Try,
    Label,
    Switch,
    With,
    Scope,
    IterScope,
    OptChain,
}

pub const MASK_CONST: u32 = 1 << 31;
pub const MASK_VAR: u32 = 1 << 30;
pub const MASK_DELETABLE: u32 = 1 << 29;
pub const MASK_STRICT: u32 = MASK_DELETABLE;
pub const MASK_TYP: u32 = MASK_CONST | MASK_VAR | MASK_DELETABLE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VarType {
    Var = 0,
    Let = 1,
    StrictConst = 2,
    Const = 3,
}

pub(crate) const THIS_BINDING_NAME: &str = " this";

#[derive(Clone, Debug)]
pub struct CompilerError {
    pub message: Vec<u8>,
    pub file: Option<Arc<File>>,
    pub offset: isize,
}

#[derive(Clone, Debug)]
pub struct CompilerSyntaxError(pub CompilerError);

#[derive(Clone, Debug)]
pub struct CompilerReferenceError(pub CompilerError);

impl CompilerSyntaxError {
    pub fn error(&self) -> Vec<u8> {
        let mut out = b"SyntaxError: ".to_vec();
        out.extend_from_slice(&self.0.message);
        if let Some(f) = &self.0.file {
            out.extend_from_slice(b" at ");
            out.extend_from_slice(f.position(self.0.offset as i64).string().as_bytes());
        }
        out
    }
}

impl fmt::Display for CompilerSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.error()))
    }
}

impl std::error::Error for CompilerSyntaxError {}

impl CompilerReferenceError {
    pub fn error(&self) -> Vec<u8> {
        let mut out = b"ReferenceError: ".to_vec();
        out.extend_from_slice(&self.0.message);
        out
    }
}

impl fmt::Display for CompilerReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.error()))
    }
}

impl std::error::Error for CompilerReferenceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrcMapItem {
    pub pc: isize,
    pub src_pos: isize,
}

#[derive(Debug, Default)]
pub struct Program {
    pub(crate) code: Vec<Instruction>,
    pub(crate) func_name: unistring::String,
    pub(crate) src: Option<Arc<File>>,
    pub(crate) src_map: Vec<SrcMapItem>,
}

impl Program {
    pub fn code(&self) -> &[Instruction] {
        &self.code
    }

    pub fn func_name(&self) -> &unistring::String {
        &self.func_name
    }

    pub fn src(&self) -> Option<&Arc<File>> {
        self.src.as_ref()
    }

    pub fn src_map(&self) -> &[SrcMapItem] {
        &self.src_map
    }

    pub fn source_offset(&self, pc: isize) -> isize {
        let i = self.src_map.partition_point(|item| item.pc <= pc);
        if i > 0 {
            return self.src_map[i - 1].src_pos;
        }
        0
    }
}

pub(crate) struct ProgBuild {
    pub(crate) code: Vec<Instruction>,
    pub(crate) func_name: unistring::String,
    pub(crate) src_map: Vec<SrcMapItem>,
    pub(crate) handle: Arc<Program>,
}

impl ProgBuild {
    pub(crate) fn add_src_map(&mut self, src_pos: isize) {
        if let Some(last) = self.src_map.last()
            && last.src_pos == src_pos
        {
            return;
        }
        self.src_map.push(SrcMapItem {
            pc: self.code.len() as isize,
            src_pos,
        });
    }
}

#[derive(Default)]
pub(crate) struct Binding {
    pub(crate) scope: ScopeId,
    pub(crate) name: unistring::String,
    pub(crate) access_points: Vec<(ScopeId, Vec<isize>)>,
    pub(crate) is_const: bool,
    pub(crate) is_strict: bool,
    pub(crate) is_arg: bool,
    pub(crate) is_var: bool,
    pub(crate) in_stash: bool,
}

pub(crate) struct Scope {
    pub(crate) prg: ProgId,
    pub(crate) outer: Option<ScopeId>,
    pub(crate) nested: Vec<ScopeId>,
    pub(crate) bound_names: HashMap<unistring::String, BindingId>,
    pub(crate) bindings: Vec<BindingId>,
    pub(crate) base: isize,
    pub(crate) num_args: usize,
    pub(crate) func_type: FuncType,
    pub(crate) strict: bool,
    pub(crate) eval: bool,
    pub(crate) dyn_lookup: bool,
    pub(crate) need_stash: bool,
    pub(crate) variable: bool,
    pub(crate) dynamic: bool,
    pub(crate) args_in_stash: bool,
    pub(crate) args_needed: bool,
}

#[derive(Clone)]
pub(crate) struct Block {
    pub(crate) typ: BlockType,
    pub(crate) label: unistring::String,
    pub(crate) cont: isize,
    pub(crate) breaks: Vec<usize>,
    pub(crate) conts: Vec<usize>,
    pub(crate) outer: Option<BlockId>,
    pub(crate) breaking: Option<BlockId>,
    pub(crate) need_result: bool,
}

impl Block {
    pub(crate) fn new(typ: BlockType, outer: Option<BlockId>) -> Block {
        Block {
            typ,
            label: unistring::String::default(),
            cont: 0,
            breaks: Vec::new(),
            conts: Vec::new(),
            outer,
            breaking: None,
            need_result: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PrivateName {
    pub(crate) idx: isize,
    pub(crate) is_static: bool,
    pub(crate) is_method: bool,
    pub(crate) has_getter: bool,
    pub(crate) has_setter: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedPrivateName {
    pub name: unistring::String,
    pub idx: u32,
    pub level: u8,
    pub is_static: bool,
    pub is_method: bool,
}

impl ResolvedPrivateName {
    pub fn string(&self) -> unistring::String {
        private_id_string(&self.name)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PrivateEnvRegistry {
    pub(crate) fields: Vec<unistring::String>,
    pub(crate) methods: Vec<unistring::String>,
}

impl PrivateEnvRegistry {
    fn create_private_method_id(&mut self, name: &unistring::String) -> isize {
        self.methods.push(name.clone());
        self.methods.len() as isize - 1
    }

    fn create_private_field_id(&mut self, name: &unistring::String) -> isize {
        self.fields.push(name.clone());
        self.fields.len() as isize - 1
    }
}

#[derive(Default)]
pub(crate) struct ClassScope {
    pub(crate) private_names: HashMap<unistring::String, PrivateName>,
    pub(crate) instance_env: PrivateEnvRegistry,
    pub(crate) static_env: PrivateEnvRegistry,
    pub(crate) outer: Option<ClassScopeId>,
}

pub trait EvalContext {
    fn func_type(&self) -> Option<FuncType>;
    fn private_id(&self, name: &unistring::String) -> Option<PrivateId>;
}

pub(crate) fn private_id_string(desc: &unistring::String) -> unistring::String {
    ascii_string("#")
        .concat(&crate::string::string_value_from_raw(desc))
        .string()
}

pub(crate) fn go_bytes(s: &unistring::String) -> Vec<u8> {
    s.as_bytes().into_owned()
}

pub(crate) struct Compiler {
    pub(crate) progs: Vec<ProgBuild>,
    pub(crate) p: ProgId,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) scope: Option<ScopeId>,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) blocks: Vec<Block>,
    pub(crate) block: Option<BlockId>,
    pub(crate) class_scopes: Vec<ClassScope>,
    pub(crate) class_scope: Option<ClassScopeId>,
    pub(crate) eval_prg: Option<ProgId>,
    pub(crate) eval_active: bool,
    pub(crate) ctx_vm: Option<Rc<dyn EvalContext>>,
    pub(crate) string_cache: HashMap<unistring::String, JsString>,
    pub(crate) src: Option<Arc<File>>,
}

impl Compiler {
    pub(crate) fn new() -> Compiler {
        let mut c = Compiler {
            progs: Vec::new(),
            p: 0,
            scopes: Vec::new(),
            scope: None,
            bindings: Vec::new(),
            blocks: Vec::new(),
            block: None,
            class_scopes: Vec::new(),
            class_scope: None,
            eval_prg: None,
            eval_active: false,
            ctx_vm: None,
            string_cache: HashMap::new(),
            src: None,
        };
        c.p = c.new_prog(unistring::String::default(), Vec::new(), Vec::new());
        c
    }

    pub(crate) fn new_prog(
        &mut self,
        func_name: unistring::String,
        code: Vec<Instruction>,
        src_map: Vec<SrcMapItem>,
    ) -> ProgId {
        self.progs.push(ProgBuild {
            code,
            func_name,
            src_map,
            handle: Arc::new(Program::default()),
        });
        self.progs.len() - 1
    }

    pub(crate) fn code_len(&self) -> usize {
        self.progs[self.p].code.len()
    }

    pub(crate) fn code_mut(&mut self) -> &mut Vec<Instruction> {
        &mut self.progs[self.p].code
    }

    pub(crate) fn set_code(&mut self, pc: usize, ins: Instruction) {
        self.progs[self.p].code[pc] = ins;
    }

    pub(crate) fn emit(&mut self, ins: Instruction) {
        self.progs[self.p].code.push(ins);
    }

    pub(crate) fn add_src_map(&mut self, src_pos: isize) {
        self.progs[self.p].add_src_map(src_pos);
    }

    pub(crate) fn add_src_map_node(&mut self, idx0: i64) {
        self.add_src_map(idx0 as isize - 1);
    }

    pub(crate) fn source_offset(&self, pc: isize) -> isize {
        let m = &self.progs[self.p].src_map;
        let i = m.partition_point(|item| item.pc <= pc);
        if i > 0 {
            return m[i - 1].src_pos;
        }
        0
    }

    pub(crate) fn cur_scope(&self) -> ScopeId {
        self.scope.expect("compiler scope")
    }

    pub(crate) fn sc(&self) -> &Scope {
        &self.scopes[self.cur_scope()]
    }

    pub(crate) fn sc_mut(&mut self) -> &mut Scope {
        let s = self.cur_scope();
        &mut self.scopes[s]
    }

    pub(crate) fn strict(&self) -> bool {
        self.sc().strict
    }

    pub(crate) fn blk(&self) -> &Block {
        &self.blocks[self.block.expect("compiler block")]
    }

    pub(crate) fn blk_mut(&mut self) -> &mut Block {
        let b = self.block.expect("compiler block");
        &mut self.blocks[b]
    }

    pub(crate) fn push_block(&mut self, b: Block) -> BlockId {
        self.blocks.push(b);
        let id = self.blocks.len() - 1;
        self.block = Some(id);
        id
    }

    pub(crate) fn syntax_error(&self, offset: isize, message: Vec<u8>) -> CompilerSyntaxError {
        CompilerSyntaxError(CompilerError {
            message,
            file: self.src.clone(),
            offset,
        })
    }

    pub(crate) fn throw_syntax_error<T>(
        &self,
        offset: isize,
        message: impl AsRef<[u8]>,
    ) -> CResult<T> {
        Err(self.syntax_error(offset, message.as_ref().to_vec()))
    }

    pub(crate) fn assert(&self, cond: bool, offset: isize, msg: impl AsRef<[u8]>) -> CResult<()> {
        if !cond {
            let mut m = b"Compiler bug: ".to_vec();
            m.extend_from_slice(msg.as_ref());
            return self.throw_syntax_error(offset, m);
        }
        Ok(())
    }

    fn new_scope_struct(&self, outer: Option<ScopeId>, strict: bool) -> Scope {
        Scope {
            prg: self.p,
            outer,
            nested: Vec::new(),
            bound_names: HashMap::new(),
            bindings: Vec::new(),
            base: 0,
            num_args: 0,
            func_type: FuncType::None,
            strict,
            eval: false,
            dyn_lookup: false,
            need_stash: false,
            variable: false,
            dynamic: false,
            args_in_stash: false,
            args_needed: false,
        }
    }

    pub(crate) fn new_scope(&mut self) {
        let strict = match self.scope {
            Some(s) => self.scopes[s].strict,
            None => false,
        };
        let s = self.new_scope_struct(self.scope, strict);
        self.scopes.push(s);
        self.scope = Some(self.scopes.len() - 1);
    }

    pub(crate) fn new_block_scope(&mut self) {
        self.new_scope();
        let s = self.cur_scope();
        if let Some(outer) = self.scopes[s].outer {
            self.scopes[outer].nested.push(s);
        }
        self.scopes[s].base = self.code_len() as isize;
    }

    pub(crate) fn pop_scope(&mut self) {
        self.scope = self.scopes[self.cur_scope()].outer;
    }

    pub(crate) fn leave_scope_block(&mut self, enter_pc: usize) -> CResult<()> {
        self.update_enter_block_at(enter_pc);
        let (stack_size, pop_stash) = match &self.progs[self.p].code[enter_pc] {
            Instruction::EnterBlock(e) => (e.stack_size, e.stash_size > 0),
            _ => (0, false),
        };
        let leave = LeaveBlock {
            stack_size,
            pop_stash,
        };
        self.emit(Instruction::LeaveBlock(leave.clone()));
        let breaks = std::mem::take(&mut self.blk_mut().breaks);
        for pc in breaks {
            self.set_code(pc, Instruction::LeaveBlock(leave.clone()));
        }
        self.leave_block();
        Ok(())
    }

    pub(crate) fn leave_block(&mut self) {
        let lbl = self.code_len();
        let b = self.block.expect("compiler block");
        let breaks = self.blocks[b].breaks.clone();
        for item in breaks {
            self.set_code(item, Instruction::Jump((lbl - item) as i32));
        }
        let typ = self.blocks[b].typ;
        if typ == BlockType::Loop || typ == BlockType::LoopEnum {
            let cont = self.blocks[b].cont;
            let conts = self.blocks[b].conts.clone();
            for item in conts {
                self.set_code(item, Instruction::Jump((cont - item as isize) as i32));
            }
        }
        self.block = self.blocks[b].outer;
    }

    pub(crate) fn update_enter_block(&self, e: &mut EnterBlock) {
        let scope = self.sc();
        let (mut stash_size, mut stack_size) = (0u32, 0u32);
        if scope.dyn_lookup {
            stash_size = scope.bindings.len() as u32;
            e.names = self.make_names_map(self.cur_scope());
        } else {
            for &b in &scope.bindings {
                if self.bindings[b].in_stash {
                    stash_size += 1;
                } else {
                    stack_size += 1;
                }
            }
        }
        e.stash_size = stash_size;
        e.stack_size = stack_size;
    }

    pub(crate) fn update_enter_block_at(&mut self, pc: usize) {
        let mut e = match &self.progs[self.p].code[pc] {
            Instruction::EnterBlock(e) => (**e).clone(),
            _ => EnterBlock::default(),
        };
        self.update_enter_block(&mut e);
        self.set_code(pc, Instruction::EnterBlock(Box::new(e)));
    }

    pub(crate) fn emit_literal_string(&mut self, s: JsString) {
        let key = s.string();
        let intern = match self.string_cache.get(&key) {
            Some(v) => v.clone(),
            None => {
                self.string_cache.insert(key, s.clone());
                s
            }
        };
        self.emit(Instruction::LoadVal(Value::String(intern)));
    }

    pub(crate) fn emit_literal_value(&mut self, v: Value) {
        if let Value::String(s) = v {
            self.emit_literal_string(s);
            return;
        }
        self.emit(Instruction::LoadVal(v));
    }

    pub(crate) fn new_binding(&mut self, scope: ScopeId) -> BindingId {
        self.bindings.push(Binding {
            scope,
            ..Binding::default()
        });
        self.bindings.len() - 1
    }

    fn access_points_for(&mut self, b: BindingId, scope: ScopeId) -> &mut Vec<isize> {
        let aps = &mut self.bindings[b].access_points;
        let i = match aps.iter().position(|(s, _)| *s == scope) {
            Some(i) => i,
            None => {
                aps.push((scope, Vec::with_capacity(1)));
                aps.len() - 1
            }
        };
        &mut aps[i].1
    }

    pub(crate) fn mark_access_point_at(&mut self, b: BindingId, pos: usize) {
        let scope = self.cur_scope();
        let base = self.scopes[scope].base;
        self.access_points_for(b, scope).push(pos as isize - base);
    }

    pub(crate) fn mark_access_point_at_scope(&mut self, b: BindingId, scope: ScopeId, pos: usize) {
        let base = self.scopes[scope].base;
        self.access_points_for(b, scope).push(pos as isize - base);
    }

    pub(crate) fn mark_access_point(&mut self, b: BindingId) {
        let scope = self.cur_scope();
        let prg = self.scopes[scope].prg;
        let pos = self.progs[prg].code.len() as isize - self.scopes[scope].base;
        self.access_points_for(b, scope).push(pos);
    }

    fn b_is_stack_var(&self, b: BindingId) -> bool {
        let b = &self.bindings[b];
        b.is_var && !b.is_arg
    }

    pub(crate) fn b_emit_get(&mut self, b: BindingId) {
        self.mark_access_point(b);
        if self.b_is_stack_var(b) {
            self.emit(Instruction::LoadStack(0));
        } else {
            self.emit(Instruction::LoadStackLex(0));
        }
    }

    pub(crate) fn b_emit_get_at(&mut self, b: BindingId, pos: usize) {
        self.mark_access_point_at(b, pos);
        if self.b_is_stack_var(b) {
            self.set_code(pos, Instruction::LoadStack(0));
        } else {
            self.set_code(pos, Instruction::LoadStackLex(0));
        }
    }

    pub(crate) fn b_emit_get_p(&mut self, b: BindingId) {
        if !self.b_is_stack_var(b) {
            self.mark_access_point(b);
            self.emit(Instruction::LoadStackLex(0));
            self.emit(Instruction::Pop);
        }
    }

    pub(crate) fn b_emit_set(&mut self, b: BindingId) {
        if self.bindings[b].is_const {
            if self.bindings[b].is_strict || self.strict() {
                self.emit(Instruction::ThrowAssignToConst);
            }
            return;
        }
        self.mark_access_point(b);
        if self.b_is_stack_var(b) {
            self.emit(Instruction::StoreStack(0));
        } else {
            self.emit(Instruction::StoreStackLex(0));
        }
    }

    pub(crate) fn b_emit_set_p(&mut self, b: BindingId) {
        if self.bindings[b].is_const {
            if self.bindings[b].is_strict || self.strict() {
                self.emit(Instruction::ThrowAssignToConst);
            }
            return;
        }
        self.mark_access_point(b);
        if self.b_is_stack_var(b) {
            self.emit(Instruction::StoreStackP(0));
        } else {
            self.emit(Instruction::StoreStackLexP(0));
        }
    }

    fn b_is_global_lex(&self, b: BindingId, scope: ScopeId) -> bool {
        !self.bindings[b].is_var && self.scopes[scope].outer.is_none()
    }

    pub(crate) fn b_emit_init_p(&mut self, b: BindingId) {
        if self.b_is_global_lex(b, self.bindings[b].scope) {
            let name = self.bindings[b].name.clone();
            self.emit(Instruction::InitGlobalP(name));
        } else {
            self.mark_access_point(b);
            self.emit(Instruction::InitStackP(0));
        }
    }

    pub(crate) fn b_emit_init(&mut self, b: BindingId) {
        if self.b_is_global_lex(b, self.bindings[b].scope) {
            let name = self.bindings[b].name.clone();
            self.emit(Instruction::InitGlobal(name));
        } else {
            self.mark_access_point(b);
            self.emit(Instruction::InitStack(0));
        }
    }

    pub(crate) fn b_emit_init_at(&mut self, b: BindingId, pos: usize) {
        if self.b_is_global_lex(b, self.bindings[b].scope) {
            let name = self.bindings[b].name.clone();
            self.set_code(pos, Instruction::InitGlobal(name));
        } else {
            self.mark_access_point_at(b, pos);
            self.set_code(pos, Instruction::InitStack(0));
        }
    }

    pub(crate) fn b_emit_init_at_scope(&mut self, b: BindingId, scope: ScopeId, pos: usize) {
        if self.b_is_global_lex(b, scope) {
            let name = self.bindings[b].name.clone();
            self.set_code(pos, Instruction::InitGlobal(name));
        } else {
            self.mark_access_point_at_scope(b, scope, pos);
            self.set_code(pos, Instruction::InitStack(0));
        }
    }

    pub(crate) fn b_emit_init_p_at_scope(&mut self, b: BindingId, scope: ScopeId, pos: usize) {
        if self.b_is_global_lex(b, scope) {
            let name = self.bindings[b].name.clone();
            self.set_code(pos, Instruction::InitGlobalP(name));
        } else {
            self.mark_access_point_at_scope(b, scope, pos);
            self.set_code(pos, Instruction::InitStackP(0));
        }
    }

    pub(crate) fn b_emit_get_var(&mut self, b: BindingId, callee: bool) {
        self.mark_access_point(b);
        let lm = Box::new(LoadMixed {
            name: self.bindings[b].name.clone(),
            idx: 0,
            callee,
        });
        if self.b_is_stack_var(b) {
            self.emit(Instruction::LoadMixed(lm));
        } else {
            self.emit(Instruction::LoadMixedLex(lm));
        }
    }

    pub(crate) fn b_emit_resolve_var(&mut self, b: BindingId, strict: bool) {
        self.mark_access_point(b);
        let bb = &self.bindings[b];
        let typ = if bb.is_var && !bb.is_arg {
            VarType::Var
        } else if bb.is_const {
            if bb.is_strict {
                VarType::StrictConst
            } else {
                VarType::Const
            }
        } else {
            VarType::Let
        };
        let name = bb.name.clone();
        self.emit(Instruction::ResolveMixed(Box::new(ResolveMixed {
            name,
            idx: 0,
            typ,
            strict,
        })));
    }

    pub(crate) fn b_move_to_stash(&mut self, b: BindingId) {
        let s = self.bindings[b].scope;
        if self.bindings[b].is_arg && !self.scopes[s].args_in_stash {
            self.move_args_to_stash(s);
        } else {
            self.bindings[b].in_stash = true;
            self.scopes[s].need_stash = true;
        }
    }

    pub(crate) fn b_use_count(&self, b: BindingId) -> usize {
        self.bindings[b]
            .access_points
            .iter()
            .map(|(_, a)| a.len())
            .sum()
    }

    pub(crate) fn lookup_name(
        &mut self,
        s: ScopeId,
        name: &unistring::String,
    ) -> CResult<(Option<BindingId>, bool)> {
        let mut no_dynamics = true;
        let mut to_stash = false;
        let mut cur = s;
        loop {
            if self.scopes[cur].outer.is_some() {
                if let Some(&b) = self.scopes[cur].bound_names.get(name) {
                    if to_stash && !self.bindings[b].in_stash {
                        self.b_move_to_stash(b);
                    }
                    return Ok((Some(b), no_dynamics));
                }
            } else {
                return Ok((None, false));
            }
            if self.scopes[cur].dynamic {
                no_dynamics = false;
            }
            let ft = self.scopes[cur].func_type;
            if *name == "arguments" && ft != FuncType::None && ft != FuncType::Arrow {
                if ft == FuncType::ClsInit {
                    return self.throw_syntax_error(
                        0,
                        "'arguments' is not allowed in class field initializer or static initialization block",
                    );
                }
                self.scopes[cur].args_needed = true;
                let (b, _) = self.bind_name(cur, name)?;
                return Ok((Some(b), no_dynamics));
            }
            if self.is_function(cur) {
                to_stash = true;
            }
            cur = self.scopes[cur].outer.expect("outer scope");
        }
    }

    pub(crate) fn lookup_this(&mut self, s: ScopeId) -> (Option<BindingId>, bool) {
        let mut to_stash = false;
        let mut cur = Some(s);
        while let Some(c) = cur {
            if self.scopes[c].outer.is_none() && self.scopes[c].eval {
                return (None, true);
            }
            if let Some(&b) = self.scopes[c]
                .bound_names
                .get(&unistring::String::new(THIS_BINDING_NAME))
            {
                if to_stash && !self.bindings[b].in_stash {
                    self.b_move_to_stash(b);
                }
                return (Some(b), false);
            }
            if self.is_function(c) {
                to_stash = true;
            }
            cur = self.scopes[c].outer;
        }
        (None, false)
    }

    pub(crate) fn add_binding(&mut self, s: ScopeId, offset: isize) -> CResult<BindingId> {
        if self.scopes[s].bindings.len() >= (1 << 24) - 1 {
            return self.throw_syntax_error(offset, "Too many variables");
        }
        let b = self.new_binding(s);
        self.scopes[s].bindings.push(b);
        Ok(b)
    }

    pub(crate) fn bind_name_lexical(
        &mut self,
        s: ScopeId,
        name: &unistring::String,
        unique: bool,
        offset: isize,
    ) -> CResult<(BindingId, bool)> {
        if let Some(&b) = self.scopes[s].bound_names.get(name) {
            if unique {
                let mut m = b"Identifier '".to_vec();
                m.extend_from_slice(&go_bytes(name));
                m.extend_from_slice(b"' has already been declared");
                return self.throw_syntax_error(offset, m);
            }
            return Ok((b, false));
        }
        let b = self.add_binding(s, offset)?;
        self.bindings[b].name = name.clone();
        self.scopes[s].bound_names.insert(name.clone(), b);
        Ok((b, true))
    }

    pub(crate) fn create_this_binding(&mut self, s: ScopeId) -> CResult<BindingId> {
        let (b, _) =
            self.bind_name_lexical(s, &unistring::String::new(THIS_BINDING_NAME), false, 0)?;
        self.bindings[b].is_var = true;
        Ok(b)
    }

    pub(crate) fn bind_name(
        &mut self,
        s: ScopeId,
        name: &unistring::String,
    ) -> CResult<(BindingId, bool)> {
        if !self.is_function(s)
            && !self.scopes[s].variable
            && let Some(outer) = self.scopes[s].outer
        {
            return self.bind_name(outer, name);
        }
        let (b, created) = self.bind_name_lexical(s, name, false, 0)?;
        if created {
            self.bindings[b].is_var = true;
        }
        Ok((b, created))
    }

    pub(crate) fn bind_name_shadow(
        &mut self,
        s: ScopeId,
        name: &unistring::String,
    ) -> (BindingId, bool) {
        if !self.is_function(s)
            && let Some(outer) = self.scopes[s].outer
        {
            return self.bind_name_shadow(outer, name);
        }
        let exists = self.scopes[s].bound_names.contains_key(name);
        let b = self.new_binding(s);
        self.bindings[b].name = name.clone();
        self.scopes[s].bindings.push(b);
        self.scopes[s].bound_names.insert(name.clone(), b);
        (b, !exists)
    }

    pub(crate) fn nearest_function(&self, s: ScopeId) -> Option<ScopeId> {
        let mut sc = Some(s);
        while let Some(c) = sc {
            if self.is_function(c) {
                return Some(c);
            }
            sc = self.scopes[c].outer;
        }
        None
    }

    pub(crate) fn nearest_this(&self, s: ScopeId) -> Option<ScopeId> {
        let mut sc = Some(s);
        while let Some(c) = sc {
            let scope = &self.scopes[c];
            if scope.eval || self.is_function(c) && scope.func_type != FuncType::Arrow {
                return Some(c);
            }
            sc = scope.outer;
        }
        None
    }

    fn access_level(&self, scope: ScopeId, s: ScopeId) -> u32 {
        let mut level = 0u32;
        let mut sc = Some(scope);
        while let Some(c) = sc {
            if c == s {
                break;
            }
            if self.scopes[c].need_stash || self.is_dynamic(c) {
                level += 1;
            }
            sc = self.scopes[c].outer;
        }
        level
    }

    pub(crate) fn finalise_var_alloc(
        &mut self,
        s: ScopeId,
        stack_offset: isize,
    ) -> CResult<(u32, isize)> {
        let args_in_stash = match self.nearest_function(s) {
            Some(f) => self.scopes[f].args_in_stash,
            None => false,
        };
        let (mut stack_idx, mut stash_idx): (isize, u32) = (0, 0);
        let all_in_stash = self.is_dynamic(s);
        let derived_ctor = match self.nearest_this(s) {
            Some(fs) => self.scopes[fs].func_type == FuncType::DerivedCtor,
            None => false,
        };
        let bindings = self.scopes[s].bindings.clone();
        let num_args = self.scopes[s].num_args;
        for (i, &b) in bindings.iter().enumerate() {
            let this = self.bindings[b].name == THIS_BINDING_NAME;
            let aps = self.bindings[b].access_points.clone();
            if all_in_stash || self.bindings[b].in_stash {
                for (scope, pcs) in &aps {
                    let level = self.access_level(*scope, s);
                    if level > 255 {
                        return self.throw_syntax_error(0, "Maximum nesting level (256) exceeded");
                    }
                    let idx = (level << 24) | stash_idx;
                    let base = self.scopes[*scope].base;
                    let prg = self.scopes[*scope].prg;
                    for &pc in pcs {
                        let at = (base + pc) as usize;
                        let ap = &mut self.progs[prg].code[at];
                        let replaced = if this {
                            if derived_ctor {
                                match ap {
                                    Instruction::LoadStack(_) => {
                                        Some(Instruction::LoadThisStash(idx))
                                    }
                                    Instruction::InitStack(_) => Some(Instruction::InitStash(idx)),
                                    Instruction::InitStackP(_) => {
                                        Some(Instruction::InitStashP(idx))
                                    }
                                    Instruction::ResolveThisStack => {
                                        Some(Instruction::ResolveThisStash(idx))
                                    }
                                    Instruction::Ret => Some(Instruction::Cret(idx)),
                                    _ => None,
                                }
                            } else {
                                match ap {
                                    Instruction::LoadStack(_) => Some(Instruction::LoadStash(idx)),
                                    Instruction::InitStack(_) => Some(Instruction::InitStash(idx)),
                                    Instruction::InitStackP(_) => {
                                        Some(Instruction::InitStashP(idx))
                                    }
                                    _ => None,
                                }
                            }
                        } else {
                            match ap {
                                Instruction::LoadStack(_) => Some(Instruction::LoadStash(idx)),
                                Instruction::StoreStack(_) => Some(Instruction::StoreStash(idx)),
                                Instruction::StoreStackP(_) => Some(Instruction::StoreStashP(idx)),
                                Instruction::LoadStackLex(_) => {
                                    Some(Instruction::LoadStashLex(idx))
                                }
                                Instruction::StoreStackLex(_) => {
                                    Some(Instruction::StoreStashLex(idx))
                                }
                                Instruction::StoreStackLexP(_) => {
                                    Some(Instruction::StoreStashLexP(idx))
                                }
                                Instruction::InitStackP(_) => Some(Instruction::InitStashP(idx)),
                                Instruction::InitStack(_) => Some(Instruction::InitStash(idx)),
                                Instruction::LoadMixed(m) | Instruction::LoadMixedLex(m) => {
                                    m.idx = idx;
                                    continue;
                                }
                                Instruction::ResolveMixed(r) => {
                                    r.idx = idx;
                                    continue;
                                }
                                _ => None,
                            }
                        };
                        match replaced {
                            Some(ins) => *ap = ins,
                            None => {
                                let msg = if this {
                                    b"Unsupported instruction for 'this'".to_vec()
                                } else {
                                    let mut m = b"Unsupported instruction for binding: ".to_vec();
                                    m.extend_from_slice(ap.go_type_name().as_bytes());
                                    m
                                };
                                let off = self.source_offset(pc);
                                self.assert(false, off, msg)?;
                                unreachable!();
                            }
                        }
                    }
                }
                stash_idx += 1;
            } else {
                let mut idx: isize = 0;
                if !this {
                    if i < num_args {
                        idx = -(i as isize + 1);
                    } else {
                        stack_idx += 1;
                        idx = stack_idx + stack_offset;
                    }
                }
                for (scope, pcs) in &aps {
                    let level = self.access_level(*scope, s);
                    if level > 255 {
                        return self.throw_syntax_error(0, "Maximum nesting level (256) exceeded");
                    }
                    let level = level as u8;
                    let base = self.scopes[*scope].base;
                    let prg = self.scopes[*scope].prg;
                    for &pc in pcs {
                        let at = (base + pc) as usize;
                        let ap = &mut self.progs[prg].code[at];
                        let replaced: Option<Option<Instruction>> = if this {
                            if derived_ctor {
                                match ap {
                                    Instruction::LoadStack(_) => {
                                        Some(Some(Instruction::LoadThisStack))
                                    }
                                    Instruction::InitStack(_)
                                    | Instruction::InitStackP(_)
                                    | Instruction::ResolveThisStack
                                    | Instruction::Ret => Some(None),
                                    _ => None,
                                }
                            } else {
                                Some(None)
                            }
                        } else if args_in_stash {
                            match ap {
                                Instruction::LoadStack(_) => {
                                    Some(Some(Instruction::LoadStack1(idx)))
                                }
                                Instruction::StoreStack(_) => {
                                    Some(Some(Instruction::StoreStack1(idx)))
                                }
                                Instruction::StoreStackP(_) => {
                                    Some(Some(Instruction::StoreStack1P(idx)))
                                }
                                Instruction::LoadStackLex(_) => {
                                    Some(Some(Instruction::LoadStack1Lex(idx)))
                                }
                                Instruction::StoreStackLex(_) => {
                                    Some(Some(Instruction::StoreStack1Lex(idx)))
                                }
                                Instruction::StoreStackLexP(_) => {
                                    Some(Some(Instruction::StoreStack1LexP(idx)))
                                }
                                Instruction::InitStackP(_) => {
                                    Some(Some(Instruction::InitStack1P(idx)))
                                }
                                Instruction::InitStack(_) => {
                                    Some(Some(Instruction::InitStack1(idx)))
                                }
                                Instruction::LoadMixed(m) => Some(Some(
                                    Instruction::LoadMixedStack1(Box::new(LoadMixedStack {
                                        name: m.name.clone(),
                                        idx,
                                        level,
                                        callee: m.callee,
                                    })),
                                )),
                                Instruction::LoadMixedLex(m) => Some(Some(
                                    Instruction::LoadMixedStack1Lex(Box::new(LoadMixedStack {
                                        name: m.name.clone(),
                                        idx,
                                        level,
                                        callee: m.callee,
                                    })),
                                )),
                                Instruction::ResolveMixed(r) => Some(Some(
                                    Instruction::ResolveMixedStack1(Box::new(ResolveMixedStack {
                                        name: r.name.clone(),
                                        idx,
                                        typ: r.typ,
                                        level,
                                        strict: r.strict,
                                    })),
                                )),
                                _ => None,
                            }
                        } else {
                            match ap {
                                Instruction::LoadStack(_) => {
                                    Some(Some(Instruction::LoadStack(idx)))
                                }
                                Instruction::StoreStack(_) => {
                                    Some(Some(Instruction::StoreStack(idx)))
                                }
                                Instruction::StoreStackP(_) => {
                                    Some(Some(Instruction::StoreStackP(idx)))
                                }
                                Instruction::LoadStackLex(_) => {
                                    Some(Some(Instruction::LoadStackLex(idx)))
                                }
                                Instruction::StoreStackLex(_) => {
                                    Some(Some(Instruction::StoreStackLex(idx)))
                                }
                                Instruction::StoreStackLexP(_) => {
                                    Some(Some(Instruction::StoreStackLexP(idx)))
                                }
                                Instruction::InitStack(_) => {
                                    Some(Some(Instruction::InitStack(idx)))
                                }
                                Instruction::InitStackP(_) => {
                                    Some(Some(Instruction::InitStackP(idx)))
                                }
                                Instruction::LoadMixed(m) => Some(Some(
                                    Instruction::LoadMixedStack(Box::new(LoadMixedStack {
                                        name: m.name.clone(),
                                        idx,
                                        level,
                                        callee: m.callee,
                                    })),
                                )),
                                Instruction::LoadMixedLex(m) => Some(Some(
                                    Instruction::LoadMixedStackLex(Box::new(LoadMixedStack {
                                        name: m.name.clone(),
                                        idx,
                                        level,
                                        callee: m.callee,
                                    })),
                                )),
                                Instruction::ResolveMixed(r) => Some(Some(
                                    Instruction::ResolveMixedStack(Box::new(ResolveMixedStack {
                                        name: r.name.clone(),
                                        idx,
                                        typ: r.typ,
                                        level,
                                        strict: r.strict,
                                    })),
                                )),
                                _ => None,
                            }
                        };
                        match replaced {
                            Some(Some(ins)) => *ap = ins,
                            Some(None) => {}
                            None => {
                                let msg = if this {
                                    b"Unsupported instruction for 'this'".to_vec()
                                } else {
                                    let mut m = b"Unsupported instruction for binding: ".to_vec();
                                    m.extend_from_slice(ap.go_type_name().as_bytes());
                                    m
                                };
                                let off = self.source_offset(pc);
                                self.assert(false, off, msg)?;
                                unreachable!();
                            }
                        }
                    }
                }
            }
        }
        let nested = self.scopes[s].nested.clone();
        for n in nested {
            self.finalise_var_alloc(n, stack_idx + stack_offset)?;
        }
        Ok((stash_idx, stack_idx))
    }

    pub(crate) fn move_args_to_stash(&mut self, s: ScopeId) {
        let bindings = self.scopes[s].bindings.clone();
        for b in bindings {
            if !self.bindings[b].is_arg {
                break;
            }
            self.bindings[b].in_stash = true;
        }
        self.scopes[s].args_in_stash = true;
        self.scopes[s].need_stash = true;
    }

    pub(crate) fn trim_code(&mut self, delta: usize) {
        let code = &mut self.progs[self.p].code;
        code.drain(..delta);
    }

    pub(crate) fn scope_trim_code(&mut self, s: ScopeId, delta: usize) {
        self.trim_code(delta);
        if delta != 0 {
            for item in self.progs[self.p].src_map.iter_mut() {
                item.pc -= delta as isize;
            }
            self.adjust_base(s, -(delta as isize));
        }
    }

    fn adjust_base(&mut self, s: ScopeId, delta: isize) {
        let mut stack = vec![s];
        while let Some(c) = stack.pop() {
            self.scopes[c].base += delta;
            stack.extend(self.scopes[c].nested.iter().copied());
        }
    }

    pub(crate) fn make_names_map(&self, s: ScopeId) -> crate::vm::NamesMap {
        let scope = &self.scopes[s];
        if scope.bindings.is_empty() {
            return None;
        }
        let mut names = HashMap::with_capacity(scope.bindings.len());
        for (i, &b) in scope.bindings.iter().enumerate() {
            let b = &self.bindings[b];
            let mut idx = i as u32;
            if b.is_const {
                idx |= MASK_CONST;
                if b.is_strict {
                    idx |= MASK_STRICT;
                }
            }
            if b.is_var {
                idx |= MASK_VAR;
            }
            names.insert(b.name.clone(), idx);
        }
        Some(Arc::new(names))
    }

    pub(crate) fn is_dynamic(&self, s: ScopeId) -> bool {
        self.scopes[s].dyn_lookup || self.scopes[s].dynamic
    }

    pub(crate) fn is_function(&self, s: ScopeId) -> bool {
        self.scopes[s].func_type != FuncType::None && !self.scopes[s].eval
    }

    pub(crate) fn delete_binding(&mut self, s: ScopeId, b: BindingId) {
        let Some(idx) = self.scopes[s].bindings.iter().position(|&bb| bb == b) else {
            return;
        };
        let name = self.bindings[b].name.clone();
        self.scopes[s].bound_names.remove(&name);
        self.scopes[s].bindings.remove(idx);
    }

    pub(crate) fn compile(
        &mut self,
        in_: &ast::Program,
        mut strict: bool,
        in_global: bool,
        eval_vm: Option<Rc<dyn EvalContext>>,
    ) -> CResult<()> {
        let eval = eval_vm.is_some();
        self.ctx_vm = eval_vm;
        self.src = in_.file.clone();
        self.new_scope();
        let mut scope = self.cur_scope();
        self.scopes[scope].dynamic = true;
        self.scopes[scope].eval = eval;
        if !strict && !in_.body.is_empty() {
            strict = self.is_strict(&in_.body).is_some();
        }
        self.scopes[scope].strict = strict;
        let own_var_scope = eval && strict;
        let own_lex_scope = !in_global || eval;
        if own_var_scope {
            self.new_block_scope();
            scope = self.cur_scope();
            self.scopes[scope].variable = true;
        }
        if eval
            && !in_global
            && let Some(ft) = self.ctx_vm.as_ref().and_then(|v| v.func_type())
        {
            self.scopes[scope].func_type = ft;
        }
        let funcs = self.extract_functions(&in_.body);
        self.create_function_bindings(&funcs)?;
        let num_funcs = self.scopes[scope].bindings.len();
        if in_global && !own_var_scope {
            if num_funcs == funcs.len() {
                self.compile_functions_global_all_unique(&funcs)?;
            } else {
                self.compile_functions_global(&funcs)?;
            }
        }
        self.compile_decl_list(&in_.declaration_list, false)?;
        let num_vars = self.scopes[scope].bindings.len() - num_funcs;
        let vars: Vec<unistring::String> = self.scopes[scope]
            .bindings
            .iter()
            .map(|&b| self.bindings[b].name.clone())
            .collect();
        if !vars.is_empty() && !own_var_scope && own_lex_scope {
            if in_global {
                self.emit(Instruction::BindGlobal(Box::new(crate::vm::BindGlobal {
                    vars: vars[num_funcs..].to_vec(),
                    funcs: vars[..num_funcs].to_vec(),
                    lets: Vec::new(),
                    consts: Vec::new(),
                    deletable: eval,
                })));
            } else {
                self.emit(Instruction::BindVars(Box::new(crate::vm::BindVars {
                    names: vars.clone(),
                    deletable: eval,
                })));
            }
        }
        let mut enter = None;
        if self.compile_lexical_declarations(&in_.body, own_var_scope || !own_lex_scope)?
            && own_lex_scope
        {
            let mut b = Block::new(BlockType::Scope, self.block);
            b.need_result = true;
            self.push_block(b);
            enter = Some(self.code_len());
            self.emit(Instruction::EnterBlock(Box::default()));
        }
        if !self.scopes[scope].bindings.is_empty() && !own_lex_scope {
            let (mut lets, mut consts) = (Vec::new(), Vec::new());
            let cur = self.cur_scope();
            for &b in &self.scopes[cur].bindings[num_funcs + num_vars..] {
                if self.bindings[b].is_const {
                    consts.push(self.bindings[b].name.clone());
                } else {
                    lets.push(self.bindings[b].name.clone());
                }
            }
            self.emit(Instruction::BindGlobal(Box::new(crate::vm::BindGlobal {
                vars: vars[num_funcs..].to_vec(),
                funcs: vars[..num_funcs].to_vec(),
                lets,
                consts,
                deletable: false,
            })));
        }
        if !in_global || own_var_scope {
            self.compile_functions(&funcs)?;
        }
        self.compile_statements(&in_.body, true)?;
        if let Some(enter) = enter {
            self.leave_scope_block(enter)?;
            self.pop_scope();
        }
        self.finalise_var_alloc(scope, 0)?;
        self.string_cache.clear();
        Ok(())
    }

    pub(crate) fn compile_decl_list(
        &mut self,
        v: &[ast::VariableDeclaration],
        in_func: bool,
    ) -> CResult<()> {
        for value in v {
            self.create_var_bindings(value, in_func)?;
        }
        Ok(())
    }

    pub(crate) fn extract_labelled<'a>(&self, st: &'a ast::Statement) -> &'a ast::Statement {
        let mut st = st;
        while let ast::Statement::LabelledStatement(l) = st {
            st = &l.statement;
        }
        st
    }

    pub(crate) fn extract_functions<'a>(
        &self,
        list: &'a [ast::Statement],
    ) -> Vec<&'a ast::FunctionDeclaration> {
        let mut funcs = Vec::new();
        for st in list {
            if let ast::Statement::FunctionDeclaration(decl) = self.extract_labelled(st) {
                funcs.push(&**decl);
            }
        }
        funcs
    }

    pub(crate) fn create_function_bindings(
        &mut self,
        funcs: &[&ast::FunctionDeclaration],
    ) -> CResult<()> {
        let s = self.cur_scope();
        if self.scopes[s].outer.is_some() {
            let unique = !self.is_function(s) && !self.scopes[s].variable && self.scopes[s].strict;
            if !unique {
                let mut has_non_standard = false;
                for decl in funcs {
                    let f = &decl.function;
                    let name = f.name.as_ref().expect("function name");
                    if !f.async_ && !f.generator {
                        self.bind_name_lexical(s, &name.name, false, name.idx1() as isize - 1)?;
                    } else {
                        has_non_standard = true;
                    }
                }
                if has_non_standard {
                    for decl in funcs {
                        let f = &decl.function;
                        let name = f.name.as_ref().expect("function name");
                        if f.async_ || f.generator {
                            self.bind_name_lexical(s, &name.name, true, name.idx1() as isize - 1)?;
                        }
                    }
                }
            } else {
                for decl in funcs {
                    let name = decl.function.name.as_ref().expect("function name");
                    self.bind_name_lexical(s, &name.name, true, name.idx1() as isize - 1)?;
                }
            }
        } else {
            for decl in funcs {
                let name = decl.function.name.as_ref().expect("function name");
                self.bind_name(s, &name.name)?;
            }
        }
        Ok(())
    }

    pub(crate) fn compile_functions(&mut self, list: &[&ast::FunctionDeclaration]) -> CResult<()> {
        for decl in list {
            self.compile_function(decl)?;
        }
        Ok(())
    }

    fn compile_functions_global_all_unique(
        &mut self,
        list: &[&ast::FunctionDeclaration],
    ) -> CResult<()> {
        for decl in list {
            let e = self.compile_function_literal(&decl.function, false)?;
            e.emit_getter(self, true)?;
        }
        Ok(())
    }

    fn compile_functions_global(&mut self, list: &[&ast::FunctionDeclaration]) -> CResult<()> {
        let mut m: HashMap<unistring::String, usize> = HashMap::with_capacity(list.len());
        for i in (0..list.len()).rev() {
            let name = &list[i].function.name.as_ref().expect("function name").name;
            m.entry(name.clone()).or_insert(i);
        }
        let mut idx = 0;
        for (i, decl) in list.iter().enumerate() {
            let name = decl
                .function
                .name
                .as_ref()
                .expect("function name")
                .name
                .clone();
            if m[&name] == i {
                let e = self.compile_function_literal(&decl.function, false)?;
                e.emit_getter(self, true)?;
                let s = self.cur_scope();
                let b = self.scopes[s].bound_names[&name];
                self.scopes[s].bindings[idx] = b;
                idx += 1;
            } else {
                let saved = self.enter_dummy_mode();
                let r = self
                    .compile_function_literal(&decl.function, false)
                    .and_then(|e| e.emit_getter(self, false));
                self.leave_dummy_mode(saved);
                r?;
            }
        }
        Ok(())
    }

    fn create_var_id_binding(
        &mut self,
        name: &unistring::String,
        offset: isize,
        in_func: bool,
    ) -> CResult<()> {
        if self.strict() {
            self.check_identifier_l_name(name, offset)?;
            self.check_identifier_name(name, offset)?;
        }
        if !in_func || *name != "arguments" {
            let s = self.cur_scope();
            self.bind_name(s, name)?;
        }
        Ok(())
    }

    pub(crate) fn create_bindings(
        &mut self,
        target: TargetRef<'_>,
        create_id_binding: &mut dyn FnMut(&mut Compiler, &unistring::String, isize) -> CResult<()>,
    ) -> CResult<()> {
        match target {
            TargetRef::Identifier(id) => create_id_binding(self, &id.name, id.idx as isize - 1),
            TargetRef::ObjectPattern(p) => {
                for prop in &p.properties {
                    match prop {
                        ast::Property::PropertyShort(ps) => {
                            create_id_binding(self, &ps.name.name, ps.name.idx as isize - 1)?
                        }
                        ast::Property::PropertyKeyed(pk) => {
                            self.create_bindings(TargetRef::of_expr(&pk.value), create_id_binding)?
                        }
                        ast::Property::SpreadElement(_) => {
                            return self.throw_syntax_error(
                                p.left_brace as isize - 1,
                                format!(
                                    "unsupported property type in ObjectPattern: {}",
                                    property_go_type(prop)
                                ),
                            );
                        }
                    }
                }
                if let Some(rest) = &p.rest {
                    self.create_bindings(TargetRef::of_expr(rest), create_id_binding)?;
                }
                Ok(())
            }
            TargetRef::ArrayPattern(p) => {
                for elt in p.elements.iter().flatten() {
                    self.create_bindings(TargetRef::of_expr(elt), create_id_binding)?;
                }
                if let Some(rest) = &p.rest {
                    self.create_bindings(TargetRef::of_expr(rest), create_id_binding)?;
                }
                Ok(())
            }
            TargetRef::Assign(a) => {
                self.create_bindings(TargetRef::of_expr(&a.left), create_id_binding)
            }
            TargetRef::Other(typ, idx0) => self.throw_syntax_error(
                idx0 as isize - 1,
                format!("unsupported binding target: {typ}"),
            ),
        }
    }

    pub(crate) fn create_var_binding(
        &mut self,
        target: TargetRef<'_>,
        in_func: bool,
    ) -> CResult<()> {
        self.create_bindings(target, &mut |c, name, offset| {
            c.create_var_id_binding(name, offset, in_func)
        })
    }

    pub(crate) fn create_var_bindings(
        &mut self,
        v: &ast::VariableDeclaration,
        in_func: bool,
    ) -> CResult<()> {
        for item in &v.list {
            self.create_var_binding(TargetRef::of_target(&item.target), in_func)?;
        }
        Ok(())
    }

    pub(crate) fn create_lexical_id_binding(
        &mut self,
        name: &unistring::String,
        is_const: bool,
        offset: isize,
    ) -> CResult<BindingId> {
        if *name == "let" {
            return self.throw_syntax_error(offset, "let is disallowed as a lexically bound name");
        }
        if self.strict() {
            self.check_identifier_l_name(name, offset)?;
            self.check_identifier_name(name, offset)?;
        }
        let s = self.cur_scope();
        let (b, _) = self.bind_name_lexical(s, name, true, offset)?;
        if is_const {
            self.bindings[b].is_const = true;
            self.bindings[b].is_strict = true;
        }
        Ok(b)
    }

    fn create_lexical_id_binding_func_body(
        &mut self,
        name: &unistring::String,
        is_const: bool,
        offset: isize,
        callee_binding: Option<BindingId>,
    ) -> CResult<BindingId> {
        if *name == "let" {
            return self.throw_syntax_error(offset, "let is disallowed as a lexically bound name");
        }
        if self.strict() {
            self.check_identifier_l_name(name, offset)?;
            self.check_identifier_name(name, offset)?;
        }
        let s = self.cur_scope();
        let param_scope = self.scopes[s].outer.expect("parameter scope");
        if let Some(&parent_binding) = self.scopes[param_scope].bound_names.get(name)
            && Some(parent_binding) != callee_binding
            && (*name != "arguments" || !self.scopes[param_scope].args_needed)
        {
            let mut m = b"Identifier '".to_vec();
            m.extend_from_slice(&go_bytes(name));
            m.extend_from_slice(b"' has already been declared");
            return self.throw_syntax_error(offset, m);
        }
        let (b, _) = self.bind_name_lexical(s, name, true, offset)?;
        if is_const {
            self.bindings[b].is_const = true;
            self.bindings[b].is_strict = true;
        }
        Ok(b)
    }

    pub(crate) fn create_lexical_binding(
        &mut self,
        target: TargetRef<'_>,
        is_const: bool,
    ) -> CResult<()> {
        self.create_bindings(target, &mut |c, name, offset| {
            c.create_lexical_id_binding(name, is_const, offset)
                .map(|_| ())
        })
    }

    pub(crate) fn create_lexical_bindings(&mut self, lex: &ast::LexicalDeclaration) -> CResult<()> {
        for d in &lex.list {
            self.create_lexical_binding(
                TargetRef::of_target(&d.target),
                lex.token == token::CONST,
            )?;
        }
        Ok(())
    }

    pub(crate) fn compile_lexical_declarations(
        &mut self,
        list: &[ast::Statement],
        mut scope_declared: bool,
    ) -> CResult<bool> {
        for st in list {
            match st {
                ast::Statement::LexicalDeclaration(lex) => {
                    if !scope_declared {
                        self.new_block_scope();
                        scope_declared = true;
                    }
                    self.create_lexical_bindings(lex)?;
                }
                ast::Statement::ClassDeclaration(cls) => {
                    if !scope_declared {
                        self.new_block_scope();
                        scope_declared = true;
                    }
                    let name = cls.class.name.as_ref().expect("class name");
                    self.create_lexical_id_binding(&name.name, false, name.idx as isize - 1)?;
                }
                _ => {}
            }
        }
        Ok(scope_declared)
    }

    pub(crate) fn compile_lexical_declarations_func_body(
        &mut self,
        list: &[ast::Statement],
        callee_binding: Option<BindingId>,
    ) -> CResult<()> {
        for st in list {
            match st {
                ast::Statement::LexicalDeclaration(lex) => {
                    let is_const = lex.token == token::CONST;
                    for d in &lex.list {
                        self.create_bindings(
                            TargetRef::of_target(&d.target),
                            &mut |c, name, offset| {
                                c.create_lexical_id_binding_func_body(
                                    name,
                                    is_const,
                                    offset,
                                    callee_binding,
                                )
                                .map(|_| ())
                            },
                        )?;
                    }
                }
                ast::Statement::ClassDeclaration(cls) => {
                    let name = cls.class.name.as_ref().expect("class name");
                    self.create_lexical_id_binding_func_body(
                        &name.name,
                        false,
                        name.idx as isize - 1,
                        callee_binding,
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn compile_function(&mut self, v: &ast::FunctionDeclaration) -> CResult<()> {
        let fname = v.function.name.as_ref().expect("function name");
        let s = self.cur_scope();
        let b = self.scopes[s].bound_names.get(&fname.name).copied();
        if b.is_none() || self.bindings[b.unwrap_or_default()].is_var {
            let e = CompiledExpr {
                offset: v.function.idx0() as isize - 1,
                kind: ExprKind::Identifier(fname.name.clone()),
            };
            let val = self.compile_function_literal(&v.function, false)?;
            e.emit_setter(self, &val, false)?;
        } else {
            let e = self.compile_function_literal(&v.function, false)?;
            e.emit_getter(self, true)?;
            self.b_emit_init_p(b.unwrap_or_default());
        }
        Ok(())
    }

    pub(crate) fn compile_standalone_function_decl(
        &mut self,
        v: &ast::FunctionDeclaration,
    ) -> CResult<()> {
        let off = v.function.idx0() as isize - 1;
        if v.function.async_ {
            return self.throw_syntax_error(
                off,
                "Async functions can only be declared at top level or inside a block.",
            );
        }
        if v.function.generator {
            return self.throw_syntax_error(
                off,
                "Generators can only be declared at top level or inside a block.",
            );
        }
        if self.strict() {
            return self.throw_syntax_error(off, "In strict mode code, functions can only be declared at top level or inside a block.");
        }
        self.throw_syntax_error(off, "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.")
    }

    pub(crate) fn is_strict<'a>(
        &self,
        list: &'a [ast::Statement],
    ) -> Option<&'a ast::StringLiteral> {
        for st in list {
            if let ast::Statement::ExpressionStatement(st) = st {
                if let ast::Expression::StringLiteral(e) = &st.expression {
                    if e.literal == b"\"use strict\"" || e.literal == b"'use strict'" {
                        return Some(e);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        None
    }

    pub(crate) fn is_strict_statement<'a>(
        &self,
        s: &'a ast::BlockStatement,
    ) -> Option<&'a ast::StringLiteral> {
        self.is_strict(&s.list)
    }

    pub(crate) fn check_identifier_name(
        &self,
        name: &unistring::String,
        offset: isize,
    ) -> CResult<()> {
        if let Some(b) = name.ascii_bytes()
            && matches!(
                b,
                b"implements"
                    | b"interface"
                    | b"let"
                    | b"package"
                    | b"private"
                    | b"protected"
                    | b"public"
                    | b"static"
                    | b"yield"
            )
        {
            return self.throw_syntax_error(offset, "Unexpected strict mode reserved word");
        }
        Ok(())
    }

    pub(crate) fn check_identifier_l_name(
        &self,
        name: &unistring::String,
        offset: isize,
    ) -> CResult<()> {
        if *name == "eval" || *name == "arguments" {
            return self.throw_syntax_error(
                offset,
                "Assignment to eval or arguments is not allowed in strict mode",
            );
        }
        Ok(())
    }

    pub(crate) fn enter_dummy_mode(&mut self) -> (Option<BlockId>, ProgId) {
        let saved_block = self.block;
        let saved_program = self.p;
        if let Some(sb) = saved_block {
            let b = &self.blocks[sb];
            let nb = Block {
                typ: b.typ,
                label: b.label.clone(),
                cont: 0,
                breaks: Vec::new(),
                conts: Vec::new(),
                outer: b.outer,
                breaking: b.breaking,
                need_result: false,
            };
            self.push_block(nb);
        }
        self.p = self.new_prog(unistring::String::default(), Vec::new(), Vec::new());
        self.new_scope();
        (saved_block, saved_program)
    }

    pub(crate) fn leave_dummy_mode(&mut self, saved: (Option<BlockId>, ProgId)) {
        self.block = saved.0;
        self.p = saved.1;
        self.pop_scope();
    }

    pub(crate) fn compile_statement_dummy(&mut self, statement: &ast::Statement) -> CResult<()> {
        let saved = self.enter_dummy_mode();
        let r = self.compile_statement(statement, false);
        self.leave_dummy_mode(saved);
        r
    }

    pub(crate) fn declare_private_id(
        &mut self,
        cs: ClassScopeId,
        name: &unistring::String,
        kind: ast::PropertyKind,
        is_static: bool,
        offset: isize,
    ) -> CResult<()> {
        if let Some(pn) = self.class_scopes[cs].private_names.get_mut(name) {
            if pn.is_static == is_static {
                match kind {
                    ast::PropertyKind::Get => {
                        if pn.has_setter && !pn.has_getter {
                            pn.has_getter = true;
                            return Ok(());
                        }
                    }
                    ast::PropertyKind::Set => {
                        if pn.has_getter && !pn.has_setter {
                            pn.has_setter = true;
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
            let mut m = b"Identifier '#".to_vec();
            m.extend_from_slice(&go_bytes(name));
            m.extend_from_slice(b"' has already been declared");
            return self.throw_syntax_error(offset, m);
        }
        let mut pn = PrivateName {
            is_static,
            has_getter: kind == ast::PropertyKind::Get,
            has_setter: kind == ast::PropertyKind::Set,
            ..PrivateName::default()
        };
        let scope = &mut self.class_scopes[cs];
        let env = if is_static {
            &mut scope.static_env
        } else {
            &mut scope.instance_env
        };
        if kind != ast::PropertyKind::Value {
            pn.idx = env.create_private_method_id(name);
            pn.is_method = true;
        } else {
            pn.idx = env.create_private_field_id(name);
        }
        scope.private_names.insert(name.clone(), pn);
        Ok(())
    }

    pub(crate) fn get_declared_private_id(&self, name: &unistring::String) -> CResult<PrivateName> {
        let cs = self.class_scope.expect("class scope");
        if let Some(n) = self.class_scopes[cs].private_names.get(name) {
            return Ok(*n);
        }
        self.assert(false, 0, "getDeclaredPrivateId() for undeclared id")?;
        unreachable!()
    }

    pub(crate) fn resolve_private_name(
        &self,
        name: &unistring::String,
        offset: isize,
    ) -> CResult<(Option<ResolvedPrivateName>, Option<PrivateId>)> {
        let mut level = 0usize;
        let mut s = self.class_scope;
        while let Some(cs) = s {
            let scope = &self.class_scopes[cs];
            if !scope.private_names.is_empty() {
                if let Some(pn) = scope.private_names.get(name) {
                    return Ok((
                        Some(ResolvedPrivateName {
                            name: name.clone(),
                            idx: pn.idx as u32,
                            level: level as u8,
                            is_static: pn.is_static,
                            is_method: pn.is_method,
                        }),
                        None,
                    ));
                }
                level += 1;
            }
            s = scope.outer;
        }
        if let Some(ctx) = &self.ctx_vm
            && let Some(id) = ctx.private_id(name)
        {
            return Ok((None, Some(id)));
        }
        let mut m = b"Private field '#".to_vec();
        m.extend_from_slice(&go_bytes(name));
        m.extend_from_slice(b"' must be declared in an enclosing class");
        self.throw_syntax_error(offset, m)
    }

    pub(crate) fn eval_const(
        &mut self,
        expr: &CompiledExpr<'_>,
    ) -> CResult<Result<Value, ConstException>> {
        if let ExprKind::Literal(v) = &expr.kind {
            return Ok(Ok(v.clone()));
        }
        let mut created = false;
        let saved_p = self.p;
        if !self.eval_active {
            let id = match self.eval_prg {
                Some(id) => {
                    self.progs[id].code.clear();
                    self.progs[id].src_map.clear();
                    id
                }
                None => {
                    let id = self.new_prog(unistring::String::default(), Vec::new(), Vec::new());
                    self.eval_prg = Some(id);
                    id
                }
            };
            self.eval_active = true;
            self.p = id;
            created = true;
        }
        let saved_pc = self.code_len();
        let emitted = expr.emit_getter(self, true);
        let res = match emitted {
            Ok(()) => eval_const_code(&self.progs[self.p].code[saved_pc..]),
            Err(e) => {
                if created {
                    self.eval_active = false;
                    self.p = saved_p;
                }
                return Err(e);
            }
        };
        if created {
            self.eval_active = false;
            self.p = saved_p;
        } else {
            self.progs[self.p].code.truncate(saved_pc);
        }
        match res {
            Ok(v) => Ok(Ok(v)),
            Err(ConstThrow::Exception(e)) => Ok(Err(e)),
            Err(ConstThrow::Unsupported(what)) => {
                self.assert(false, 0, format!("unsupported constant expression: {what}"))?;
                unreachable!()
            }
        }
    }

    pub(crate) fn emit_throw(&mut self, v: &ConstException) -> CResult<()> {
        if v.name == "TypeError" || v.name == "RangeError" {
            self.emit(Instruction::LoadDynamic(unistring::String::new(&v.name)));
            match &v.message {
                Some(msg) => {
                    self.emit_literal_value(msg.clone());
                    self.emit(Instruction::New(1));
                }
                None => self.emit(Instruction::New(0)),
            }
            self.emit(Instruction::Throw);
            return Ok(());
        }
        let mut m =
            b"unknown exception type thrown while evaluating constant expression: ".to_vec();
        m.extend_from_slice(v.name.as_bytes());
        if let Some(msg) = &v.message {
            m.extend_from_slice(b": ");
            m.extend_from_slice(&crate::vm::value_go_string(msg));
        }
        self.assert(false, 0, m)
    }

    pub(crate) fn link(&mut self) -> Arc<Program> {
        let mut index: HashMap<usize, ProgId> = HashMap::with_capacity(self.progs.len());
        for (i, p) in self.progs.iter().enumerate() {
            index.insert(Arc::as_ptr(&p.handle) as usize, i);
        }
        let mut done: Vec<Option<Arc<Program>>> = vec![None; self.progs.len()];
        for id in (0..self.progs.len()).rev() {
            let mut code = std::mem::take(&mut self.progs[id].code);
            for ins in code.iter_mut() {
                relink(ins, &index, &done);
            }
            done[id] = Some(Arc::new(Program {
                code,
                func_name: self.progs[id].func_name.clone(),
                src: self.src.clone(),
                src_map: std::mem::take(&mut self.progs[id].src_map),
            }));
        }
        done[0].take().expect("main program")
    }
}

fn relink_prg(p: &mut Arc<Program>, index: &HashMap<usize, ProgId>, done: &[Option<Arc<Program>>]) {
    if let Some(&id) = index.get(&(Arc::as_ptr(p) as usize))
        && let Some(real) = &done[id]
    {
        *p = real.clone();
    }
}

fn relink(ins: &mut Instruction, index: &HashMap<usize, ProgId>, done: &[Option<Arc<Program>>]) {
    match ins {
        Instruction::NewFunc(f)
        | Instruction::NewAsyncFunc(f)
        | Instruction::NewGeneratorFunc(f)
        | Instruction::NewArrowFunc(f)
        | Instruction::NewAsyncArrowFunc(f) => relink_prg(&mut f.prg, index, done),
        Instruction::NewMethod(m)
        | Instruction::NewAsyncMethod(m)
        | Instruction::NewGeneratorMethod(m) => relink_prg(&mut m.new_func.prg, index, done),
        Instruction::NewClass(c) | Instruction::NewDerivedClass(c) => {
            if let Some(p) = &mut c.ctor {
                relink_prg(p, index, done);
            }
            if let Some(p) = &mut c.init_fields {
                relink_prg(p, index, done);
            }
        }
        Instruction::NewStaticFieldInit(s) => {
            if let Some(p) = &mut s.init_fields {
                relink_prg(p, index, done);
            }
        }
        _ => {}
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TargetRef<'a> {
    Identifier(&'a ast::Identifier),
    ObjectPattern(&'a ast::ObjectPattern),
    ArrayPattern(&'a ast::ArrayPattern),
    Assign(&'a ast::AssignExpression),
    Other(&'static str, i64),
}

impl<'a> TargetRef<'a> {
    pub(crate) fn of_expr(e: &'a ast::Expression) -> TargetRef<'a> {
        match e {
            ast::Expression::Identifier(id) => TargetRef::Identifier(id),
            ast::Expression::ObjectPattern(p) => TargetRef::ObjectPattern(p),
            ast::Expression::ArrayPattern(p) => TargetRef::ArrayPattern(p),
            ast::Expression::AssignExpression(a) => TargetRef::Assign(a),
            _ => TargetRef::Other(expression_go_type(e), e.idx0()),
        }
    }

    pub(crate) fn of_target(t: &'a ast::BindingTarget) -> TargetRef<'a> {
        match t {
            ast::BindingTarget::Identifier(id) => TargetRef::Identifier(id),
            ast::BindingTarget::ObjectPattern(p) => TargetRef::ObjectPattern(p),
            ast::BindingTarget::ArrayPattern(p) => TargetRef::ArrayPattern(p),
            ast::BindingTarget::BadExpression(b) => TargetRef::Other("*ast.BadExpression", b.from),
        }
    }
}

pub(crate) fn expression_go_type(e: &ast::Expression) -> &'static str {
    use ast::Expression as E;
    match e {
        E::ArrayLiteral(_) => "*ast.ArrayLiteral",
        E::ArrayPattern(_) => "*ast.ArrayPattern",
        E::ArrowFunctionLiteral(_) => "*ast.ArrowFunctionLiteral",
        E::AssignExpression(_) => "*ast.AssignExpression",
        E::AwaitExpression(_) => "*ast.AwaitExpression",
        E::BadExpression(_) => "*ast.BadExpression",
        E::BinaryExpression(_) => "*ast.BinaryExpression",
        E::BooleanLiteral(_) => "*ast.BooleanLiteral",
        E::BracketExpression(_) => "*ast.BracketExpression",
        E::CallExpression(_) => "*ast.CallExpression",
        E::ClassLiteral(_) => "*ast.ClassLiteral",
        E::ConditionalExpression(_) => "*ast.ConditionalExpression",
        E::DotExpression(_) => "*ast.DotExpression",
        E::FunctionLiteral(_) => "*ast.FunctionLiteral",
        E::Identifier(_) => "*ast.Identifier",
        E::MetaProperty(_) => "*ast.MetaProperty",
        E::NewExpression(_) => "*ast.NewExpression",
        E::NullLiteral(_) => "*ast.NullLiteral",
        E::NumberLiteral(_) => "*ast.NumberLiteral",
        E::ObjectLiteral(_) => "*ast.ObjectLiteral",
        E::ObjectPattern(_) => "*ast.ObjectPattern",
        E::Optional(_) => "*ast.Optional",
        E::OptionalChain(_) => "*ast.OptionalChain",
        E::PrivateDotExpression(_) => "*ast.PrivateDotExpression",
        E::PrivateIdentifier(_) => "*ast.PrivateIdentifier",
        E::RegExpLiteral(_) => "*ast.RegExpLiteral",
        E::SequenceExpression(_) => "*ast.SequenceExpression",
        E::SpreadElement(_) => "*ast.SpreadElement",
        E::StringLiteral(_) => "*ast.StringLiteral",
        E::SuperExpression(_) => "*ast.SuperExpression",
        E::TemplateLiteral(_) => "*ast.TemplateLiteral",
        E::ThisExpression(_) => "*ast.ThisExpression",
        E::UnaryExpression(_) => "*ast.UnaryExpression",
        E::YieldExpression(_) => "*ast.YieldExpression",
    }
}

pub(crate) fn property_go_type(p: &ast::Property) -> &'static str {
    match p {
        ast::Property::PropertyShort(_) => "*ast.PropertyShort",
        ast::Property::PropertyKeyed(_) => "*ast.PropertyKeyed",
        ast::Property::SpreadElement(_) => "*ast.SpreadElement",
    }
}
