use crate::compiler::{Program, ResolvedPrivateName};
use crate::object::PrivateId;
use crate::regexp::{Regexp2Wrapper, RegexpPattern, RegexpWrapper};
use crate::string::JsString;
use crate::unistring;
use crate::value::{Value, ValueError, ValueProperty};
use crate::vm::{
    EnterBlock, Instruction, LoadMixed, LoadMixedStack, NamesMap, NewClass, NewFunc, NewMethod,
    ResolveMixed, ResolveMixedStack,
};
use gocompat::strconv;
use std::sync::Arc;

pub fn program(prog: &Program) -> String {
    let mut b = String::new();
    dump_program(&mut b, prog, "");
    b
}

fn dump_program(b: &mut String, prog: &Program, indent: &str) {
    b.push_str(&format!("{indent}func: {}\n", ustr(&prog.func_name)));
    b.push_str(&format!("{indent}code:\n"));
    for (pc, ins) in prog.code.iter().enumerate() {
        let mut nested: Vec<Arc<Program>> = Vec::new();
        let ops = operands(ins, &mut nested);
        b.push_str(&format!("{indent}{pc}\t{}\t{ops}\n", ins.go_type_name()));
        let inner = format!("{indent}  ");
        for n in nested {
            dump_program(b, &n, &inner);
        }
    }
    b.push_str(&format!("{indent}positions:\n"));
    for item in &prog.src_map {
        b.push_str(&format!("{indent}{}\t{}\n", item.pc, item.src_pos));
    }
}

fn quote(s: &[u8]) -> String {
    strconv::quote(s)
}

fn ustr(s: &unistring::String) -> String {
    quote(&s.as_bytes())
}

fn ustr_slice(v: &[unistring::String]) -> String {
    let parts: Vec<String> = v.iter().map(ustr).collect();
    format!("[{}]", parts.join(" "))
}

fn names_map(m: &NamesMap) -> String {
    let mut pairs: Vec<(String, String)> = match m {
        Some(m) => m.iter().map(|(k, v)| (ustr(k), v.to_string())).collect(),
        None => Vec::new(),
    };
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let parts: Vec<String> = pairs.into_iter().map(|(k, v)| format!("{k}:{v}")).collect();
    format!("map[{}]", parts.join(" "))
}

fn float(f: f64) -> String {
    strconv::format_float(f, b'g', -1, 64)
}

fn js_string(s: &JsString) -> String {
    match s {
        JsString::Ascii(a) => quote(a.as_bytes()),
        JsString::Unicode(u) => {
            let mut out = Vec::with_capacity(u.raw().len() * 3);
            for &c in u.raw() {
                let r = if (0xD800..=0xDFFF).contains(&c) {
                    0xFFFD
                } else {
                    i32::from(c)
                };
                gocompat::utf8::append_rune(&mut out, r);
            }
            quote(&out)
        }
        JsString::Imported(i) => {
            let u = match i.unicode() {
                Some(u) => js_string(&JsString::Unicode(u.clone())),
                None => "[]".to_string(),
            };
            format!("&{{s:{} u:{} scanned:true}}", quote(i.utf8()), u)
        }
    }
}

fn value(v: &Value) -> String {
    match v {
        Value::Undefined => "{valueNull:{}}".to_string(),
        Value::Null => "{}".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => float(*f),
        Value::String(s) => js_string(s),
        Value::BigInt(b) => {
            let bytes = b.bytes();
            let mut words: Vec<u64> = Vec::new();
            let mut end = bytes.len();
            while end > 0 {
                let start = end.saturating_sub(8);
                let mut w: u64 = 0;
                for &byte in &bytes[start..end] {
                    w = (w << 8) | u64::from(byte);
                }
                words.push(w);
                end = start;
            }
            while words.last() == Some(&0) {
                words.pop();
            }
            let parts: Vec<String> = words.iter().map(|w| w.to_string()).collect();
            format!("&{{neg:{} abs:[{}]}}", b.is_neg(), parts.join(" "))
        }
        Value::Symbol(_) => "&{h:0 desc:<nil>}".to_string(),
        Value::Object(_) => "&{}".to_string(),
        _ => "<nil>".to_string(),
    }
}

fn value_property(p: &ValueProperty) -> String {
    format!(
        "&{{value:{} writable:{} configurable:{} enumerable:{} accessor:{} getterFunc:<nil> setterFunc:<nil>}}",
        value(&p.value),
        p.writable,
        p.configurable,
        p.enumerable,
        p.accessor
    )
}

fn prog_ref(p: &Option<Arc<Program>>, nested: &mut Vec<Arc<Program>>) -> String {
    match p {
        Some(p) => {
            nested.push(p.clone());
            "<prog>".to_string()
        }
        None => "<nil>".to_string(),
    }
}

fn new_func(f: &NewFunc, nested: &mut Vec<Arc<Program>>) -> String {
    nested.push(f.prg.clone());
    format!(
        "{{prg:<prog> name:{} source:{} length:{} strict:{}}}",
        ustr(&f.name),
        quote(&f.source),
        f.length,
        f.strict
    )
}

fn new_method(m: &NewMethod, nested: &mut Vec<Arc<Program>>) -> String {
    format!(
        "{{newFunc:{} homeObjOffset:{}}}",
        new_func(&m.new_func, nested),
        m.home_obj_offset
    )
}

fn new_class(c: &NewClass, nested: &mut Vec<Arc<Program>>) -> String {
    let ctor = prog_ref(&c.ctor, nested);
    let init_fields = prog_ref(&c.init_fields, nested);
    format!(
        "{{ctor:{ctor} name:{} source:{} initFields:{init_fields} privateFields:{} privateMethods:{} numPrivateFields:{} numPrivateMethods:{} length:{} hasPrivateEnv:{}}}",
        ustr(&c.name),
        quote(&c.source),
        ustr_slice(&c.private_fields),
        ustr_slice(&c.private_methods),
        c.num_private_fields,
        c.num_private_methods,
        c.length,
        c.has_private_env
    )
}

fn enter_block(e: &EnterBlock) -> String {
    format!(
        "{{names:{} stashSize:{} stackSize:{}}}",
        names_map(&e.names),
        e.stash_size,
        e.stack_size
    )
}

fn load_mixed(m: &LoadMixed) -> String {
    format!(
        "&{{name:{} idx:{} callee:{}}}",
        ustr(&m.name),
        m.idx,
        m.callee
    )
}

fn load_mixed_stack(m: &LoadMixedStack) -> String {
    format!(
        "&{{name:{} idx:{} level:{} callee:{}}}",
        ustr(&m.name),
        m.idx,
        m.level,
        m.callee
    )
}

fn resolve_mixed(m: &ResolveMixed) -> String {
    format!(
        "&{{name:{} idx:{} typ:{} strict:{}}}",
        ustr(&m.name),
        m.idx,
        m.typ as u8,
        m.strict
    )
}

fn resolve_mixed_stack(m: &ResolveMixedStack) -> String {
    format!(
        "&{{name:{} idx:{} typ:{} level:{} strict:{}}}",
        ustr(&m.name),
        m.idx,
        m.typ as u8,
        m.level,
        m.strict
    )
}

fn resolved_private_name(r: &ResolvedPrivateName) -> String {
    format!(
        "&{{name:{} idx:{} level:{} isStatic:{} isMethod:{}}}",
        ustr(&r.name),
        r.idx,
        r.level,
        r.is_static,
        r.is_method
    )
}

fn private_id(p: &PrivateId) -> String {
    format!(
        "&{{typ:&{{numFields:{} numMethods:{}}} name:{} idx:{} isMethod:{}}}",
        p.typ.num_fields,
        p.typ.num_methods,
        ustr(&p.name),
        p.idx,
        p.is_method
    )
}

fn regexp_wrapper(w: &Option<Arc<RegexpWrapper>>) -> String {
    let Some(w) = w else {
        return "<nil>".to_string();
    };
    let r = &w.0;
    let names: Vec<String> = r.subexp_names.iter().map(|n| quote(n)).collect();
    let prefix_bytes: Vec<String> = r.prefix.iter().map(|b| b.to_string()).collect();
    format!(
        "&{{expr:{} prog:<syntax.Prog> onepass:<regexp.onePassProg> numSubexp:{} maxBitStateLen:{} subexpNames:[{}] prefix:{} prefixBytes:[{}] prefixRune:{} prefixEnd:{} mpool:{} matchcap:{} prefixComplete:{} cond:{} minInputLen:{} longest:{}}}",
        quote(&r.expr),
        r.num_subexp,
        r.max_bit_state_len,
        names.join(" "),
        quote(&r.prefix),
        prefix_bytes.join(" "),
        r.prefix_rune,
        r.prefix_end,
        r.mpool,
        r.matchcap,
        r.prefix_complete,
        r.cond,
        r.min_input_len,
        r.longest
    )
}

fn regexp2_wrapper(w: &Option<Regexp2Wrapper>) -> String {
    match w {
        Some(w) => {
            let cache = if w.cache.is_some() { "&{}" } else { "<nil>" };
            format!("&{{rx:<regexp2.Regexp> cache:{cache}}}")
        }
        None => "<nil>".to_string(),
    }
}

fn regexp_pattern(p: &RegexpPattern) -> String {
    format!(
        "&{{src:{} global:{} ignoreCase:{} multiline:{} dotAll:{} sticky:{} unicode:{} regexpWrapper:{} regexp2Wrapper:{}}}",
        quote(&p.src),
        p.global,
        p.ignore_case,
        p.multiline,
        p.dot_all,
        p.sticky,
        p.unicode,
        regexp_wrapper(&p.regexp_wrapper),
        regexp2_wrapper(&p.regexp2_wrapper)
    )
}

fn value_error(e: &ValueError) -> String {
    match e {
        ValueError::TypeError(m)
        | ValueError::RangeError(m)
        | ValueError::ReferenceError(m)
        | ValueError::SyntaxError(m) => quote(m.as_bytes()),
    }
}

fn operands(ins: &Instruction, nested: &mut Vec<Arc<Program>>) -> String {
    use Instruction as I;
    match ins {
        I::Placeholder => "<nil>".to_string(),
        I::LoadVal(v) => format!("{{v:{}}}", value(v)),
        I::LoadStack(n)
        | I::LoadStack1(n)
        | I::LoadStackLex(n)
        | I::LoadStack1Lex(n)
        | I::StoreStack(n)
        | I::StoreStack1(n)
        | I::StoreStackLex(n)
        | I::StoreStack1Lex(n)
        | I::InitStack(n)
        | I::InitStackP(n)
        | I::InitStack1(n)
        | I::InitStack1P(n)
        | I::StoreStackP(n)
        | I::StoreStack1P(n)
        | I::StoreStackLexP(n)
        | I::StoreStack1LexP(n)
        | I::CreateArgsRestStack(n)
        | I::ConcatStrings(n)
        | I::DefineComputedKey(n)
        | I::LoadComputedKey(n) => n.to_string(),
        I::Jump(n)
        | I::JneP(n)
        | I::JeqP(n)
        | I::Jeq(n)
        | I::Jne(n)
        | I::Jdef(n)
        | I::JdefP(n)
        | I::Jopt(n)
        | I::Joptc(n)
        | I::Jcoalesc(n)
        | I::JcoalescP(n)
        | I::EnumNext(n)
        | I::IterNext(n) => n.to_string(),
        I::DupN(n)
        | I::RdupN(n)
        | I::DupLast(n)
        | I::NewArray(n)
        | I::StoreStash(n)
        | I::StoreStashP(n)
        | I::StoreStashLex(n)
        | I::StoreStashLexP(n)
        | I::InitStash(n)
        | I::InitStashP(n)
        | I::LoadStash(n)
        | I::LoadStashLex(n)
        | I::CallEval(n)
        | I::CallEvalStrict(n)
        | I::Call(n)
        | I::Cret(n)
        | I::New(n)
        | I::SuperCall(n)
        | I::CreateArgsMapped(n)
        | I::CreateArgsUnmapped(n)
        | I::LoadThisStash(n)
        | I::ResolveThisStash(n) => n.to_string(),
        I::DeleteProp(s)
        | I::DeletePropStrict(s)
        | I::GetPropRef(s)
        | I::GetPropRefRecv(s)
        | I::GetPropRefStrict(s)
        | I::GetPropRefRecvStrict(s)
        | I::SetProp(s)
        | I::SetPropP(s)
        | I::SetPropStrict(s)
        | I::SetPropRecv(s)
        | I::SetPropRecvStrict(s)
        | I::SetPropRecvP(s)
        | I::SetPropRecvStrictP(s)
        | I::SetPropStrictP(s)
        | I::PutProp(s)
        | I::DefinePropKeyed(s)
        | I::GetProp(s)
        | I::GetPropRecv(s)
        | I::GetPropRecvCallee(s)
        | I::GetPropCallee(s)
        | I::InitGlobalP(s)
        | I::InitGlobal(s)
        | I::ResolveVar1(s)
        | I::DeleteVar(s)
        | I::DeleteGlobal(s)
        | I::ResolveVar1Strict(s)
        | I::SetGlobal(s)
        | I::SetGlobalStrict(s)
        | I::LoadDynamic(s)
        | I::LoadDynamicRef(s)
        | I::LoadDynamicCallee(s) => ustr(s),
        I::DefineMethod(d) | I::DefineGetter(d) | I::DefineSetter(d) => {
            format!("&{{enumerable:{}}}", d.enumerable)
        }
        I::DefineMethodKeyed(d) | I::DefineGetterKeyed(d) | I::DefineSetterKeyed(d) => {
            format!("&{{key:{} enumerable:{}}}", ustr(&d.key), d.enumerable)
        }
        I::NewRegexp(n) => format!(
            "&{{pattern:{} src:{}}}",
            regexp_pattern(&n.pattern),
            js_string(&n.src)
        ),
        I::LoadMixed(m) | I::LoadMixedLex(m) => load_mixed(m),
        I::LoadMixedStack(m)
        | I::LoadMixedStack1(m)
        | I::LoadMixedStackLex(m)
        | I::LoadMixedStack1Lex(m) => load_mixed_stack(m),
        I::ResolveMixed(m) => resolve_mixed(m),
        I::ResolveMixedStack(m) | I::ResolveMixedStack1(m) => resolve_mixed_stack(m),
        I::EnterBlock(e) | I::EnterCatchBlock(e) => format!("&{}", enter_block(e)),
        I::LeaveBlock(l) => format!("&{{stackSize:{} popStash:{}}}", l.stack_size, l.pop_stash),
        I::EnterFunc(e) => format!(
            "&{{names:{} stashSize:{} stackSize:{} numArgs:{} funcType:{} argsToStash:{} extensible:{}}}",
            names_map(&e.names),
            e.stash_size,
            e.stack_size,
            e.num_args,
            e.func_type as u8,
            e.args_to_stash,
            e.extensible
        ),
        I::EnterFunc1(e) => format!(
            "&{{names:{} stashSize:{} numArgs:{} argsToCopy:{} funcType:{} extensible:{}}}",
            names_map(&e.names),
            e.stash_size,
            e.num_args,
            e.args_to_copy,
            e.func_type as u8,
            e.extensible
        ),
        I::EnterFuncBody(e) => format!(
            "&{{enterBlock:{} funcType:{} extensible:{} adjustStack:{}}}",
            enter_block(&e.enter_block),
            e.func_type as u8,
            e.extensible,
            e.adjust_stack
        ),
        I::EnterFuncStashless(e) => format!("&{{stackSize:{} args:{}}}", e.stack_size, e.args),
        I::NewFunc(f) => format!("&{}", new_func(f, nested)),
        I::NewAsyncFunc(f) | I::NewGeneratorFunc(f) | I::NewArrowFunc(f) => {
            format!("&{{newFunc:{}}}", new_func(f, nested))
        }
        I::NewAsyncArrowFunc(f) => format!("&{{newArrowFunc:{{newFunc:{}}}}}", new_func(f, nested)),
        I::NewMethod(m) => format!("&{}", new_method(m, nested)),
        I::NewAsyncMethod(m) | I::NewGeneratorMethod(m) => {
            format!("&{{newMethod:{}}}", new_method(m, nested))
        }
        I::BindVars(b) => format!(
            "&{{names:{} deletable:{}}}",
            ustr_slice(&b.names),
            b.deletable
        ),
        I::BindGlobal(b) => format!(
            "&{{vars:{} funcs:{} lets:{} consts:{} deletable:{}}}",
            ustr_slice(&b.vars),
            ustr_slice(&b.funcs),
            ustr_slice(&b.lets),
            ustr_slice(&b.consts),
            b.deletable
        ),
        I::Try(t) => format!(
            "{{catchOffset:{} finallyOffset:{}}}",
            t.catch_offset, t.finally_offset
        ),
        I::GetTaggedTmplObject(g) => {
            let raw: Vec<String> = g.raw.iter().map(value_property).collect();
            let cooked: Vec<String> = g.cooked.iter().map(value_property).collect();
            format!("&{{raw:[{}] cooked:[{}]}}", raw.join(" "), cooked.join(" "))
        }
        I::NewClass(c) => format!("&{}", new_class(c, nested)),
        I::NewDerivedClass(c) => format!("&{{newClass:{}}}", new_class(c, nested)),
        I::NewStaticFieldInit(s) => {
            let init_fields = prog_ref(&s.init_fields, nested);
            format!(
                "&{{initFields:{init_fields} numPrivateFields:{} numPrivateMethods:{}}}",
                s.num_private_fields, s.num_private_methods
            )
        }
        I::ThrowConst(e) => format!("{{v:{}}}", value_error(e)),
        I::InitStaticElements(i) => format!(
            "&{{privateFields:{} privateMethods:{}}}",
            ustr_slice(&i.private_fields),
            ustr_slice(&i.private_methods)
        ),
        I::DefinePrivateMethod(d) => format!("&{{idx:{} targetOffset:{}}}", d.idx, d.target_offset),
        I::DefinePrivateGetter(d) | I::DefinePrivateSetter(d) => format!(
            "&{{definePrivateMethod:{{idx:{} targetOffset:{}}}}}",
            d.idx, d.target_offset
        ),
        I::DefinePrivateProp(d) => format!("&{{idx:{}}}", d.idx),
        I::GetPrivatePropRes(r)
        | I::GetPrivatePropResCallee(r)
        | I::SetPrivatePropRes(r)
        | I::SetPrivatePropResP(r)
        | I::PrivateInRes(r)
        | I::GetPrivateRefRes(r) => resolved_private_name(r),
        I::GetPrivatePropId(p)
        | I::GetPrivatePropIdCallee(p)
        | I::SetPrivatePropId(p)
        | I::SetPrivatePropIdP(p)
        | I::PrivateInId(p)
        | I::GetPrivateRefId(p) => private_id(p),
        I::YieldMarker(y) => format!("&{{valueNull:{{}} resultType:{}}}", y.result_type() as u8),
        _ => "{}".to_string(),
    }
}
