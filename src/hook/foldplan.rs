use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use goja::Program;
use goja::ast::{
    BindingTarget, BlockStatement, CallExpression, Expression, ForLoopInitializer, ForStatement,
    FunctionLiteral, IfStatement, Node, NumberValue, Property, Statement, VariableStatement,
    walk_nodes,
};
use goja::token;

use crate::crypto::sha256;
use crate::gocompat::strconv;
use crate::query::{Filters, Ops};
use crate::sync::Mutex;

use super::foldeval::{Expr, NULL_VAL, Stmt, UNDEF_VAL, ascii_only, bool_val, num_val};

pub(crate) const FOLD_GLOBAL: &str = "__elysian_hook";
const FOLD_RESULT_VAR: &str = "__elysian_r";
const MAX_HELPER_DEPTH: usize = 8;
const MAX_SCRIPT_PLANS: usize = 1024;
pub(crate) const ENTITY_FREE_LABEL: &str = "\0entity.";

const FOLD_DENIED: [&str; 13] = [
    "prototype",
    "__proto__",
    "constructor",
    "eval",
    "Function",
    "globalThis",
    "Reflect",
    "Proxy",
    "arguments",
    "setPrototypeOf",
    "getPrototypeOf",
    "defineProperty",
    "__elysian",
];

const OBJECT_MEMBERS: [&str; 12] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

fn object_member(name: &str) -> bool {
    OBJECT_MEMBERS.contains(&name)
}

#[derive(Clone, Debug)]
pub(crate) struct FoldDef {
    pub(crate) id: Vec<u8>,
    pub(crate) target: String,
    pub(crate) part_field: String,
    pub(crate) base_filters: Filters,
    pub(crate) static_base: bool,
    pub(crate) fields: Vec<String>,
    pub(crate) regs: Vec<String>,
    pub(crate) frees: Vec<String>,
    pub(crate) lits: Vec<String>,
    pub(crate) locals: usize,
    pub(crate) body: Stmt,
    pub(crate) accumulating: bool,
}

pub(crate) struct ScriptPlan {
    pub(crate) program: Arc<Program>,
    pub(crate) folds: Vec<Arc<FoldDef>>,
}

type PlanCache = HashMap<(String, String), Option<Arc<ScriptPlan>>>;

static PLANS: Mutex<Option<PlanCache>> = Mutex::new(None);

pub(crate) fn plan_for(script: &str, fn_name: &str) -> Option<Arc<ScriptPlan>> {
    let key = (script.to_string(), fn_name.to_string());

    if let Some(p) = PLANS.lock().as_ref().and_then(|m| m.get(&key).cloned()) {
        return p;
    }

    let p = build_plan(script, fn_name).map(Arc::new);

    let mut g = PLANS.lock();
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() >= MAX_SCRIPT_PLANS {
        m.clear();
    }
    m.insert(key, p.clone());

    p
}

pub struct PlanInfo {
    pub source: Vec<u8>,
    pub dump: String,
    pub fold_ids: Vec<Vec<u8>>,
}

pub fn plan_info(script: &str, fn_name: &str) -> Option<PlanInfo> {
    let mut source = Vec::new();
    let plan = build_plan_with(script, fn_name, &mut source)?;

    Some(PlanInfo {
        source,
        dump: goja::gojadump::program(&plan.program),
        fold_ids: plan.folds.iter().map(|f| f.id.clone()).collect(),
    })
}

fn build_plan(script: &str, fn_name: &str) -> Option<ScriptPlan> {
    let mut source = Vec::new();
    build_plan_with(script, fn_name, &mut source)
}

fn build_plan_with(script: &str, fn_name: &str, source: &mut Vec<u8>) -> Option<ScriptPlan> {
    for word in FOLD_DENIED {
        if script.contains(word) {
            return None;
        }
    }

    let program = goja::parse("", script.as_bytes(), &[]).ok()?;

    let mut helpers: HashMap<String, &FunctionLiteral> = HashMap::new();
    let mut entry: Option<&FunctionLiteral> = None;
    for st in &program.body {
        let Statement::FunctionDeclaration(decl) = st else {
            return None;
        };

        let name = decl
            .function
            .name
            .as_ref()
            .map(|n| n.name.string())
            .unwrap_or_default();
        if helpers.contains_key(&name) {
            return None;
        }
        helpers.insert(name.clone(), &decl.function);
        if name == fn_name {
            entry = Some(&decl.function);
        }
    }

    let entry = entry?;
    if entry.parameter_list.list.len() != 1 || entry.parameter_list.rest.is_some() {
        return None;
    }

    let BindingTarget::Identifier(param) = &entry.parameter_list.list[0].target else {
        return None;
    };
    if entry.parameter_list.list[0].initializer.is_some() {
        return None;
    }

    if !safe_assignments(&program.body) {
        return None;
    }

    let a = Analysis {
        script,
        fn_name,
        helpers,
        entry,
        param: param.name.string(),
    };

    a.plan(source)
}

struct Edit {
    from: usize,
    to: usize,
    text: Vec<u8>,
}

struct Analysis<'a> {
    script: &'a str,
    fn_name: &'a str,
    helpers: HashMap<String, &'a FunctionLiteral>,
    entry: &'a FunctionLiteral,
    param: String,
}

struct QueryDecl<'a> {
    name: String,
    call: &'a CallExpression,
    target: String,
    base: Filters,
    static_: bool,
    part: String,
    binding: usize,
}

struct NotFoldable;

type Compiled<T> = Result<T, NotFoldable>;

fn addr<T>(x: &T) -> usize {
    x as *const T as usize
}

impl<'a> Analysis<'a> {
    fn plan(&self, source: &mut Vec<u8>) -> Option<ScriptPlan> {
        let mut queries: BTreeMap<String, QueryDecl<'a>> = BTreeMap::new();
        for st in &self.entry.body.list {
            let Statement::VariableStatement(vs) = st else {
                continue;
            };

            for b in &vs.list {
                let BindingTarget::Identifier(id) = &b.target else {
                    continue;
                };

                let Some(Expression::CallExpression(call)) = &b.initializer else {
                    continue;
                };

                if let Some(mut q) = self.query_call(id.name.string(), call) {
                    if queries.contains_key(&q.name) {
                        queries.remove(&q.name);
                        continue;
                    }
                    q.binding = addr(&**b);
                    queries.insert(q.name.clone(), q);
                }
            }
        }

        if queries.is_empty() {
            return None;
        }

        let mut defs: Vec<Option<FoldDef>> = Vec::new();
        let mut loops: Vec<(&'a Statement, &'a ForStatement)> = Vec::new();
        let mut loops_by_query: HashMap<String, Vec<usize>> = HashMap::new();
        for st in &self.entry.body.list {
            let Statement::ForStatement(lp) = st else {
                continue;
            };

            let Some((name, idx)) = loop_shape(lp) else {
                continue;
            };

            let Some(q) = queries.get(&name) else {
                continue;
            };

            let Ok(def) = self.compile_loop(q, st, lp, &idx) else {
                continue;
            };

            loops_by_query.entry(name).or_default().push(defs.len());
            defs.push(Some(def));
            loops.push((st, lp));
        }

        if defs.is_empty() {
            return None;
        }

        let script = self.script.as_bytes();
        let mut edits: Vec<Edit> = Vec::new();
        let mut kept: Vec<Arc<FoldDef>> = Vec::new();
        for (name, q) in &queries {
            let Some(list) = loops_by_query.get(name) else {
                continue;
            };
            if list.is_empty() || !self.used_only_in_loops(name, list, &loops, q) {
                continue;
            }

            let callee = &q.call.callee;
            edits.push(Edit {
                from: (callee.idx0() - 1) as usize,
                to: (callee.idx1() - 1) as usize,
                text: format!("{FOLD_GLOBAL}.view").into_bytes(),
            });
            for &i in list {
                let mut def = defs[i].take().expect("fold definition");
                let st = loops[i].0;
                let (start, end) = ((st.idx0() - 1) as usize, (st.idx1() - 1) as usize);
                edits.push(Edit {
                    from: start,
                    to: end,
                    text: fold_call_text(kept.len(), name, &def, &script[start..end], &self.param),
                });
                def.id = fold_id(self.script, self.fn_name, kept.len());
                kept.push(Arc::new(def));
            }
        }

        if kept.is_empty() {
            return None;
        }

        edits.sort_by(|a, b| b.from.cmp(&a.from));
        let mut src = script.to_vec();
        for e in &edits {
            let mut next = Vec::with_capacity(src.len() + e.text.len());
            next.extend_from_slice(&src[..e.from]);
            next.extend_from_slice(&e.text);
            next.extend_from_slice(&src[e.to..]);
            src = next;
        }

        let program = goja::compile("", &src, false).ok()?;
        *source = src;

        Some(ScriptPlan {
            program,
            folds: kept,
        })
    }

    fn query_call(&self, name: String, call: &'a CallExpression) -> Option<QueryDecl<'a>> {
        let Expression::DotExpression(dot) = &call.callee else {
            return None;
        };
        if dot.identifier.name != "query" || call.argument_list.len() != 2 {
            return None;
        }

        let Expression::Identifier(id) = &dot.left else {
            return None;
        };
        if id.name.string() != self.param {
            return None;
        }

        let Expression::StringLiteral(target) = &call.argument_list[0] else {
            return None;
        };

        let Expression::ObjectLiteral(object) = &call.argument_list[1] else {
            return None;
        };

        let mut q = QueryDecl {
            name,
            call,
            target: target.value.string(),
            base: Filters::new(),
            static_: true,
            part: String::new(),
            binding: 0,
        };
        for prop in &object.value {
            let Property::PropertyKeyed(keyed) = prop else {
                return None;
            };
            if keyed.computed {
                return None;
            }

            let field = literal_key(&keyed.key);
            let Expression::ObjectLiteral(cond) = &keyed.value else {
                return None;
            };
            if field.is_empty() || field.contains('.') {
                return None;
            }

            let mut ops = Ops::new();
            let mut dynamic = false;
            for op in &cond.value {
                let Property::PropertyKeyed(kv) = op else {
                    return None;
                };
                if kv.computed {
                    return None;
                }

                let name = literal_key(&kv.key);
                if name.is_empty() {
                    return None;
                }

                match &kv.value {
                    Expression::StringLiteral(lit) => {
                        ops.insert(name.into_bytes(), lit.value.string().into_bytes());
                    }
                    Expression::NumberLiteral(_) | Expression::BooleanLiteral(_) => {
                        q.static_ = false;
                    }
                    other => {
                        if !pure_expression(other) {
                            return None;
                        }
                        if name != "eq" || cond.value.len() != 1 {
                            return None;
                        }
                        dynamic = true;
                    }
                }
            }

            if dynamic {
                if !q.part.is_empty() {
                    return None;
                }
                q.part = field;
                continue;
            }

            q.base.insert(field.into_bytes(), ops);
        }

        Some(q)
    }

    fn used_only_in_loops(
        &self,
        name: &str,
        list: &[usize],
        loops: &[(&Statement, &ForStatement)],
        q: &QueryDecl<'_>,
    ) -> bool {
        let in_loop: HashSet<usize> = list.iter().map(|&i| addr(loops[i].0)).collect();

        for st in &self.entry.body.list {
            if matches!(st, Statement::ForStatement(_)) && in_loop.contains(&addr(st)) {
                continue;
            }

            let mut used: HashMap<String, usize> = HashMap::new();
            if let Statement::VariableStatement(vs) = st {
                for b in &vs.list {
                    if addr(&**b) == q.binding {
                        continue;
                    }
                    if let Some(init) = &b.initializer {
                        collect_identifiers(Node::from(init), &mut used);
                    }
                }
            } else {
                collect_identifiers(Node::from(st), &mut used);
            }

            if used.get(name).copied().unwrap_or(0) > 0 {
                return false;
            }
        }

        true
    }

    fn compile_loop(
        &self,
        q: &QueryDecl<'_>,
        loop_st: &Statement,
        lp: &ForStatement,
        idx: &str,
    ) -> Compiled<FoldDef> {
        let mut outside: HashMap<String, usize> = HashMap::new();
        let mut outer: HashSet<String> = HashSet::new();
        for st in &self.entry.body.list {
            if addr(st) == addr(loop_st) {
                continue;
            }
            walk_nodes(Node::from(st), &mut |n| {
                match n {
                    Node::VariableStatement(t) => {
                        for b in &t.list {
                            if let BindingTarget::Identifier(id) = &b.target {
                                outer.insert(id.name.string());
                            }
                        }
                    }
                    Node::ForLoopInitializerVarDeclList(t) => {
                        for b in &t.list {
                            if let BindingTarget::Identifier(id) = &b.target {
                                outer.insert(id.name.string());
                            }
                        }
                    }
                    _ => {}
                }
                true
            });
            collect_identifiers(Node::from(st), &mut outside);
        }

        let Statement::BlockStatement(body) = &lp.body else {
            return Err(NotFoldable);
        };
        let mut declared: HashSet<String> = HashSet::new();
        let mut assigned: HashSet<String> = HashSet::new();
        walk_nodes(Node::BlockStatement(body), &mut |n| {
            match n {
                Node::VariableStatement(t) => {
                    for b in &t.list {
                        if let BindingTarget::Identifier(id) = &b.target {
                            declared.insert(id.name.string());
                        }
                    }
                }
                Node::AssignExpression(t) => {
                    if let Expression::Identifier(id) = &t.left {
                        assigned.insert(id.name.string());
                    }
                }
                Node::UnaryExpression(t) => {
                    if (t.operator == token::INCREMENT || t.operator == token::DECREMENT)
                        && let Expression::Identifier(id) = &t.operand
                    {
                        assigned.insert(id.name.string());
                    }
                }
                _ => {}
            }
            true
        });

        let used_outside = |name: &str| outside.get(name).copied().unwrap_or(0) > 0;
        for name in &declared {
            if used_outside(name)
                || outer.contains(name)
                || name == idx
                || *name == q.name
                || *name == self.param
            {
                return Err(NotFoldable);
            }
        }

        if used_outside(idx)
            || outer.contains(idx)
            || assigned.contains(idx)
            || assigned.contains(&q.name)
            || assigned.contains(&self.param)
        {
            return Err(NotFoldable);
        }

        let mut c = FoldCompiler {
            a: self,
            query: q.name.clone(),
            idx: idx.to_string(),
            outer,
            declared,
            assigned,
            regs: HashMap::new(),
            reg_names: Vec::new(),
            frees: HashMap::new(),
            free_names: Vec::new(),
            fields: HashMap::new(),
            field_names: Vec::new(),
            lits: Vec::new(),
            lit_index: HashMap::new(),
            next_local: 0,
            reg_read: false,
            reg_assign: false,
            depth: 0,
            nested: 0,
            scopes: vec![FoldScope::default()],
        };

        let mut fields: HashSet<String> = HashSet::new();
        collect_row_fields(body, &q.name, idx, &mut fields);
        c.field_names = fields.into_iter().collect();
        c.field_names.sort();
        for (i, f) in c.field_names.iter().enumerate() {
            c.fields.insert(f.clone(), i);
        }

        let (block, _) = c.stmt_list(&body.list)?;

        let def = FoldDef {
            id: Vec::new(),
            target: q.target.clone(),
            part_field: q.part.clone(),
            base_filters: q.base.clone(),
            static_base: q.static_,
            fields: c.field_names,
            regs: c.reg_names,
            frees: c.free_names,
            lits: c.lits,
            locals: c.next_local,
            body: block,
            accumulating: !c.reg_read && !c.reg_assign,
        };

        if def.regs.is_empty() {
            return Err(NotFoldable);
        }

        Ok(def)
    }
}

fn fold_id(script: &str, fn_name: &str, index: usize) -> Vec<u8> {
    let h = sha256::sum256(format!("{script}\0{fn_name}\0{index}").as_bytes());

    h[..16].to_vec()
}

fn fold_call_text(index: usize, query: &str, def: &FoldDef, lp: &[u8], param: &str) -> Vec<u8> {
    let mut args: Vec<String> = vec![index.to_string(), query.to_string()];
    args.extend(def.regs.iter().cloned());
    for f in &def.frees {
        match f.strip_prefix(ENTITY_FREE_LABEL) {
            Some(rest) => args.push(format!(
                "{param}.entity[{}]",
                strconv::quote(rest.as_bytes())
            )),
            None => args.push(f.clone()),
        }
    }

    let result = format!("{FOLD_RESULT_VAR}{index}");

    let mut b = String::new();
    b.push_str(&format!(
        "var {result} = {FOLD_GLOBAL}.fold({}); if ({result}) {{ ",
        args.join(", ")
    ));
    for (i, r) in def.regs.iter().enumerate() {
        b.push_str(&format!("{r} = {result}[{i}]; "));
    }
    b.push_str(&format!(
        "}} else {{ {query} = {FOLD_GLOBAL}.rows({query}); "
    ));

    let mut out = b.into_bytes();
    out.extend_from_slice(lp);
    out.extend_from_slice(b" }");

    out
}

fn pure_expression(e: &Expression) -> bool {
    match e {
        Expression::Identifier(_) => true,
        Expression::StringLiteral(_)
        | Expression::NumberLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::DotExpression(t) => pure_expression(&t.left),
        _ => false,
    }
}

fn literal_key(k: &Expression) -> String {
    match k {
        Expression::StringLiteral(t) => t.value.string(),
        Expression::Identifier(t) => t.name.string(),
        _ => String::new(),
    }
}

fn loop_shape(lp: &ForStatement) -> Option<(String, String)> {
    if !matches!(lp.body, Statement::BlockStatement(_)) {
        return None;
    }

    let Some(ForLoopInitializer::ForLoopInitializerVarDeclList(init)) = &lp.initializer else {
        return None;
    };
    if init.list.len() != 1 {
        return None;
    }
    let BindingTarget::Identifier(id) = &init.list[0].target else {
        return None;
    };
    if !is_zero(init.list[0].initializer.as_ref()) {
        return None;
    }
    let idx = id.name.string();

    let Some(Expression::BinaryExpression(test)) = &lp.test else {
        return None;
    };
    if test.operator != token::LESS {
        return None;
    }

    let Expression::Identifier(left) = &test.left else {
        return None;
    };
    if left.name.string() != idx {
        return None;
    }

    let Expression::DotExpression(length) = &test.right else {
        return None;
    };
    if length.identifier.name != "length" {
        return None;
    }

    let Expression::Identifier(query) = &length.left else {
        return None;
    };

    let Some(Expression::UnaryExpression(update)) = &lp.update else {
        return None;
    };
    if update.operator != token::INCREMENT {
        return None;
    }

    let Expression::Identifier(uid) = &update.operand else {
        return None;
    };
    if uid.name.string() != idx {
        return None;
    }

    Some((query.name.string(), idx))
}

fn is_zero(e: Option<&Expression>) -> bool {
    let Some(Expression::NumberLiteral(n)) = e else {
        return false;
    };

    match &n.value {
        NumberValue::Int64(v) => *v == 0,
        NumberValue::Float64(v) => *v == 0.0,
        _ => false,
    }
}

fn root_identifier(mut e: &Expression) -> String {
    loop {
        match e {
            Expression::Identifier(t) => return t.name.string(),
            Expression::DotExpression(t) => e = &t.left,
            Expression::BracketExpression(t) => e = &t.left,
            _ => return String::new(),
        }
    }
}

fn binding_names(list: &[Arc<goja::ast::Binding>], into: &mut HashSet<String>) {
    for b in list {
        if let BindingTarget::Identifier(id) = &b.target {
            into.insert(id.name.string());
        }
    }
}

fn safe_assignments(body: &[Statement]) -> bool {
    let mut safe = true;
    for st in body {
        let Statement::FunctionDeclaration(decl) = st else {
            continue;
        };
        let f = &decl.function;
        let mut locals: HashSet<String> = HashSet::new();
        binding_names(&f.parameter_list.list, &mut locals);

        walk_nodes(Node::BlockStatement(&f.body), &mut |n| {
            match n {
                Node::VariableStatement(t) => binding_names(&t.list, &mut locals),
                Node::ForLoopInitializerVarDeclList(t) => binding_names(&t.list, &mut locals),
                Node::FunctionLiteral(_) | Node::ArrowFunctionLiteral(_) => safe = false,
                Node::WithStatement(_)
                | Node::ThisExpression(_)
                | Node::TryStatement(_)
                | Node::YieldExpression(_)
                | Node::AwaitExpression(_) => safe = false,
                Node::BracketExpression(t) => {
                    if !matches!(
                        t.member,
                        Expression::StringLiteral(_)
                            | Expression::NumberLiteral(_)
                            | Expression::Identifier(_)
                    ) {
                        safe = false;
                    }
                }
                _ => {}
            }
            true
        });

        walk_nodes(Node::BlockStatement(&f.body), &mut |n| {
            match n {
                Node::AssignExpression(t) => {
                    if !locals.contains(&root_identifier(&t.left)) {
                        safe = false;
                    }
                }
                Node::UnaryExpression(t) => {
                    if (t.operator == token::INCREMENT
                        || t.operator == token::DECREMENT
                        || t.operator == token::DELETE)
                        && !locals.contains(&root_identifier(&t.operand))
                    {
                        safe = false;
                    }
                }
                _ => {}
            }
            true
        });
    }

    safe
}

fn collect_identifiers(node: Node<'_>, into: &mut HashMap<String, usize>) {
    walk_nodes(node, &mut |n| match n {
        Node::Identifier(t) => {
            *into.entry(t.name.string()).or_insert(0) += 1;
            true
        }
        Node::PropertyKeyed(t) if !t.computed => {
            collect_identifiers(Node::from(&t.value), into);
            false
        }
        _ => true,
    });
}

fn is_row_of(e: Option<&Expression>, query: &str, idx: &str) -> bool {
    let Some(Expression::BracketExpression(b)) = e else {
        return false;
    };
    let Expression::Identifier(left) = &b.left else {
        return false;
    };
    if left.name.string() != query {
        return false;
    }
    matches!(&b.member, Expression::Identifier(m) if m.name.string() == idx)
}

fn collect_row_fields(body: &BlockStatement, query: &str, idx: &str, into: &mut HashSet<String>) {
    let mut aliases: HashSet<String> = HashSet::new();
    for st in &body.list {
        let Statement::VariableStatement(vs) = st else {
            continue;
        };
        for b in &vs.list {
            if let BindingTarget::Identifier(id) = &b.target
                && is_row_of(b.initializer.as_ref(), query, idx)
            {
                aliases.insert(id.name.string());
            }
        }
    }

    let is_ref = |e: &Expression| -> bool {
        if let Expression::Identifier(id) = e {
            return aliases.contains(&id.name.string());
        }
        is_row_of(Some(e), query, idx)
    };

    walk_nodes(Node::BlockStatement(body), &mut |n| {
        match n {
            Node::DotExpression(t) => {
                if is_ref(&t.left) {
                    into.insert(t.identifier.name.string());
                }
            }
            Node::BracketExpression(t) => {
                if let Expression::StringLiteral(s) = &t.member
                    && is_ref(&t.left)
                {
                    into.insert(s.value.string());
                }
            }
            _ => {}
        }
        true
    });
}

#[derive(Default)]
struct FoldScope {
    slots: HashMap<String, usize>,
    set: HashSet<String>,
    aliases: HashSet<String>,
    inner: bool,
}

struct FoldCompiler<'a> {
    a: &'a Analysis<'a>,
    query: String,
    idx: String,
    outer: HashSet<String>,
    declared: HashSet<String>,
    assigned: HashSet<String>,
    regs: HashMap<String, usize>,
    reg_names: Vec<String>,
    frees: HashMap<String, usize>,
    free_names: Vec<String>,
    fields: HashMap<String, usize>,
    field_names: Vec<String>,
    lits: Vec<String>,
    lit_index: HashMap<String, usize>,
    next_local: usize,
    reg_read: bool,
    reg_assign: bool,
    depth: usize,
    nested: usize,
    scopes: Vec<FoldScope>,
}

impl FoldCompiler<'_> {
    fn scope(&mut self) -> &mut FoldScope {
        self.scopes.last_mut().expect("scope")
    }

    fn scope_ref(&self) -> &FoldScope {
        self.scopes.last().expect("scope")
    }

    fn local_slot(&mut self, name: &str) -> usize {
        if let Some(i) = self.scope_ref().slots.get(name) {
            return *i;
        }

        let i = self.next_local;
        self.next_local += 1;
        self.scope().slots.insert(name.to_string(), i);

        i
    }

    fn lit(&mut self, s: &str) -> usize {
        if let Some(i) = self.lit_index.get(s) {
            return *i;
        }

        let i = self.lits.len();
        self.lits.push(s.to_string());
        self.lit_index.insert(s.to_string(), i);

        i
    }

    fn reg(&mut self, name: &str) -> usize {
        if let Some(i) = self.regs.get(name) {
            return *i;
        }

        let i = self.reg_names.len();
        self.regs.insert(name.to_string(), i);
        self.reg_names.push(name.to_string());

        i
    }

    fn free(&mut self, name: &str) -> usize {
        if let Some(i) = self.frees.get(name) {
            return *i;
        }

        let i = self.free_names.len();
        self.frees.insert(name.to_string(), i);
        self.free_names.push(name.to_string());

        i
    }

    fn is_local(&self, name: &str) -> bool {
        let s = self.scope_ref();
        if s.inner {
            return s.slots.contains_key(name);
        }

        self.declared.contains(name)
    }

    fn is_reg(&self, name: &str) -> bool {
        !self.scope_ref().inner && self.outer.contains(name) && self.assigned.contains(name)
    }

    fn is_free(&self, name: &str) -> bool {
        !self.scope_ref().inner && self.outer.contains(name) && !self.assigned.contains(name)
    }

    fn stmt_list(&mut self, list: &[Statement]) -> Compiled<(Stmt, bool)> {
        let mut out: Vec<Stmt> = Vec::with_capacity(list.len());
        let mut ends = false;
        for st in list {
            if ends {
                return Err(NotFoldable);
            }

            let (s, stop) = self.statement(st)?;
            if let Some(s) = s {
                out.push(s);
            }
            ends = stop;
        }

        Ok(match out.len() {
            0 => (Stmt::Nop, ends),
            1 => (out.pop().expect("one statement"), ends),
            _ => (Stmt::List(out), ends),
        })
    }

    fn statement(&mut self, st: &Statement) -> Compiled<(Option<Stmt>, bool)> {
        match st {
            Statement::EmptyStatement(_) => Ok((None, false)),
            Statement::BlockStatement(t) => {
                let (s, ends) = self.stmt_list(&t.list)?;
                Ok((Some(s), ends))
            }
            Statement::VariableStatement(t) => self.variables(t),
            Statement::ExpressionStatement(t) => {
                let s = self.effect(&t.expression)?;
                Ok((Some(s), false))
            }
            Statement::BranchStatement(t) => {
                if t.token != token::CONTINUE || t.label.is_some() || self.scope_ref().inner {
                    return Err(NotFoldable);
                }
                Ok((Some(Stmt::Continue), true))
            }
            Statement::ReturnStatement(t) => {
                if !self.scope_ref().inner {
                    return Err(NotFoldable);
                }
                let Some(arg) = &t.argument else {
                    return Ok((Some(Stmt::Return(None)), true));
                };
                let e = self.expression(arg)?;
                Ok((Some(Stmt::Return(Some(e))), true))
            }
            Statement::IfStatement(t) => self.if_statement(t),
            _ => Err(NotFoldable),
        }
    }

    fn if_statement(&mut self, t: &IfStatement) -> Compiled<(Option<Stmt>, bool)> {
        let test = self.expression(&t.test)?;

        self.nested += 1;
        let r = self.if_branches(t, test);
        self.nested -= 1;

        r
    }

    fn if_branches(&mut self, t: &IfStatement, test: Expr) -> Compiled<(Option<Stmt>, bool)> {
        let before = self.scope_ref().set.clone();
        let (cons, cons_ends) = self.statement(&t.consequent)?;
        let after_cons = self.scope_ref().set.clone();

        self.scope().set = before.clone();
        let mut alt: Option<Stmt> = None;
        let mut alt_ends = false;
        if let Some(a) = &t.alternate {
            let (s, e) = self.statement(a)?;
            alt = s;
            alt_ends = e;
        }
        let after_alt = self.scope_ref().set.clone();

        self.scope().set = match (cons_ends, alt_ends) {
            (true, true) => before,
            (true, false) => after_alt,
            (false, true) => after_cons,
            (false, false) => after_cons
                .iter()
                .filter(|k| after_alt.contains(*k))
                .cloned()
                .collect(),
        };

        let cons = cons.unwrap_or(Stmt::Nop);

        Ok((
            Some(Stmt::If(test, Box::new(cons), alt.map(Box::new))),
            cons_ends && alt_ends,
        ))
    }

    fn variables(&mut self, t: &VariableStatement) -> Compiled<(Option<Stmt>, bool)> {
        let mut out: Vec<Stmt> = Vec::new();
        for b in &t.list {
            let BindingTarget::Identifier(id) = &b.target else {
                return Err(NotFoldable);
            };

            let name = id.name.string();
            let Some(init) = &b.initializer else {
                continue;
            };

            if !self.scope_ref().inner && self.is_row_expr(init) {
                let s = self.scope_ref();
                if self.nested > 0 || s.aliases.contains(&name) || s.set.contains(&name) {
                    return Err(NotFoldable);
                }
                self.scope().aliases.insert(name);
                continue;
            }

            if self.scope_ref().aliases.contains(&name) {
                return Err(NotFoldable);
            }

            let e = self.expression(init)?;

            let slot = self.local_slot(&name);
            self.scope().set.insert(name);
            out.push(Stmt::SetLocal(slot, e));
        }

        Ok(match out.len() {
            0 => (None, false),
            1 => (out.pop(), false),
            _ => (Some(Stmt::List(out)), false),
        })
    }

    fn is_row_expr(&self, e: &Expression) -> bool {
        is_row_of(Some(e), &self.query, &self.idx)
    }

    fn effect(&mut self, e: &Expression) -> Compiled<Stmt> {
        match e {
            Expression::AssignExpression(t) => self.assignment(t),
            Expression::UnaryExpression(t) => {
                if t.operator != token::INCREMENT && t.operator != token::DECREMENT {
                    return Err(NotFoldable);
                }
                let Expression::Identifier(id) = &t.operand else {
                    return Err(NotFoldable);
                };
                let delta = if t.operator == token::DECREMENT {
                    -1.0
                } else {
                    1.0
                };
                self.update(&id.name.string(), delta)
            }
            _ => Err(NotFoldable),
        }
    }

    fn update(&mut self, name: &str, delta: f64) -> Compiled<Stmt> {
        if self.is_reg(name) {
            let i = self.reg(name);
            return Ok(Stmt::UpdateReg(i, delta));
        }

        if self.is_local(name) && self.scope_ref().set.contains(name) {
            let slot = self.local_slot(name);
            return Ok(Stmt::UpdateLocal(slot, delta));
        }

        Err(NotFoldable)
    }

    fn assignment(&mut self, t: &goja::ast::AssignExpression) -> Compiled<Stmt> {
        let Expression::Identifier(id) = &t.left else {
            return Err(NotFoldable);
        };

        let name = id.name.string();
        if self.scope_ref().aliases.contains(&name) {
            return Err(NotFoldable);
        }

        let op: u8 = if t.operator == token::ASSIGN {
            0
        } else if t.operator == token::PLUS {
            b'+'
        } else if t.operator == token::MINUS {
            b'-'
        } else if t.operator == token::MULTIPLY {
            b'*'
        } else {
            return Err(NotFoldable);
        };

        let right = self.expression(&t.right)?;

        if self.is_reg(&name) {
            let i = self.reg(&name);
            if op == 0 || op == b'*' {
                self.reg_assign = true;
            }
            return Ok(Stmt::AssignReg(i, op, right));
        }

        if self.is_local(&name) {
            if op != 0 && !self.scope_ref().set.contains(&name) {
                return Err(NotFoldable);
            }
            let slot = self.local_slot(&name);
            self.scope().set.insert(name);
            return Ok(Stmt::AssignLocal(slot, op, right));
        }

        Err(NotFoldable)
    }

    fn row_field(&self, field: &str) -> Compiled<Expr> {
        if field.is_empty() || object_member(field) {
            return Err(NotFoldable);
        }

        let Some(col) = self.fields.get(field) else {
            return Err(NotFoldable);
        };

        Ok(Expr::Row(*col))
    }

    fn is_row_ref(&self, e: &Expression) -> bool {
        if self.is_row_expr(e) {
            return true;
        }

        let s = self.scope_ref();
        matches!(e, Expression::Identifier(id) if !s.inner && s.aliases.contains(&id.name.string()))
    }

    fn expression(&mut self, e: &Expression) -> Compiled<Expr> {
        match e {
            Expression::NumberLiteral(t) => {
                let n = match &t.value {
                    NumberValue::Int64(v) => *v as f64,
                    NumberValue::Float64(v) => *v,
                    _ => return Err(NotFoldable),
                };
                Ok(Expr::Const(num_val(n)))
            }
            Expression::StringLiteral(t) => {
                let i = self.lit(&t.value.string());
                Ok(Expr::Lit(i))
            }
            Expression::BooleanLiteral(t) => Ok(Expr::Const(bool_val(t.value))),
            Expression::NullLiteral(_) => Ok(Expr::Const(NULL_VAL)),
            Expression::Identifier(t) => self.identifier(&t.name.string()),
            Expression::DotExpression(t) => self.member(&t.left, &t.identifier.name.string()),
            Expression::BracketExpression(t) => self.bracket(t),
            Expression::UnaryExpression(t) => self.unary(t),
            Expression::BinaryExpression(t) => self.binary(t),
            Expression::ConditionalExpression(t) => {
                let test = self.expression(&t.test)?;
                let cons = self.expression(&t.consequent)?;
                let alt = self.expression(&t.alternate)?;
                Ok(Expr::Cond(Box::new(test), Box::new(cons), Box::new(alt)))
            }
            Expression::CallExpression(t) => self.call(t),
            _ => Err(NotFoldable),
        }
    }

    fn identifier(&mut self, name: &str) -> Compiled<Expr> {
        let s = self.scope_ref();
        if s.aliases.contains(name) || (!s.inner && (name == self.query || name == self.idx)) {
            return Err(NotFoldable);
        }

        if self.is_local(name) {
            if !self.scope_ref().set.contains(name) {
                return Err(NotFoldable);
            }
            let slot = self.local_slot(name);
            return Ok(Expr::Local(slot));
        }

        if self.is_reg(name) {
            let i = self.reg(name);
            self.reg_read = true;
            return Ok(Expr::Reg(i));
        }

        if self.is_free(name) {
            let i = self.free(name);
            return Ok(Expr::Free(i));
        }

        if name == "undefined" {
            return Ok(Expr::Const(UNDEF_VAL));
        }

        Err(NotFoldable)
    }

    fn entity_field(&self, left: &Expression) -> bool {
        let Expression::DotExpression(dot) = left else {
            return false;
        };
        if dot.identifier.name != "entity" {
            return false;
        }

        matches!(&dot.left, Expression::Identifier(id) if id.name.string() == self.a.param)
    }

    fn member(&mut self, left: &Expression, name: &str) -> Compiled<Expr> {
        if self.is_row_ref(left) {
            return self.row_field(name);
        }

        if self.entity_field(left) && !self.scope_ref().inner {
            if object_member(name) || !ascii_only(name) {
                return Err(NotFoldable);
            }
            let i = self.free(&format!("{ENTITY_FREE_LABEL}{name}"));
            return Ok(Expr::Free(i));
        }

        if name != "length" {
            return Err(NotFoldable);
        }

        let e = self.expression(left)?;

        Ok(Expr::Length(Box::new(e)))
    }

    fn bracket(&mut self, t: &goja::ast::BracketExpression) -> Compiled<Expr> {
        if let Expression::StringLiteral(s) = &t.member {
            return self.member(&t.left, &s.value.string());
        }

        let Expression::NumberLiteral(n) = &t.member else {
            return Err(NotFoldable);
        };

        let NumberValue::Int64(idx) = n.value else {
            return Err(NotFoldable);
        };

        let e = self.expression(&t.left)?;

        Ok(Expr::Element(Box::new(e), idx))
    }

    fn unary(&mut self, t: &goja::ast::UnaryExpression) -> Compiled<Expr> {
        if t.operator == token::INCREMENT || t.operator == token::DECREMENT {
            return Err(NotFoldable);
        }

        let e = Box::new(self.expression(&t.operand)?);

        if t.operator == token::NOT {
            return Ok(Expr::Not(e));
        }
        if t.operator == token::MINUS {
            return Ok(Expr::Neg(e));
        }
        if t.operator == token::PLUS {
            return Ok(Expr::Plus(e));
        }
        if t.operator == token::TYPEOF {
            return Ok(Expr::TypeOf(e));
        }

        Err(NotFoldable)
    }

    fn binary(&mut self, t: &goja::ast::BinaryExpression) -> Compiled<Expr> {
        if t.operator == token::LOGICAL_AND || t.operator == token::LOGICAL_OR {
            return self.logical(t);
        }

        let l = Box::new(self.expression(&t.left)?);
        let r = Box::new(self.expression(&t.right)?);

        let op = t.operator;
        Ok(if op == token::PLUS {
            Expr::Arith(b'+', l, r)
        } else if op == token::MINUS {
            Expr::Arith(b'-', l, r)
        } else if op == token::MULTIPLY {
            Expr::Arith(b'*', l, r)
        } else if op == token::SLASH {
            Expr::Arith(b'/', l, r)
        } else if op == token::REMAINDER {
            Expr::Arith(b'%', l, r)
        } else if op == token::STRICT_EQUAL {
            Expr::StrictEq(false, l, r)
        } else if op == token::STRICT_NOT_EQUAL {
            Expr::StrictEq(true, l, r)
        } else if op == token::EQUAL {
            Expr::LooseEq(false, l, r)
        } else if op == token::NOT_EQUAL {
            Expr::LooseEq(true, l, r)
        } else if op == token::LESS {
            Expr::Less(true, false, l, r)
        } else if op == token::LESS_OR_EQUAL {
            Expr::Less(false, false, l, r)
        } else if op == token::GREATER {
            Expr::Less(true, true, l, r)
        } else if op == token::GREATER_OR_EQUAL {
            Expr::Less(false, true, l, r)
        } else {
            return Err(NotFoldable);
        })
    }

    fn logical(&mut self, t: &goja::ast::BinaryExpression) -> Compiled<Expr> {
        let l = Box::new(self.expression(&t.left)?);

        let before = self.scope_ref().set.clone();
        let r = Box::new(self.expression(&t.right)?);
        self.scope().set = before;

        if t.operator == token::LOGICAL_AND {
            return Ok(Expr::And(l, r));
        }

        Ok(Expr::Or(l, r))
    }

    fn call(&mut self, t: &CallExpression) -> Compiled<Expr> {
        match &t.callee {
            Expression::Identifier(callee) => {
                let name = callee.name.string();
                if self.is_local(&name) || self.outer.contains(&name) {
                    return Err(NotFoldable);
                }
                if let Some(f) = self.a.helpers.get(&name).copied() {
                    return self.helper_call(f, &t.argument_list);
                }
                if name == "parseInt" && t.argument_list.len() == 2 {
                    let Expression::NumberLiteral(radix) = &t.argument_list[1] else {
                        return Err(NotFoldable);
                    };
                    if radix.value != NumberValue::Int64(10) {
                        return Err(NotFoldable);
                    }
                    let e = self.expression(&t.argument_list[0])?;
                    return Ok(Expr::ParseInt(Box::new(e)));
                }
            }
            Expression::DotExpression(callee) => {
                if callee.identifier.name == "split" && t.argument_list.len() == 1 {
                    let Expression::StringLiteral(sep) = &t.argument_list[0] else {
                        return Err(NotFoldable);
                    };
                    let s = sep.value.string();
                    if s.is_empty() {
                        return Err(NotFoldable);
                    }
                    let e = self.expression(&callee.left)?;
                    return Ok(Expr::Split(Box::new(e), s));
                }
            }
            _ => {}
        }

        Err(NotFoldable)
    }

    fn helper_call(&mut self, f: &FunctionLiteral, args: &[Expression]) -> Compiled<Expr> {
        let Some(fname) = &f.name else {
            return Err(NotFoldable);
        };
        if self.depth >= MAX_HELPER_DEPTH
            || fname.name.string() == self.a.fn_name
            || f.parameter_list.rest.is_some()
        {
            return Err(NotFoldable);
        }

        let params = &f.parameter_list.list;
        if args.len() != params.len() {
            return Err(NotFoldable);
        }

        let mut compiled: Vec<Expr> = Vec::with_capacity(args.len());
        for a in args {
            compiled.push(self.expression(a)?);
        }

        self.scopes.push(FoldScope {
            inner: true,
            ..FoldScope::default()
        });
        self.depth += 1;
        let r = self.helper_body(f, compiled);
        self.scopes.pop();
        self.depth -= 1;

        r
    }

    fn helper_body(&mut self, f: &FunctionLiteral, compiled: Vec<Expr>) -> Compiled<Expr> {
        let mut slots: Vec<usize> = Vec::with_capacity(compiled.len());
        for p in &f.parameter_list.list {
            let BindingTarget::Identifier(id) = &p.target else {
                return Err(NotFoldable);
            };
            if p.initializer.is_some() {
                return Err(NotFoldable);
            }
            let name = id.name.string();
            slots.push(self.local_slot(&name));
            self.scope().set.insert(name);
        }

        let mut names: Vec<String> = Vec::new();
        walk_nodes(Node::BlockStatement(&f.body), &mut |n| {
            if let Node::VariableStatement(vs) = n {
                for b in &vs.list {
                    if let BindingTarget::Identifier(id) = &b.target {
                        names.push(id.name.string());
                    }
                }
            }
            true
        });
        for name in names {
            self.local_slot(&name);
        }

        let (body, _) = self.stmt_list(&f.body.list)?;

        Ok(Expr::Helper(
            slots.into_iter().zip(compiled).collect(),
            Box::new(body),
        ))
    }
}
