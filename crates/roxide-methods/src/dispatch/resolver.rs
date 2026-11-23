use std::collections::HashMap;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{SEXP_MISSING, SexpType, ValueApi};

use super::metadata::{Method, attr, methods, texts};
use super::rank::{Ranked, distances};
use crate::registry::{self, GenericInfo, MethodsExt};

#[derive(Clone)]
pub(super) struct Class {
    pub(super) name: String,
    package: Option<String>,
}

impl Class {
    pub(super) fn unqualified(name: String) -> Self {
        Self {
            name,
            package: None,
        }
    }

    pub(super) fn of(ctx: &mut Ctx, value: SexpId) -> EvalResult<Self> {
        if value == SEXP_MISSING {
            return Ok(Self::unqualified("missing".to_owned()));
        }
        let class = attr(ctx, value, "class")?;
        if let Some(name) = texts(ctx, class)?.into_iter().next() {
            let package = attr(ctx, class, "package")?;
            return Ok(Self {
                name,
                package: texts(ctx, package)?.into_iter().next(),
            });
        }
        let dim = attr(ctx, value, "dim")?;
        let name = if ctx.session().xlength(dim)? == 2 {
            "matrix"
        } else if ctx.session().xlength(dim)? != 0 {
            "array"
        } else {
            let tag = ctx.session().type_of(value)?;
            match tag {
                SexpType::Real => "numeric",
                SexpType::Clo | SexpType::Builtin | SexpType::Special => "function",
                _ => tag.type_name(ctx.session().is_s4(value)?),
            }
        };
        Ok(Self::unqualified(name.to_owned()))
    }

    pub(super) fn ancestors(&self, ctx: &mut Ctx) -> Vec<(String, usize, usize)> {
        if self.name == "missing" || self.name == "ANY" {
            return Vec::new();
        }
        let parents = registry::superclasses(ctx, &self.name, self.package.as_deref());
        if !parents.is_empty()
            || registry::class_info(ctx, &self.name, self.package.as_deref()).is_some()
        {
            return parents;
        }
        let basic: &[&str] = match self.name.as_str() {
            "integer" | "double" => &["numeric", "vector"],
            "numeric" | "complex" | "character" | "logical" | "list" | "raw" | "expression" => {
                &["vector"]
            }
            "matrix" => &["array", "structure", "vector"],
            "array" => &["structure", "vector"],
            "factor" => &["integer", "numeric", "vector"],
            "ordered" => &["factor", "integer", "numeric", "vector"],
            _ => &[],
        };
        basic
            .iter()
            .enumerate()
            .map(|(i, name)| ((*name).to_owned(), i + 1, 0))
            .collect()
    }
}

type CacheKey = (String, String, Vec<String>);

pub(super) struct Selection {
    pub(super) inherited: bool,
    pub(super) next: bool,
    pub(super) cache: bool,
}

#[derive(Default)]
struct SelectionCache {
    epoch: u64,
    ordinary: HashMap<CacheKey, CachedMethod>,
    next: HashMap<CacheKey, (SexpId, Method)>,
}

struct CachedMethod {
    table: SexpId,
    defined: Vec<String>,
    packages: Vec<Option<String>>,
}

impl Trace for SelectionCache {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for item in self.ordinary.values() {
            tracer.edge(item.table);
        }
        for (table, method) in self.next.values() {
            tracer.edge(*table);
            tracer.edge(method.fun);
        }
    }
}

impl CtxExt for SelectionCache {}

pub(super) fn select(
    ctx: &mut Ctx,
    info: &GenericInfo,
    target: &[Class],
    opts: Selection,
    call: SexpId,
) -> EvalResult<Option<Method>> {
    let names = target.iter().map(|cl| cl.name.clone()).collect::<Vec<_>>();
    let key = (info.name.clone(), info.package.clone(), names.clone());
    let epoch = ctx.ext::<MethodsExt>().epoch;
    if opts.cache && opts.inherited {
        let state = ctx.ext::<SelectionCache>();
        if state.epoch != epoch {
            *state = SelectionCache {
                epoch,
                ..SelectionCache::default()
            };
        }
        if opts.next {
            if let Some((table, method)) = state.next.get(&key)
                && *table == info.table
            {
                return Ok(Some(method.clone()));
            }
        } else if let Some(cached) = state.ordinary.get(&key)
            && cached.table == info.table
            && cached
                .packages
                .iter()
                .zip(target)
                .all(|(package, class)| package == &class.package)
        {
            let signature = cached.defined.clone();
            if let Some(&(cached_epoch, fun)) = ctx.ext::<MethodsExt>().method_cache.get(&key)
                && cached_epoch == epoch
            {
                return Ok(Some(Method { signature, fun }));
            }
        }
    }
    let all = methods(ctx, info)?;
    if !opts.inherited {
        return Ok(all.into_iter().find(|method| method.signature == names));
    }
    let distances = target
        .iter()
        .map(|cl| distances(&cl.name, cl.ancestors(ctx)))
        .collect::<Vec<_>>();
    let mut ranked = all
        .into_iter()
        .filter(|method| !opts.next || method.signature != names)
        .filter_map(|method| {
            let score = Ranked::score(&method.signature, &distances)?;
            Some((score, method))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(a, _), (b, _)| a.order(b));
    let Some(((best, method), rest)) = ranked.split_first() else {
        return Ok(None);
    };
    let method = method.clone();
    ctx.protect(method.fun);
    ctx.protect(info.table);
    let alternatives = rest
        .iter()
        .take_while(|(alt, _)| alt.total() == best.total())
        .filter(|(alt, _)| best.ambiguous_with(alt))
        .collect::<Vec<_>>();
    if !alternatives.is_empty() {
        let alternatives = alternatives
            .iter()
            .map(|(_, alt)| format!("\"{}\"", alt.signature.join("#")))
            .collect::<Vec<_>>()
            .join(", ");
        let note = format!(
            "Note: method with signature ‘{}’ chosen for function ‘{}’,\n target signature ‘{}’.\n {alternatives} would also be valid\n",
            method.signature.join("#"),
            info.name,
            names.join("#")
        );
        ctx.message(call, note)?;
    }
    if opts.cache && ctx.ext::<MethodsExt>().epoch == epoch {
        if opts.next {
            ctx.ext::<SelectionCache>()
                .next
                .insert(key, (info.table, method.clone()));
        } else {
            ctx.ext::<SelectionCache>().ordinary.insert(
                key.clone(),
                CachedMethod {
                    table: info.table,
                    defined: method.signature.clone(),
                    packages: target.iter().map(|class| class.package.clone()).collect(),
                },
            );
            ctx.ext::<MethodsExt>()
                .method_cache
                .insert(key, (epoch, method.fun));
        }
    }
    Ok(Some(method))
}

pub(super) fn inherited(ctx: &mut Ctx, info: &GenericInfo) -> Vec<(Vec<String>, Method)> {
    let epoch = ctx.ext::<MethodsExt>().epoch;
    let state = ctx.ext::<SelectionCache>();
    if state.epoch != epoch {
        return Vec::new();
    }
    let matches = state
        .ordinary
        .iter()
        .filter(|(key, item)| item.table == info.table && key.2 != item.defined)
        .map(|(key, item)| (key.clone(), item.defined.clone()))
        .collect::<Vec<_>>();
    let cache = &ctx.ext::<MethodsExt>().method_cache;
    matches
        .into_iter()
        .filter_map(|(key, signature)| {
            let &(cached_epoch, fun) = cache.get(&key)?;
            (cached_epoch == epoch).then_some((key.2, Method { signature, fun }))
        })
        .collect()
}
