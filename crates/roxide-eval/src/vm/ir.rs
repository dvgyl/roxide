//! Register instructions and the data that a compiled expression needs.

use std::fmt::{self, Write};

use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{RSession, SEXP_MISSING, SexpType, ValueApi};

/// Current version of the Roxide instruction set.
pub const VERSION: u32 = 1;

/// Virtual register for a temporary value or loop cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Reg(pub u16);

impl fmt::Debug for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// Original call and cache slot for a guarded operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallSite {
    /// Slot in the call cache for this program.
    pub cache_idx: u16,
    /// Index of the original call in the constant table.
    pub call_const: u32,
}

/// Environment binding and cache slot for a variable access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarSite {
    /// Index of the symbol in the symbol table.
    pub sym_idx: u32,
    /// Slot in the variable cache for this program.
    pub cache_idx: u16,
}

/// Operands of a guarded unary primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unary {
    /// Result register.
    pub dst: Reg,
    /// Evaluated operand.
    pub src: Reg,
    /// Call resolved before operand evaluation.
    pub site: CallSite,
}

/// Operands of a guarded binary primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binary {
    /// Result register.
    pub dst: Reg,
    /// First evaluated operand.
    pub lhs: Reg,
    /// Second evaluated operand.
    pub rhs: Reg,
    /// Call resolved before operand evaluation.
    pub site: CallSite,
}

/// Operands of a guarded subscript assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexSet {
    /// Register containing the right-hand side and receiving the invisible result.
    pub value: Reg,
    /// Target variable. Lookup starts after evaluation of the right-hand side.
    pub var: VarSite,
    /// Unevaluated index expression in the constant table.
    pub index_const: u32,
    /// Original assignment. Resolve the replacement function at its normal
    /// evaluation point, before evaluation of the index.
    pub site: CallSite,
    /// Call cache slot for the replacement function.
    pub repl_cache_idx: u16,
}

/// One instruction in a compiled R expression.
///
/// Jump targets are absolute instruction indices. Branch and primitive
/// fallbacks after operand evaluation use the original AST with the values
/// already computed. Only a guard before operand evaluation can replay a call
/// from the start. A guard that cannot check a binding without side effects
/// uses the AST evaluator for the complete call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Load a self-evaluating constant and make the result visible.
    LoadConst {
        /// Result register.
        dst: Reg,
        /// Index in the constant table.
        const_idx: u32,
    },
    /// Load a visible R `NULL`.
    LoadNil(Reg),
    /// Copy a register and preserve visibility.
    Move {
        /// Result register.
        dst: Reg,
        /// Register to copy.
        src: Reg,
    },
    /// Set visibility of the current result.
    SetVisible(bool),
    /// Read a variable using the normal symbol evaluation rules.
    LoadVar {
        /// Result register.
        dst: Reg,
        /// Variable to read.
        var: VarSite,
    },
    /// Assign in the current environment and make the result invisible.
    StoreVar {
        /// Variable to write.
        var: VarSite,
        /// Value register.
        src: Reg,
        /// Original assignment for binding errors.
        call_const: u32,
    },
    /// Assign in an enclosing environment and make the result invisible.
    SuperStoreVar {
        /// Variable to write.
        var: VarSite,
        /// Value register.
        src: Reg,
        /// Original assignment for binding errors.
        call_const: u32,
    },
    /// Continue if the call resolves to the expected base primitive. Keep that
    /// resolution for this execution of the site. Otherwise evaluate the
    /// original call and jump over the compiled instructions.
    GuardCall {
        /// Site that holds the original call.
        site: CallSite,
        /// Result register on fallback.
        dst: Reg,
        /// Instruction after the compiled call.
        fallback: u32,
    },
    /// Add.
    Add(Binary),
    /// Subtract.
    Sub(Binary),
    /// Multiply.
    Mul(Binary),
    /// Divide.
    Div(Binary),
    /// Raise to a power.
    Pow(Binary),
    /// Apply unary plus.
    Pos(Unary),
    /// Negate.
    Neg(Unary),
    /// Test for less than.
    Lt(Binary),
    /// Test for less than or equal to.
    Le(Binary),
    /// Test for greater than.
    Gt(Binary),
    /// Test for greater than or equal to.
    Ge(Binary),
    /// Test for equality.
    Eq(Binary),
    /// Test for inequality.
    Ne(Binary),
    /// Apply logical negation.
    Not(Unary),
    /// Jump to an instruction.
    Jump(u32),
    /// Test an `if` or `while` condition and jump if false.
    JumpIfFalse {
        /// Evaluated condition.
        cond: Reg,
        /// Destination if the condition is false.
        target: u32,
        /// Original control call for the condition check.
        call_const: u32,
    },
    /// Test an `if` or `while` condition and jump if true.
    JumpIfTrue {
        /// Evaluated condition.
        cond: Reg,
        /// Destination if the condition is true.
        target: u32,
        /// Original control call for the condition check.
        call_const: u32,
    },
    /// Inspect the left operand of `&&` or `||` before evaluating the right operand.
    /// On a decisive value, write the visible logical result and jump.
    /// On a failed scalar logical guard, finish via the original AST and the
    /// evaluated left operand, then jump. The right operand is still lazy.
    ShortCircuit {
        /// Result register on a jump.
        dst: Reg,
        /// Register with the evaluated left operand.
        lhs: Reg,
        /// Decisive value. False for `&&`, true for `||`.
        decisive: bool,
        /// Instruction after the right operand and its result instruction.
        target: u32,
        /// Original logical call.
        site: CallSite,
    },
    /// Finish `&&` or `||` after evaluation of both operands. Use the original
    /// call for coercion or error handling if the scalar logical guard fails.
    FinishLogical(Binary),
    /// Finish the compiled expression and preserve its visibility.
    Return(Reg),
    /// Transfer a return value through the current R function frame.
    /// Report an error if there is no function frame.
    ReturnFromFunction {
        /// Register with the return value.
        src: Reg,
        /// Original `return` call.
        site: CallSite,
    },
    /// Set the loop variable to `NULL`, then initialize a cursor from the
    /// evaluated sequence. Compact ALTREP sequences can use an unboxed cursor.
    ForInit {
        /// Cursor register.
        cursor: Reg,
        /// Sequence register.
        sequence: Reg,
        /// Loop variable.
        var: VarSite,
        /// Original `for` call for sequence and binding errors.
        call_const: u32,
    },
    /// Assign the next element to the loop variable or jump if exhausted.
    ForNext {
        /// Cursor register from `ForInit`.
        cursor: Reg,
        /// Loop variable.
        var: VarSite,
        /// Destination when the sequence is exhausted.
        target: u32,
        /// Original `for` call for binding errors.
        call_const: u32,
    },
    /// Register a loop with the R evaluator to catch transfers from AST code.
    LoopPush {
        /// Instruction that removes this loop frame.
        break_target: u32,
        /// Instruction that begins the next iteration.
        next_target: u32,
    },
    /// Remove the current loop frame.
    LoopPop,
    /// Jump to the current loop's break target.
    Break,
    /// Jump to the current loop's next target.
    Next,
    /// Extract with the subscript primitive in the call site.
    IndexGet {
        /// Result register.
        dst: Reg,
        /// Evaluated target.
        target: Reg,
        /// Unevaluated index expression in the constant table.
        index_const: u32,
        /// Original `[` or `[[` call.
        site: CallSite,
    },
    /// Replace with `x[i] <- v`. The fast path can reuse an unshared vector.
    Index1Set(IndexSet),
    /// Replace with `x[[i]] <- v`. The fast path can reuse an unshared vector.
    Index2Set(IndexSet),
    /// Run the original AST in the current environment and preserve visibility.
    EvalAst {
        /// Result register.
        dst: Reg,
        /// Index of the AST in the constant table.
        ast_const: u32,
    },
    /// Try a strict base builtin with positional arguments. Check the function
    /// before evaluating arguments. Use the original call for all other
    /// functions so their promises retain the unevaluated AST.
    CallFast {
        /// Result register.
        dst: Reg,
        /// Call to resolve and evaluate.
        site: CallSite,
    },
}

/// A compiled expression and its GC roots.
#[derive(Clone, Debug)]
pub struct CompiledCode {
    /// Version of the Roxide instruction set.
    pub version: u32,
    /// Original expression.
    pub source: SexpId,
    /// Literals and original AST expressions used by instructions.
    pub consts: Vec<SexpId>,
    /// Interned R symbols encoded as handles, checked when added to this table.
    pub syms: Vec<SexpId>,
    /// Instructions in execution order.
    pub code: Vec<Op>,
    /// Number of virtual registers.
    pub num_regs: u16,
    /// Number of call cache slots.
    pub num_call_caches: u16,
    /// Number of variable cache slots.
    pub num_var_caches: u16,
    /// True if the compiler emitted a loop in this expression.
    pub has_loops: bool,
}

impl Trace for CompiledCode {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edge(self.source);
        tracer.edges(&self.consts);
        tracer.edges(&self.syms);
    }
}

impl CompiledCode {
    /// Format the symbol table, constant table, and instructions for inspection.
    #[must_use]
    pub fn disassemble(&self, session: &RSession) -> String {
        let mut out = format!(
            "Roxide IR v{} ({} registers, {} call caches, {} variable caches)\n",
            self.version, self.num_regs, self.num_call_caches, self.num_var_caches,
        );
        for (idx, &sym) in self.syms.iter().enumerate() {
            let name = session.sym_name_of(sym).unwrap_or("<invalid symbol>");
            let _ = writeln!(out, "  symbol {idx} = {name:?}");
        }
        for (idx, &value) in self.consts.iter().enumerate() {
            let _ = write!(out, "  const {idx} = ");
            describe_constant(&mut out, session, value);
        }
        for (pc, op) in self.code.iter().enumerate() {
            let _ = writeln!(out, "{pc:04}  {op:?}");
        }
        out
    }
}

fn describe_constant(out: &mut String, session: &RSession, value: SexpId) {
    if value == SEXP_MISSING {
        let _ = writeln!(out, "<missing>");
        return;
    }
    match session.type_of(value) {
        Ok(SexpType::Lang) => {
            let head = session
                .cons_parts(value)
                .and_then(|(head, _, _)| session.sym_name_of(head));
            let _ = writeln!(out, "call {:?}", head.unwrap_or("<computed head>"));
        }
        Ok(SexpType::Sym) => {
            let name = session.sym_name_of(value).unwrap_or("<invalid symbol>");
            let _ = writeln!(out, "symbol {name:?}");
        }
        Ok(SexpType::Int | SexpType::Real | SexpType::Lgl) if session.xlength(value) == Ok(1) => {
            let lines = roxide_value::deparse(session, value, Default::default());
            let _ = writeln!(out, "{}", lines.unwrap_or_default().join(" "));
        }
        Ok(ty) => {
            let _ = writeln!(out, "{}", ty.type_name(false));
        }
        Err(_) => {
            let _ = writeln!(out, "<stale handle>");
        }
    }
}

#[cfg(test)]
mod tests {
    use roxide_gc::RootSet;
    use roxide_value::SEXP_NIL;

    use super::*;

    #[test]
    fn compiled_code_traces_the_source_and_all_constants() {
        struct Roots(CompiledCode);

        impl RootSet for Roots {
            fn trace_roots(&self, tracer: &mut Tracer<'_>) {
                self.0.trace(tracer);
            }
        }

        let mut session = RSession::new(Default::default()).unwrap();
        let head = session.intern("f").unwrap().as_sexp();
        let source = session.lcons(head, SEXP_NIL, SEXP_NIL).unwrap();
        let literal = session.scalar_int(42).unwrap();
        let child = session.scalar_int(7).unwrap();
        let args = session.cons(child, SEXP_NIL, SEXP_NIL).unwrap();
        let fallback = session.lcons(head, args, SEXP_NIL).unwrap();
        let unrooted = session.scalar_int(5).unwrap();
        let mut code = crate::vm::compile_expr(&session, source).unwrap();
        // Make each root independent so tracing one cannot keep another alive.
        code.consts = vec![literal, fallback];
        session.collect(&Roots(code));
        for id in [source, literal, fallback, child] {
            assert!(session.heap.get(id).is_ok(), "{id:?}");
        }
        assert!(session.heap.get(unrooted).is_err());
    }
}
