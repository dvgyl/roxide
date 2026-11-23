use roxide_gc::{SexpId, Tracer};
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, ValueApi,
};

use crate::eval::ArgInput;
use crate::{
    Args, Arity, Call, Ctx, Dispatch, EvalResult, GroupGeneric, Module, PrimDef, Transfer, Unwind,
};

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!(
            "UseMethod",
            use_method,
            Unevaluated,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!("NextMethod", next_method, Unevaluated, Arity::Any, SetByFn),
        primitive!(
            "chooseOpsMethod",
            choose_ops_method,
            Eval,
            Arity::Exact(6),
            On
        ),
    ],
    prelude: "",
};

#[derive(Clone)]
pub(crate) struct MethodFrame {
    generic: String,
    group: Option<GroupGeneric>,
    group_method: bool,
    names: Vec<String>,
    classes: Vec<String>,
    class: SexpId,
    offset: usize,
    side: usize,
    caller: SexpId,
    definition: SexpId,
    primitive: Option<Call>,
    is_default: bool,
}

impl MethodFrame {
    fn handles(&self) -> impl Iterator<Item = SexpId> {
        [self.class, self.caller, self.definition]
            .into_iter()
            .chain(
                self.primitive
                    .into_iter()
                    .flat_map(|call| [call.expr, call.env, call.user_call]),
            )
    }

    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for root in self.handles() {
            tracer.edge(root);
        }
    }

    pub(crate) fn protect(&self, ctx: &mut Ctx) {
        for root in self.handles() {
            ctx.protect(root);
        }
    }

    pub(crate) fn bind(&self, ctx: &mut Ctx, env: SexpId) -> EvalResult<()> {
        let generic = ctx.session.scalar_str(&self.generic)?;
        let method = ctx.strings(self.names.iter().map(String::as_str))?;
        for (name, value) in [
            (".Generic", generic),
            (".Method", method),
            (".Class", self.class),
            (".GenericCallEnv", self.caller),
            (".GenericDefEnv", self.definition),
        ] {
            let tag = ctx.session.intern(name)?;
            ctx.assign(tag, value, env)?;
        }
        if self.primitive.is_some() {
            let group = if self.group_method {
                self.group.map_or("", group_name)
            } else {
                ""
            };
            let group = ctx.session.scalar_str(group)?;
            let tag = ctx.session.intern(".Group")?;
            ctx.assign(tag, group, env)?;
        }
        Ok(())
    }

    fn select(&mut self, ctx: &mut Ctx, candidate: &Candidate, previous: SexpId) -> EvalResult<()> {
        self.offset = candidate.index;
        self.is_default = candidate.default;
        self.group_method = candidate.group_method;
        self.names[self.side] = candidate.name.clone();
        self.class = if candidate.default && previous == SEXP_NIL {
            SEXP_NIL
        } else {
            let class = ctx.strings(self.classes[self.offset..].iter().map(String::as_str))?;
            let previous = if previous == SEXP_NIL && self.offset > 0 {
                ctx.strings(self.classes.iter().map(String::as_str))?
            } else {
                previous
            };
            if previous != SEXP_NIL {
                let tag = ctx.session.intern("previous")?;
                ctx.session.set_attrib(class, tag, previous)?;
            }
            class
        };
        Ok(())
    }
}

struct Candidate {
    fun: SexpId,
    name: String,
    index: usize,
    default: bool,
    group_method: bool,
}

fn group_name(group: GroupGeneric) -> &'static str {
    match group {
        GroupGeneric::Ops => "Ops",
        GroupGeneric::Math => "Math",
        GroupGeneric::Summary => "Summary",
        GroupGeneric::Complex => "Complex",
        GroupGeneric::MatrixOps => "matrixOps",
    }
}

impl Ctx {
    /// Return the calling environment of the current closure frame.
    pub fn closure_caller(&self, env: SexpId) -> SexpId {
        self.frame(env).map_or(env, |frame| frame.caller)
    }

    /// Find an explicit S3 method for an object without invoking its default method.
    pub fn explicit_s3_method(
        &mut self,
        generic: &str,
        x: SexpId,
        caller: SexpId,
        definition: SexpId,
    ) -> EvalResult<Option<SexpId>> {
        for class in self.classes(x, false)? {
            if let Some(fun) =
                self.lookup_method(&format!("{generic}.{class}"), caller, definition)?
            {
                return Ok(Some(fun));
            }
        }
        Ok(None)
    }

    /// Return the namespace name for a registered package environment.
    pub fn namespace_name(&self, env: SexpId) -> Option<&'static str> {
        if env == BASE_NAMESPACE {
            return Some("base");
        }
        self.packages
            .iter()
            .find(|pkg| pkg.namespace == env)
            .map(|pkg| pkg.name)
    }

    pub(crate) fn classes(&mut self, value: SexpId, implicit: bool) -> EvalResult<Vec<String>> {
        let class_tag = self.session.intern("class")?;
        let explicit = self.session.attr(value, class_tag);
        if explicit != SEXP_NIL {
            return self
                .session
                .str_slice(explicit)?
                .iter()
                .map(|&id| Ok(String::from_utf8_lossy(self.session.char_bytes(id)?).into_owned()))
                .collect();
        }
        if !implicit {
            return Ok(Vec::new());
        }
        let mut classes = Vec::new();
        let dim_tag = self.session.intern("dim")?;
        let dim = self.session.attr(value, dim_tag);
        if dim != SEXP_NIL {
            if self.session.xlength(dim)? == 2 {
                classes.push("matrix");
            }
            classes.push("array");
        }
        match self.session.type_of(value)? {
            SexpType::Real => classes.extend(["double", "numeric"]),
            SexpType::Int => classes.extend(["integer", "numeric"]),
            SexpType::Builtin | SexpType::Special | SexpType::Clo => classes.push("function"),
            SexpType::Sym => classes.push("name"),
            SexpType::Lang => classes.push(self.session.call_class(value)?),
            ty => classes.push(ty.type_name(false)),
        }
        Ok(classes.into_iter().map(str::to_owned).collect())
    }

    fn optional_fun(
        &mut self,
        name: &str,
        mut env: SexpId,
        end: SexpId,
    ) -> EvalResult<Option<SexpId>> {
        let sym = self.session.intern(name)?;
        while env != SEXP_NIL && env != end {
            if !self.packages.iter().any(|pkg| pkg.attached == Some(env))
                && let Some(binding) = self.session.env_get(env, sym)
                && binding.value != SEXP_MISSING
                && binding.value != SEXP_UNBOUND
            {
                let value = self.read_binding(binding, env)?;
                if matches!(
                    self.session.type_of(value)?,
                    SexpType::Clo | SexpType::Builtin | SexpType::Special
                ) {
                    return Ok(Some(value));
                }
            }
            env = self.session.env_enclos(env)?;
        }
        Ok(None)
    }

    fn lookup_method(
        &mut self,
        generic: &str,
        name: &str,
        caller: SexpId,
        definition: SexpId,
        default: bool,
    ) -> EvalResult<Option<SexpId>> {
        if let Some(fun) = self.optional_fun(name, caller, BASE_ENV)? {
            return Ok(Some(fun));
        }
        let s3 = self.session.intern(".__S3MethodsTable__.")?;
        if let Some(table) = self.session.env_get(definition, s3) {
            let tag = self.session.intern(name)?;
            if let Some(fun) = self.session.env_get(table.value, tag) {
                return Ok(Some(fun.value));
            }
        }
        if definition != BASE_NAMESPACE && definition != BASE_ENV {
            return self.optional_fun(name, definition, self.session.env_enclos(definition)?);
        }
        // Built-in packages register S3 methods for these base generics until M3 NAMESPACE tables.
        let base_cross_pkg = !default
            && name != "format.pval"
            && matches!(
                generic,
                "print"
                    | "format"
                    | "summary"
                    | "diff"
                    | "as.matrix"
                    | "as.character"
                    | "as.data.frame"
                    | "labels"
                    | "["
                    | "[["
            );
        if base_cross_pkg {
            for index in 0..self.packages.len() {
                let ns = self.packages[index].namespace;
                if let Some(fun) = self.optional_fun(name, ns, self.session.env_enclos(ns)?)? {
                    return Ok(Some(fun));
                }
            }
        }
        self.optional_fun(name, BASE_ENV, SEXP_NIL)
    }

    fn find_method(
        &mut self,
        method: &MethodFrame,
        start: usize,
        default: bool,
    ) -> EvalResult<Option<Candidate>> {
        for (index, class) in method.classes.iter().enumerate().skip(start) {
            let mut names = vec![format!("{}.{class}", method.generic)];
            if let Some(group) = method.group {
                names.push(format!("{}.{class}", group_name(group)));
            }
            for (index_in_class, name) in names.into_iter().enumerate() {
                if let Some(fun) = self.lookup_method(
                    &method.generic,
                    &name,
                    method.caller,
                    method.definition,
                    false,
                )? {
                    return Ok(Some(Candidate {
                        fun,
                        name,
                        index,
                        default: false,
                        group_method: index_in_class != 0,
                    }));
                }
            }
        }
        if default {
            let name = format!("{}.default", method.generic);
            if let Some(fun) = self.lookup_method(
                &method.generic,
                &name,
                method.caller,
                method.definition,
                true,
            )? {
                return Ok(Some(Candidate {
                    fun,
                    name,
                    index: method.classes.len(),
                    default: true,
                    group_method: false,
                }));
            }
        }
        Ok(None)
    }

    fn invoke_method(
        &mut self,
        fun: SexpId,
        name: &str,
        args: Args,
        caller: SexpId,
        method: MethodFrame,
    ) -> EvalResult {
        let head = self.session.intern(name)?.as_sexp();
        let raw = args
            .iter()
            .map(|(tag, value)| Ok((tag, self.promise_expr(value)?)))
            .collect::<EvalResult<Vec<_>>>()?;
        let call = self.make_call(head, &Args::from_pairs(raw))?;
        if self.session.type_of(fun)? == SexpType::Clo {
            self.invoke_closure(fun, args, call, caller, Some(method))
        } else {
            let (prim, _) = self.session.prim_parts(fun)?;
            self.run_primitive(
                Call {
                    expr: call,
                    user_call: call,
                    env: caller,
                    prim,
                },
                args,
                ArgInput::Expressions,
                false,
            )
        }
    }

    pub(crate) fn dispatch_primitive(
        &mut self,
        def: &PrimDef,
        call: Call,
        args: &Args,
        promised: Option<&Args>,
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
        let candidates: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(_, (tag, _))| group != Some(GroupGeneric::Summary) || *tag != Some(na_rm))
            .take(if ops { 2 } else { 1 })
            .collect();
        if let Some(hooks) = self.s4_hooks {
            for &(_, (_, value)) in &candidates {
                if self.session.is_s4(value)? {
                    let symbol = self.session.intern(generic)?;
                    if let Some(result) = hooks.dispatch(self, symbol, &call, args)? {
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
        let mut pairs = Vec::with_capacity(args.len() + 1);
        for (i, (tag, value)) in args.iter().enumerate() {
            let expr = promised.map_or(Ok(value), |args| self.promise_expr(args.get(i)))?;
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

fn use_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).cloned() else {
        return Err(ctx.stop(
            call.err_call(),
            "'UseMethod' used in an inappropriate fashion",
        ));
    };
    let matched = ctx.match_names(args, &["generic", "object"], call.err_call())?;
    let generic = ctx.eval_arg(args, &matched, 0, call, SEXP_NIL)?;
    let generic = ctx.string(generic, call.err_call())?;
    let (formals, _, definition) = ctx.session.closure_parts(frame.fun)?;
    let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
    let object = if let Some(expr) = ctx.matched_arg(args, &matched, 1) {
        ctx.eval(expr, call.env)?
    } else {
        let matches = ctx.match_args(&formals, &frame.actuals, frame.call, false)?;
        let slot = matches.slots.first().copied().flatten().or_else(|| {
            let first_dots = formals
                .name(0)
                .is_some_and(|tag| ctx.session.sym_name(tag).ok() == Some("..."));
            if first_dots {
                matches.dots.first().copied()
            } else {
                None
            }
        });
        match slot.map(|i| frame.actuals.get(i)) {
            Some(value) if value != SEXP_MISSING => ctx.eval(value, frame.caller)?,
            _ => SEXP_NIL,
        }
    };
    let classes = ctx.classes(object, true)?;
    let mut method = MethodFrame {
        generic,
        group: None,
        group_method: false,
        names: vec![String::new()],
        classes,
        class: SEXP_NIL,
        offset: 0,
        side: 0,
        caller: frame.caller,
        definition,
        primitive: None,
        is_default: false,
    };
    let Some(candidate) = ctx.find_method(&method, 0, true)? else {
        let classes = if method.classes.len() == 1 {
            format!("\"{}\"", method.classes[0])
        } else {
            format!("c(\"{}\")", method.classes.join("\", \""))
        };
        return Err(ctx.stop(
            call.err_call(),
            format!(
                "no applicable method for '{}' applied to an object of class {classes}",
                method.generic
            ),
        ));
    };
    method.select(ctx, &candidate, SEXP_NIL)?;
    let value = ctx.invoke_method(
        candidate.fun,
        &candidate.name,
        frame.actuals,
        frame.caller,
        method,
    )?;
    Err(Unwind::Transfer {
        target: frame.id,
        kind: Transfer::Return(value),
    })
}

fn next_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).cloned() else {
        return Err(ctx.stop(
            call.err_call(),
            "'NextMethod' called from outside a function",
        ));
    };
    let Some(mut method) = frame.dispatch else {
        return Err(ctx.stop(
            call.err_call(),
            "'NextMethod' called from an anonymous function",
        ));
    };
    let args = ctx.promise_args(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    let matched = ctx.match_names(&args, &["generic", "object", "..."], call.err_call())?;
    if let Some(generic) = ctx.matched_arg(&args, &matched, 0) {
        let generic = ctx.eval(generic, call.env)?;
        if generic != SEXP_NIL {
            method.generic = ctx.string(generic, call.err_call())?;
        }
    }
    let (formals, _, _) = ctx.session.closure_parts(frame.fun)?;
    let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
    let formal_match = ctx.match_args(&formals, &frame.actuals, frame.call, false)?;
    let mut forwarded: Vec<_> = frame.actuals.iter().collect();
    for (position, &i) in formal_match.slots.iter().enumerate() {
        if let Some(i) = i
            && let Some(tag) = formals.name(position)
            && frame.actuals.get(i) != SEXP_MISSING
            && let Some(binding) = ctx.session.env_get(call.env, tag)
        {
            let old = frame.actuals.get(i);
            if old != binding.value {
                let expr = ctx.promise_expr(old)?;
                forwarded[i].1 = if ctx.session.type_of(binding.value)? == SexpType::Prom {
                    binding.value
                } else {
                    ctx.value_promise(expr, binding.value)?
                };
            }
        }
    }
    for i in matched.dots {
        let tag = args.name(i);
        let value = args.get(i);
        if let Some(slot) =
            tag.and_then(|tag| forwarded.iter_mut().find(|(name, _)| *name == Some(tag)))
        {
            slot.1 = value;
        } else {
            forwarded.push((tag, value));
        }
    }
    let forwarded = Args::from_pairs(forwarded);
    for &value in forwarded.values() {
        ctx.protect(value);
    }
    if !method.is_default
        && let Some(candidate) = ctx.find_method(&method, method.offset + 1, true)?
    {
        let old = method.class;
        method.select(ctx, &candidate, old)?;
        return ctx.invoke_method(
            candidate.fun,
            &candidate.name,
            forwarded,
            method.caller,
            method,
        );
    }
    if let Some(primitive) = method.primitive {
        let mut values = Vec::with_capacity(forwarded.len());
        for (tag, value) in forwarded.iter() {
            let value = if value == SEXP_MISSING {
                value
            } else {
                ctx.eval(value, call.env)?
            };
            ctx.protect(value);
            values.push((tag, value));
        }
        return ctx.run_primitive(
            primitive,
            Args::from_pairs(values),
            ArgInput::Values(None),
            false,
        );
    }
    let sym = ctx.session.intern(&method.generic)?;
    if let Some(binding) = ctx.session.env_get(BASE_ENV, sym)
        && matches!(
            ctx.session.type_of(binding.value)?,
            SexpType::Builtin | SexpType::Special
        )
    {
        let (prim, _) = ctx.session.prim_parts(binding.value)?;
        return ctx.run_primitive(
            Call { prim, ..*call },
            forwarded,
            ArgInput::Expressions,
            false,
        );
    }
    Err(ctx.stop(call.err_call(), "no method to invoke"))
}

fn choose_ops_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    for class in ctx.classes(args.get(0), true)? {
        let name = format!("chooseOpsMethod.{class}");
        if let Some(fun) = ctx.lookup_method(&name, call.env, BASE_NAMESPACE)? {
            let pairs = args.iter().collect::<Vec<_>>();
            return ctx.apply(fun, &pairs, call.expr, call.env);
        }
    }
    Ok(roxide_value::FALSE_SCALAR)
}
