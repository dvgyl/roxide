#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 package loader, `NAMESPACE` evaluator, lazy-load DB, and `R CMD` engine (ADR-0013, ADR-0014).

pub mod batch;
mod data;
pub mod description;
pub mod install;
mod install_compiled;
pub mod lazyload;
pub mod library;
pub mod loader;
pub mod makeconf;
pub mod nsfile;
pub mod s3reg;
pub mod shlib;
pub mod tar;

use roxide_eval::{Ctx, InternalError};

/// Register package-loading base primitives and install the [`roxide_eval::NamespaceLoader`].
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_module(&loader::MODULE)?;
    ctx.register_module(&library::MODULE)?;
    ctx.register_module(&s3reg::MODULE)?;
    ctx.register_module(&lazyload::MODULE)?;
    ctx.register_module(&batch::ARGUMENTS)?;
    ctx.set_namespace_loader(&loader::LOADER);
    Ok(())
}
