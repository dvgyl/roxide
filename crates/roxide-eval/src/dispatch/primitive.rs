use roxide_gc::SexpId;
use roxide_value::{BASE_NAMESPACE, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, Dispatch, EvalResult, GroupGeneric, PrimDef};

use super::{Candidate, DispatchInput, MethodFrame};

pub(super) fn choose_ops_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    for class in ctx.classes(args.get(0), true)? {
        let name = format!("chooseOpsMethod.{class}");
        if let Some(fun) =
            ctx.lookup_method("chooseOpsMethod", &name, call.env, BASE_NAMESPACE, false)?
        {
            let pairs = args.iter().collect::<Vec<_>>();
            return ctx.apply(fun, &pairs, call.expr, call.env);
        }
    }
    Ok(roxide_value::FALSE_SCALAR)
}

impl Ctx {
    /// Try method dispatch before running the primitive.
    pub(crate) fn dispatch_primitive(
        &mut self,
        def: &PrimDef,
        call: Call,
        input: DispatchInput<'_>,
    ) -> EvalResult<Option<SexpId>> {
        let (generic, group) = match def.dispatch {
            Dispatch::None => return Ok(None),
            Dispatch::Internal(name) => (name, None),
            Dispatch::Group(group) => (def.name, Some(group)),
        };
        if self
            .session
            .sym_name_of(self.session.cons_parts(call.expr).map_or(SEXP_NIL, |p| p.0))
            .is_ok_and(|name| name.ends_with(".default"))
        {
            return Ok(None);
        }
        let na_rm = self.session.intern("na.rm")?;
        let ops = matches!(group, Some(GroupGeneric::Ops | GroupGeneric::MatrixOps));
        let (args, candidates) = match input {
            DispatchInput::Values { args, .. } => {
                let candidates = args
                    .iter()
                    .enumerate()
                    .filter(|(_, (tag, _))| {
                        group != Some(GroupGeneric::Summary) || *tag != Some(na_rm)
                    })
                    .take(if ops { 2 } else { 1 })
                    .collect();
                (args, candidates)
            }
            DispatchInput::Subscript { receiver, actuals } => {
                (actuals, vec![(0, (actuals.name(0), receiver))])
            }
        };
        if let Some(hooks) = self.s4_hooks {
            for &(_, (_, value)) in &candidates {
                if self.session.is_s4(value)? {
                    let symbol = self.session.intern(generic)?;
                    let dollar = self.dollar_method_args(generic, call, args)?;
                    if let Some(result) =
                        hooks.dispatch(self, symbol, &call, dollar.as_ref().unwrap_or(args))?
                    {
                        return Ok(Some(result));
                    }
                    break;
                }
            }
        }
        let size = candidates.len();
        let mut methods = Vec::new();
        for &(i, (_, value)) in &candidates {
            if value == SEXP_MISSING || !self.session.is_object(value)? {
                continue;
            }
            let classes = self.classes(value, false)?;
            let mut method = MethodFrame {
                generic: generic.to_owned(),
                group,
                group_method: false,
                classes,
                offset: 0,
                class: SEXP_NIL,
                side: if ops { i } else { 0 },
                caller: call.env,
                definition: BASE_NAMESPACE,
                names: vec![String::new(); if ops { size } else { 1 }],
                primitive: Some(call),
                is_default: false,
            };
            if let Some(candidate) = self.find_method(&method, 0, false)? {
                self.protect(candidate.fun);
                method.select(self, &candidate, SEXP_NIL)?;
                self.protect(method.class);
                methods.push((candidate, method));
            }
        }
        if methods.len() == 2 {
            if methods[0].0.fun == methods[1].0.fun {
                methods[0].1.names[1] = methods[1].0.name.clone();
                methods.pop();
            } else {
                let winner = self.resolve_ops(call, args, [&methods[0].0, &methods[1].0])?;
                match winner {
                    Some(0) => {
                        methods.pop();
                    }
                    Some(_) => {
                        methods.remove(0);
                    }
                    None => {
                        let (left, right) = (&methods[0].0.name, &methods[1].0.name);
                        self.warn_now(
                            call.err_call(),
                            format!(
                                "Incompatible methods (\"{left}\", \"{right}\") for \"{generic}\""
                            ),
                        )?;
                        return Ok(None);
                    }
                }
            }
        }
        let Some((candidate, method)) = methods.pop() else {
            return Ok(None);
        };
        let source = match input {
            DispatchInput::Values { source, .. } => source,
            DispatchInput::Subscript { actuals, .. } => {
                return self
                    .invoke_method(
                        candidate.fun,
                        &candidate.name,
                        actuals.clone(),
                        call.env,
                        method,
                    )
                    .map(Some);
            }
        };
        let dollar = self.dollar_method_args(generic, call, args)?;
        let args = dollar.as_ref().unwrap_or(args);
        let mut pairs = Vec::with_capacity(args.len() + 1);
        for (i, (tag, value)) in args.iter().enumerate() {
            let expr = if dollar.is_some() && i == 1 {
                value
            } else {
                source.map_or(Ok(value), |args| self.promise_expr(args.get(i)))?
            };
            let tag = if group == Some(GroupGeneric::Ops) {
                None
            } else {
                tag
            };
            pairs.push((tag, self.value_promise(expr, value)?));
        }
        if group == Some(GroupGeneric::Summary) && !pairs.iter().any(|(tag, _)| *tag == Some(na_rm))
        {
            let value = roxide_value::FALSE_SCALAR;
            pairs.push((Some(na_rm), self.value_promise(value, value)?));
        }
        self.invoke_method(
            candidate.fun,
            &candidate.name,
            Args::from_pairs(pairs),
            call.env,
            method,
        )
        .map(Some)
    }

    fn dollar_method_args(
        &mut self,
        generic: &str,
        call: Call,
        args: &Args,
    ) -> EvalResult<Option<Args>> {
        if !matches!(generic, "$" | "$<-") || self.session.type_of(args.get(1))? == SexpType::Str {
            return Ok(None);
        }
        let name = self.name(args.get(1), call.err_call())?;
        let name = self.session.scalar_str(&name)?;
        self.protect(name);
        Ok(Some(Args::from_pairs(
            args.iter()
                .enumerate()
                .map(|(i, (tag, value))| (tag, if i == 1 { name } else { value }))
                .collect(),
        )))
    }

    fn resolve_ops(
        &mut self,
        call: Call,
        args: &Args,
        methods: [&Candidate; 2],
    ) -> EvalResult<Option<usize>> {
        for side in 0..2 {
            let other = 1 - side;
            let pairs = [
                (None, args.get(side)),
                (None, args.get(other)),
                (None, methods[side].fun),
                (None, methods[other].fun),
                (None, call.expr),
                (
                    None,
                    if side == 0 {
                        roxide_value::FALSE_SCALAR
                    } else {
                        roxide_value::TRUE_SCALAR
                    },
                ),
            ];
            let fun = self.optional_fun("chooseOpsMethod", call.env, SEXP_NIL)?;
            let Some(fun) = fun else { continue };
            let result = self.apply(fun, &pairs, SEXP_NIL, call.env)?;
            if self.truth(result, call.err_call())? {
                return Ok(Some(side));
            }
        }
        Ok(None)
    }

    /// Copy applicable class attributes to the result of a group primitive.
    pub(crate) fn preserve_group_attributes(
        &mut self,
        def: &PrimDef,
        args: &Args,
        result: SexpId,
    ) -> EvalResult {
        if !matches!(
            def.dispatch,
            Dispatch::Group(GroupGeneric::Ops | GroupGeneric::Math)
        ) || result == SEXP_NIL
            || matches!(
                def.name,
                "==" | "!="
                    | "<"
                    | "<="
                    | ">"
                    | ">="
                    | "&"
                    | "|"
                    | "!"
                    | "cumsum"
                    | "cumprod"
                    | "cummin"
                    | "cummax"
            )
        {
            return Ok(result);
        }
        let mut values = args
            .values()
            .iter()
            .take(if def.dispatch == Dispatch::Group(GroupGeneric::Ops) {
                2
            } else {
                1
            })
            .copied();
        let result_len = self.session.xlength(result)?;
        let tag = self.session.intern("class")?;
        let mut class = SEXP_NIL;
        for value in values.by_ref() {
            if self.session.is_object(value)? {
                let date_offset = matches!(def.name, "+" | "-")
                    && self
                        .classes(value, false)?
                        .iter()
                        .any(|class| class == "Date");
                if date_offset || self.session.xlength(value)? == result_len {
                    class = self.session.attr(value, tag);
                    break;
                }
            }
        }
        if class != SEXP_NIL {
            let result = self.session.shallow_duplicate(result)?;
            self.session.set_attrib(result, tag, class)?;
            return Ok(result);
        }
        Ok(result)
    }
}
