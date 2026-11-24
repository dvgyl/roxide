use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_value::{FALSE_SCALAR, TRUE_SCALAR};

use super::tools_rd::internal;

pub(super) const MODULE: Module = Module {
    prims: &[internal("rox_tools_bsd_signals", 0, bsd_signals)],
    prelude: include_str!("tools_build.R"),
};

fn bsd_signals(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let os = ctx.host().sys_info().sysname;
    Ok(
        if matches!(
            os.as_str(),
            "macos" | "darwin" | "Darwin" | "freebsd" | "FreeBSD"
        ) {
            TRUE_SCALAR
        } else {
            FALSE_SCALAR
        },
    )
}
