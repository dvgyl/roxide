use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{CharId, ValueApi};

use crate::{Ctx, CtxExt, EvalResult};

use super::cache::Caches;
use super::exec::execute;
use super::{CompiledCode, compile_expr, is_jit_enabled};

enum State {
    Pending,
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
            if let State::Compiled(code) = state {
                code.code.trace(tracer);
            }
        }
    }
}

// Ctx exposes mutable extension access only. The weak index supports read-only queries.
thread_local! {
    static QUERIES: RefCell<HashMap<usize, Weak<RefCell<Entries>>>> = RefCell::default();
}

// The permanent NA string owns a byte buffer for the life of this session.
fn key(ctx: &Ctx) -> Option<usize> {
    Some(ctx.session.char_bytes(CharId::NA).ok()?.as_ptr().addr())
}

/// Evaluate a closure body, compiling it when the JIT level and call count permit.
pub fn run_body(ctx: &mut Ctx, fun: SexpId, body: SexpId, env: SexpId) -> EvalResult {
    if !is_jit_enabled(ctx.jit_level()) {
        return ctx.eval(body, env);
    }

    let table = Rc::clone(&ctx.ext::<JitTable>().0);
    let mut entries = table.borrow_mut();
    if entries.lookups == 0 {
        entries
            .closures
            .retain(|&closure, _| ctx.session.heap.get(closure).is_ok());
    }
    entries.lookups = entries.lookups.wrapping_add(1);
    let state = entries
        .closures
        .get(&fun)
        .filter(|(source, _)| *source == body);
    let code = match state.map(|(_, state)| state) {
        Some(State::Compiled(code)) => Some(Rc::clone(code)),
        Some(State::Failed) => None,
        pending => {
            let first = pending.is_none();
            entries
                .closures
                .retain(|&fun, _| ctx.session.heap.get(fun).is_ok());
            let state = match compile_expr(&ctx.session, body) {
                Ok(code) if !first || code.has_loops => State::Compiled(Rc::new(Program {
                    caches: RefCell::new(Caches::new(&code)),
                    code,
                })),
                Ok(_) => State::Pending,
                Err(_) => State::Failed,
            };
            let code = match &state {
                State::Compiled(code) => Some(Rc::clone(code)),
                _ => None,
            };
            entries.closures.insert(fun, (body, state));
            if let Some(key) = key(ctx) {
                QUERIES.with_borrow_mut(|queries| {
                    queries.retain(|_, table| table.strong_count() != 0);
                    queries.insert(key, Rc::downgrade(&table));
                });
            }
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
    let Some(key) = key(ctx) else {
        return false;
    };
    QUERIES.with_borrow(|queries| {
        queries
            .get(&key)
            .and_then(Weak::upgrade)
            .is_some_and(|table| {
                matches!(
                    table.borrow().closures.get(&fun),
                    Some((source, State::Compiled(_))) if *source == body
                )
            })
    })
}
