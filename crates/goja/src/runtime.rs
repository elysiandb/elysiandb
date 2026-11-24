use crate::ast;
use crate::builtin_symbol::{SYM_ITERATOR, SYM_SPECIES};
use crate::compiler::{Compiler, CompilerError, CompilerSyntaxError, EvalContext, Program};
use crate::file;
use crate::func::{
    BaseJsFuncObject, FuncKind, NATIVE_FUNC_CLASS, NativeCtor, NativeFn, NativeFuncObject,
};
use crate::heap::{FxHashMap, Heap, ObjRef, StashRef, SymRef, SymbolData, Tracer};
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_FUNCTION, CLASS_OBJECT, CallFn, CtorFn, FunctionCall,
    GUARDED_OBJECT_CLASS, ObjExt, PRIMITIVE_VALUE_OBJECT_CLASS, PropertyDescriptor, to_method,
};
use crate::parser::{self, ParserOption};
use crate::string::{
    JsString, StringBuilder, ascii_string, new_string_value, string_value_from_raw,
};
use crate::unistring;
use crate::value::{
    MAX_INT, Value, float_to_int, go_float_to_int, int_to_value, nil_safe, value_prop,
};
use crate::vm::{Context, Stash, Vm};
use std::any::Any;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationKind {
    Key,
    Value,
    KeyValue,
}

pub type JsResult<T> = Result<T, Throw>;

#[derive(Debug)]
pub enum Throw {
    Value(Value),
    Exception(Box<Exception>),
    TypeError(String),
    RangeError(String),
    ReferenceError(String),
    ReferenceErrorName(unistring::String),
    SyntaxError(String),
    Interrupted(Box<InterruptedError>),
    StackOverflow(Box<StackOverflowError>),
}

impl Throw {
    pub fn is_uncatchable(&self) -> bool {
        matches!(self, Throw::Interrupted(_) | Throw::StackOverflow(_))
    }

    pub fn trace(&self, t: &mut Tracer) {
        match self {
            Throw::Value(v) => t.value(v),
            Throw::Exception(e) => t.value(&e.val),
            Throw::Interrupted(e) => t.value(&e.exception.val),
            Throw::StackOverflow(e) => t.value(&e.exception.val),
            _ => {}
        }
    }
}

pub fn type_error<T>(msg: impl Into<String>) -> JsResult<T> {
    Err(Throw::TypeError(msg.into()))
}

pub fn range_error<T>(msg: impl Into<String>) -> JsResult<T> {
    Err(Throw::RangeError(msg.into()))
}

pub fn reference_error<T>(msg: impl Into<String>) -> JsResult<T> {
    Err(Throw::ReferenceError(msg.into()))
}

pub fn syntax_error<T>(msg: impl Into<String>) -> JsResult<T> {
    Err(Throw::SyntaxError(msg.into()))
}

#[derive(Clone, Debug)]
pub struct StackFrame {
    pub prg: Option<Arc<Program>>,
    pub func_name: unistring::String,
    pub pc: isize,
}

impl StackFrame {
    pub fn src_name(&self) -> String {
        match &self.prg {
            None => "<native>".to_string(),
            Some(p) => match &p.src {
                Some(src) => src.name().to_string(),
                None => String::new(),
            },
        }
    }

    pub fn func_name(&self) -> String {
        if self.func_name.is_empty() && self.prg.is_none() {
            return "<native>".to_string();
        }
        if self.func_name.is_empty() {
            return "<anonymous>".to_string();
        }
        self.func_name.to_string()
    }

    pub fn position(&self) -> file::Position {
        match &self.prg {
            None => file::Position::default(),
            Some(p) => match &p.src {
                Some(src) => src.position(p.source_offset(self.pc) as i64),
                None => file::Position::default(),
            },
        }
    }

    pub fn write_to_value_builder(&self, b: &mut StringBuilder) {
        if let Some(prg) = &self.prg {
            let n = &prg.func_name;
            if !n.is_empty() {
                b.write_string(&string_value_from_raw(n));
                b.write_ascii(b" (");
            }
            let p = self.position();
            if !p.filename.is_empty() {
                b.write_utf8_string(p.filename.as_bytes());
            } else {
                b.write_ascii(b"<eval>");
            }
            b.write_rune(':' as i32);
            b.write_ascii(p.line.to_string().as_bytes());
            b.write_rune(':' as i32);
            b.write_ascii(p.column.to_string().as_bytes());
            b.write_rune('(' as i32);
            b.write_ascii(self.pc.to_string().as_bytes());
            b.write_rune(')' as i32);
            if !n.is_empty() {
                b.write_rune(')' as i32);
            }
        } else {
            if !self.func_name.is_empty() {
                b.write_string(&string_value_from_raw(&self.func_name));
                b.write_ascii(b" (");
            }
            b.write_ascii(b"native");
            if !self.func_name.is_empty() {
                b.write_rune(')' as i32);
            }
        }
    }

    pub fn write(&self, b: &mut String) {
        if let Some(prg) = &self.prg {
            let n = &prg.func_name;
            if !n.is_empty() {
                b.push_str(&n.to_string());
                b.push_str(" (");
            }
            let p = self.position();
            if !p.filename.is_empty() {
                b.push_str(&p.filename);
            } else {
                b.push_str("<eval>");
            }
            b.push(':');
            b.push_str(&p.line.to_string());
            b.push(':');
            b.push_str(&p.column.to_string());
            b.push('(');
            b.push_str(&self.pc.to_string());
            b.push(')');
            if !n.is_empty() {
                b.push(')');
            }
        } else {
            if !self.func_name.is_empty() {
                b.push_str(&self.func_name.to_string());
                b.push_str(" (");
            }
            b.push_str("native");
            if !self.func_name.is_empty() {
                b.push(')');
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Exception {
    pub val: Value,
    pub stack: Vec<StackFrame>,
    pub val_string: Option<String>,
}

impl Exception {
    pub fn new(val: Value) -> Exception {
        Exception {
            val,
            stack: Vec::new(),
            val_string: None,
        }
    }

    pub fn value(&self) -> &Value {
        &self.val
    }

    pub fn stack(&self) -> &[StackFrame] {
        &self.stack
    }

    pub fn write_full_stack(&self, b: &mut String) {
        for frame in &self.stack {
            b.push_str("\tat ");
            frame.write(b);
            b.push('\n');
        }
    }

    pub fn write_short_stack(&self, b: &mut String) {
        if let Some(f) = self.stack.first()
            && (f.prg.is_some() || !f.func_name.is_empty())
        {
            b.push_str(" at ");
            f.write(b);
        }
    }

    pub fn resolve_value_string(&mut self, rt: &mut Runtime) {
        if self.val_string.is_none() {
            let s = self.val.go_string(rt).unwrap_or_default();
            self.val_string = Some(s);
        }
    }

    pub fn string(&self) -> String {
        let mut b = String::new();
        if let Some(s) = &self.val_string {
            b.push_str(s);
            b.push('\n');
        }
        self.write_full_stack(&mut b);
        b
    }

    pub fn error(&self) -> String {
        let mut b = String::new();
        if let Some(s) = &self.val_string {
            b.push_str(s);
        }
        self.write_short_stack(&mut b);
        b
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error())
    }
}

impl std::error::Error for Exception {}

pub type InterruptValue = Arc<dyn Any + Send + Sync>;

#[derive(Clone)]
pub struct InterruptedError {
    pub exception: Exception,
    pub iface: Option<InterruptValue>,
    pub text: String,
}

impl fmt::Debug for InterruptedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InterruptedError({})", self.text)
    }
}

impl InterruptedError {
    pub fn value(&self) -> Option<&InterruptValue> {
        self.iface.as_ref()
    }

    pub fn string(&self) -> String {
        let mut b = String::new();
        if self.iface.is_some() {
            b.push_str(&self.text);
            b.push('\n');
        }
        self.exception.write_full_stack(&mut b);
        b
    }

    pub fn error(&self) -> String {
        if self.iface.is_none() {
            return "<nil>".to_string();
        }
        let mut b = self.text.clone();
        self.exception.write_short_stack(&mut b);
        b
    }
}

impl fmt::Display for InterruptedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error())
    }
}

impl std::error::Error for InterruptedError {}

#[derive(Clone, Debug)]
pub struct StackOverflowError {
    pub exception: Exception,
}

#[derive(Debug)]
pub enum RunError {
    Exception(Box<Exception>),
    Interrupted(Box<InterruptedError>),
    StackOverflow(Box<StackOverflowError>),
}

impl RunError {
    pub fn error(&self) -> String {
        match self {
            RunError::Exception(e) => e.error(),
            RunError::Interrupted(e) => e.error(),
            RunError::StackOverflow(e) => e.exception.error(),
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error())
    }
}

impl std::error::Error for RunError {}

pub struct InterruptState {
    flag: AtomicBool,
    val: Mutex<Option<(InterruptValue, String)>>,
}

impl InterruptState {
    fn new() -> InterruptState {
        InterruptState {
            flag: AtomicBool::new(false),
            val: Mutex::new(None),
        }
    }

    #[inline]
    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub fn interrupt<V: fmt::Display + Send + Sync + 'static>(&self, v: V) {
        let text = v.to_string();
        let mut g = match self.val.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *g = Some((Arc::new(v), text));
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    pub fn take_value(&self) -> Option<(InterruptValue, String)> {
        let g = match self.val.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        g.clone()
    }
}

#[derive(Clone)]
pub struct InterruptHandle(Arc<InterruptState>);

impl InterruptHandle {
    pub fn interrupt<V: fmt::Display + Send + Sync + 'static>(&self, v: V) {
        self.0.interrupt(v);
    }

    pub fn clear(&self) {
        self.0.clear();
    }
}

pub trait Job {
    fn run(self: Box<Self>, rt: &mut Runtime) -> JsResult<()>;
    fn trace(&self, t: &mut Tracer);
}

#[derive(Default)]
pub struct Global {
    pub stash: StashRef,

    pub object: Option<ObjRef>,
    pub array: Option<ObjRef>,
    pub function: Option<ObjRef>,
    pub string: Option<ObjRef>,
    pub number: Option<ObjRef>,
    pub big_int: Option<ObjRef>,
    pub boolean: Option<ObjRef>,
    pub reg_exp: Option<ObjRef>,
    pub date: Option<ObjRef>,
    pub symbol: Option<ObjRef>,
    pub proxy: Option<ObjRef>,
    pub reflect: Option<ObjRef>,
    pub promise: Option<ObjRef>,
    pub math: Option<ObjRef>,
    pub json: Option<ObjRef>,

    pub async_function: Option<ObjRef>,

    pub array_buffer: Option<ObjRef>,
    pub data_view: Option<ObjRef>,
    pub typed_array: Option<ObjRef>,
    pub uint8_array: Option<ObjRef>,
    pub uint8_clamped_array: Option<ObjRef>,
    pub int8_array: Option<ObjRef>,
    pub uint16_array: Option<ObjRef>,
    pub int16_array: Option<ObjRef>,
    pub uint32_array: Option<ObjRef>,
    pub int32_array: Option<ObjRef>,
    pub float32_array: Option<ObjRef>,
    pub float64_array: Option<ObjRef>,
    pub big_int64_array: Option<ObjRef>,
    pub big_uint64_array: Option<ObjRef>,

    pub weak_set: Option<ObjRef>,
    pub weak_map: Option<ObjRef>,
    pub map: Option<ObjRef>,
    pub set: Option<ObjRef>,

    pub error: Option<ObjRef>,
    pub aggregate_error: Option<ObjRef>,
    pub type_error: Option<ObjRef>,
    pub reference_error: Option<ObjRef>,
    pub syntax_error: Option<ObjRef>,
    pub range_error: Option<ObjRef>,
    pub eval_error: Option<ObjRef>,
    pub uri_error: Option<ObjRef>,

    pub object_prototype: Option<ObjRef>,
    pub array_prototype: Option<ObjRef>,
    pub number_prototype: Option<ObjRef>,
    pub big_int_prototype: Option<ObjRef>,
    pub string_prototype: Option<ObjRef>,
    pub boolean_prototype: Option<ObjRef>,
    pub function_prototype: Option<ObjRef>,
    pub reg_exp_prototype: Option<ObjRef>,
    pub date_prototype: Option<ObjRef>,
    pub symbol_prototype: Option<ObjRef>,

    pub array_buffer_prototype: Option<ObjRef>,
    pub data_view_prototype: Option<ObjRef>,
    pub typed_array_prototype: Option<ObjRef>,
    pub weak_set_prototype: Option<ObjRef>,
    pub weak_map_prototype: Option<ObjRef>,
    pub map_prototype: Option<ObjRef>,
    pub set_prototype: Option<ObjRef>,
    pub promise_prototype: Option<ObjRef>,

    pub generator_function_prototype: Option<ObjRef>,
    pub generator_function: Option<ObjRef>,
    pub generator_prototype: Option<ObjRef>,

    pub async_function_prototype: Option<ObjRef>,

    pub iterator_prototype: Option<ObjRef>,
    pub array_iterator_prototype: Option<ObjRef>,
    pub map_iterator_prototype: Option<ObjRef>,
    pub set_iterator_prototype: Option<ObjRef>,
    pub string_iterator_prototype: Option<ObjRef>,
    pub reg_exp_string_iterator_prototype: Option<ObjRef>,

    pub error_prototype: Option<ObjRef>,

    pub eval: Option<ObjRef>,

    pub thrower: Option<ObjRef>,

    pub std_regexp_proto: Option<ObjRef>,

    pub weak_set_adder: Option<ObjRef>,
    pub weak_map_adder: Option<ObjRef>,
    pub map_adder: Option<ObjRef>,
    pub set_adder: Option<ObjRef>,
    pub array_values: Option<ObjRef>,
    pub array_to_string: Option<ObjRef>,

    pub stringproto_trim_end: Option<ObjRef>,
    pub stringproto_trim_start: Option<ObjRef>,

    pub parse_float: Option<ObjRef>,
    pub parse_int: Option<ObjRef>,

    pub typed_array_values: Option<ObjRef>,
}

impl Global {
    pub fn new(stash: StashRef) -> Global {
        Global {
            stash,
            ..Global::default()
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.stash(self.stash);
        for o in [
            self.object,
            self.array,
            self.function,
            self.string,
            self.number,
            self.big_int,
            self.boolean,
            self.reg_exp,
            self.date,
            self.symbol,
            self.proxy,
            self.reflect,
            self.promise,
            self.math,
            self.json,
            self.async_function,
            self.array_buffer,
            self.data_view,
            self.typed_array,
            self.uint8_array,
            self.uint8_clamped_array,
            self.int8_array,
            self.uint16_array,
            self.int16_array,
            self.uint32_array,
            self.int32_array,
            self.float32_array,
            self.float64_array,
            self.big_int64_array,
            self.big_uint64_array,
            self.weak_set,
            self.weak_map,
            self.map,
            self.set,
            self.error,
            self.aggregate_error,
            self.type_error,
            self.reference_error,
            self.syntax_error,
            self.range_error,
            self.eval_error,
            self.uri_error,
            self.object_prototype,
            self.array_prototype,
            self.number_prototype,
            self.big_int_prototype,
            self.string_prototype,
            self.boolean_prototype,
            self.function_prototype,
            self.reg_exp_prototype,
            self.date_prototype,
            self.symbol_prototype,
            self.array_buffer_prototype,
            self.data_view_prototype,
            self.typed_array_prototype,
            self.weak_set_prototype,
            self.weak_map_prototype,
            self.map_prototype,
            self.set_prototype,
            self.promise_prototype,
            self.generator_function_prototype,
            self.generator_function,
            self.generator_prototype,
            self.async_function_prototype,
            self.iterator_prototype,
            self.array_iterator_prototype,
            self.map_iterator_prototype,
            self.set_iterator_prototype,
            self.string_iterator_prototype,
            self.reg_exp_string_iterator_prototype,
            self.error_prototype,
            self.eval,
            self.thrower,
            self.std_regexp_proto,
            self.weak_set_adder,
            self.weak_map_adder,
            self.map_adder,
            self.set_adder,
            self.array_values,
            self.array_to_string,
            self.stringproto_trim_end,
            self.stringproto_trim_start,
            self.parse_float,
            self.parse_int,
            self.typed_array_values,
        ] {
            t.opt_obj(o);
        }
    }
}

pub type RandSource = Box<dyn FnMut() -> f64>;
pub type Now = Box<dyn FnMut() -> gocompat::time::Time>;

pub struct Runtime {
    pub global: Global,
    pub global_object: ObjRef,
    pub string_singleton: Option<ObjRef>,
    pub rand: Option<RandSource>,
    pub now: Option<Now>,
    pub parser_options: Vec<ParserOption>,
    pub symbol_registry: FxHashMap<unistring::String, SymRef>,
    pub heap: Heap,
    pub vm: Vm,
    pub job_queue: std::collections::VecDeque<Box<dyn Job>>,
    pub id_seq: u64,
    pub handles: Vec<ObjRef>,
    pub in_native: bool,
    pub extra_roots: Vec<Value>,
    pub pending_uncatchable: Option<Throw>,
    pub _collator: Option<crate::deps::xtext::collate::Collator>,
    pub interrupt_state: Arc<InterruptState>,
    pub host: crate::host::HostState,
}

impl Default for Runtime {
    fn default() -> Runtime {
        Runtime::new()
    }
}

impl Runtime {
    pub fn new() -> Runtime {
        let mut heap = Heap::new();
        let global_stash = heap.alloc_stash(Stash::default());
        let interrupt_state = Arc::new(InterruptState::new());
        let mut r = Runtime {
            global: Global::new(global_stash),
            global_object: ObjRef(0),
            string_singleton: None,
            rand: None,
            now: None,
            parser_options: Vec::new(),
            symbol_registry: FxHashMap::default(),
            heap,
            vm: Vm::new(global_stash, interrupt_state.clone()),
            job_queue: std::collections::VecDeque::new(),
            id_seq: 0,
            handles: Vec::new(),
            in_native: false,
            extra_roots: Vec::new(),
            pending_uncatchable: None,
            _collator: None,
            interrupt_state,
            host: crate::host::HostState::default(),
        };
        r.init();
        r
    }

    fn init(&mut self) {
        let op = self.reserve_object();
        self.global.object_prototype = Some(op);
        self.new_templated_object(crate::builtin_object::get_object_proto_template(), Some(op));
        let go = self.reserve_object();
        self.global_object = go;
        self.new_templated_object(
            crate::builtin_global::get_global_object_template(),
            Some(go),
        );
        self.vm.init();
    }

    pub fn trace_roots(&self, t: &mut Tracer) {
        self.global.trace(t);
        t.obj(self.global_object);
        t.opt_obj(self.string_singleton);
        for s in self.symbol_registry.values() {
            t.sym(*s);
        }
        self.vm.trace(t);
        for j in &self.job_queue {
            j.trace(t);
        }
        for h in &self.handles {
            t.obj(*h);
        }
        t.values(&self.extra_roots);
        self.host.trace(t);
    }

    pub fn collator(&mut self) -> &mut crate::deps::xtext::collate::Collator {
        self._collator.get_or_insert_with(|| {
            crate::deps::xtext::collate::Collator::new(crate::deps::xtext::language::UND, &[])
        })
    }

    pub fn interrupt_handle(&self) -> InterruptHandle {
        InterruptHandle(self.interrupt_state.clone())
    }

    pub fn interrupt<V: fmt::Display + Send + Sync + 'static>(&self, v: V) {
        self.interrupt_state.interrupt(v);
    }

    pub fn clear_interrupt(&self) {
        self.interrupt_state.clear();
    }

    pub fn symbol_desc(&self, s: SymRef) -> Option<JsString> {
        if let Some(desc) = crate::builtin_symbol::well_known_symbol_desc(s) {
            return Some(desc);
        }
        self.heap.symbol(s).and_then(|d| d.desc.clone())
    }

    pub fn new_symbol(&mut self, desc: Option<JsString>) -> SymRef {
        self.heap.alloc_symbol(SymbolData { desc })
    }

    pub fn symbol_descriptive_string(&self, s: SymRef) -> JsString {
        let desc = self
            .symbol_desc(s)
            .unwrap_or_else(|| crate::string::STRING_EMPTY.clone());
        ascii_string("Symbol(")
            .concat(&desc)
            .concat(&ascii_string(")"))
    }

    pub fn type_error_result(&mut self, throw: bool, msg: &str) -> JsResult<()> {
        if throw {
            return Err(Throw::Value(self.new_type_error(msg)));
        }
        Ok(())
    }

    pub fn new_error(&mut self, typ: ObjRef, msg: &str) -> Value {
        let m = Value::String(new_string_value(msg.as_bytes()));
        match self.builtin_new(typ, &[m]) {
            Ok(o) => Value::Object(o),
            Err(Throw::Value(v)) => v,
            Err(_) => Value::Undefined,
        }
    }

    pub fn throw_reference_error<T>(&mut self, name: &unistring::String) -> JsResult<T> {
        Err(Throw::Value(self.new_reference_error(name)))
    }

    pub fn new_reference_error(&mut self, name: &unistring::String) -> Value {
        let e = self.get_reference_error();
        self.new_error(e, &format!("{name} is not defined"))
    }

    pub fn new_syntax_error(&mut self, msg: &str, _offset: isize) -> Value {
        let e = self.get_syntax_error();
        self.new_error(e, msg)
    }

    pub fn new_type_error(&mut self, msg: &str) -> Value {
        let e = self.get_type_error();
        self.new_error(e, msg)
    }

    pub fn new_range_error(&mut self, msg: &str) -> Value {
        let e = self.get_range_error();
        self.new_error(e, msg)
    }

    pub fn new_base_object(&mut self, proto: Option<ObjRef>, class: &'static str) -> ObjRef {
        self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::None,
        )
    }

    pub fn new_base_object_obj(&mut self, obj: ObjRef, proto: Option<ObjRef>, class: &'static str) {
        self.set_object_impl(
            obj,
            &BASE_OBJECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::None,
        );
    }

    pub fn new_guarded_object(&mut self, proto: Option<ObjRef>, class: &'static str) -> ObjRef {
        self.alloc_object(
            &GUARDED_OBJECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::Guarded(Default::default()),
        )
    }

    pub fn new_object(&mut self) -> ObjRef {
        let proto = self.global.object_prototype;
        self.new_base_object(proto, CLASS_OBJECT)
    }

    pub fn create_object(&mut self, proto: Option<ObjRef>) -> ObjRef {
        self.new_base_object(proto, CLASS_OBJECT)
    }

    pub fn new_primitive_object(
        &mut self,
        value: Value,
        proto: Option<ObjRef>,
        class: &'static str,
    ) -> ObjRef {
        self.alloc_object(
            &PRIMITIVE_VALUE_OBJECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::Primitive(value),
        )
    }

    pub fn builtin_number(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Some(t) = call.arguments.first() {
            match t {
                Value::Object(o) => {
                    let prim_value = o.to_primitive_number(self)?;
                    if let Value::BigInt(bigint) = &prim_value {
                        return Ok(int_to_value(bigint.int64()));
                    }
                    prim_value.to_number(self)
                }
                Value::BigInt(t) => Ok(int_to_value(t.int64())),
                t => t.to_number(self),
            }
        } else {
            Ok(Value::Int(0))
        }
    }

    pub fn builtin_new_number(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let v = if let Some(t) = args.first() {
            match t {
                Value::Object(o) => {
                    let prim_value = o.to_primitive_number(self)?;
                    if let Value::BigInt(bigint) = &prim_value {
                        int_to_value(bigint.int64())
                    } else {
                        prim_value.to_number(self)?
                    }
                }
                Value::BigInt(t) => int_to_value(t.int64()),
                t => t.to_number(self)?,
            }
        } else {
            int_to_value(0)
        };
        Ok(self.new_primitive_object(v, proto, crate::object::CLASS_NUMBER))
    }

    pub fn new_func(&mut self, name: &unistring::String, length: isize, strict: bool) -> ObjRef {
        let f = self.init_base_js_function(&crate::func::FUNC_CLASS, FuncKind::Func, strict);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn new_async_func(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_base_js_function(&crate::func::ASYNC_FUNC_CLASS, FuncKind::Async, strict);
        let p = self.get_async_function_prototype();
        self.obj_mut(f).base.prototype = Some(p);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn new_generator_func(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_base_js_function(
            &crate::func::GENERATOR_FUNC_CLASS,
            FuncKind::Generator,
            strict,
        );
        let p = self.get_generator_function_prototype();
        self.obj_mut(f).base.prototype = Some(p);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        let gp = self.get_generator_prototype();
        let proto = self.new_base_object(Some(gp), CLASS_OBJECT);
        BaseObject::_put_prop(
            self,
            f,
            &crate::vm::STR_PROTOTYPE,
            Value::Object(proto),
            true,
            false,
            false,
        );
        f
    }

    pub fn new_class_func(
        &mut self,
        name: &unistring::String,
        length: isize,
        proto: Option<ObjRef>,
        derived: bool,
    ) -> ObjRef {
        let mut d = BaseJsFuncObject::new(FuncKind::Class, true);
        d.derived = derived;
        let f = self.alloc_object(
            &crate::func::CLASS_FUNC_CLASS,
            BaseObject::new(CLASS_FUNCTION, proto),
            ObjExt::JsFunc(Box::new(d)),
        );
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn init_base_js_function(
        &mut self,
        class: &'static crate::object::ObjClass,
        kind: FuncKind,
        strict: bool,
    ) -> ObjRef {
        let proto = self.get_function_prototype();
        self.alloc_object(
            class,
            BaseObject::new(CLASS_FUNCTION, Some(proto)),
            ObjExt::JsFunc(Box::new(BaseJsFuncObject::new(kind, strict))),
        )
    }

    pub fn new_method(&mut self, name: &unistring::String, length: isize, strict: bool) -> ObjRef {
        let f =
            self.init_base_js_function(&crate::func::METHOD_FUNC_CLASS, FuncKind::Method, strict);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn new_generator_method(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_base_js_function(
            &crate::func::GENERATOR_METHOD_FUNC_CLASS,
            FuncKind::GeneratorMethod,
            strict,
        );
        let p = self.get_generator_function_prototype();
        self.obj_mut(f).base.prototype = Some(p);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        let gp = self.get_generator_prototype();
        let proto = self.new_base_object(Some(gp), CLASS_OBJECT);
        BaseObject::_put_prop(
            self,
            f,
            &crate::vm::STR_PROTOTYPE,
            Value::Object(proto),
            true,
            false,
            false,
        );
        f
    }

    pub fn new_async_method(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_base_js_function(
            &crate::func::ASYNC_METHOD_FUNC_CLASS,
            FuncKind::AsyncMethod,
            strict,
        );
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn init_arrow_func(
        &mut self,
        class: &'static crate::object::ObjClass,
        kind: FuncKind,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_base_js_function(class, kind, strict);
        let nt = self.vm.new_target.clone();
        BaseJsFuncObject::data_mut(self, f).new_target = nt;
        f
    }

    pub fn new_arrow_func(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_arrow_func(&crate::func::ARROW_FUNC_CLASS, FuncKind::Arrow, strict);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn new_async_arrow_func(
        &mut self,
        name: &unistring::String,
        length: isize,
        strict: bool,
    ) -> ObjRef {
        let f = self.init_arrow_func(
            &crate::func::ASYNC_ARROW_FUNC_CLASS,
            FuncKind::AsyncArrow,
            strict,
        );
        self.obj_mut(f).base.class = CLASS_OBJECT;
        let p = self.get_async_function_prototype();
        self.obj_mut(f).base.prototype = Some(p);
        crate::func::base_func_init(self, f, name, int_to_value(length as i64));
        f
    }

    pub fn new_native_func(&mut self, call: NativeFn, name: &str, length: i64) -> ObjRef {
        let proto = self.get_function_prototype();
        let v = self.alloc_object(
            &NATIVE_FUNC_CLASS,
            BaseObject::new(CLASS_FUNCTION, Some(proto)),
            ObjExt::NativeFunc(Box::new(NativeFuncObject {
                f: Some(call),
                construct: None,
                wrapped: None,
            })),
        );
        crate::func::base_func_init(self, v, &name.into(), int_to_value(length));
        v
    }

    pub fn new_native_func_and_construct(
        &mut self,
        v: Option<ObjRef>,
        call: Option<NativeFn>,
        ctor: Option<NativeCtor>,
        default_proto: Option<ObjRef>,
        name: &str,
        l: Value,
    ) -> ObjRef {
        let v = match v {
            Some(v) => v,
            None => self.reserve_object(),
        };
        let proto = self.get_function_prototype();
        self.set_object_impl(
            v,
            &NATIVE_FUNC_CLASS,
            BaseObject::new(CLASS_FUNCTION, Some(proto)),
            ObjExt::NativeFunc(Box::new(NativeFuncObject {
                f: call,
                construct: ctor,
                wrapped: None,
            })),
        );
        crate::func::base_func_init(self, v, &name.into(), l);
        if let Some(dp) = default_proto {
            v._put_prop(
                self,
                &"prototype".into(),
                Value::Object(dp),
                false,
                false,
                false,
            );
        }
        v
    }

    pub fn new_native_construct_only(
        &mut self,
        v: Option<ObjRef>,
        ctor: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        default_proto: Option<ObjRef>,
        name: &str,
        length: i64,
    ) -> ObjRef {
        let v = match v {
            Some(v) => v,
            None => self.reserve_object(),
        };
        self.new_native_func_and_construct(
            Some(v),
            Some(NativeFn::closure_with(
                vec![],
                move |r: &mut Runtime, _d: &[Value], call: FunctionCall<'_>| {
                    Ok(Value::Object(ctor(r, call.arguments, None)?))
                },
            )),
            Some(NativeCtor::closure_with(
                vec![Value::Object(v)],
                move |r: &mut Runtime, d: &[Value], args: &[Value], nt: Option<ObjRef>| {
                    let nt = nt.or(d[0].as_object());
                    ctor(r, args, nt)
                },
            )),
            default_proto,
            name,
            int_to_value(length),
        )
    }

    pub fn new_native_func_construct_obj(
        &mut self,
        v: ObjRef,
        construct: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        name: &str,
        proto: Option<ObjRef>,
        length: i64,
    ) -> ObjRef {
        let fproto = self.get_function_prototype();
        let f = self.construct_to_call(construct, proto);
        let c = self.wrap_native_construct(Some(construct), v, proto);
        self.set_object_impl(
            v,
            &NATIVE_FUNC_CLASS,
            BaseObject::new(CLASS_FUNCTION, Some(fproto)),
            ObjExt::NativeFunc(Box::new(NativeFuncObject {
                f: Some(f),
                construct: c,
                wrapped: None,
            })),
        );
        crate::func::base_func_init(self, v, &name.into(), int_to_value(length));
        if let Some(p) = proto {
            v._put_prop(
                self,
                &"prototype".into(),
                Value::Object(p),
                false,
                false,
                false,
            );
        }
        v
    }

    pub fn new_native_func_construct(
        &mut self,
        v: ObjRef,
        construct: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        name: &str,
        prototype: Option<ObjRef>,
        length: i64,
    ) -> ObjRef {
        let fp = self.get_function_prototype();
        self.new_native_func_construct_proto(v, construct, name, prototype, Some(fp), length)
    }

    pub fn new_native_func_construct_proto(
        &mut self,
        v: ObjRef,
        construct: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        name: &str,
        prototype: Option<ObjRef>,
        proto: Option<ObjRef>,
        length: i64,
    ) -> ObjRef {
        let f = self.construct_to_call(construct, prototype);
        let c = self.wrap_native_construct(Some(construct), v, prototype);
        self.set_object_impl(
            v,
            &NATIVE_FUNC_CLASS,
            BaseObject::new(CLASS_FUNCTION, proto),
            ObjExt::NativeFunc(Box::new(NativeFuncObject {
                f: Some(f),
                construct: c,
                wrapped: None,
            })),
        );
        crate::func::base_func_init(self, v, &name.into(), int_to_value(length));
        if let Some(p) = prototype {
            v._put_prop(
                self,
                &"prototype".into(),
                Value::Object(p),
                false,
                false,
                false,
            );
        }
        v
    }

    pub fn construct_to_call(
        &mut self,
        construct: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        proto: Option<ObjRef>,
    ) -> NativeFn {
        let data = match proto {
            Some(p) => vec![Value::Object(p)],
            None => vec![Value::Undefined],
        };
        NativeFn::closure_with(
            data,
            move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                Ok(Value::Object(construct(
                    r,
                    call.arguments,
                    d[0].as_object(),
                )?))
            },
        )
    }

    pub fn wrap_native_construct(
        &mut self,
        c: Option<crate::func::NativeCtorPtr>,
        ctor_obj: ObjRef,
        def_proto: Option<ObjRef>,
    ) -> Option<NativeCtor> {
        let c = c?;
        let dp = match def_proto {
            Some(p) => Value::Object(p),
            None => Value::Undefined,
        };
        Some(NativeCtor::closure_with(
            vec![Value::Object(ctor_obj), dp],
            move |r: &mut Runtime, d: &[Value], args: &[Value], new_target: Option<ObjRef>| {
                let ctor_obj = d[0].as_object();
                let def_proto = d[1].as_object();
                let proto = match new_target {
                    Some(nt) => r.get_prototype_from_ctor(nt, ctor_obj, def_proto)?,
                    None => def_proto,
                };
                c(r, args, proto)
            },
        ))
    }

    pub fn builtin_new(&mut self, construct: ObjRef, args: &[Value]) -> JsResult<ObjRef> {
        let c = self.to_constructor(&Value::Object(construct))?;
        c.construct(self, args, Some(construct))
    }

    pub fn builtin_thrower(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = self.to_object(call.this)?;
        let mut strict = true;
        if let ObjExt::JsFunc(f) = &self.obj(obj).ext
            && f.kind == crate::func::FuncKind::Func
        {
            strict = f.strict;
        }
        self.type_error_result(
            strict,
            "'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them",
        )?;
        Ok(Value::Undefined)
    }

    pub fn to_callable(&mut self, v: &Value) -> JsResult<CallFn> {
        let o = self.to_object(v)?;
        if let Some(call) = o.assert_callable(self) {
            return Ok(call);
        }
        let s = v.to_string(self)?.to_rust_string();
        Err(Throw::Value(
            self.new_type_error(&format!("Value is not callable: {s}")),
        ))
    }

    pub fn to_constructor(&mut self, v: &Value) -> JsResult<CtorFn> {
        let o = self.to_object(v)?;
        if let Some(c) = o.assert_constructor(self) {
            return Ok(c);
        }
        Err(Throw::Value(
            self.new_type_error("Value is not a constructor"),
        ))
    }

    pub fn check_object_coercible(&mut self, v: &Value) -> JsResult<()> {
        if matches!(v, Value::Undefined | Value::Null) {
            return self.type_error_result(true, "Value is not object coercible");
        }
        Ok(())
    }

    pub fn to_object(&mut self, v: &Value) -> JsResult<ObjRef> {
        if let Value::Object(o) = v {
            return Ok(*o);
        }
        let s = if v.is_empty() {
            "undefined".to_string()
        } else {
            v.go_string(self)?
        };
        Err(Throw::Value(
            self.new_type_error(&format!("Value is not an object: {s}")),
        ))
    }

    pub fn to_object_msg(&mut self, v: &Value, msg: &str) -> JsResult<ObjRef> {
        if let Value::Object(o) = v {
            return Ok(*o);
        }
        Err(Throw::Value(self.new_type_error(msg)))
    }

    pub fn species_constructor(
        &mut self,
        o: ObjRef,
        default_constructor: ObjRef,
    ) -> JsResult<CtorFn> {
        let mut c = o.get_str(self, &"constructor".into(), None)?;
        if let Some(cv) = &c
            && !cv.is_undefined()
        {
            let co = self.to_object(cv)?;
            c = co.get_sym(self, SYM_SPECIES, None)?;
        }
        let c = match c {
            None | Some(Value::Undefined) | Some(Value::Null) => Value::Object(default_constructor),
            Some(c) => c,
        };
        self.to_constructor(&c)
    }

    pub fn species_constructor_obj(
        &mut self,
        o: ObjRef,
        default_constructor: ObjRef,
    ) -> JsResult<ObjRef> {
        let mut c = o.get_str(self, &"constructor".into(), None)?;
        if let Some(cv) = &c
            && !cv.is_undefined()
        {
            let co = self.to_object(cv)?;
            c = co.get_sym(self, SYM_SPECIES, None)?;
        }
        match c {
            None | Some(Value::Undefined) | Some(Value::Null) => Ok(default_constructor),
            Some(c) => {
                let obj = self.to_object(&c)?;
                if obj.assert_constructor(self).is_none() {
                    return Err(Throw::Value(
                        self.new_type_error("Value is not a constructor"),
                    ));
                }
                Ok(obj)
            }
        }
    }

    pub fn return_this(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(call.this.clone())
    }

    pub fn get_v_str(&mut self, v: &Value, p: &unistring::String) -> JsResult<Option<Value>> {
        let o = v.to_object(self)?;
        o.get_str(self, p, Some(v))
    }

    pub fn get_v(&mut self, v: &Value, p: &Value) -> JsResult<Option<Value>> {
        let o = v.to_object(self)?;
        o.get(self, p, Some(v))
    }

    pub fn get_iterator(
        &mut self,
        obj: &Value,
        method: Option<CallFn>,
    ) -> JsResult<IteratorRecord> {
        let method = match method {
            Some(m) => m,
            None => {
                let m = self.get_v(obj, &Value::Symbol(SYM_ITERATOR))?;
                match to_method(self, m)? {
                    Some(m) => m,
                    None => {
                        return Err(Throw::Value(self.new_type_error("object is not iterable")));
                    }
                }
            }
        };
        let r = method.call(self, obj.clone(), &[])?;
        let iter = self.to_object(&r)?;
        let mut next = None;
        if let Some(Value::Object(o)) = iter.get_str(self, &crate::vm::STR_NEXT, None)?
            && let Some(call) = o.assert_callable(self)
        {
            next = Some(call);
        }
        Ok(IteratorRecord {
            iterator: Some(iter),
            next,
        })
    }

    pub fn create_iter_result_object(&mut self, value: Value, done: bool) -> Value {
        let o = self.new_object();
        let _ = o.set_own_str(self, &crate::vm::STR_VALUE, value, false);
        let _ = o.set_own_str(self, &crate::vm::STR_DONE, Value::Bool(done), false);
        Value::Object(o)
    }

    pub fn leave(&mut self) -> JsResult<()> {
        while let Some(job) = self.job_queue.pop_front() {
            job.run(self)?;
        }
        self.job_queue = std::collections::VecDeque::new();
        self.vm.stack = Vec::new();
        Ok(())
    }

    pub fn leave_abrupt(&mut self) {
        self.job_queue = std::collections::VecDeque::new();
        self.clear_interrupt();
    }

    pub fn gen_id(&mut self) -> u64 {
        if self.id_seq == 0 {
            self.id_seq = 1;
        }
        let ret = self.id_seq;
        self.id_seq += 1;
        ret
    }

    pub fn set_global(&mut self, name: &unistring::String, v: Value, strict: bool) -> JsResult<()> {
        let gs = self.global.stash;
        if let Some(mut r) = crate::vm::stash_get_ref_by_name(self, gs, name, strict)? {
            return r.set(self, v);
        }
        let o = self.global_object;
        if strict {
            if o.has_own_property_str(self, name)? {
                o.set_own_str(self, name, v, true)?;
            } else {
                return self.throw_reference_error(name);
            }
        } else {
            o.set_own_str(self, name, v, false)?;
        }
        Ok(())
    }

    pub fn invoke(&mut self, v: &Value, p: &unistring::String, args: &[Value]) -> JsResult<Value> {
        let o = v.to_object(self)?;
        let m = nil_safe(o.get_str(self, p, None)?);
        let c = self.to_callable(&m)?;
        c.call(self, v.clone(), args)
    }

    pub fn iterable_to_list(
        &mut self,
        iterable: &Value,
        method: Option<CallFn>,
    ) -> JsResult<Vec<Value>> {
        let mut iter = self.get_iterator(iterable, method)?;
        let mut values = Vec::new();
        iter.iterate(self, &mut |rt, item| {
            rt.root(&item);
            crate::array::go_append(&mut values, item);
            Ok(())
        })?;
        Ok(values)
    }

    pub fn put_species_return_this(&mut self, o: ObjRef) {
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.return_this(c)),
            "get [Symbol.species]",
            0,
        );
        o._put_sym(
            self,
            SYM_SPECIES,
            Value::Property(Box::new(crate::value::ValueProperty {
                getter_func: Some(f),
                accessor: true,
                configurable: true,
                ..Default::default()
            })),
        );
    }

    pub fn instance_of(&mut self, left: &Value, right: ObjRef) -> JsResult<bool> {
        crate::object::instance_of_operator(self, left, right)
    }

    pub fn method_prop(
        &mut self,
        f: fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value>,
        name: &str,
        n_args: i64,
    ) -> Value {
        let o = self.new_native_func(NativeFn::Fn(f), name, n_args);
        value_prop(Value::Object(o), true, false, true)
    }

    pub fn get_prototype_from_ctor(
        &mut self,
        new_target: ObjRef,
        def_ctor: Option<ObjRef>,
        def_proto: Option<ObjRef>,
    ) -> JsResult<Option<ObjRef>> {
        if Some(new_target) == def_ctor {
            return Ok(def_proto);
        }
        let proto = new_target.get_str(self, &"prototype".into(), None)?;
        if let Some(Value::Object(o)) = proto {
            return Ok(Some(o));
        }
        Ok(def_proto)
    }

    pub fn to_length_uint32(&mut self, v: &Value) -> JsResult<u32> {
        let mut v = v.clone();
        let int_val: i64;
        loop {
            match &v {
                Value::Int(i) => {
                    int_val = *i;
                    break;
                }
                Value::Float(f) => {
                    if *f == 0.0 && f.is_sign_negative() {
                        int_val = 0;
                        break;
                    }
                    match float_to_int(*f) {
                        Some(i) => {
                            int_val = i;
                            break;
                        }
                        None => return Err(self.invalid_array_length()),
                    }
                }
                Value::String(s) => {
                    v = s.to_number();
                }
                _ => {
                    let n2 = to_uint32(self, &v)?;
                    let n1 = v.to_number(self)?;
                    if let Value::Float(f) = n1
                        && (f != 0.0 || !f.is_sign_negative())
                    {
                        return Err(self.invalid_array_length());
                    }
                    if n1.to_integer(self)? != i64::from(n2) {
                        return Err(self.invalid_array_length());
                    }
                    return Ok(n2);
                }
            }
        }
        if (0..=u32::MAX as i64).contains(&int_val) {
            return Ok(int_val as u32);
        }
        Err(self.invalid_array_length())
    }

    fn invalid_array_length(&mut self) -> Throw {
        let e = self.get_range_error();
        Throw::Value(self.new_error(e, "Invalid array length"))
    }

    pub fn to_index(&mut self, v: &Value) -> JsResult<i64> {
        let num = v.to_integer(self)?;
        if (0..MAX_INT).contains(&num) {
            return Ok(num);
        }
        let s = v.go_string(self)?;
        let e = self.get_range_error();
        Err(Throw::Value(
            self.new_error(e, &format!("Invalid index {s}")),
        ))
    }

    pub fn to_boolean(&self, b: bool) -> Value {
        Value::Bool(b)
    }

    pub fn compile_with_runtime(
        &mut self,
        name: &str,
        src: &[u8],
        strict: bool,
        in_global: bool,
        eval_vm: Option<Rc<dyn EvalContext>>,
    ) -> JsResult<Arc<Program>> {
        match compile_with(name, src, strict, in_global, eval_vm, &self.parser_options) {
            Ok(p) => Ok(p),
            Err(e) => {
                let msg = e.error();
                let se = self.get_syntax_error();
                let v = self.builtin_new(se, &[Value::String(new_string_value(&msg))])?;
                Err(Throw::Exception(Box::new(Exception::new(Value::Object(v)))))
            }
        }
    }

    pub fn run_string(&mut self, src: &str) -> Result<Value, RunError> {
        self.run_script("", src)
    }

    pub fn run_script(&mut self, name: &str, src: &str) -> Result<Value, RunError> {
        let p = match self.compile_with_runtime(name, src.as_bytes(), false, true, None) {
            Ok(p) => p,
            Err(t) => return Err(self.throw_to_run_error(t)),
        };
        self.run_program(&p)
    }

    pub fn throw_to_run_error(&mut self, t: Throw) -> RunError {
        match t {
            Throw::Interrupted(e) => RunError::Interrupted(e),
            Throw::StackOverflow(e) => RunError::StackOverflow(e),
            t => {
                let mut ex = self.vm_exception_from_throw(t);
                ex.resolve_value_string(self);
                RunError::Exception(Box::new(ex))
            }
        }
    }

    pub fn run_program(&mut self, p: &Arc<Program>) -> Result<Value, RunError> {
        let recursive = !self.vm.call_stack.is_empty();
        if recursive {
            if let Err(t) = self.vm_push_ctx_for_call() {
                return Err(self.throw_to_run_error(t));
            }
            self.vm.stash = Some(self.global.stash);
            self.vm.priv_env = None;
            self.vm.new_target = Value::Empty;
            self.vm.args = 0;
            let sp = self.vm.sp;
            self.vm.stack_expand(sp + 1);
            self.vm.stack[sp as usize] = Value::Undefined;
            self.vm.stack[sp as usize + 1] = Value::Empty;
            self.vm.sb = sp + 1;
            self.vm.sp = sp + 2;
        } else {
            self.vm.call_stack.push(Context::default());
        }
        self.vm.prg = Some(p.clone());
        self.vm.pc = 0;
        self.vm.result = Value::Undefined;
        let res = self.vm_run_try();
        let mut uncatchable = false;
        let mut out = match res {
            Ok(None) => Ok(self.vm.result.clone()),
            Ok(Some(mut ex)) => {
                ex.resolve_value_string(self);
                Err(RunError::Exception(ex))
            }
            Err(t) => {
                uncatchable = true;
                Err(self.throw_to_run_error(t))
            }
        };
        if !uncatchable {
            if recursive {
                self.vm.clear_stack();
            } else {
                self.vm.prg = None;
                self.vm.sb = -1;
                if let Err(t) = self.leave() {
                    uncatchable = t.is_uncatchable();
                    out = Err(self.throw_to_run_error(t));
                }
            }
        }
        if recursive {
            self.vm.sp -= 2;
            self.vm.pop_ctx();
        } else {
            let l = self.vm.call_stack.len();
            self.vm.call_stack.truncate(l.saturating_sub(1));
        }
        if uncatchable && self.vm.call_stack.is_empty() {
            self.leave_abrupt();
        }
        out
    }

    pub fn capture_call_stack(&mut self, depth: usize, stack: Vec<StackFrame>) -> Vec<StackFrame> {
        let l = self.vm.call_stack.len();
        let mut offset = 0;
        if depth > 0 {
            offset = (l + 1).saturating_sub(depth);
        }
        self.vm_materialise_frame_names(offset);
        self.vm.capture_stack(self, stack, offset)
    }

    pub fn global_object(&self) -> ObjRef {
        self.global_object
    }

    pub fn set(&mut self, name: &str, value: Value) -> JsResult<()> {
        let g = self.global_object;
        g.set_own_str(self, &name.into(), value, true).map(|_| ())
    }

    pub fn get(&mut self, name: &str) -> JsResult<Option<Value>> {
        let g = self.global_object;
        g.get_str(self, &name.into(), None)
    }

    pub fn go_struct_dump(&self, o: ObjRef) -> String {
        format!("&{{{}}}", self.obj(o).base.class)
    }

    pub fn go_type_name(&self, _o: ObjRef) -> String {
        "*goja.baseObject".to_string()
    }

    pub fn is_proxy(&self, o: ObjRef) -> bool {
        self.obj(o).base.class == "Proxy"
    }

    pub fn to_property_descriptor(&mut self, v: &Value) -> JsResult<PropertyDescriptor> {
        crate::builtin_object::to_property_descriptor(self, v)
    }
}

#[derive(Clone, Copy)]
pub struct IteratorRecord {
    pub iterator: Option<ObjRef>,
    pub next: Option<CallFn>,
}

impl IteratorRecord {
    pub fn trace(&self, t: &mut Tracer) {
        t.opt_obj(self.iterator);
        if let Some(n) = self.next {
            t.obj(n.obj);
        }
    }

    pub fn iterate(
        &mut self,
        r: &mut Runtime,
        step: &mut dyn FnMut(&mut Runtime, Value) -> JsResult<()>,
    ) -> JsResult<()> {
        loop {
            let Some(next) = self.next else {
                return Err(Throw::Value(
                    r.new_type_error("iterator.next is missing or not a function"),
                ));
            };
            let it = self.iterator.map(Value::Object).unwrap_or_default();
            let res = next.call(r, it, &[])?;
            let res = r.to_object(&res)?;
            if iterator_complete(r, res)? {
                break;
            }
            let value = iterator_value(r, res)?;
            if let Err(e) = step(r, value) {
                let _ = self.return_iter(r);
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn step(&mut self, r: &mut Runtime) -> JsResult<Option<Value>> {
        let next = match self.next {
            Some(n) => n,
            None => {
                return Err(Throw::Value(
                    r.new_type_error("iterator.next is missing or not a function"),
                ));
            }
        };
        let it = self.iterator.map(Value::Object).unwrap_or_default();
        let res = next.call(r, it, &[])?;
        let res = r.to_object(&res)?;
        let done = iterator_complete(r, res)?;
        if !done {
            return Ok(Some(iterator_value(r, res)?));
        }
        self.close();
        Ok(None)
    }

    pub fn return_iter(&mut self, r: &mut Runtime) -> JsResult<()> {
        let Some(iterator) = self.iterator else {
            return Ok(());
        };
        let m = iterator.get_str(r, &crate::vm::STR_RETURN, None)?;
        if let Some(ret_method) = to_method(r, m)? {
            let v = ret_method.call(r, Value::Object(iterator), &[])?;
            r.to_object(&v)?;
        }
        self.iterator = None;
        self.next = None;
        Ok(())
    }

    pub fn close(&mut self) {
        self.iterator = None;
        self.next = None;
    }
}

pub fn iterator_complete(r: &mut Runtime, iter_result: ObjRef) -> JsResult<bool> {
    Ok(nil_safe(iter_result.get_str(r, &crate::vm::STR_DONE, None)?).to_boolean())
}

pub fn iterator_value(r: &mut Runtime, iter_result: ObjRef) -> JsResult<Value> {
    Ok(nil_safe(iter_result.get_str(
        r,
        &crate::vm::STR_VALUE,
        None,
    )?))
}

pub fn compile(name: &str, src: &[u8], strict: bool) -> Result<Arc<Program>, CompilerSyntaxError> {
    compile_with(name, src, strict, true, None, &[])
}

pub fn compile_ast(prg: &ast::Program, strict: bool) -> Result<Arc<Program>, CompilerSyntaxError> {
    compile_ast_with(prg, strict, true, None)
}

pub fn must_compile(name: &str, src: &[u8], strict: bool) -> Arc<Program> {
    match compile(name, src, strict) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

pub fn parse(
    name: &str,
    src: &[u8],
    options: &[ParserOption],
) -> Result<ast::Program, CompilerSyntaxError> {
    let (prg, err) = parser::parse_file(None, name, src, 0, options);
    match err {
        Some(err1) => Err(CompilerSyntaxError(CompilerError {
            message: err1.error().into_bytes(),
            file: None,
            offset: 0,
        })),
        None => Ok(prg),
    }
}

pub fn compile_with(
    name: &str,
    src: &[u8],
    strict: bool,
    in_global: bool,
    eval_vm: Option<Rc<dyn EvalContext>>,
    parser_options: &[ParserOption],
) -> Result<Arc<Program>, CompilerSyntaxError> {
    let prg = parse(name, src, parser_options)?;
    compile_ast_with(&prg, strict, in_global, eval_vm)
}

pub fn compile_ast_with(
    prg: &ast::Program,
    strict: bool,
    in_global: bool,
    eval_vm: Option<Rc<dyn EvalContext>>,
) -> Result<Arc<Program>, CompilerSyntaxError> {
    let mut c = Compiler::new();
    c.compile(prg, strict, in_global, eval_vm)?;
    Ok(c.link())
}

pub fn to_int8(r: &mut Runtime, v: &Value) -> JsResult<i8> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as i8,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as i8,
        _ => 0,
    })
}

pub fn to_uint8(r: &mut Runtime, v: &Value) -> JsResult<u8> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as u8,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as u8,
        _ => 0,
    })
}

pub fn to_uint8_clamp(r: &mut Runtime, v: &Value) -> JsResult<u8> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => {
            if i < 0 {
                0
            } else if i <= 255 {
                i as u8
            } else {
                255
            }
        }
        Value::Float(num) if !num.is_nan() => {
            if num < 0.0 {
                return Ok(0);
            }
            if num > 255.0 {
                return Ok(255);
            }
            let f = num.floor();
            let f1 = f + 0.5;
            if f1 < num {
                return Ok((f + 1.0) as u8);
            }
            if f1 > num {
                return Ok(f as u8);
            }
            let r = f as u8;
            if r & 1 != 0 { r + 1 } else { r }
        }
        _ => 0,
    })
}

pub fn to_int16(r: &mut Runtime, v: &Value) -> JsResult<i16> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as i16,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as i16,
        _ => 0,
    })
}

pub fn to_uint16(r: &mut Runtime, v: &Value) -> JsResult<u16> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as u16,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as u16,
        _ => 0,
    })
}

pub fn to_int32(r: &mut Runtime, v: &Value) -> JsResult<i32> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as i32,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as i32,
        _ => 0,
    })
}

pub fn to_uint32(r: &mut Runtime, v: &Value) -> JsResult<u32> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as u32,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as u32,
        _ => 0,
    })
}

pub fn to_int64(r: &mut Runtime, v: &Value) -> JsResult<i64> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f),
        _ => 0,
    })
}

pub fn to_uint64(r: &mut Runtime, v: &Value) -> JsResult<u64> {
    Ok(match v.to_number(r)? {
        Value::Int(i) => i as u64,
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => go_float_to_int(f) as u64,
        _ => 0,
    })
}

pub fn to_int(r: &mut Runtime, v: &Value) -> JsResult<i64> {
    to_int64(r, v)
}

pub fn to_uint(r: &mut Runtime, v: &Value) -> JsResult<u64> {
    to_uint64(r, v)
}

pub fn to_float32(r: &mut Runtime, v: &Value) -> JsResult<f32> {
    Ok(v.to_float(r)? as f32)
}

pub fn to_length(r: &mut Runtime, v: &Value) -> JsResult<i64> {
    if v.is_empty() {
        return Ok(0);
    }
    let i = v.to_integer(r)?;
    if i < 0 {
        return Ok(0);
    }
    if i >= MAX_INT {
        return Ok(MAX_INT - 1);
    }
    Ok(i)
}

pub fn to_int_strict(i: i64) -> i64 {
    i
}

pub fn to_int_clamp(i: i64) -> i64 {
    i
}

pub fn shrink_cap(new_size: usize, old_cap: usize) -> usize {
    if old_cap > 8 {
        let cap = old_cap / 2;
        if cap >= new_size {
            return cap;
        }
    }
    old_cap
}

pub fn grow_cap(new_size: usize, old_size: usize, old_cap: usize) -> usize {
    let doublecap = old_cap + old_cap;
    if new_size > doublecap {
        return new_size;
    }
    if old_size < 1024 {
        return doublecap;
    }
    let mut cap = old_cap;
    while 0 < cap && cap < new_size {
        cap += cap / 4;
    }
    if cap == 0 {
        return new_size;
    }
    cap
}

pub fn is_array(r: &mut Runtime, object: ObjRef) -> JsResult<bool> {
    if let Some(target) = crate::builtin_proxy::proxy_target(r, object) {
        match target {
            None => return type_error("Cannot perform 'IsArray' on a proxy that has been revoked"),
            Some(t) => return is_array(r, t),
        }
    }
    Ok(object.class_name(r) == crate::object::CLASS_ARRAY)
}

pub fn limit_call_args<'a>(call: FunctionCall<'a>, n: usize) -> FunctionCall<'a> {
    if call.arguments.len() > n {
        FunctionCall {
            this: call.this,
            arguments: &call.arguments[..n],
        }
    } else {
        call
    }
}

pub fn create_data_property(r: &mut Runtime, o: ObjRef, p: &Value, v: Value) -> JsResult<bool> {
    o.define_own_property(
        r,
        p,
        PropertyDescriptor {
            writable: crate::object::Flag::True,
            enumerable: crate::object::Flag::True,
            configurable: crate::object::Flag::True,
            value: Some(v),
            ..PropertyDescriptor::default()
        },
        false,
    )
}

pub fn create_data_property_or_throw(
    r: &mut Runtime,
    o: ObjRef,
    p: &Value,
    v: Value,
) -> JsResult<bool> {
    o.define_own_property(
        r,
        p,
        PropertyDescriptor {
            writable: crate::object::Flag::True,
            enumerable: crate::object::Flag::True,
            configurable: crate::object::Flag::True,
            value: Some(v),
            ..PropertyDescriptor::default()
        },
        true,
    )
}

pub fn to_property_key(r: &mut Runtime, key: &Value) -> JsResult<Value> {
    key.to_string_value(r)
}

fn ascii_of(s: &unistring::String) -> Option<&[u8]> {
    s.ascii_bytes()
}

pub fn str_to_array_idx(s: &unistring::String) -> u32 {
    let Some(s) = ascii_of(s) else {
        return u32::MAX;
    };
    if s.is_empty() {
        return u32::MAX;
    }
    let l = s.len();
    if s[0] == b'0' {
        if l == 1 {
            return 0;
        }
        return u32::MAX;
    }
    let mut n: u32 = 0;
    if l < 10 {
        for &c in s {
            if !c.is_ascii_digit() {
                return u32::MAX;
            }
            n = n * 10 + u32::from(c - b'0');
        }
        return n;
    }
    if l > 10 {
        return u32::MAX;
    }
    let c9 = s[9];
    if !c9.is_ascii_digit() {
        return u32::MAX;
    }
    for &c in &s[..9] {
        if !c.is_ascii_digit() {
            return u32::MAX;
        }
        n = n * 10 + u32::from(c - b'0');
    }
    if n > u32::MAX / 10 {
        return u32::MAX;
    }
    n *= 10;
    let (n1, overflow) = n.overflowing_add(u32::from(c9 - b'0'));
    if overflow {
        return u32::MAX;
    }
    n1
}

pub fn str_to_int32(s: &unistring::String) -> Option<i32> {
    let s = ascii_of(s)?;
    if s.is_empty() {
        return None;
    }
    let neg = s[0] == b'-';
    let s = if neg { &s[1..] } else { s };
    let l = s.len();
    if s[0] == b'0' {
        if l == 1 {
            return if neg { None } else { Some(0) };
        }
        return None;
    }
    let mut n: u32 = 0;
    if l < 10 {
        for &c in s {
            if !c.is_ascii_digit() {
                return None;
            }
            n = n * 10 + u32::from(c - b'0');
        }
    } else if l > 10 {
        return None;
    } else {
        let c9 = s[9];
        if c9 >= b'0' {
            if !neg && c9 > b'7' || c9 > b'8' {
                return None;
            }
            for &c in &s[..9] {
                if !c.is_ascii_digit() {
                    return None;
                }
                n = n * 10 + u32::from(c - b'0');
            }
            if n > i32::MAX as u32 / 10 {
                return None;
            }
            n = n * 10 + u32::from(c9 - b'0');
        } else {
            return None;
        }
    }
    if neg {
        return Some((n as i32).wrapping_neg());
    }
    Some(n as i32)
}

pub fn str_to_int64(s: &unistring::String) -> (i64, bool) {
    let Some(s) = ascii_of(s) else {
        return (-1, false);
    };
    if s.is_empty() {
        return (-1, false);
    }
    let neg = s[0] == b'-';
    let s = if neg { &s[1..] } else { s };
    let l = s.len();
    if s[0] == b'0' {
        if l == 1 {
            return (0, !neg);
        }
        return (-1, false);
    }
    let mut n: u64 = 0;
    if l < 19 {
        for &c in s {
            if !c.is_ascii_digit() {
                return (-1, false);
            }
            n = n * 10 + u64::from(c - b'0');
        }
    } else if l > 19 {
        return (-1, false);
    } else {
        let c18 = s[18];
        if c18 >= b'0' {
            if !neg && c18 > b'7' || c18 > b'8' {
                return (-1, false);
            }
            for &c in &s[..18] {
                if !c.is_ascii_digit() {
                    return (-1, false);
                }
                n = n * 10 + u64::from(c - b'0');
            }
            if n > i64::MAX as u64 / 10 {
                return (0, false);
            }
            n = n * 10 + u64::from(c18 - b'0');
        } else {
            return (-1, false);
        }
    }
    if neg {
        return ((n as i64).wrapping_neg(), true);
    }
    (n as i64, true)
}

pub fn str_to_int(s: &unistring::String) -> (i64, bool) {
    str_to_int64(s)
}

pub fn str_to_int_num(s: &unistring::String) -> (i64, bool) {
    let (n, ok) = str_to_int64(s);
    if n == 0 {
        return (0, ok);
    }
    if ok && (-MAX_INT..=MAX_INT).contains(&n) {
        return (n, true);
    }
    let str_ = string_value_from_raw(s);
    let num = str_.to_number();
    if num.prim_to_string().same_as(&Value::String(str_)) {
        return (0, false);
    }
    (-1, false)
}

pub fn str_to_go_idx(s: &unistring::String) -> i64 {
    let (n, ok) = str_to_int(s);
    if ok {
        return n;
    }
    -1
}

pub fn str_to_idx64(s: &unistring::String) -> i64 {
    let (n, ok) = str_to_int64(s);
    if ok {
        return n;
    }
    -1
}

pub fn assert_callable(r: &Runtime, v: &Value) -> Option<CallFn> {
    if let Value::Object(o) = v {
        return o.assert_callable(r);
    }
    None
}

impl Runtime {
    pub fn eval(&mut self, src_val: &JsString, direct: bool, strict: bool) -> JsResult<Value> {
        let src = escape_invalid_utf16(src_val);
        let mut in_global = true;
        if direct {
            let mut s = self.vm.stash;
            while let Some(st) = s {
                let stash = self.heap.stash(st);
                if stash.is_variable() {
                    in_global = false;
                    break;
                }
                s = stash.outer;
            }
        }
        self.vm_push_ctx()?;
        let mut func_obj = Value::Undefined;
        if !direct {
            self.vm.stash = Some(self.global.stash);
            self.vm.priv_env = None;
        } else if self.vm.sb > 0 {
            func_obj = self.vm.stack[self.vm.sb as usize - 1].clone();
        }
        let ctx: Rc<dyn EvalContext> = Rc::new(crate::vm::VmEvalContext::new(self));
        let p = self.compile_with_runtime("<eval>", &src, strict, in_global, Some(ctx))?;
        self.vm.prg = Some(p);
        self.vm.pc = 0;
        self.vm.args = 0;
        self.vm.result = Value::Undefined;
        self.vm.push(func_obj);
        self.vm.sb = self.vm.sp;
        self.vm.push(Value::Empty);
        let ex = self.vm_run_try()?;
        let retval = self.vm.result.clone();
        self.vm.pop_ctx();
        if let Some(ex) = ex {
            return Err(Throw::Exception(ex));
        }
        self.vm.sp -= 2;
        Ok(retval)
    }

    pub fn builtin_eval(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if call.arguments.is_empty() {
            return Ok(Value::Undefined);
        }
        if let Value::String(s) = &call.arguments[0] {
            let s = s.clone();
            return self.eval(&s, false, false);
        }
        Ok(call.arguments[0].clone())
    }
}

fn push_rune(b: &mut Vec<u8>, r: u32) {
    if let Some(c) = char::from_u32(r) {
        let mut buf = [0u8; 4];
        b.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

fn write_hex4(b: &mut Vec<u8>, i: u32) {
    const HEX: &[u8] = b"0123456789abcdef";
    b.push(HEX[(i >> 12) as usize & 0xF]);
    b.push(HEX[(i >> 8) as usize & 0xF]);
    b.push(HEX[(i >> 4) as usize & 0xF]);
    b.push(HEX[i as usize & 0xF]);
}

fn read_lenient_utf16(
    chars: &[u16],
    i: &mut usize,
    prev: &mut Option<u16>,
) -> Option<(u32, usize)> {
    let c = match prev.take() {
        Some(p) => p,
        None => {
            if *i >= chars.len() {
                return None;
            }
            let c = chars[*i];
            *i += 1;
            c
        }
    };
    if (0xD800..0xDC00).contains(&c) {
        if *i >= chars.len() {
            return Some((u32::from(c), 1));
        }
        let second = chars[*i];
        *i += 1;
        if (0xDC00..0xE000).contains(&second) {
            let r = 0x10000 + ((u32::from(c) - 0xD800) << 10) + (u32::from(second) - 0xDC00);
            return Some((r, 2));
        }
        *prev = Some(second);
    }
    Some((u32::from(c), 1))
}

pub fn escape_invalid_utf16(s: &JsString) -> Vec<u8> {
    let u = match s {
        JsString::Imported(i) => return i.utf8().to_vec(),
        JsString::Ascii(a) => return a.as_bytes().to_vec(),
        JsString::Unicode(u) => u,
    };
    let chars = &u.raw()[1..];
    let mut sb: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    let mut i = 0usize;
    let mut prev = None;
    while let Some((c, size)) = read_lenient_utf16(chars, &mut i, &mut prev) {
        if (0xD800..0xE000).contains(&c) {
            if sb.is_empty() {
                let mut last = 0u32;
                let mut hi = 0usize;
                let mut hprev = None;
                let head = &chars[..pos];
                while let Some((hc, _)) = read_lenient_utf16(head, &mut hi, &mut hprev) {
                    push_rune(&mut sb, hc);
                    last = hc;
                }
                if last == u32::from(b'\\') {
                    sb.push(b'\\');
                }
            }
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, c);
        } else if !sb.is_empty() {
            push_rune(&mut sb, c);
        } else {
            pos += size;
        }
    }
    if !sb.is_empty() {
        return sb;
    }
    u.go_string()
}
