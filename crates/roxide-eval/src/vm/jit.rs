use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::ValueApi;

use crate::{Ctx, CtxExt, EvalResult};

use super::cache::Caches;
use super::exec::execute;
use super::{CompiledCode, compile_expr, is_jit_enabled};

enum State {
    Pending(Rc<Program>),
    Compiled(Rc<Program>),
    Failed,
}

struct Program {
    code: CompiledCode,
    caches: RefCell<Caches>,
}

#[derive(Default)]
struct Entries {
    closures: HashMap<SexpId, (SexpId, State)>,
    lookups: u8,
}

#[derive(Default)]
struct JitTable(Rc<RefCell<Entries>>);

impl CtxExt for JitTable {}

impl Trace for JitTable {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for (_, state) in self.0.borrow().closures.values() {
            if let State::Pending(code) | State::Compiled(code) = state {
                code.code.trace(tracer);
            }
        }
    }
}

/// Evaluate a closure body, compiling it when the JIT level and call count permit.
pub fn run_body(ctx: &mut Ctx, fun: SexpId, body: SexpId, env: SexpId) -> EvalResult {
    if !is_jit_enabled(ctx.jit_level()) {
        return ctx.eval(body, env);
    }

    let table = Rc::clone(&ctx.ext::<JitTable>().0);
    let mut entries = table.borrow_mut();
    entries.lookups = entries.lookups.wrapping_add(1);
    if entries.lookups == 0 {
        entries
            .closures
            .retain(|&closure, _| ctx.session.heap.get(closure).is_ok());
    }
    let state = entries
        .closures
        .get_mut(&fun)
        .filter(|(source, _)| *source == body);
    let code = match state.map(|(_, state)| state) {
        Some(state) => match state {
            State::Compiled(code) => Some(Rc::clone(code)),
            State::Failed => None,
            State::Pending(code) => {
                let code = Rc::clone(code);
                *state = State::Compiled(Rc::clone(&code));
                Some(code)
            }
        },
        None => {
            let state = match compile_expr(&mut ctx.session, body) {
                Ok(code) => {
                    let has_loops = code.has_loops;
                    let program = Rc::new(Program {
                        caches: RefCell::new(Caches::new(&code)),
                        code,
                    });
                    if has_loops {
                        State::Compiled(program)
                    } else {
                        State::Pending(program)
                    }
                }
                Err(_) => State::Failed,
            };
            let code = match &state {
                State::Compiled(code) => Some(Rc::clone(code)),
                _ => None,
            };
            entries.closures.insert(fun, (body, state));
            code
        }
    };
    drop(entries);

    match code {
        Some(program) => execute(ctx, &program.code, env, &program.caches, None),
        None => ctx.eval(body, env),
    }
}

/// Return whether this session holds compiled code for the current body of `fun`.
#[must_use]
pub fn is_compiled(ctx: &Ctx, fun: SexpId) -> bool {
    let Ok((_, body, _)) = ctx.session.closure_parts(fun) else {
        return false;
    };
    ctx.ext_ref::<JitTable>().is_some_and(|table| {
        matches!(
            table.0.borrow().closures.get(&fun),
            Some((source, State::Compiled(_))) if *source == body
        )
    })
}
