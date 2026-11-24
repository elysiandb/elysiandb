use crate::ast;
use crate::compiler::{Block, BlockId, BlockType, CResult, Compiler, TargetRef, go_bytes};
use crate::compiler_expr::{CompiledExpr, ExprKind, FuncType, PatternRef};
use crate::file::Idx;
use crate::token;
use crate::unistring;
use crate::vm::{EnterBlock, Instruction, Try};

pub(crate) fn statement_go_type(v: &ast::Statement) -> &'static str {
    use ast::Statement as S;
    match v {
        S::BadStatement(_) => "*ast.BadStatement",
        S::BlockStatement(_) => "*ast.BlockStatement",
        S::BranchStatement(_) => "*ast.BranchStatement",
        S::ClassDeclaration(_) => "*ast.ClassDeclaration",
        S::DebuggerStatement(_) => "*ast.DebuggerStatement",
        S::DoWhileStatement(_) => "*ast.DoWhileStatement",
        S::EmptyStatement(_) => "*ast.EmptyStatement",
        S::ExpressionStatement(_) => "*ast.ExpressionStatement",
        S::ForInStatement(_) => "*ast.ForInStatement",
        S::ForOfStatement(_) => "*ast.ForOfStatement",
        S::ForStatement(_) => "*ast.ForStatement",
        S::FunctionDeclaration(_) => "*ast.FunctionDeclaration",
        S::IfStatement(_) => "*ast.IfStatement",
        S::LabelledStatement(_) => "*ast.LabelledStatement",
        S::LexicalDeclaration(_) => "*ast.LexicalDeclaration",
        S::ReturnStatement(_) => "*ast.ReturnStatement",
        S::SwitchStatement(_) => "*ast.SwitchStatement",
        S::ThrowStatement(_) => "*ast.ThrowStatement",
        S::TryStatement(_) => "*ast.TryStatement",
        S::VariableStatement(_) => "*ast.VariableStatement",
        S::WhileStatement(_) => "*ast.WhileStatement",
        S::WithStatement(_) => "*ast.WithStatement",
    }
}

impl Compiler {
    pub(crate) fn compile_statement(
        &mut self,
        v: &ast::Statement,
        need_result: bool,
    ) -> CResult<()> {
        use ast::Statement as S;
        match v {
            S::BlockStatement(v) => self.compile_block_statement(v, need_result),
            S::ExpressionStatement(v) => self.compile_expression_statement(v, need_result),
            S::VariableStatement(v) => self.compile_variable_statement(v),
            S::LexicalDeclaration(v) => self.compile_lexical_declaration(v),
            S::ReturnStatement(v) => self.compile_return_parts(v.return_, v.argument.as_ref()),
            S::IfStatement(v) => self.compile_if_statement(v, need_result),
            S::DoWhileStatement(v) => self.compile_labeled_do_while_statement(
                v,
                need_result,
                unistring::String::default(),
            ),
            S::ForStatement(v) => {
                self.compile_labeled_for_statement(v, need_result, unistring::String::default())
            }
            S::ForInStatement(v) => self.compile_labeled_for_in_of_statement(
                &v.into,
                &v.source,
                &v.body,
                false,
                need_result,
                unistring::String::default(),
            ),
            S::ForOfStatement(v) => self.compile_labeled_for_in_of_statement(
                &v.into,
                &v.source,
                &v.body,
                true,
                need_result,
                unistring::String::default(),
            ),
            S::WhileStatement(v) => {
                self.compile_labeled_while_statement(v, need_result, unistring::String::default())
            }
            S::BranchStatement(v) => self.compile_branch_statement(v),
            S::TryStatement(v) => self.compile_try_statement(v, need_result),
            S::ThrowStatement(v) => {
                self.compile_expression(&v.argument)?
                    .emit_getter(self, true)?;
                self.add_src_map_node(v.throw);
                self.emit(Instruction::Throw);
                Ok(())
            }
            S::SwitchStatement(v) => self.compile_switch_statement(v, need_result),
            S::LabelledStatement(v) => self.compile_labeled_statement(v, need_result),
            S::EmptyStatement(_) => {
                if need_result {
                    self.emit(Instruction::ClearResult);
                }
                Ok(())
            }
            S::FunctionDeclaration(v) => self.compile_standalone_function_decl(v),
            S::ClassDeclaration(v) => {
                let name = v.class.name.as_ref().expect("class name");
                let e = self.compile_class_literal_expr(&v.class)?;
                self.emit_lexical_assign(&name.name, v.class.class as isize - 1, Some(&e))
            }
            S::WithStatement(v) => self.compile_with_statement(v, need_result),
            S::DebuggerStatement(_) => Ok(()),
            S::BadStatement(b) => {
                self.assert(
                    false,
                    b.from as isize - 1,
                    format!("Unknown statement type: {}", statement_go_type(v)),
                )?;
                unreachable!()
            }
        }
    }

    fn compile_labeled_statement(
        &mut self,
        v: &ast::LabelledStatement,
        need_result: bool,
    ) -> CResult<()> {
        let label = v.label.name.clone();
        if self.strict() {
            self.check_identifier_name(&label, v.label.idx as isize - 1)?;
        }
        let mut b = self.block;
        while let Some(bb) = b {
            if self.blocks[bb].label == label {
                let mut m = b"Label '".to_vec();
                m.extend_from_slice(&go_bytes(&label));
                m.extend_from_slice(b"' has already been declared");
                return self.throw_syntax_error(v.label.idx as isize - 1, m);
            }
            b = self.blocks[bb].outer;
        }
        use ast::Statement as S;
        match &v.statement {
            S::ForInStatement(s) => self.compile_labeled_for_in_of_statement(
                &s.into,
                &s.source,
                &s.body,
                false,
                need_result,
                label,
            ),
            S::ForOfStatement(s) => self.compile_labeled_for_in_of_statement(
                &s.into,
                &s.source,
                &s.body,
                true,
                need_result,
                label,
            ),
            S::ForStatement(s) => self.compile_labeled_for_statement(s, need_result, label),
            S::WhileStatement(s) => self.compile_labeled_while_statement(s, need_result, label),
            S::DoWhileStatement(s) => {
                self.compile_labeled_do_while_statement(s, need_result, label)
            }
            s => self.compile_generic_labeled_statement(s, need_result, label),
        }
    }

    fn compile_try_statement(&mut self, v: &ast::TryStatement, need_result: bool) -> CResult<()> {
        let ob = self.block;
        self.push_block(Block::new(BlockType::Try, ob));
        let mut lp: isize = 0;
        let mut body_need_result = false;
        let mut finally_breaking: Option<BlockId> = None;
        if let Some(f) = &v.finally {
            let (l, fb) = self.scan_statements(&f.list)?;
            lp = l;
            finally_breaking = fb;
        }
        if let Some(fb) = finally_breaking {
            self.blk_mut().breaking = Some(fb);
            if lp == -1 {
                body_need_result = self.blocks[fb].need_result;
            }
        } else {
            body_need_result = need_result;
        }
        let lbl = self.code_len();
        self.emit(Instruction::Placeholder);
        if need_result {
            self.emit(Instruction::ClearResult);
        }
        self.compile_block_statement(&v.body, body_need_result)?;
        let mut catch_offset = 0usize;
        if let Some(catch) = &v.catch {
            let lbl2 = self.code_len();
            self.emit(Instruction::Placeholder);
            catch_offset = self.code_len() - lbl;
            if let Some(param) = &catch.parameter {
                let ob = self.block;
                self.push_block(Block::new(BlockType::Scope, ob));
                self.new_block_scope();
                let list = &catch.body.list;
                let funcs = self.extract_functions(list);
                let is_pattern = matches!(
                    param,
                    ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_)
                );
                if is_pattern {
                    let s = self.cur_scope();
                    self.add_binding(s, catch.catch as isize - 1)?;
                }
                self.create_bindings(TargetRef::of_target(param), &mut |c, name, offset| {
                    if c.strict() && (*name == "arguments" || *name == "eval") {
                        return c.throw_syntax_error(
                            offset,
                            "Catch variable may not be eval or arguments in strict mode",
                        );
                    }
                    let s = c.cur_scope();
                    c.bind_name_lexical(s, name, true, offset)?;
                    Ok(())
                })?;
                let enter = self.code_len();
                self.emit(Instruction::EnterBlock(Box::default()));
                if is_pattern {
                    let b0 = self.sc().bindings[0];
                    self.b_emit_get(b0);
                    self.emit_pattern(
                        PatternRef::of_target(param),
                        &|c, target, init| c.emit_pattern_lexical_assign(&target, &init),
                        false,
                    )?;
                }
                for decl in &funcs {
                    let name = decl.function.name.as_ref().expect("function name");
                    let s = self.cur_scope();
                    self.bind_name_lexical(s, &name.name, true, name.idx1() as isize - 1)?;
                }
                self.compile_lexical_declarations(list, true)?;
                self.compile_functions(&funcs)?;
                self.compile_statements(list, body_need_result)?;
                self.leave_scope_block(enter)?;
                let b0 = self.sc().bindings[0];
                let e = match &self.progs[self.p].code[enter] {
                    Instruction::EnterBlock(e) => (**e).clone(),
                    _ => EnterBlock::default(),
                };
                if self.sc().dyn_lookup || self.bindings[b0].in_stash {
                    self.set_code(
                        lbl + catch_offset,
                        Instruction::EnterCatchBlock(Box::new(e)),
                    );
                } else {
                    let mut e = e;
                    e.stack_size = e.stack_size.wrapping_sub(1);
                    self.set_code(enter, Instruction::EnterBlock(Box::new(e)));
                }
                self.pop_scope();
            } else {
                self.emit(Instruction::Pop);
                self.compile_block_statement(&catch.body, body_need_result)?;
            }
            let d = self.code_len() - lbl2;
            self.set_code(lbl2, Instruction::Jump(d as i32));
        }
        let mut finally_offset = 0usize;
        if let Some(f) = &v.finally {
            self.emit(Instruction::EnterFinally);
            finally_offset = self.code_len() - lbl;
            if body_need_result && finally_breaking.is_some() && lp == -1 {
                self.emit(Instruction::ClearResult);
            }
            self.compile_block_statement(f, false)?;
            self.emit(Instruction::LeaveFinally);
        } else {
            self.emit(Instruction::LeaveTry);
        }
        self.set_code(
            lbl,
            Instruction::Try(Try {
                catch_offset: catch_offset as i32,
                finally_offset: finally_offset as i32,
            }),
        );
        self.leave_block();
        Ok(())
    }

    fn compile_labeled_do_while_statement(
        &mut self,
        v: &ast::DoWhileStatement,
        need_result: bool,
        label: unistring::String,
    ) -> CResult<()> {
        let ob = self.block;
        let mut b = Block::new(BlockType::Loop, ob);
        b.label = label;
        b.need_result = need_result;
        self.push_block(b);
        let start = self.code_len();
        self.compile_statement(&v.body, need_result)?;
        let cont = self.code_len() as isize;
        self.blk_mut().cont = cont;
        let e = self.compile_expression(&v.test)?;
        self.emit_expr(&e, true)?;
        let d = start as isize - self.code_len() as isize;
        self.emit(Instruction::JeqP(d as i32));
        self.leave_block();
        Ok(())
    }

    fn compile_for_head_lex_decl(
        &mut self,
        decl: &ast::LexicalDeclaration,
        need_result: bool,
    ) -> CResult<usize> {
        let ob = self.block;
        let mut b = Block::new(BlockType::IterScope, ob);
        b.need_result = need_result;
        self.push_block(b);
        self.new_block_scope();
        let enter_iter_block = self.code_len();
        self.emit(Instruction::EnterBlock(Box::default()));
        self.create_lexical_bindings(decl)?;
        self.compile_lexical_declaration(decl)?;
        Ok(enter_iter_block)
    }

    fn compile_labeled_for_statement(
        &mut self,
        v: &ast::ForStatement,
        need_result: bool,
        label: unistring::String,
    ) -> CResult<()> {
        let ob = self.block;
        let mut lb = Block::new(BlockType::Loop, ob);
        lb.label = label;
        lb.need_result = need_result;
        let loop_block = self.push_block(lb);

        let mut enter_iter_block = None;
        match &v.initializer {
            None => {}
            Some(ast::ForLoopInitializer::ForLoopInitializerLexicalDecl(init)) => {
                enter_iter_block =
                    Some(self.compile_for_head_lex_decl(&init.lexical_declaration, need_result)?);
            }
            Some(ast::ForLoopInitializer::ForLoopInitializerVarDeclList(init)) => {
                for expr in &init.list {
                    self.compile_var_binding(expr)?;
                }
            }
            Some(ast::ForLoopInitializer::ForLoopInitializerExpression(init)) => {
                self.compile_expression(&init.expression)?
                    .emit_getter(self, false)?;
            }
        }

        if need_result {
            self.emit(Instruction::ClearResult);
        }

        if enter_iter_block.is_some() {
            self.emit(Instruction::Jump(1));
        }

        let start = self.code_len();
        let mut j = 0usize;
        let mut test_const = false;
        let mut skip_to_end = false;
        if let Some(test) = &v.test {
            let expr = self.compile_expression(test)?;
            if expr.constant(self)? {
                match self.eval_const(&expr)? {
                    Ok(r) => {
                        if r.to_boolean() {
                            test_const = true;
                        } else {
                            let saved = self.enter_dummy_mode();
                            let r = self.compile_statement(&v.body, false).and_then(|_| {
                                if let Some(u) = &v.update {
                                    self.compile_expression(u)?.emit_getter(self, false)?;
                                }
                                Ok(())
                            });
                            self.leave_dummy_mode(saved);
                            r?;
                            skip_to_end = true;
                        }
                    }
                    Err(ex) => {
                        expr.add_src_map(self);
                        self.emit_throw(&ex)?;
                        skip_to_end = true;
                    }
                }
            } else {
                expr.emit_getter(self, true)?;
                j = self.code_len();
                self.emit(Instruction::Placeholder);
            }
        }
        if !skip_to_end {
            if need_result {
                self.emit(Instruction::ClearResult);
            }
            self.compile_statement(&v.body, need_result)?;
            let cont = self.code_len() as isize;
            self.blocks[loop_block].cont = cont;
            if enter_iter_block.is_some() {
                self.emit(Instruction::Jump(1));
            }
            if let Some(u) = &v.update {
                self.compile_expression(u)?.emit_getter(self, false)?;
            }
            if enter_iter_block.is_some() {
                let s = self.cur_scope();
                if self.scopes[s].need_stash || self.is_dynamic(s) {
                    self.set_code(start - 1, Instruction::CopyStash);
                    let cont = self.blocks[loop_block].cont as usize;
                    self.set_code(cont, Instruction::CopyStash);
                } else {
                    let l = self.code_len();
                    if l as isize > self.blocks[loop_block].cont {
                        self.blocks[loop_block].cont += 1;
                    } else {
                        self.code_mut().truncate(l - 1);
                    }
                }
            }
            let d = start as isize - self.code_len() as isize;
            self.emit(Instruction::Jump(d as i32));
            if v.test.is_some() && !test_const {
                let d = self.code_len() - j;
                self.set_code(j, Instruction::JneP(d as i32));
            }
        }
        if let Some(e) = enter_iter_block {
            self.leave_scope_block(e)?;
            self.pop_scope();
        }
        self.leave_block();
        Ok(())
    }

    fn compile_for_into(
        &mut self,
        into: &ast::ForInto,
        need_result: bool,
    ) -> CResult<Option<usize>> {
        let mut enter = None;
        match into {
            ast::ForInto::ForIntoExpression(into) => {
                let enum_get = CompiledExpr {
                    offset: -1,
                    kind: ExprKind::EnumGet,
                };
                self.compile_expression(&into.expression)?
                    .emit_setter(self, &enum_get, false)?;
            }
            ast::ForInto::ForIntoVar(into) => {
                if self.strict()
                    && let Some(init) = &into.binding.initializer
                {
                    return self.throw_syntax_error(
                        init.idx0() as isize - 1,
                        "for-in loop variable declaration may not have an initializer.",
                    );
                }
                match &into.binding.target {
                    ast::BindingTarget::Identifier(target) => {
                        let enum_get = CompiledExpr {
                            offset: -1,
                            kind: ExprKind::EnumGet,
                        };
                        self.compile_identifier_expression(target)?
                            .emit_setter(self, &enum_get, false)?;
                    }
                    ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_) => {
                        self.emit(Instruction::EnumGet);
                        self.emit_pattern(
                            PatternRef::of_target(&into.binding.target),
                            &|c, t, i| c.emit_pattern_var_assign(&t, &i),
                            false,
                        )?;
                    }
                    ast::BindingTarget::BadExpression(b) => {
                        return self.throw_syntax_error(
                            b.from as isize - 1,
                            "unsupported for-in var target: *ast.BadExpression",
                        );
                    }
                }
            }
            ast::ForInto::ForDeclaration(into) => {
                let ob = self.block;
                let mut b = Block::new(BlockType::IterScope, ob);
                b.need_result = need_result;
                self.push_block(b);
                self.new_block_scope();
                let e = self.code_len();
                enter = Some(e);
                self.emit(Instruction::EnterBlock(Box::default()));
                match &into.target {
                    ast::BindingTarget::Identifier(target) => {
                        let b = self.create_lexical_id_binding(
                            &target.name,
                            into.is_const,
                            into.idx as isize - 1,
                        )?;
                        self.emit(Instruction::EnumGet);
                        self.b_emit_init_p(b);
                    }
                    ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_) => {
                        self.create_lexical_binding(
                            TargetRef::of_target(&into.target),
                            into.is_const,
                        )?;
                        self.emit(Instruction::EnumGet);
                        self.emit_pattern(
                            PatternRef::of_target(&into.target),
                            &|c, t, i| c.emit_pattern_lexical_assign(&t, &i),
                            false,
                        )?;
                    }
                    ast::BindingTarget::BadExpression(_) => {
                        self.assert(
                            false,
                            into.idx as isize - 1,
                            "Unsupported ForBinding: *ast.BadExpression",
                        )?;
                    }
                }
            }
        }
        Ok(enter)
    }

    fn compile_labeled_for_in_of_statement(
        &mut self,
        into: &ast::ForInto,
        source: &ast::Expression,
        body: &ast::Statement,
        iter: bool,
        need_result: bool,
        label: unistring::String,
    ) -> CResult<()> {
        let ob = self.block;
        let mut lb = Block::new(BlockType::LoopEnum, ob);
        lb.label = label;
        lb.need_result = need_result;
        self.push_block(lb);
        let mut enter_pos: Option<usize> = None;
        if let ast::ForInto::ForDeclaration(for_decl) = into {
            let ob = self.block;
            self.push_block(Block::new(BlockType::Scope, ob));
            self.new_block_scope();
            enter_pos = Some(self.code_len());
            self.emit(Instruction::Jump(1));
            self.create_lexical_binding(TargetRef::of_target(&for_decl.target), for_decl.is_const)?;
        }
        self.compile_expression(source)?.emit_getter(self, true)?;
        if let Some(enter_pos) = enter_pos {
            let s = self.cur_scope();
            let mut used = !self.blk().breaks.is_empty() || self.is_dynamic(s);
            if !used {
                for &b in &self.scopes[s].bindings {
                    if self.b_use_count(b) > 0 {
                        used = true;
                        break;
                    }
                }
            }
            if used {
                let bs = self.scopes[s].bindings.clone();
                for b in bs {
                    self.b_move_to_stash(b);
                }
                self.set_code(enter_pos, Instruction::EnterBlock(Box::default()));
                self.leave_scope_block(enter_pos)?;
            } else {
                let b = self.block.expect("compiler block");
                self.block = self.blocks[b].outer;
            }
            self.pop_scope();
        }
        if iter {
            self.emit(Instruction::IterateP);
        } else {
            self.emit(Instruction::Enumerate);
        }
        if need_result {
            self.emit(Instruction::ClearResult);
        }
        let start = self.code_len();
        self.blk_mut().cont = start as isize;
        self.emit(Instruction::Placeholder);
        let enter_iter_block = self.compile_for_into(into, need_result)?;
        if need_result {
            self.emit(Instruction::ClearResult);
        }
        self.compile_statement(body, need_result)?;
        if let Some(e) = enter_iter_block {
            self.leave_scope_block(e)?;
            self.pop_scope();
        }
        let d = start as isize - self.code_len() as isize;
        self.emit(Instruction::Jump(d as i32));
        let d = (self.code_len() - start) as i32;
        if iter {
            self.set_code(start, Instruction::IterNext(d));
        } else {
            self.set_code(start, Instruction::EnumNext(d));
        }
        self.emit(Instruction::EnumPop);
        self.emit(Instruction::Jump(2));
        self.leave_block();
        self.emit(Instruction::EnumPopClose);
        Ok(())
    }

    fn compile_labeled_while_statement(
        &mut self,
        v: &ast::WhileStatement,
        need_result: bool,
        label: unistring::String,
    ) -> CResult<()> {
        let ob = self.block;
        let mut b = Block::new(BlockType::Loop, ob);
        b.label = label;
        b.need_result = need_result;
        self.push_block(b);

        if need_result {
            self.emit(Instruction::ClearResult);
        }
        let start = self.code_len();
        self.blk_mut().cont = start as isize;
        let expr = self.compile_expression(&v.test)?;
        let mut test_true = false;
        let mut j = 0usize;
        if expr.constant(self)? {
            match self.eval_const(&expr)? {
                Ok(t) => {
                    if t.to_boolean() {
                        test_true = true;
                    } else {
                        self.compile_statement_dummy(&v.body)?;
                        self.leave_block();
                        return Ok(());
                    }
                }
                Err(ex) => {
                    self.emit_throw(&ex)?;
                    self.leave_block();
                    return Ok(());
                }
            }
        } else {
            expr.emit_getter(self, true)?;
            j = self.code_len();
            self.emit(Instruction::Placeholder);
        }
        if need_result {
            self.emit(Instruction::ClearResult);
        }
        self.compile_statement(&v.body, need_result)?;
        let d = start as isize - self.code_len() as isize;
        self.emit(Instruction::Jump(d as i32));
        if !test_true {
            let d = self.code_len() - j;
            self.set_code(j, Instruction::JneP(d as i32));
        }
        self.leave_block();
        Ok(())
    }

    fn compile_branch_statement(&mut self, v: &ast::BranchStatement) -> CResult<()> {
        match v.token {
            token::BREAK => self.compile_break(v.label.as_deref(), v.idx),
            token::CONTINUE => self.compile_continue(v.label.as_deref(), v.idx),
            _ => self.assert(
                false,
                v.idx as isize - 1,
                format!("Unknown branch statement token: {}", v.token.string()),
            ),
        }
    }

    fn find_branch_block(&self, st: &ast::BranchStatement) -> CResult<Option<BlockId>> {
        match st.token {
            token::BREAK => self.find_break_block(st.label.as_deref(), true),
            token::CONTINUE => self.find_break_block(st.label.as_deref(), false),
            _ => Ok(None),
        }
    }

    fn find_break_block(
        &self,
        label: Option<&ast::Identifier>,
        is_break: bool,
    ) -> CResult<Option<BlockId>> {
        let mut res = None;
        if let Some(label) = label {
            let mut found = None;
            let mut b = self.block;
            while let Some(bb) = b {
                if res.is_none()
                    && let Some(brk) = self.blocks[bb].breaking
                {
                    res = Some(brk);
                    if is_break {
                        return Ok(res);
                    }
                }
                if self.blocks[bb].label == label.name {
                    found = Some(bb);
                    break;
                }
                b = self.blocks[bb].outer;
            }
            if !is_break && let Some(f) = found {
                let t = self.blocks[f].typ;
                if t != BlockType::Loop && t != BlockType::LoopEnum {
                    let mut m = b"Illegal continue statement: '".to_vec();
                    m.extend_from_slice(&go_bytes(&label.name));
                    m.extend_from_slice(b"' does not denote an iteration statement");
                    return self.throw_syntax_error(label.idx as isize - 1, m);
                }
            }
            if res.is_none() {
                res = found;
            }
        } else {
            let mut b = self.block;
            while let Some(bb) = b {
                if let Some(brk) = self.blocks[bb].breaking {
                    return Ok(Some(brk));
                }
                match self.blocks[bb].typ {
                    BlockType::Loop | BlockType::LoopEnum => {
                        res = Some(bb);
                        break;
                    }
                    BlockType::Switch => {
                        if is_break {
                            res = Some(bb);
                            break;
                        }
                    }
                    _ => {}
                }
                b = self.blocks[bb].outer;
            }
        }
        Ok(res)
    }

    fn emit_block_exit_code(
        &mut self,
        label: Option<&ast::Identifier>,
        idx: Idx,
        is_break: bool,
    ) -> CResult<BlockId> {
        let Some(block) = self.find_break_block(label, is_break)? else {
            return self.throw_syntax_error(idx as isize - 1, "Could not find block");
        };
        let cont_for_loop = !is_break && self.blocks[block].typ == BlockType::Loop;
        let mut b = self.block;
        while let Some(bb) = b {
            if bb == block {
                break;
            }
            match self.blocks[bb].typ {
                BlockType::IterScope | BlockType::Scope => {
                    if self.blocks[bb].typ == BlockType::IterScope
                        && cont_for_loop
                        && self.blocks[bb].outer == Some(block)
                    {
                        break;
                    }
                    let pc = self.code_len();
                    self.blocks[bb].breaks.push(pc);
                    self.emit(Instruction::Placeholder);
                }
                BlockType::Try => self.emit(Instruction::LeaveTry),
                BlockType::With => self.emit(Instruction::LeaveWith),
                BlockType::LoopEnum => self.emit(Instruction::EnumPopClose),
                _ => {}
            }
            b = self.blocks[bb].outer;
        }
        Ok(block)
    }

    fn compile_break(&mut self, label: Option<&ast::Identifier>, idx: Idx) -> CResult<()> {
        let block = self.emit_block_exit_code(label, idx, true)?;
        let pc = self.code_len();
        self.blocks[block].breaks.push(pc);
        self.emit(Instruction::Placeholder);
        Ok(())
    }

    fn compile_continue(&mut self, label: Option<&ast::Identifier>, idx: Idx) -> CResult<()> {
        let block = self.emit_block_exit_code(label, idx, false)?;
        let pc = self.code_len();
        self.blocks[block].conts.push(pc);
        self.emit(Instruction::Placeholder);
        Ok(())
    }

    fn compile_if_body(&mut self, s: &ast::Statement, need_result: bool) -> CResult<()> {
        if !self.strict()
            && let ast::Statement::FunctionDeclaration(f) = s
            && !f.function.async_
            && !f.function.generator
        {
            self.compile_function(f)?;
            if need_result {
                self.emit(Instruction::ClearResult);
            }
            return Ok(());
        }
        self.compile_statement(s, need_result)
    }

    fn compile_if_body_dummy(&mut self, s: &ast::Statement) -> CResult<()> {
        let saved = self.enter_dummy_mode();
        let r = self.compile_if_body(s, false);
        self.leave_dummy_mode(saved);
        r
    }

    fn compile_if_statement(&mut self, v: &ast::IfStatement, need_result: bool) -> CResult<()> {
        let test = self.compile_expression(&v.test)?;
        if need_result {
            self.emit(Instruction::ClearResult);
        }
        if test.constant(self)? {
            let r = match self.eval_const(&test)? {
                Ok(r) => r,
                Err(ex) => {
                    test.add_src_map(self);
                    return self.emit_throw(&ex);
                }
            };
            if r.to_boolean() {
                self.compile_if_body(&v.consequent, need_result)?;
                if let Some(alt) = &v.alternate {
                    self.compile_if_body_dummy(alt)?;
                }
            } else {
                self.compile_if_body_dummy(&v.consequent)?;
                if let Some(alt) = &v.alternate {
                    self.compile_if_body(alt, need_result)?;
                } else if need_result {
                    self.emit(Instruction::ClearResult);
                }
            }
            return Ok(());
        }
        test.emit_getter(self, true)?;
        let jmp = self.code_len();
        self.emit(Instruction::Placeholder);
        self.compile_if_body(&v.consequent, need_result)?;
        if let Some(alt) = &v.alternate {
            let jmp1 = self.code_len();
            self.emit(Instruction::Placeholder);
            let d = self.code_len() - jmp;
            self.set_code(jmp, Instruction::JneP(d as i32));
            self.compile_if_body(alt, need_result)?;
            let d = self.code_len() - jmp1;
            self.set_code(jmp1, Instruction::Jump(d as i32));
        } else if need_result {
            self.emit(Instruction::Jump(2));
            let d = self.code_len() - jmp;
            self.set_code(jmp, Instruction::JneP(d as i32));
            self.emit(Instruction::ClearResult);
        } else {
            let d = self.code_len() - jmp;
            self.set_code(jmp, Instruction::JneP(d as i32));
        }
        Ok(())
    }

    pub(crate) fn compile_return_parts(
        &mut self,
        return_: Idx,
        argument: Option<&ast::Expression>,
    ) -> CResult<()> {
        if let Some(s) = self.nearest_function(self.cur_scope())
            && self.scopes[s].func_type == FuncType::ClsInit
        {
            return self.throw_syntax_error(return_ as isize - 1, "Illegal return statement");
        }
        match argument {
            Some(a) => {
                let e = self.compile_expression(a)?;
                self.emit_expr(&e, true)?;
            }
            None => self.emit(Instruction::LoadUndef),
        }
        let mut b = self.block;
        while let Some(bb) = b {
            match self.blocks[bb].typ {
                BlockType::Try => {
                    self.emit(Instruction::SaveResult);
                    self.emit(Instruction::LeaveTry);
                    self.emit(Instruction::LoadResult);
                }
                BlockType::LoopEnum => self.emit(Instruction::EnumPopClose),
                _ => {}
            }
            b = self.blocks[bb].outer;
        }
        if let Some(s) = self.nearest_function(self.cur_scope())
            && self.scopes[s].func_type == FuncType::DerivedCtor
        {
            let b = self.scopes[s]
                .bound_names
                .get(&unistring::String::new(crate::compiler::THIS_BINDING_NAME))
                .copied();
            self.assert(
                b.is_some(),
                return_ as isize - 1,
                "Derived constructor, but no 'this' binding",
            )?;
            self.mark_access_point(b.unwrap_or_default());
        }
        self.emit(Instruction::Ret);
        Ok(())
    }

    fn check_var_conflict(&self, name: &unistring::String, offset: isize) -> CResult<()> {
        let cur = self.cur_scope();
        let mut sc = Some(cur);
        while let Some(s) = sc {
            if let Some(&b) = self.scopes[s].bound_names.get(name) {
                let b = &self.bindings[b];
                if !(b.is_var || b.is_arg && s != cur) {
                    let mut m = b"Identifier '".to_vec();
                    m.extend_from_slice(&go_bytes(name));
                    m.extend_from_slice(b"' has already been declared");
                    return self.throw_syntax_error(offset, m);
                }
            }
            if self.is_function(s) {
                break;
            }
            sc = self.scopes[s].outer;
        }
        Ok(())
    }

    fn emit_var_assign(
        &mut self,
        name: &unistring::String,
        offset: isize,
        init: Option<&CompiledExpr<'_>>,
    ) -> CResult<()> {
        self.check_var_conflict(name, offset)?;
        if let Some(init) = init {
            let s = self.cur_scope();
            let (b, no_dyn) = self.lookup_name(s, name)?;
            if no_dyn {
                self.emit_named_or_const(init, name)?;
                self.add_src_map(offset);
                self.b_emit_init_p(b.expect("binding"));
            } else {
                self.emit_var_ref(name, offset, b)?;
                self.emit_named_or_const(init, name)?;
                self.add_src_map(offset);
                self.emit(Instruction::InitValueP);
            }
        }
        Ok(())
    }

    pub(crate) fn compile_var_binding(&mut self, expr: &ast::Binding) -> CResult<()> {
        match &expr.target {
            ast::BindingTarget::Identifier(target) => {
                let init = self.compile_expression_opt(expr.initializer.as_ref())?;
                self.emit_var_assign(&target.name, target.idx as isize - 1, init.as_ref())
            }
            ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_) => {
                self.compile_initializer_getter(expr.initializer.as_ref())?;
                self.emit_pattern(
                    PatternRef::of_target(&expr.target),
                    &|c, t, i| c.emit_pattern_var_assign(&t, &i),
                    false,
                )
            }
            ast::BindingTarget::BadExpression(b) => self.throw_syntax_error(
                b.from as isize - 1,
                "unsupported variable binding target: *ast.BadExpression",
            ),
        }
    }

    fn compile_initializer_getter(&mut self, init: Option<&ast::Expression>) -> CResult<()> {
        match init {
            Some(e) => self.compile_expression(e)?.emit_getter(self, true),
            None => panic!("runtime error: invalid memory address or nil pointer dereference"),
        }
    }

    pub(crate) fn emit_lexical_assign(
        &mut self,
        name: &unistring::String,
        offset: isize,
        init: Option<&CompiledExpr<'_>>,
    ) -> CResult<()> {
        let s = self.cur_scope();
        let b = self.scopes[s].bound_names.get(name).copied();
        self.assert(
            b.is_some(),
            offset,
            "Lexical declaration for an unbound name",
        )?;
        let b = b.unwrap_or_default();
        match init {
            Some(init) => {
                self.emit_named_or_const(init, name)?;
                self.add_src_map(offset);
            }
            None => {
                if self.bindings[b].is_const {
                    return self
                        .throw_syntax_error(offset, "Missing initializer in const declaration");
                }
                self.emit(Instruction::LoadUndef);
            }
        }
        self.b_emit_init_p(b);
        Ok(())
    }

    pub(crate) fn emit_pattern_var_assign(
        &mut self,
        target: &CompiledExpr<'_>,
        init: &CompiledExpr<'_>,
    ) -> CResult<()> {
        let ExprKind::Identifier(name) = &target.kind else {
            panic!("interface conversion: goja.compiledExpr is not *goja.compiledIdentifierExpr");
        };
        self.emit_var_assign(name, target.offset, Some(init))
    }

    pub(crate) fn emit_pattern_lexical_assign(
        &mut self,
        target: &CompiledExpr<'_>,
        init: &CompiledExpr<'_>,
    ) -> CResult<()> {
        let ExprKind::Identifier(name) = &target.kind else {
            panic!("interface conversion: goja.compiledExpr is not *goja.compiledIdentifierExpr");
        };
        self.emit_lexical_assign(name, target.offset, Some(init))
    }

    pub(crate) fn emit_pattern_assign(
        &mut self,
        target: &CompiledExpr<'_>,
        init: &CompiledExpr<'_>,
    ) -> CResult<()> {
        if let ExprKind::Identifier(name) = &target.kind {
            let s = self.cur_scope();
            let (b, no_dyn) = self.lookup_name(s, name)?;
            if no_dyn {
                self.emit_named_or_const(init, name)?;
                match b {
                    Some(b) => self.b_emit_set_p(b),
                    None => {
                        panic!("runtime error: invalid memory address or nil pointer dereference")
                    }
                }
            } else {
                self.emit_var_ref(name, target.offset, b)?;
                self.emit_named_or_const(init, name)?;
                self.emit(Instruction::PutValueP);
            }
        } else {
            target.emit_ref(self)?;
            self.emit_expr(init, true)?;
            self.emit(Instruction::PutValueP);
        }
        Ok(())
    }

    fn compile_lexical_binding(&mut self, expr: &ast::Binding) -> CResult<()> {
        match &expr.target {
            ast::BindingTarget::Identifier(target) => {
                let init = self.compile_expression_opt(expr.initializer.as_ref())?;
                self.emit_lexical_assign(&target.name, target.idx as isize - 1, init.as_ref())
            }
            ast::BindingTarget::ObjectPattern(_) | ast::BindingTarget::ArrayPattern(_) => {
                self.compile_initializer_getter(expr.initializer.as_ref())?;
                self.emit_pattern(
                    PatternRef::of_target(&expr.target),
                    &|c, t, i| c.emit_pattern_lexical_assign(&t, &i),
                    false,
                )
            }
            ast::BindingTarget::BadExpression(b) => self.throw_syntax_error(
                b.from as isize - 1,
                "unsupported lexical binding target: *ast.BadExpression",
            ),
        }
    }

    fn compile_variable_statement(&mut self, v: &ast::VariableStatement) -> CResult<()> {
        for expr in &v.list {
            self.compile_var_binding(expr)?;
        }
        Ok(())
    }

    pub(crate) fn compile_lexical_declaration(
        &mut self,
        v: &ast::LexicalDeclaration,
    ) -> CResult<()> {
        for e in &v.list {
            self.compile_lexical_binding(e)?;
        }
        Ok(())
    }

    fn is_empty_result(st: &ast::Statement) -> bool {
        use ast::Statement as S;
        match st {
            S::EmptyStatement(_)
            | S::VariableStatement(_)
            | S::LexicalDeclaration(_)
            | S::FunctionDeclaration(_)
            | S::ClassDeclaration(_)
            | S::BranchStatement(_)
            | S::DebuggerStatement(_) => true,
            S::LabelledStatement(l) => Self::is_empty_result(&l.statement),
            S::BlockStatement(b) => {
                for s in &b.list {
                    if let S::BranchStatement(_) = s {
                        return true;
                    }
                    if !Self::is_empty_result(s) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn scan_statements(
        &self,
        list: &[ast::Statement],
    ) -> CResult<(isize, Option<BlockId>)> {
        let mut last_producing_idx: isize = -1;
        let mut breaking_block = None;
        for (i, st) in list.iter().enumerate() {
            if let ast::Statement::BranchStatement(bs) = st {
                if let Some(blk) = self.find_branch_block(bs)? {
                    breaking_block = Some(blk);
                }
                break;
            }
            if !Self::is_empty_result(st) {
                last_producing_idx = i as isize;
            }
        }
        Ok((last_producing_idx, breaking_block))
    }

    fn compile_statements_need_result(
        &mut self,
        list: &[ast::Statement],
        last_producing_idx: isize,
    ) -> CResult<()> {
        if last_producing_idx >= 0 {
            let lpi = last_producing_idx as usize;
            for st in &list[..lpi] {
                if let ast::Statement::FunctionDeclaration(_) = st {
                    continue;
                }
                self.compile_statement(st, false)?;
            }
            self.compile_statement(&list[lpi], true)?;
        }
        let mut leave = None;
        let mut result = Ok(());
        for st in &list[(last_producing_idx + 1) as usize..] {
            if let ast::Statement::FunctionDeclaration(_) = st {
                continue;
            }
            if let Err(e) = self.compile_statement(st, false) {
                result = Err(e);
                break;
            }
            if leave.is_none()
                && let ast::Statement::BranchStatement(_) = st
            {
                leave = Some(self.enter_dummy_mode());
            }
        }
        if let Some(saved) = leave {
            self.leave_dummy_mode(saved);
        }
        result
    }

    pub(crate) fn compile_statements(
        &mut self,
        list: &[ast::Statement],
        mut need_result: bool,
    ) -> CResult<()> {
        let (last_producing_idx, blk) = self.scan_statements(list)?;
        if let Some(blk) = blk {
            need_result = self.blocks[blk].need_result;
        }
        if need_result {
            return self.compile_statements_need_result(list, last_producing_idx);
        }
        for st in list {
            if let ast::Statement::FunctionDeclaration(_) = st {
                continue;
            }
            self.compile_statement(st, false)?;
        }
        Ok(())
    }

    fn compile_generic_labeled_statement(
        &mut self,
        v: &ast::Statement,
        need_result: bool,
        label: unistring::String,
    ) -> CResult<()> {
        let ob = self.block;
        let mut b = Block::new(BlockType::Label, ob);
        b.label = label;
        b.need_result = need_result;
        self.push_block(b);
        self.compile_statement(v, need_result)?;
        self.leave_block();
        Ok(())
    }

    pub(crate) fn compile_block_statement(
        &mut self,
        v: &ast::BlockStatement,
        need_result: bool,
    ) -> CResult<()> {
        let mut scope_declared = false;
        let funcs = self.extract_functions(&v.list);
        if !funcs.is_empty() {
            self.new_block_scope();
            scope_declared = true;
        }
        self.create_function_bindings(&funcs)?;
        scope_declared = self.compile_lexical_declarations(&v.list, scope_declared)?;

        let mut enter = None;
        if scope_declared {
            let ob = self.block;
            let mut b = Block::new(BlockType::Scope, ob);
            b.need_result = need_result;
            self.push_block(b);
            enter = Some(self.code_len());
            self.emit(Instruction::EnterBlock(Box::default()));
        }
        self.compile_functions(&funcs)?;
        self.compile_statements(&v.list, need_result)?;
        if let Some(e) = enter {
            self.leave_scope_block(e)?;
            self.pop_scope();
        }
        Ok(())
    }

    fn compile_expression_statement(
        &mut self,
        v: &ast::ExpressionStatement,
        need_result: bool,
    ) -> CResult<()> {
        let e = self.compile_expression(&v.expression)?;
        self.emit_expr(&e, need_result)?;
        if need_result {
            self.emit(Instruction::SaveResult);
        }
        Ok(())
    }

    fn compile_with_statement(&mut self, v: &ast::WithStatement, need_result: bool) -> CResult<()> {
        if self.strict() {
            return self.throw_syntax_error(
                v.with as isize - 1,
                "Strict mode code may not include a with statement",
            );
        }
        self.compile_expression(&v.object)?
            .emit_getter(self, true)?;
        self.emit(Instruction::EnterWith);
        let ob = self.block;
        let mut b = Block::new(BlockType::With, ob);
        b.need_result = need_result;
        self.push_block(b);
        self.new_block_scope();
        self.sc_mut().dynamic = true;
        self.compile_statement(&v.body, need_result)?;
        self.emit(Instruction::LeaveWith);
        self.leave_block();
        self.pop_scope();
        Ok(())
    }

    fn compile_switch_statement(
        &mut self,
        v: &ast::SwitchStatement,
        need_result: bool,
    ) -> CResult<()> {
        let ob = self.block;
        let mut b = Block::new(BlockType::Switch, ob);
        b.need_result = need_result;
        self.push_block(b);

        self.compile_expression(&v.discriminant)?
            .emit_getter(self, true)?;

        let mut funcs = Vec::new();
        for s in &v.body {
            funcs.extend(self.extract_functions(&s.consequent));
        }
        let mut scope_declared = false;
        if !funcs.is_empty() {
            self.new_block_scope();
            scope_declared = true;
            self.create_function_bindings(&funcs)?;
        }

        for s in &v.body {
            scope_declared = self.compile_lexical_declarations(&s.consequent, scope_declared)?;
        }

        let mut enter = None;
        let mut db = None;
        if scope_declared {
            let ob = self.block;
            let mut b = Block::new(BlockType::Scope, ob);
            b.need_result = need_result;
            self.push_block(b);
            enter = Some(self.code_len());
            self.emit(Instruction::EnterBlock(Box::default()));
            let s = self.cur_scope();
            let d = self.new_binding(s);
            self.bindings[d].is_const = true;
            self.bindings[d].is_strict = true;
            self.scopes[s].bindings.insert(0, d);
            db = Some(d);
        }

        self.compile_functions(&funcs)?;

        if need_result {
            self.emit(Instruction::ClearResult);
        }

        let mut jumps = vec![0usize; v.body.len()];

        for (i, s) in v.body.iter().enumerate() {
            if let Some(test) = &s.test {
                match db {
                    Some(d) => self.b_emit_get(d),
                    None => self.emit(Instruction::Dup),
                }
                self.compile_expression(test)?.emit_getter(self, true)?;
                self.emit(Instruction::OpStrictEq);
                if db.is_some() {
                    self.emit(Instruction::JneP(2));
                } else {
                    self.emit(Instruction::JneP(3));
                    self.emit(Instruction::Pop);
                }
                jumps[i] = self.code_len();
                self.emit(Instruction::Placeholder);
            }
        }

        if db.is_none() {
            self.emit(Instruction::Pop);
        }
        let mut jump_no_match = None;
        if v.default != -1 {
            if v.default != 0 {
                jumps[v.default as usize] = self.code_len();
                self.emit(Instruction::Placeholder);
            }
        } else {
            jump_no_match = Some(self.code_len());
            self.emit(Instruction::Placeholder);
        }

        for (i, s) in v.body.iter().enumerate() {
            if s.test.is_some() || i != 0 {
                let d = self.code_len() - jumps[i];
                self.set_code(jumps[i], Instruction::Jump(d as i32));
            }
            self.compile_statements(&s.consequent, need_result)?;
        }

        if let Some(j) = jump_no_match {
            let d = self.code_len() - j;
            self.set_code(j, Instruction::Jump(d as i32));
        }
        if let Some(e) = enter {
            self.leave_scope_block(e)?;
            if let Instruction::EnterBlock(eb) = &mut self.progs[self.p].code[e] {
                eb.stack_size = eb.stack_size.wrapping_sub(1);
            }
            self.pop_scope();
        }
        self.leave_block();
        Ok(())
    }
}
