use roxide_diag::{SrcFileId, format as diag_format};
use roxide_gc::SexpId;
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    BASE_ENV, DeparseOpts, GLOBAL_ENV, KeepSource, SEXP_NIL, ValueApi, deparse, lower,
};

use crate::api::{EvalResult, ScriptOpts, Unwind};
use crate::builtins;

use super::Ctx;

impl Ctx {
    /// Emit a diagnostic message (signaling `simpleMessage` when handlers are installed).
    pub fn message(&mut self, call: SexpId, text: impl Into<String>) -> EvalResult<()> {
        self.emit_condition("message", call, text.into(), false)
    }

    /// Emit a warning that participates in condition handlers and restarts.
    pub fn warn_now(&mut self, call: SexpId, msg: impl Into<String>) -> EvalResult<()> {
        self.emit_condition("warning", call, msg.into(), false)
    }

    /// Print a value using Roxide's core default vector printer.
    pub fn print_value_default(&mut self, val: SexpId, digits: usize) -> EvalResult<()> {
        builtins::print_value(self, val, digits)
    }

    /// Return the default `dput` / `deparse` line output for a value.
    pub fn deparse_lines(&self, val: SexpId, opts: DeparseOpts) -> EvalResult<Vec<String>> {
        Ok(deparse(&self.session, val, opts)?)
    }

    /// Record a deferred warning (not a safepoint; flushed at expression/error boundary).
    pub fn warn(&mut self, call: SexpId, msg: impl Into<String>) {
        let call_lines = if call == SEXP_NIL {
            None
        } else {
            deparse(&self.session, call, DeparseOpts::default()).ok()
        };
        self.pending_warnings.push((call_lines, msg.into()));
    }

    /// Emit a warning at `call` honoring `options(warn=)` and active warning handlers (`Rf_warning`).
    pub fn warning_at(&mut self, call: SexpId, msg: &str) -> EvalResult<()> {
        self.warn_now(call, msg)
    }

    /// Flush accumulated deferred warnings to `stderr`.
    pub fn flush_warnings(&mut self) {
        if self.pending_warnings.is_empty() {
            return;
        }
        let total = self.pending_warnings.len();
        let text = diag_format::warnings(&self.pending_warnings, total);
        self.pending_warnings.clear();
        let _ = self.host.write_err(text.as_bytes());
    }

    /// Signal an R error and return the resulting unwind.
    pub fn stop(&mut self, call: SexpId, msg: impl Into<String>) -> Unwind {
        self.error_condition(call, msg.into())
    }

    /// Public C-API error signaling helper (`Rf_error` / `Rf_errorcall`).
    pub fn signal_error_at(&mut self, call: SexpId, msg: &str) -> Unwind {
        self.stop(call, msg)
    }

    /// Parse and execute a complete R script string in `env`.
    ///
    /// When `auto_print` is true, visible top-level expression results are printed.
    pub fn run_script(
        &mut self,
        name: &str,
        src: &str,
        env: SexpId,
        auto_print: bool,
    ) -> EvalResult<()> {
        self.run_script_with_opts(
            name,
            src,
            env,
            ScriptOpts {
                auto_print,
                echo: false,
            },
        )
    }

    /// Parse and execute a complete R script string in `env` with explicit [`ScriptOpts`].
    pub fn run_script_with_opts(
        &mut self,
        name: &str,
        src: &str,
        env: SexpId,
        opts: ScriptOpts,
    ) -> EvalResult<()> {
        let fid: SrcFileId = self.sources.add_file(name, src);
        let mut parser = Parser::new(src, fid);
        let mut last_echoed = 0usize;
        loop {
            match parser.parse_next() {
                ParseStep::Eof => {
                    if opts.echo
                        && let Some(tail) = src.get(last_echoed..)
                        && !tail.is_empty()
                    {
                        for line in tail.lines() {
                            let _ = self.host.write_out(b"> ");
                            let _ = self.host.write_out(line.as_bytes());
                            let _ = self.host.write_out(b"\n");
                        }
                    }
                    let _ = self.flush_stream_captures();
                    self.flush_warnings();
                    let _ = self.flush_stream_captures();
                    return Ok(());
                }
                ParseStep::Incomplete => {
                    return Err(self.stop(SEXP_NIL, "unexpected end of input"));
                }
                ParseStep::Error(err) => {
                    let call = parser.error_call().map_or(Ok(SEXP_NIL), |node| {
                        lower(parser.ast(), node, &mut self.session, KeepSource::No)
                    });
                    let call = self.condition_result(SEXP_NIL, call.map_err(Into::into))?;
                    return Err(self.stop(call, err.message));
                }
                ParseStep::Expr(node) => {
                    if opts.echo {
                        let span = parser.ast().span(node);
                        let lo = (span.lo as usize).min(src.len());
                        let hi = (span.hi as usize).min(src.len());
                        if lo >= last_echoed
                            && let Some(between) = src.get(last_echoed..lo)
                        {
                            for line in between.lines() {
                                let _ = self.host.write_out(b"> ");
                                let _ = self.host.write_out(line.as_bytes());
                                let _ = self.host.write_out(b"\n");
                            }
                        }
                        if lo <= hi
                            && let Some(snippet) = src.get(lo..hi)
                        {
                            for (idx, line) in snippet.lines().enumerate() {
                                let prefix = if idx == 0 { "> " } else { "+ " };
                                let _ = self.host.write_out(prefix.as_bytes());
                                let _ = self.host.write_out(line.as_bytes());
                                let _ = self.host.write_out(b"\n");
                            }
                            last_echoed = hi;
                            if src.as_bytes().get(last_echoed) == Some(&b'\r') {
                                last_echoed += 1;
                            }
                            if src.as_bytes().get(last_echoed) == Some(&b'\n') {
                                last_echoed += 1;
                            }
                        }
                    }
                    let expr = lower(parser.ast(), node, &mut self.session, KeepSource::No);
                    let expr = self.condition_result(SEXP_NIL, expr.map_err(Into::into))?;
                    let val = self.scoped(|ctx| {
                        ctx.protect(expr);
                        ctx.visible = true;
                        let v = ctx.eval(expr, env)?;
                        if opts.auto_print && ctx.visible {
                            ctx.protect(v);
                            if ctx.session.is_s4(v)? && ctx.show_s4(v, env)? {
                                // Printed by S4 `show()`.
                            } else if ctx.session.is_object(v)? {
                                let print = ctx.session.intern("print")?;
                                let print = ctx.find_fun(print, BASE_ENV)?;
                                ctx.apply(print, &[(None, v)], SEXP_NIL, env)?;
                            } else {
                                let default = ctx.session.intern("print.default")?;
                                if let Some(print) = ctx.get_local(default, BASE_ENV)? {
                                    ctx.apply(print, &[(None, v)], SEXP_NIL, env)?;
                                } else {
                                    builtins::print_value(ctx, v, ctx.opts.digits)?;
                                }
                            }
                        }
                        Ok(v)
                    })?;
                    let _ = val;
                    self.run_pending_finalizers();
                    let _ = self.flush_stream_captures();
                    self.flush_warnings();
                    let _ = self.flush_stream_captures();
                }
            }
        }
    }

    /// Convenience entrypoint for running top-level Rscript code in `.GlobalEnv`.
    pub fn run_global(&mut self, name: &str, src: &str) -> EvalResult<()> {
        self.run_script(name, src, GLOBAL_ENV, true)
    }
}
