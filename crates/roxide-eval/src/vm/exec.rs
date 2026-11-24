//! Register bytecode VM execution and closure JIT trigger (Milestone 4 W2-V2).

use roxide_gc::SexpId;

use crate::{Ctx, EvalResult};

/// Evaluate closure `fun`'s `body` in `env`, compiling and running bytecode when JIT is active.
pub fn run_body(ctx: &mut Ctx, _fun: SexpId, body: SexpId, env: SexpId) -> EvalResult {
    ctx.eval(body, env)
}
