use std::collections::HashSet;

use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, Sortedness, ValueApi};

use crate::api::{AltOp, AltReply, EvalResult, InternalError, Unwind, altrep_hook_missing};

use super::Ctx;

fn altrep_reply_error() -> Unwind {
    Unwind::Internal(InternalError {
        message: "unexpected foreign ALTREP hook reply".into(),
    })
}

impl Ctx {
    /// Skip automatic ALTREP materialization for these primitives.
    pub fn mark_altrep_aware(&mut self, names: &[&'static str]) {
        self.altrep_aware.extend(names.iter().copied());
    }

    /// Return whether the primitive handles ALTREP values directly.
    pub(crate) fn is_altrep_aware(&self, name: &str) -> bool {
        self.altrep_aware.contains(name)
    }

    /// Materialize a lazy foreign ALTREP value and its direct lazy foreign ALTREP attributes.
    pub fn resolve_altrep(&mut self, x: SexpId) -> EvalResult<()> {
        if !self.session.foreign_alt_seen() {
            return Ok(());
        }
        self.scoped(|ctx| {
            ctx.protect(x);
            ctx.materialize_foreign_altrep(x)?;
            let attrs = ctx.session.attrs(x)?;
            for &(_, value) in &attrs {
                ctx.protect(value);
            }
            for (_, value) in attrs {
                ctx.materialize_foreign_altrep(value)?;
            }
            Ok(())
        })
    }

    /// Resolve foreign ALTREP values in lists, pairlists, calls, expressions, and attributes.
    /// Stop at environments, closures, and promises.
    pub fn resolve_altrep_deep(&mut self, x: SexpId) -> EvalResult<()> {
        if !self.session.foreign_alt_seen() {
            return Ok(());
        }
        let mut pending = vec![x];
        let mut seen = HashSet::from([x]);
        self.session.heap.preserve(x);
        let result = (|| {
            while let Some(value) = pending.pop() {
                let ty = self.session.type_of(value)?;
                if matches!(ty, SexpType::Env | SexpType::Clo | SexpType::Prom) {
                    continue;
                }
                self.resolve_altrep(value)?;
                let mut children: Vec<_> = self
                    .session
                    .attrs(value)?
                    .into_iter()
                    .map(|(_, attr)| attr)
                    .collect();
                match ty {
                    SexpType::Vec | SexpType::Expr => {
                        children.extend_from_slice(self.session.list_slice(value)?);
                    }
                    SexpType::List | SexpType::Lang => {
                        let (car, cdr, _) = self.session.cons_parts(value)?;
                        children.push(car);
                        if cdr != SEXP_NIL {
                            children.push(cdr);
                        }
                    }
                    _ => {}
                }
                for child in children {
                    if seen.insert(child) {
                        self.session.heap.preserve(child);
                        pending.push(child);
                    }
                }
            }
            Ok(())
        })();
        for value in seen {
            self.session.heap.release(value);
        }
        result
    }

    /// Get the vector length without materializing its elements.
    pub fn altrep_len(&mut self, x: SexpId) -> EvalResult<usize> {
        if self.session.foreign_alt_seen()
            && self
                .session
                .foreign_alt(x)?
                .is_some_and(|info| !info.owned && info.len.is_none())
        {
            let AltReply::Len(len) = self.invoke_altrep(AltOp::Length(x))? else {
                return Err(altrep_reply_error());
            };
            self.session.seal_foreign_alt_len(x, len)?;
        }
        Ok(self.session.xlength(x)?)
    }

    /// Get the vector's known sortedness without materializing its elements.
    pub fn altrep_sorted(&mut self, x: SexpId) -> EvalResult<Sortedness> {
        if !self.session.foreign_alt_seen()
            || !self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
        {
            return Ok(self.session.alt_is_sorted(x)?);
        }
        let AltReply::Flag(code) = self.invoke_altrep(AltOp::IsSorted(x))? else {
            return Err(altrep_reply_error());
        };
        Ok(match code {
            1 => Sortedness::Increasing,
            2 => Sortedness::IncreasingNaFirst,
            -1 => Sortedness::Decreasing,
            -2 => Sortedness::DecreasingNaFirst,
            _ => Sortedness::Unknown,
        })
    }

    /// Check for a guarantee that the vector has no missing elements.
    pub fn altrep_no_na(&mut self, x: SexpId) -> EvalResult<bool> {
        if !self.session.foreign_alt_seen()
            || !self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
        {
            return Ok(self.session.alt_no_na(x)?);
        }
        let AltReply::Flag(flag) = self.invoke_altrep(AltOp::NoNa(x))? else {
            return Err(altrep_reply_error());
        };
        Ok(flag != 0)
    }

    /// Run a foreign ALTREP method that returns an optional R value.
    /// Resolve vector results from subset, coercion, and summary methods.
    pub fn altrep_method(&mut self, op: AltOp) -> EvalResult<Option<SexpId>> {
        let AltReply::Value(value) = self.invoke_altrep(op)? else {
            return Err(altrep_reply_error());
        };
        if let Some(x) = value
            && matches!(
                op,
                AltOp::ExtractSubset { .. }
                    | AltOp::Coerce { .. }
                    | AltOp::Sum { .. }
                    | AltOp::Min { .. }
                    | AltOp::Max { .. }
            )
        {
            self.resolve_altrep(x)?;
        }
        Ok(value)
    }

    fn materialize_foreign_altrep(&mut self, x: SexpId) -> EvalResult<()> {
        if self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
            && self.invoke_altrep(AltOp::Materialize(x))? != AltReply::Unit
        {
            return Err(altrep_reply_error());
        }
        Ok(())
    }

    fn invoke_altrep(&mut self, op: AltOp) -> EvalResult<AltReply> {
        let hooks = self.native_hooks().ok_or_else(altrep_hook_missing)?;
        let roots: &[SexpId] = match op {
            AltOp::Length(x)
            | AltOp::Materialize(x)
            | AltOp::IsSorted(x)
            | AltOp::NoNa(x)
            | AltOp::Sum { x, .. }
            | AltOp::Min { x, .. }
            | AltOp::Max { x, .. }
            | AltOp::Coerce { x, .. }
            | AltOp::SerializedState(x) => &[x],
            AltOp::ExtractSubset { x, indx, call } => &[x, indx, call],
            AltOp::Unserialize {
                class,
                package,
                state,
                attrs,
                ..
            } => &[class.as_sexp(), package.as_sexp(), state, attrs],
        };
        self.scoped(|ctx| {
            for &root in roots {
                ctx.protect(root);
            }
            hooks.altrep(ctx, op)
        })
    }
}
