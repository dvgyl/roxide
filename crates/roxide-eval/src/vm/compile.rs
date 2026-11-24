//! Lower R language objects to the register instruction set.

use std::collections::HashMap;

use roxide_gc::SexpId;
use roxide_value::{RSession, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi, ValueError};

use super::ir::{Binary, CallSite, CompiledCode, IndexSet, Op, Reg, Unary, VERSION, VarSite};
use crate::closure::dots_index;

const MAX_DEPTH: usize = 128;
const MAX_ITEMS: usize = 1 << 16;

/// Compile `expr` and intern any replacement function names it uses.
///
/// The compiler returns an error for stale handles and when its depth or
/// instruction limits are exceeded. Callers can evaluate the original AST.
pub fn compile_expr(session: &mut RSession, expr: SexpId) -> Result<CompiledCode, ValueError> {
    let index1_replacement = session.intern("[<-")?;
    let index2_replacement = session.intern("[[<-")?;
    let mut compiler = Compiler {
        session,
        index1_replacement,
        index2_replacement,
        out: CompiledCode {
            version: VERSION,
            source: expr,
            consts: Vec::new(),
            syms: Vec::new(),
            code: Vec::new(),
            num_regs: 1,
            num_call_caches: 0,
            num_var_caches: 0,
            has_loops: false,
        },
        constants: HashMap::new(),
        symbols: HashMap::new(),
        next_reg: 1,
        loop_depth: 0,
    };
    let result = Reg(0);
    compiler.expr(expr, result, 0)?;
    compiler.emit(Op::Return(result))?;
    Ok(compiler.out)
}

struct Compiler<'a> {
    session: &'a RSession,
    index1_replacement: Sym,
    index2_replacement: Sym,
    out: CompiledCode,
    constants: HashMap<SexpId, u32>,
    symbols: HashMap<SexpId, u32>,
    next_reg: u16,
    loop_depth: usize,
}

impl Compiler<'_> {
    fn expr(&mut self, expr: SexpId, dst: Reg, depth: usize) -> Result<(), ValueError> {
        check_limit(depth, MAX_DEPTH)?;
        match self.session.type_of(expr)? {
            SexpType::Nil => self.emit(Op::LoadNil(dst))?,
            SexpType::Lang => {
                let saved = self.next_reg;
                self.call(expr, dst, depth + 1)?;
                self.next_reg = saved;
            }
            SexpType::Prom => self.eval_ast(expr, dst)?,
            SexpType::Sym => {
                if self.is_variable(expr)? {
                    let var = self.variable(expr)?;
                    self.emit(Op::LoadVar { dst, var })?;
                } else {
                    self.eval_ast(expr, dst)?;
                }
            }
            _ => {
                let const_idx = self.constant(expr)?;
                self.emit(Op::LoadConst { dst, const_idx })?;
            }
        }
        Ok(())
    }

    fn call(&mut self, expr: SexpId, dst: Reg, depth: usize) -> Result<(), ValueError> {
        let (head, tail, _) = self.session.cons_parts(expr)?;
        if !self.is_variable(head)? {
            return self.eval_ast(expr, dst);
        }
        let name = self.session.sym_name_of(head)?;
        if matches!(
            name,
            "function"
                | "quote"
                | "substitute"
                | "match.call"
                | "missing"
                | "on.exit"
                | "eval"
                | "evalq"
                | "local"
                | "bquote"
                | "expression"
                | "withVisible"
                | "nargs"
                | "delayedAssign"
                | "~"
                | ".Internal"
                | ".Primitive"
        ) {
            return self.eval_ast(expr, dst);
        }
        let Some(args) = self.positional_args(tail)? else {
            return self.eval_ast(expr, dst);
        };
        if let [lhs, rhs] = args.as_slice()
            && let Some(op) = binary_op(name)
        {
            let (pc, site) = self.guard_call(expr, dst)?;
            let operands = self.binary(dst, *lhs, *rhs, site, depth)?;
            self.emit(op(operands))?;
            return self.end_guard_retaining(pc, Some(operands.lhs));
        }
        match (name, args.as_slice()) {
            ("{", body) => {
                let (pc, _) = self.guard_call(expr, dst)?;
                if body.is_empty() {
                    self.emit(Op::LoadNil(dst))?;
                }
                for &stmt in body {
                    self.expr(stmt, dst, depth)?;
                }
                self.end_guard(pc)
            }
            ("(", [arg]) => {
                let (pc, _) = self.guard_call(expr, dst)?;
                self.expr(*arg, dst, depth)?;
                self.emit(Op::SetVisible(true))?;
                self.end_guard(pc)
            }
            ("<-" | "=" | "<<-", [lhs, rhs]) => self.assign(expr, name, *lhs, *rhs, dst, depth),
            ("[" | "[[", [receiver, index]) => {
                let (pc, site) = self.guard_call(expr, dst)?;
                let target = self.reg()?;
                self.expr(*receiver, target, depth)?;
                let index_const = self.constant(*index)?;
                self.emit(Op::IndexGet {
                    dst,
                    target,
                    index_const,
                    site,
                })?;
                self.end_guard(pc)
            }
            ("if", [cond, yes] | [cond, yes, _]) => {
                self.if_expr(expr, dst, *cond, *yes, args.get(2).copied(), depth)
            }
            ("while", [_, body]) | ("repeat", [body]) | ("for", [_, _, body]) => {
                self.loop_expr(expr, name, &args, *body, dst, depth)
            }
            ("break" | "next", []) if self.loop_depth != 0 => {
                let (pc, _) = self.guard_call(expr, dst)?;
                self.emit(if name == "break" { Op::Break } else { Op::Next })?;
                self.end_guard(pc)
            }
            ("return", [] | [_]) => {
                let (pc, _) = self.guard_call(expr, dst)?;
                self.expr(args.first().copied().unwrap_or(SEXP_NIL), dst, depth)?;
                self.emit(Op::ReturnFromFunction(dst))?;
                self.end_guard(pc)
            }
            ("&&" | "||", [lhs, rhs]) => {
                self.short_circuit(expr, dst, *lhs, *rhs, name == "||", depth)
            }
            ("+" | "-" | "!", [arg]) => {
                let (pc, site) = self.guard_call(expr, dst)?;
                let src = self.reg()?;
                self.expr(*arg, src, depth)?;
                let unary = Unary { dst, src, site };
                self.emit(match name {
                    "+" => Op::Pos(unary),
                    "-" => Op::Neg(unary),
                    _ => Op::Not(unary),
                })?;
                self.end_guard(pc)
            }
            (
                "if" | "for" | "while" | "repeat" | "break" | "next" | "return" | "(" | "<-" | "="
                | "<<-" | "[" | "[[" | "[<-" | "[[<-",
                _,
            ) => self.eval_ast(expr, dst),
            _ => {
                let site = self.call_site(expr)?;
                self.emit(Op::CallFast { dst, site })
            }
        }
    }

    fn assign(
        &mut self,
        expr: SexpId,
        name: &str,
        lhs: SexpId,
        rhs: SexpId,
        dst: Reg,
        depth: usize,
    ) -> Result<(), ValueError> {
        if self.is_variable(lhs)? {
            let (pc, _) = self.guard_call(expr, dst)?;
            let var = self.variable(lhs)?;
            self.expr(rhs, dst, depth)?;
            self.emit(if name == "<<-" {
                Op::SuperStoreVar { var, src: dst }
            } else {
                Op::StoreVar { var, src: dst }
            })?;
            return self.end_guard(pc);
        }
        if name == "<<-" || self.session.type_of(lhs)? != SexpType::Lang {
            return self.eval_ast(expr, dst);
        }
        let (head, tail, _) = self.session.cons_parts(lhs)?;
        if self.session.type_of(head)? != SexpType::Sym {
            return self.eval_ast(expr, dst);
        }
        let (op, replacement): (fn(IndexSet) -> Op, _) = match self.session.sym_name_of(head)? {
            "[" => (Op::Index1Set, self.index1_replacement),
            "[[" => (Op::Index2Set, self.index2_replacement),
            _ => return self.eval_ast(expr, dst),
        };
        let Some(args) = self.positional_args(tail)? else {
            return self.eval_ast(expr, dst);
        };
        let &[target, index] = args.as_slice() else {
            return self.eval_ast(expr, dst);
        };
        if !self.is_variable(target)? {
            return self.eval_ast(expr, dst);
        }
        let (pc, site) = self.guard_call(expr, dst)?;
        let var = self.variable(target)?;
        let index_const = self.constant(index)?;
        self.expr(rhs, dst, depth)?;
        let repl_cache_idx = take_slot(&mut self.out.num_call_caches)?;
        self.emit(op(IndexSet {
            value: dst,
            var,
            index_const,
            site,
            replacement,
            repl_cache_idx,
        }))?;
        self.end_guard(pc)
    }

    fn if_expr(
        &mut self,
        expr: SexpId,
        dst: Reg,
        cond: SexpId,
        yes: SexpId,
        no: Option<SexpId>,
        depth: usize,
    ) -> Result<(), ValueError> {
        let (guard, site) = self.guard_call(expr, dst)?;
        self.expr(cond, dst, depth)?;
        let branch = self.placeholder()?;
        self.expr(yes, dst, depth)?;
        let jump = self.placeholder()?;
        self.patch(
            branch,
            Op::JumpIfFalse {
                cond: dst,
                target: self.pc(),
                call_const: site.call_const,
            },
        )?;
        if let Some(no) = no {
            self.expr(no, dst, depth)?;
        } else {
            self.emit(Op::LoadNil(dst))?;
            self.emit(Op::SetVisible(false))?;
        }
        self.patch(jump, Op::Jump(self.pc()))?;
        self.end_guard(guard)
    }

    fn loop_expr(
        &mut self,
        expr: SexpId,
        name: &str,
        args: &[SexpId],
        body: SexpId,
        dst: Reg,
        depth: usize,
    ) -> Result<(), ValueError> {
        if let ("for", [sym, _, _]) = (name, args)
            && !self.is_variable(*sym)?
        {
            return self.eval_ast(expr, dst);
        }
        let (guard, site) = self.guard_call(expr, dst)?;
        // The sequence expression is outside the new loop's scope.
        let iterator = if let ("for", [sym, seq, _]) = (name, args) {
            let var = self.variable(*sym)?;
            let cursor = self.reg()?;
            self.expr(*seq, dst, depth)?;
            self.emit(Op::ForInit {
                cursor,
                sequence: dst,
                var,
                call_const: site.call_const,
            })?;
            Some((cursor, var))
        } else {
            None
        };
        let push = self.placeholder()?;
        let step = self.pc();
        self.loop_depth += 1;
        let condition = if let ("while", [cond, _]) = (name, args) {
            self.expr(*cond, dst, depth)?;
            Some(self.placeholder()?)
        } else if iterator.is_some() {
            Some(self.placeholder()?)
        } else {
            None
        };
        self.expr(body, dst, depth)?;
        self.loop_depth -= 1;
        self.emit(Op::Jump(step))?;
        let exit = self.pc();
        self.patch(
            push,
            Op::LoopPush {
                break_target: exit,
                next_target: step,
                cursor: iterator.map(|(cursor, _)| cursor),
            },
        )?;
        if let Some(pc) = condition {
            self.patch(
                pc,
                match iterator {
                    Some((cursor, var)) => Op::ForNext {
                        cursor,
                        var,
                        target: exit,
                    },
                    None => Op::JumpIfFalse {
                        cond: dst,
                        target: exit,
                        call_const: site.call_const,
                    },
                },
            )?;
        }
        self.emit(Op::LoopPop)?;
        self.emit(Op::LoadNil(dst))?;
        self.emit(Op::SetVisible(false))?;
        self.out.has_loops = true;
        self.end_guard(guard)
    }

    fn short_circuit(
        &mut self,
        expr: SexpId,
        dst: Reg,
        lhs: SexpId,
        rhs: SexpId,
        decisive: bool,
        depth: usize,
    ) -> Result<(), ValueError> {
        let (guard, site) = self.guard_call(expr, dst)?;
        let left = self.reg()?;
        self.expr(lhs, left, depth)?;
        let branch = self.placeholder()?;
        let right = self.reg()?;
        self.expr(rhs, right, depth)?;
        self.emit(Op::FinishLogical {
            args: Binary {
                dst,
                lhs: left,
                rhs: right,
                site,
            },
            and: !decisive,
        })?;
        self.patch(
            branch,
            Op::ShortCircuit {
                dst,
                lhs: left,
                decisive,
                target: self.pc(),
                site,
            },
        )?;
        self.end_guard_retaining(guard, Some(left))
    }

    fn binary(
        &mut self,
        dst: Reg,
        lhs: SexpId,
        rhs: SexpId,
        site: CallSite,
        depth: usize,
    ) -> Result<Binary, ValueError> {
        let left = self.reg()?;
        self.expr(lhs, left, depth)?;
        let right = self.reg()?;
        self.expr(rhs, right, depth)?;
        Ok(Binary {
            dst,
            lhs: left,
            rhs: right,
            site,
        })
    }

    fn positional_args(&self, mut tail: SexpId) -> Result<Option<Vec<SexpId>>, ValueError> {
        let mut args = Vec::new();
        while tail != SEXP_NIL {
            check_limit(args.len(), MAX_ITEMS)?;
            let (arg, next, tag) = self.session.cons_parts(tail)?;
            if tag != SEXP_NIL || arg == SEXP_MISSING {
                return Ok(None);
            }
            if self.session.type_of(arg)? == SexpType::Sym
                && self.session.sym_name_of(arg)? == "..."
            {
                return Ok(None);
            }
            args.push(arg);
            tail = next;
        }
        Ok(Some(args))
    }

    fn is_variable(&self, id: SexpId) -> Result<bool, ValueError> {
        if id == SEXP_MISSING || self.session.type_of(id)? != SexpType::Sym {
            return Ok(false);
        }
        let name = self.session.sym_name_of(id)?;
        Ok(name != "..." && dots_index(name).is_none())
    }

    fn eval_ast(&mut self, expr: SexpId, dst: Reg) -> Result<(), ValueError> {
        let ast_const = self.constant(expr)?;
        self.emit(Op::EvalAst { dst, ast_const })
    }

    fn constant(&mut self, id: SexpId) -> Result<u32, ValueError> {
        table_index(&mut self.constants, &mut self.out.consts, id)
    }

    fn variable(&mut self, id: SexpId) -> Result<VarSite, ValueError> {
        let sym_idx = table_index(&mut self.symbols, &mut self.out.syms, id)?;
        let cache_idx = take_slot(&mut self.out.num_var_caches)?;
        Ok(VarSite { sym_idx, cache_idx })
    }

    fn call_site(&mut self, expr: SexpId) -> Result<CallSite, ValueError> {
        let call_const = self.constant(expr)?;
        let cache_idx = take_slot(&mut self.out.num_call_caches)?;
        Ok(CallSite {
            cache_idx,
            call_const,
        })
    }

    fn guard_call(&mut self, expr: SexpId, dst: Reg) -> Result<(u32, CallSite), ValueError> {
        let site = self.call_site(expr)?;
        let pc = self.pc();
        self.emit(Op::GuardCall {
            site,
            dst,
            fallback: 0,
            retain: None,
        })?;
        Ok((pc, site))
    }

    fn end_guard(&mut self, pc: u32) -> Result<(), ValueError> {
        self.end_guard_retaining(pc, None)
    }

    fn end_guard_retaining(&mut self, pc: u32, retained: Option<Reg>) -> Result<(), ValueError> {
        let end = self.pc();
        let Some(Op::GuardCall {
            fallback, retain, ..
        }) = self.out.code.get_mut(pc as usize)
        else {
            return Err(ValueError::TypeMismatch {
                expected: "guard instruction",
                actual: "other instruction",
            });
        };
        *fallback = end;
        *retain = retained;
        Ok(())
    }

    fn reg(&mut self) -> Result<Reg, ValueError> {
        let reg = Reg(take_slot(&mut self.next_reg)?);
        self.out.num_regs = self.out.num_regs.max(self.next_reg);
        Ok(reg)
    }

    fn pc(&self) -> u32 {
        self.out.code.len() as u32
    }

    fn placeholder(&mut self) -> Result<u32, ValueError> {
        let pc = self.pc();
        self.emit(Op::Jump(0))?;
        Ok(pc)
    }

    fn emit(&mut self, op: Op) -> Result<(), ValueError> {
        check_limit(self.out.code.len(), MAX_ITEMS)?;
        self.out.code.push(op);
        Ok(())
    }

    fn patch(&mut self, pc: u32, op: Op) -> Result<(), ValueError> {
        let index = pc as usize;
        check_limit(index, self.out.code.len())?;
        self.out.code[index] = op;
        Ok(())
    }
}

fn binary_op(name: &str) -> Option<fn(Binary) -> Op> {
    Some(match name {
        "+" => Op::Add,
        "-" => Op::Sub,
        "*" => Op::Mul,
        "/" => Op::Div,
        "^" => Op::Pow,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        "==" => Op::Eq,
        "!=" => Op::Ne,
        _ => return None,
    })
}

fn check_limit(index: usize, len: usize) -> Result<(), ValueError> {
    if index >= len {
        return Err(ValueError::IndexOutOfBounds { index, len });
    }
    Ok(())
}

fn take_slot(count: &mut u16) -> Result<u16, ValueError> {
    let slot = *count;
    check_limit(usize::from(slot), usize::from(u16::MAX))?;
    *count += 1;
    Ok(slot)
}

fn table_index(
    entries: &mut HashMap<SexpId, u32>,
    table: &mut Vec<SexpId>,
    id: SexpId,
) -> Result<u32, ValueError> {
    if let Some(&idx) = entries.get(&id) {
        return Ok(idx);
    }
    check_limit(table.len(), MAX_ITEMS)?;
    let idx = table.len() as u32;
    table.push(id);
    entries.insert(id, idx);
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use roxide_value::{KeepSource, lower};

    use super::*;

    fn lower_source(session: &mut RSession, source: &str) -> SexpId {
        let mut parser = roxide_syntax::Parser::new(source, Default::default());
        let nodes = parser.parse_all().expect("valid R syntax");
        assert_eq!(nodes.len(), 1);
        lower(parser.ast(), nodes[0], session, KeepSource::No).expect("lower R AST")
    }

    fn compile(source: &str) -> (RSession, CompiledCode) {
        let mut session = RSession::new(Default::default()).expect("session");
        let expr = lower_source(&mut session, source);
        let ir = compile_expr(&mut session, expr).expect("compile R AST");
        assert_eq!(ir.source, expr);
        for op in &ir.code {
            let target = match op {
                Op::Jump(target)
                | Op::GuardCall {
                    fallback: target, ..
                }
                | Op::JumpIfFalse { target, .. }
                | Op::ShortCircuit { target, .. }
                | Op::ForNext { target, .. } => *target,
                Op::LoopPush {
                    break_target,
                    next_target,
                    ..
                } => {
                    assert!((*next_target as usize) < ir.code.len());
                    *break_target
                }
                _ => continue,
            };
            assert!((target as usize) < ir.code.len(), "{op:?}");
        }
        (session, ir)
    }

    #[test]
    fn variables_stay_in_environment_and_registers_hold_temporaries() {
        let (session, ir) = compile("{x <- 1L; y = x; y <<- (x + y); x}");
        assert_eq!(ir.num_regs, 3);
        assert_eq!(ir.num_var_caches, 7);
        assert_eq!(
            ir.syms
                .iter()
                .map(|&sym| session.sym_name_of(sym).unwrap())
                .collect::<Vec<_>>(),
            ["x", "y"],
        );
        let Op::Add(add) = ir.code.iter().find(|op| matches!(op, Op::Add(_))).unwrap() else {
            unreachable!();
        };
        assert_ne!(add.lhs, add.rhs);
        let call = ir.consts[add.site.call_const as usize];
        assert_eq!(
            session
                .sym_name_of(session.cons_parts(call).unwrap().0)
                .unwrap(),
            "+"
        );
        assert!(ir.code.iter().any(|op| matches!(op, Op::StoreVar { .. })));
        assert!(
            ir.code
                .iter()
                .any(|op| matches!(op, Op::SuperStoreVar { .. }))
        );
        assert!(matches!(ir.code.last(), Some(Op::Return(Reg(0)))));
        assert!(!ir.has_loops);
    }

    #[test]
    fn arithmetic_and_comparison_keep_the_original_calls() {
        let (session, ir) =
            compile("{+x; -x; !x; x-y; x*y; x/y; x^y; x<y; x<=y; x>y; x>=y; x==y; x!=y}");
        let calls: Vec<_> = ir
            .code
            .iter()
            .filter_map(|op| {
                let site = match op {
                    Op::Pos(u) | Op::Neg(u) | Op::Not(u) => u.site,
                    Op::Sub(b)
                    | Op::Mul(b)
                    | Op::Div(b)
                    | Op::Pow(b)
                    | Op::Lt(b)
                    | Op::Le(b)
                    | Op::Gt(b)
                    | Op::Ge(b)
                    | Op::Eq(b)
                    | Op::Ne(b) => b.site,
                    _ => return None,
                };
                let (head, _, _) = session
                    .cons_parts(ir.consts[site.call_const as usize])
                    .unwrap();
                Some(session.sym_name_of(head).unwrap())
            })
            .collect();
        assert_eq!(
            calls,
            [
                "+", "-", "!", "-", "*", "/", "^", "<", "<=", ">", ">=", "==", "!="
            ]
        );
    }

    #[test]
    fn conditional_branches_keep_visibility_and_return_transfers() {
        let (_, ir) = compile("if (x) return(2L)");
        let branch = ir
            .code
            .iter()
            .position(|op| matches!(op, Op::JumpIfFalse { .. }))
            .unwrap();
        let Op::JumpIfFalse { target, .. } = ir.code[branch] else {
            unreachable!()
        };
        assert!(
            ir.code[branch + 1..target as usize]
                .iter()
                .any(|op| matches!(op, Op::ReturnFromFunction(_)))
        );
        assert!(matches!(
            &ir.code[target as usize..],
            [
                Op::LoadNil(Reg(0)),
                Op::SetVisible(false),
                Op::Return(Reg(0))
            ]
        ));
    }

    #[test]
    fn short_circuit_skips_the_right_operand_and_its_result() {
        for (source, decisive) in [
            ("lhs && side_effect()", false),
            ("lhs || side_effect()", true),
        ] {
            let (session, ir) = compile(source);
            let [
                Op::GuardCall { fallback, site, .. },
                Op::LoadVar { dst: left, .. },
                Op::ShortCircuit {
                    lhs,
                    decisive: value,
                    site: branch_site,
                    target,
                    ..
                },
                Op::CallFast { dst: right, .. },
                Op::FinishLogical { args: finish, and },
                Op::Return(_),
            ] = ir.code.as_slice()
            else {
                panic!("{}", ir.disassemble(&session))
            };
            assert_eq!(*value, decisive);
            assert_ne!(*and, decisive);
            assert_eq!((*target, *fallback), (5, 5));
            assert_eq!(finish.site, *site);
            assert_eq!(branch_site, site);
            assert_eq!((lhs, &finish.lhs, &finish.rhs), (left, left, right));
        }
    }

    #[test]
    fn nested_loops_patch_each_exit_and_step() {
        let (_, ir) = compile(
            "for (i in seq_len(n)) { while (i < n) { if (i) next; break }; repeat { break }; next }",
        );
        assert!(ir.has_loops);
        let frames: Vec<_> = ir
            .code
            .iter()
            .filter_map(|op| match op {
                Op::LoopPush {
                    break_target,
                    next_target,
                    ..
                } => Some((*break_target as usize, *next_target as usize)),
                _ => None,
            })
            .collect();
        let [
            (outer_exit, outer_step),
            (while_exit, while_step),
            (repeat_exit, repeat_step),
        ] = frames.as_slice()
        else {
            panic!("expected three loop contexts");
        };
        assert!(
            *outer_step < *while_step && *while_exit < *repeat_step && *repeat_exit < *outer_exit
        );
        for &(exit, step) in &frames {
            assert_eq!(ir.code[exit], Op::LoopPop);
            assert_eq!(ir.code[exit - 1], Op::Jump(step as u32));
        }
        assert!(
            matches!(ir.code[*outer_step], Op::ForNext { target, .. } if target as usize == *outer_exit)
        );
        assert!(matches!(ir.code[*outer_step - 2], Op::ForInit { .. }));
        assert_eq!(
            ir.code.iter().filter(|op| matches!(op, Op::Break)).count(),
            2
        );
        assert_eq!(
            ir.code.iter().filter(|op| matches!(op, Op::Next)).count(),
            2
        );
    }

    #[test]
    fn subscript_assignment_keeps_index_lazy_and_returns_rhs() {
        let (session, ir) = compile("{x[i()] <- rhs(); x[[j()]] <- 2L}");
        let set = ir
            .code
            .iter()
            .position(|op| matches!(op, Op::Index1Set(_)))
            .unwrap();
        let Op::Index1Set(args) = ir.code[set] else {
            unreachable!()
        };
        let (head, _, _) = session
            .cons_parts(ir.consts[args.index_const as usize])
            .unwrap();
        assert_eq!(session.sym_name_of(head).unwrap(), "i");
        assert_eq!(
            session
                .sym_name_of(ir.syms[args.var.sym_idx as usize])
                .unwrap(),
            "x"
        );
        assert!(matches!(ir.code[set - 1], Op::CallFast { dst, .. } if dst == args.value));
        assert!(ir.code.iter().any(|op| matches!(op, Op::Index2Set(_))));
        assert_ne!(args.site.cache_idx, args.repl_cache_idx);
        assert!(args.repl_cache_idx < ir.num_call_caches);
    }

    #[test]
    fn subscript_extraction_keeps_index_as_an_expression() {
        for (source, name) in [("x[i()]", "["), ("x[[i()]]", "[[")] {
            let (session, ir) = compile(source);
            let [
                Op::GuardCall { .. },
                Op::LoadVar { dst, .. },
                Op::IndexGet {
                    target,
                    index_const,
                    site,
                    ..
                },
                Op::Return(_),
            ] = ir.code.as_slice()
            else {
                panic!("unexpected subscript instructions: {:?}", ir.code)
            };
            assert_eq!(dst, target);
            for (slot, expected) in [(*index_const, "i"), (site.call_const, name)] {
                let (head, _, _) = session.cons_parts(ir.consts[slot as usize]).unwrap();
                assert_eq!(session.sym_name_of(head).unwrap(), expected);
            }
        }
    }

    #[test]
    fn fallback_keeps_original_language_objects() {
        for source in [
            "substitute(x + y)",
            "match.call()",
            "quote(while (TRUE) next)",
            "f(...)",
            "f(x = a)",
            "f(, a)",
            "(get('f'))(1L)",
            "x$a[1L] <- 2L",
            "function(x) { while (x) break }",
            "next",
            "..1",
        ] {
            let (_, ir) = compile(source);
            assert_eq!(
                ir.code,
                [
                    Op::EvalAst {
                        dst: Reg(0),
                        ast_const: 0
                    },
                    Op::Return(Reg(0))
                ],
                "{source}"
            );
            assert_eq!(ir.consts, [ir.source]);
            assert!(!ir.has_loops);
        }
        let (_, ir) = compile("f(x + y)");
        assert!(matches!(
            ir.code.as_slice(),
            [Op::CallFast { .. }, Op::Return(_)]
        ));
        assert_eq!(ir.consts, [ir.source]);
    }

    #[test]
    fn disassembly_shows_instructions_and_their_tables() {
        let (session, ir) = compile("x + 2L");
        let printed = ir.disassemble(&session);
        assert!(
            printed.starts_with("Roxide IR v2 (3 registers, 1 call caches, 1 variable caches)\n")
        );
        assert!(printed.contains("symbol 0 = \"x\"\n"));
        assert!(printed.contains("const 0 = call \"+\"\n"));
        assert!(printed.contains("const 1 = 2L\n"));
        assert!(printed.contains("0000  GuardCall"));
        assert!(printed.contains("0003  Add"));
        assert!(printed.ends_with("0004  Return(r0)\n"));
    }

    #[test]
    fn rejects_excessive_compiler_depth() {
        let mut session = RSession::new(Default::default()).expect("session");
        let paren = session.intern("(").unwrap().as_sexp();
        let mut expr = SEXP_NIL;
        for _ in 0..MAX_DEPTH - 1 {
            let args = session.cons(expr, SEXP_NIL, SEXP_NIL).unwrap();
            expr = session.lcons(paren, args, SEXP_NIL).unwrap();
        }
        assert!(compile_expr(&mut session, expr).is_ok());
        let args = session.cons(expr, SEXP_NIL, SEXP_NIL).unwrap();
        let expr = session.lcons(paren, args, SEXP_NIL).unwrap();
        assert_eq!(
            compile_expr(&mut session, expr).unwrap_err(),
            ValueError::IndexOutOfBounds {
                index: MAX_DEPTH,
                len: MAX_DEPTH
            },
        );
    }
}
