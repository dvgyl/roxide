use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, SEXP_NIL, SexpType, ValueApi};

use crate::control::logical;
use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    /// Read a single character string that has a value.
    pub(crate) fn string(&mut self, value: SexpId, call: SexpId) -> EvalResult<String> {
        if let Ok([id]) = self.session.str_slice(value)
            && *id != CharId::NA
        {
            return Ok(String::from_utf8_lossy(self.session.char_bytes(*id)?).into_owned());
        }
        Err(self.stop(call, "invalid character argument"))
    }

    /// Read a symbol name or a single character string.
    pub(crate) fn name(&mut self, value: SexpId, call: SexpId) -> EvalResult<String> {
        match self.session.sym_name_of(value) {
            Ok(name) => Ok(name.to_owned()),
            Err(_) => self.string(value, call),
        }
    }

    /// Convert the value to one integer that has a value.
    pub(crate) fn integer(&mut self, value: SexpId, call: SexpId) -> EvalResult<i32> {
        let (int, _) = self.session.coerce(value, SexpType::Int)?;
        if self.session.xlength(int)? != 1
            || self.session.int(int)?.get(0) == roxide_value::NA_INTEGER
        {
            return Err(self.stop(call, "invalid integer argument"));
        }
        Ok(self.session.int(int)?.get(0))
    }

    /// Create a character vector with native encoding.
    pub(crate) fn strings<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
        let strings = values
            .into_iter()
            .map(|value| self.session.mk_char(value.as_bytes(), Encoding::Native))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.session.alloc_str_from(&strings)?)
    }

    /// Create a list from the argument values and names.
    pub(crate) fn list(&mut self, args: &Args) -> EvalResult {
        let list = self.session.alloc_list(args.len())?;
        for (i, &value) in args.values().iter().enumerate() {
            self.session.list_set(list, i, value)?;
        }
        if args.iter().any(|(tag, _)| tag.is_some()) {
            let tags = args
                .iter()
                .map(|(tag, _)| tag.map_or(Ok(""), |tag| self.session.sym_name(tag)))
                .collect::<Result<Vec<_>, _>>()?;
            let tags: Vec<_> = tags.iter().map(|tag| (*tag).to_owned()).collect();
            let names = self.strings(tags.iter().map(String::as_str))?;
            let names_sym = self.session.intern("names")?;
            self.session.set_attrib(list, names_sym, names)?;
        }
        Ok(list)
    }

    /// Read the values and names of a list or pairlist as arguments.
    pub(crate) fn list_args(&mut self, list: SexpId) -> EvalResult<Args> {
        if matches!(self.session.type_of(list)?, SexpType::Vec | SexpType::Expr) {
            let names_sym = self.session.intern("names")?;
            let names = self.session.attr(list, names_sym);
            let tags = if names == SEXP_NIL {
                Vec::new()
            } else {
                self.session.str_slice(names)?.to_vec()
            };
            let mut pairs = Vec::new();
            for i in 0..self.session.xlength(list)? {
                let tag = if let Some(&name) = tags.get(i) {
                    let name = self.session.char_bytes(name)?;
                    if name.is_empty() || tags[i] == CharId::NA {
                        None
                    } else {
                        let name = String::from_utf8_lossy(name).into_owned();
                        Some(self.session.intern(&name)?)
                    }
                } else {
                    None
                };
                pairs.push((tag, self.session.list_get(list, i)?));
            }
            Ok(Args::from_pairs(pairs))
        } else {
            Ok(Args::from_pairs(self.session.pairlist_iter(list).collect()))
        }
    }
}

pub(super) fn alist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = ctx.expand_dots(args, call.env, call.err_call())?;
    let pairs = args
        .iter()
        .map(|(tag, value)| Ok((tag, ctx.promise_expr(value)?)))
        .collect::<EvalResult<Vec<_>>>()?;
    ctx.list(&Args::from_pairs(pairs))
}

pub(super) fn as_pairlist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut value = args.get(0);
    match ctx.session.type_of(value)? {
        SexpType::List | SexpType::Nil => return Ok(value),
        SexpType::Vec | SexpType::Expr => {}
        _ => {
            value = ctx
                .session
                .coerce(value, SexpType::Vec)
                .map_err(|_| ctx.stop(call.err_call(), "cannot coerce to pairlist"))?
                .0;
        }
    }
    let pairs = ctx.list_args(value)?;
    ctx.pairlist(&pairs, false)
}

pub(super) fn construct_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.string(args.get(0), call.err_call())?;
    let head = ctx.session.intern(&name)?.as_sexp();
    ctx.make_call(head, &Args::from_pairs(args.iter().skip(1).collect()))
}

pub(super) fn as_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    if ctx.session.type_of(value)? == SexpType::Lang {
        return Ok(value);
    }
    let pairs = ctx.list_args(value)?;
    if pairs.is_empty() {
        return Err(ctx.stop(call.err_call(), "invalid length 0 argument"));
    }
    ctx.make_call(
        pairs.get(0),
        &Args::from_pairs(pairs.iter().skip(1).collect()),
    )
}

pub(super) fn language_predicate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use SexpType::*;
    let value = args.get(0);
    let ty = ctx.session.type_of(value)?;
    let flag = match ctx.prims[usize::from(call.prim.0)].name {
        "is.primitive" => matches!(ty, Builtin | Special),
        "is.call" => ty == Lang,
        "is.name" | "is.symbol" => ty == Sym,
        "is.pairlist" => matches!(ty, List | Nil),
        "is.expression" => ty == Expr,
        "is.language" => matches!(ty, Sym | Lang | Expr),
        "is.recursive" => matches!(
            ty,
            Vec | List | Expr | Lang | Clo | Env | Prom | Dot | Builtin | Special
        ),
        _ => ctx.session.is_object(value)?,
    };
    Ok(ctx.session.scalar_lgl(logical(flag))?)
}
