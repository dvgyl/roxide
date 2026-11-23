use std::collections::{HashMap, HashSet};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{DeparseOpts, RSession, SEXP_NIL, SexpType, ValueApi, deparse};

use super::{call, lang, strings};
use crate::distn::support::{attr, limit, logical, set_attr, set_dim, set_dimnames};

type Term = Vec<usize>;

fn args(s: &RSession, expr: SexpId) -> Vec<SexpId> {
    s.pairlist_iter(expr).skip(1).map(|(_, x)| x).collect()
}

fn operator(s: &RSession, expr: SexpId) -> &str {
    s.cons_parts(expr)
        .ok()
        .and_then(|(head, _, _)| s.sym_name_of(head).ok())
        .unwrap_or_default()
}

fn parts(ctx: &mut Ctx, call: &Call, formula: SexpId) -> EvalResult<(SexpId, SexpId)> {
    if operator(ctx.session(), formula) == "~" {
        match args(ctx.session(), formula).as_slice() {
            [rhs] => return Ok((SEXP_NIL, *rhs)),
            [lhs, rhs] => return Ok((*lhs, *rhs)),
            _ => {}
        }
    }
    Err(ctx.stop(call.err_call(), "invalid formula"))
}

fn mark_formula(ctx: &mut Ctx, expr: SexpId, env: SexpId, classes: &[&str]) -> EvalResult {
    let class = strings(ctx.session_mut(), classes.iter().copied())?;
    set_attr(ctx, expr, "class", class)?;
    set_attr(ctx, expr, ".Environment", env)?;
    Ok(expr)
}

fn build_formula(s: &mut RSession, lhs: SexpId, rhs: SexpId) -> EvalResult {
    if lhs == SEXP_NIL {
        lang(s, "~", &[rhs])
    } else {
        lang(s, "~", &[lhs, rhs])
    }
}

pub(super) fn tilde(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let formula = ctx.session_mut().shallow_duplicate(call.expr)?;
    mark_formula(ctx, formula, call.env, &["formula"])
}

pub(super) fn as_formula(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (lhs, rhs) = parts(ctx, call, args.get(0))?;
    let formula = build_formula(ctx.session_mut(), lhs, rhs)?;
    mark_formula(ctx, formula, args.get(1), &["formula"])
}

pub(super) fn from_names(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let names = ctx.session().str_slice(args.get(0))?.to_vec();
    let mut symbols = Vec::with_capacity(names.len());
    for name in names {
        let name = String::from_utf8_lossy(ctx.session().char_bytes(name)?).into_owned();
        symbols.push(ctx.session_mut().intern(&name)?.as_sexp());
    }
    let [lhs, rest @ ..] = symbols.as_slice() else {
        return Err(ctx.stop(
            call.err_call(),
            "cannot create a formula from a zero-column data frame",
        ));
    };
    let formula = if rest.is_empty() {
        build_formula(ctx.session_mut(), SEXP_NIL, *lhs)?
    } else {
        let rhs = sum(ctx.session_mut(), rest.iter().copied())?;
        build_formula(ctx.session_mut(), *lhs, rhs)?
    };
    mark_formula(ctx, formula, args.get(1), &["formula"])
}

fn sum(s: &mut RSession, mut xs: impl Iterator<Item = SexpId>) -> EvalResult {
    match xs.next() {
        Some(first) => xs.try_fold(first, |left, right| lang(s, "+", &[left, right])),
        None => Ok(s.scalar_real(1.0)?),
    }
}

fn label(s: &RSession, expr: SexpId) -> EvalResult<String> {
    let opts = DeparseOpts {
        width_cutoff: usize::MAX,
        keep_integer: false,
        ..DeparseOpts::default()
    };
    let mut name = deparse(s, expr, opts)?.join(" ");
    if s.type_of(expr)? == SexpType::Sym {
        let wrapped = s.sym_name_of(expr)?;
        let mut chars = wrapped.chars();
        let first = chars.next();
        let valid_first = first.is_some_and(|c| c.is_alphabetic() || c == '.')
            && !(first == Some('.') && chars.clone().next().is_some_and(|c| c.is_ascii_digit()));
        if !valid_first || !chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            name = format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"));
        }
    }
    Ok(name)
}

fn union(left: &[usize], right: &[usize]) -> Term {
    let mut result = left.to_vec();
    result.extend_from_slice(right);
    result.sort_unstable();
    result.dedup();
    result
}

fn append_terms(output: &mut Vec<Term>, seen: &mut HashSet<Term>, terms: Vec<Term>) {
    output.extend(terms.into_iter().filter(|term| seen.insert(term.clone())));
}

struct Algebra<'a> {
    ctx: &'a mut Ctx,
    site: &'a Call,
    variables: Vec<SexpId>,
    labels: Vec<String>,
    indices: HashMap<String, usize>,
    offsets: Vec<usize>,
    intercept: bool,
    dot: Option<Vec<SexpId>>,
    allow_dot: bool,
    budget: usize,
}

impl Algebra<'_> {
    fn error(&mut self, message: &str) -> roxide_eval::Unwind {
        self.ctx.stop(self.site.err_call(), message)
    }

    fn spend(&mut self, steps: usize) -> EvalResult<()> {
        self.budget = self
            .budget
            .checked_sub(steps)
            .ok_or_else(|| self.error("formula expansion exceeds size limit"))?;
        Ok(())
    }

    fn variable(&mut self, expr: SexpId) -> EvalResult<usize> {
        let name = label(self.ctx.session(), expr)?;
        if let Some(&index) = self.indices.get(&name) {
            return Ok(index);
        }
        let index = self.variables.len();
        self.indices.insert(name.clone(), index);
        self.variables.push(expr);
        self.labels.push(name);
        if operator(self.ctx.session(), expr) == "offset" {
            self.offsets.push(index);
        }
        Ok(index)
    }

    fn cross(&mut self, left: &[Term], right: &[Term]) -> EvalResult<Vec<Term>> {
        self.spend(left.len().saturating_mul(right.len()))?;
        let mut seen = HashSet::new();
        Ok(left
            .iter()
            .flat_map(|l| right.iter().map(move |r| union(l, r)))
            .filter(|term| seen.insert(term.clone()))
            .collect())
    }

    fn power_terms(
        &mut self,
        items: &[Term],
        degree: usize,
        start: usize,
        prefix: &[usize],
        seen: &mut HashSet<Term>,
        result: &mut Vec<Term>,
    ) -> EvalResult<()> {
        for index in start..items.len() {
            self.spend(1)?;
            let term = union(prefix, &items[index]);
            if seen.insert(term.clone()) {
                result.push(term.clone());
            }
            if degree > 1 && term != prefix {
                self.power_terms(items, degree - 1, index + 1, &term, seen, result)?;
            }
        }
        Ok(())
    }

    fn binary(&self, expr: SexpId, op: &str) -> Option<(SexpId, SexpId)> {
        let s = self.ctx.session();
        if operator(s, expr) == op
            && let [left, right] = args(s, expr).as_slice()
        {
            return Some((*left, *right));
        }
        None
    }

    fn chain(&mut self, expr: SexpId, op: &str, add: bool, depth: usize) -> EvalResult<Vec<Term>> {
        let mut pending = vec![expr];
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        while let Some(node) = pending.pop() {
            if let Some((left, right)) = self.binary(node, op) {
                self.spend(1)?;
                pending.extend([right, left]);
            } else {
                let terms = self.walk(node, add, depth + 1)?;
                self.spend(terms.len())?;
                append_terms(&mut output, &mut seen, terms);
            }
        }
        Ok(output)
    }

    fn merge(&mut self, left: &mut Vec<Term>, right: Vec<Term>) -> EvalResult<()> {
        self.spend(left.len().saturating_add(right.len()))?;
        let mut seen = left.iter().cloned().collect();
        append_terms(left, &mut seen, right);
        Ok(())
    }

    fn walk(&mut self, expr: SexpId, add: bool, depth: usize) -> EvalResult<Vec<Term>> {
        self.spend(1)?;
        if depth >= 128 {
            return Err(self.error("formula nesting is too deep"));
        }
        let s = self.ctx.session();
        match s.type_of(expr)? {
            SexpType::Real | SexpType::Int => {
                match number(s, expr) {
                    Some(0.0) => self.intercept = !add,
                    Some(1.0) => self.intercept = add,
                    _ => return Err(self.error("invalid model formula in ExtractVars")),
                }
                return Ok(Vec::new());
            }
            SexpType::Sym if s.sym_name_of(expr)? == "." => {
                if let Some(dot) = self.dot.clone() {
                    return dot
                        .into_iter()
                        .map(|x| Ok(vec![self.variable(x)?]))
                        .collect();
                }
                if !self.allow_dot {
                    return Err(self.error("'.' in formula and no 'data' argument"));
                }
            }
            SexpType::Sym | SexpType::Lang => {}
            _ => return Err(self.error("invalid model formula in ExtractVars")),
        }

        let op = operator(s, expr).to_owned();
        if matches!(
            op.as_str(),
            "(" | "+" | "-" | "*" | ":" | "/" | "^" | "%in%"
        ) {
            match args(s, expr).as_slice() {
                [inner] if matches!(op.as_str(), "(" | "+" | "-") => {
                    let terms = self.walk(*inner, if op == "-" { !add } else { add }, depth + 1)?;
                    return Ok(if op == "-" { Vec::new() } else { terms });
                }
                [_, _] if op == "+" => return self.chain(expr, &op, add, depth),
                [_, _] if matches!(op.as_str(), "*" | ":") => {
                    return self.products(expr, &op, add, depth);
                }
                [lhs, rhs] => {
                    let mut left = self.walk(*lhs, add, depth + 1)?;
                    if op == "^" {
                        let Some(degree) = number(self.ctx.session(), *rhs)
                            .filter(|n| n.is_finite() && *n > 0.0 && n.fract() == 0.0)
                        else {
                            return Err(self.error("invalid power in formula"));
                        };
                        let mut result = Vec::new();
                        let degree = (degree as usize).min(left.len()).min(128);
                        self.power_terms(&left, degree, 0, &[], &mut HashSet::new(), &mut result)?;
                        return Ok(result);
                    }
                    let right = self.walk(*rhs, if op == "-" { !add } else { add }, depth + 1)?;
                    return match op.as_str() {
                        "-" => {
                            let removed: HashSet<_> = right.into_iter().collect();
                            left.retain(|term| !removed.contains(term));
                            Ok(left)
                        }
                        "/" => {
                            if !left.is_empty() {
                                let nested = self.cross(&[all_variables(&left)], &right)?;
                                self.merge(&mut left, nested)?;
                            }
                            Ok(left)
                        }
                        "%in%" => self.cross(&left, &[all_variables(&right)]),
                        _ => Err(self.error("invalid model formula")),
                    };
                }
                _ => {}
            }
        }
        Ok(vec![vec![self.variable(expr)?]])
    }

    fn products(
        &mut self,
        expr: SexpId,
        op: &str,
        add: bool,
        depth: usize,
    ) -> EvalResult<Vec<Term>> {
        let mut pending = vec![(expr, false)];
        let mut values: Vec<Vec<Term>> = Vec::new();
        while let Some((node, combine)) = pending.pop() {
            if combine {
                let right = values.pop().unwrap_or_default();
                let mut left = values.pop().unwrap_or_default();
                let cross = self.cross(&left, &right)?;
                if op == "*" {
                    self.merge(&mut left, right)?;
                    self.merge(&mut left, cross)?;
                    values.push(left);
                } else {
                    values.push(cross);
                }
            } else if let Some((left, right)) = self.binary(node, op) {
                self.spend(1)?;
                pending.extend([(node, true), (right, false), (left, false)]);
            } else {
                values.push(self.walk(node, add, depth + 1)?);
            }
        }
        Ok(values.pop().unwrap_or_default())
    }

    fn expanded(&mut self, expr: SexpId, nested: bool, depth: usize) -> EvalResult {
        self.spend(1)?;
        if depth >= 128 {
            return Err(self.error("formula nesting is too deep"));
        }
        let s = self.ctx.session();
        if s.sym_name_of(expr).ok() == Some(".") {
            if let Some(dot) = &self.dot
                && !dot.is_empty()
            {
                let replacement = sum(self.ctx.session_mut(), dot.iter().copied())?;
                return group(self.ctx.session_mut(), replacement, nested && dot.len() > 1);
            }
            return Ok(expr);
        }
        let op = operator(s, expr);
        if !matches!(op, "(" | "+" | "-" | "*" | ":" | "/" | "^" | "%in%") {
            return Ok(expr);
        }
        let op = op.to_owned();
        if matches!(op.as_str(), "+" | "*" | ":") && self.binary(expr, &op).is_some() {
            return self.expand_chain(expr, &op, nested, depth);
        }
        let (head, _, _) = s.cons_parts(expr)?;
        let children = args(s, expr)
            .into_iter()
            .enumerate()
            .map(|(i, arg)| self.expanded(arg, op != "(" && (op != "+" || i != 0), depth + 1))
            .collect::<EvalResult<Vec<_>>>()?;
        call(
            self.ctx.session_mut(),
            head,
            children.into_iter().map(|x| (SEXP_NIL, x)),
        )
    }
    fn expand_chain(&mut self, expr: SexpId, op: &str, nested: bool, depth: usize) -> EvalResult {
        let mut pending = vec![(expr, nested, false)];
        let mut values = Vec::new();
        while let Some((node, nested, combine)) = pending.pop() {
            if combine {
                let right = values.pop().unwrap_or(SEXP_NIL);
                let left = values.pop().unwrap_or(SEXP_NIL);
                values.push(lang(self.ctx.session_mut(), op, &[left, right])?);
            } else if let Some((left, right)) = self.binary(node, op) {
                self.spend(1)?;
                pending.extend([
                    (node, nested, true),
                    (right, true, false),
                    (left, op != "+", false),
                ]);
            } else {
                values.push(self.expanded(node, nested, depth + 1)?);
            }
        }
        Ok(values.pop().unwrap_or(expr))
    }
}

fn group(s: &mut RSession, value: SexpId, needed: bool) -> EvalResult {
    if needed {
        lang(s, "(", &[value])
    } else {
        Ok(value)
    }
}

fn number(s: &RSession, value: SexpId) -> Option<f64> {
    s.real(value)
        .ok()
        .and_then(|x| (x.len() == 1).then(|| x.get(0)))
        .or_else(|| {
            s.int(value)
                .ok()
                .and_then(|x| (x.len() == 1).then(|| x.get(0)))
                .map(f64::from)
        })
}

fn all_variables(terms: &[Term]) -> Term {
    let mut variables: Vec<_> = terms.iter().flatten().copied().collect();
    variables.sort_unstable();
    variables.dedup();
    variables
}

fn symbols(s: &RSession, expr: SexpId, out: &mut Vec<SexpId>) {
    let mut seen = HashSet::new();
    let mut pending = vec![(expr, 0)];
    while let Some((expr, depth)) = pending.pop() {
        if depth >= 128 || !seen.insert(expr) {
            continue;
        }
        match s.type_of(expr).ok() {
            Some(SexpType::Sym) => out.push(expr),
            Some(SexpType::Lang) => {
                let op = operator(s, expr);
                let children = args(s, expr);
                let depth =
                    depth + usize::from(!matches!(op, "+" | "*" | ":") || children.len() != 2);
                pending.extend(children.into_iter().rev().map(|x| (x, depth)));
            }
            _ => {}
        }
    }
}

fn term_expr(s: &mut RSession, vars: &[SexpId], term: &Term) -> EvalResult {
    let mut components = term.iter().map(|&index| vars[index]);
    match components.next() {
        Some(first) => components.try_fold(first, |left, right| lang(s, ":", &[left, right])),
        None => Ok(s.scalar_real(1.0)?),
    }
}

fn simplified(
    s: &mut RSession,
    vars: &[SexpId],
    terms: &[Term],
    offsets: &[usize],
    intercept: bool,
) -> EvalResult {
    let mut exprs = terms
        .iter()
        .map(|term| term_expr(s, vars, term))
        .collect::<EvalResult<Vec<_>>>()?;
    exprs.extend(offsets.iter().map(|&index| vars[index]));
    let rhs = sum(s, exprs.into_iter())?;
    if intercept {
        Ok(rhs)
    } else {
        let one = s.scalar_real(1.0)?;
        lang(s, "-", &[rhs, one])
    }
}

fn indices(ctx: &mut Ctx, values: impl IntoIterator<Item = usize>) -> EvalResult {
    let vals = values
        .into_iter()
        .map(|i| i32::try_from(i + 1))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ctx.stop(SEXP_NIL, "formula has too many variables"))?;
    Ok(ctx.session_mut().alloc_int_from(&vals)?)
}

pub(super) fn terms(ctx: &mut Ctx, site: &Call, args: &mut Args) -> EvalResult {
    let (lhs, rhs) = parts(ctx, site, args.get(0))?;
    let env = attr(ctx, args.get(0), ".Environment")?;
    let mut response_names = Vec::new();
    symbols(ctx.session(), lhs, &mut response_names);
    let dot = if args.get(1) == SEXP_NIL {
        None
    } else {
        let names = ctx.session().str_slice(args.get(1))?.to_vec();
        let mut dot = Vec::new();
        for name in names {
            let name = String::from_utf8_lossy(ctx.session().char_bytes(name)?).into_owned();
            let sym = ctx.session_mut().intern(&name)?.as_sexp();
            if !response_names.contains(&sym) && !dot.contains(&sym) {
                dot.push(sym);
            }
        }
        Some(dot)
    };
    let keep_order = logical(ctx, site, args.get(3))? == Some(true);
    let simplify = logical(ctx, site, args.get(4))? == Some(true);
    let allow_dot = logical(ctx, site, args.get(5))? == Some(true);
    let budget = (ctx.host().limits().max_vector_bytes / size_of::<Term>()).min(1_000_000);
    let mut algebra = Algebra {
        ctx,
        site,
        variables: Vec::new(),
        labels: Vec::new(),
        indices: HashMap::new(),
        offsets: Vec::new(),
        intercept: true,
        dot,
        allow_dot,
        budget,
    };
    if lhs != SEXP_NIL {
        algebra.variable(lhs)?;
        algebra.offsets.clear();
    }
    let mut terms = algebra.walk(rhs, true, 0)?;
    terms.retain(|term| !algebra.offsets.iter().any(|offset| term.contains(offset)));
    if !keep_order {
        terms.sort_by_key(Vec::len);
    }
    let rhs = if simplify {
        simplified(
            algebra.ctx.session_mut(),
            &algebra.variables,
            &terms,
            &algebra.offsets,
            algebra.intercept,
        )?
    } else {
        algebra.expanded(rhs, false, 0)?
    };
    let size = terms.len().saturating_mul(algebra.variables.len());
    // Include dense factors in the work limit while allowing moderately wide additive models.
    algebra.spend(size.div_ceil(32))?;
    let Algebra {
        ctx,
        variables,
        labels,
        offsets,
        intercept,
        budget,
        ..
    } = algebra;
    let mut remaining = budget;
    let result = build_formula(ctx.session_mut(), lhs, rhs)?;
    let vars = lang(ctx.session_mut(), "list", &variables)?;
    set_attr(ctx, result, "variables", vars)?;
    if !offsets.is_empty() {
        let offset = indices(ctx, offsets)?;
        set_attr(ctx, result, "offset", offset)?;
    }
    let term_labels = terms
        .iter()
        .map(|term| {
            term.iter()
                .map(|&index| labels[index].as_str())
                .collect::<Vec<_>>()
                .join(":")
        })
        .collect::<Vec<_>>();
    let col_names = strings(ctx.session_mut(), term_labels.iter().map(String::as_str))?;
    limit::<i32>(ctx, site, size)?;
    let mut factors = Vec::with_capacity(size);
    let mut preceding: HashSet<&[usize]> = HashSet::new();
    let mut margins = HashSet::new();
    let mut margin = Vec::new();
    let mut longest = 0;
    for (index, term) in terms.iter().enumerate() {
        let start = factors.len();
        factors.resize(start + variables.len(), 0);
        for (i, &var) in term.iter().enumerate() {
            margin.clear();
            margin.extend_from_slice(&term[..i]);
            margin.extend_from_slice(&term[i + 1..]);
            let mut earlier = margin.is_empty()
                || preceding.contains(margin.as_slice())
                || margins.contains(margin.as_slice());
            if !earlier && longest > term.len() {
                for prior in &terms[..index] {
                    remaining = remaining.checked_sub(1).ok_or_else(|| {
                        ctx.stop(site.err_call(), "formula expansion exceeds size limit")
                    })?;
                    if prior.len() > term.len()
                        && margin.iter().all(|v| prior.binary_search(v).is_ok())
                    {
                        earlier = true;
                        break;
                    }
                }
            }
            factors[start + var] = if earlier { 1 } else { 2 };
        }
        for i in 0..term.len() {
            margin.clear();
            margin.extend_from_slice(&term[..i]);
            margin.extend_from_slice(&term[i + 1..]);
            margins.insert(margin.clone());
        }
        preceding.insert(term.as_slice());
        longest = longest.max(term.len());
    }
    let factors = ctx.session_mut().alloc_int_from(&factors)?;
    if !terms.is_empty() {
        let row_names = strings(ctx.session_mut(), labels.iter().map(String::as_str))?;
        set_dim(ctx, site, factors, labels.len(), terms.len())?;
        set_dimnames(ctx, factors, row_names, col_names)?;
    }
    set_attr(ctx, result, "factors", factors)?;
    set_attr(ctx, result, "term.labels", col_names)?;
    if args.get(2) != SEXP_NIL {
        let specials = ctx.session().str_slice(args.get(2))?.to_vec();
        let mut list = SEXP_NIL;
        for name in specials.into_iter().rev() {
            let name = String::from_utf8_lossy(ctx.session().char_bytes(name)?).into_owned();
            let tag = ctx.session_mut().intern(&name)?.as_sexp();
            let matches = variables
                .iter()
                .enumerate()
                .filter_map(|(i, &expr)| (operator(ctx.session(), expr) == name).then_some(i))
                .collect::<Vec<_>>();
            let value = if matches.is_empty() {
                SEXP_NIL
            } else {
                indices(ctx, matches)?
            };
            list = ctx.session_mut().cons(value, list, tag)?;
        }
        set_attr(ctx, result, "specials", list)?;
    }
    let orders = indices(ctx, terms.iter().map(|term| term.len() - 1))?;
    set_attr(ctx, result, "order", orders)?;
    let int = ctx.session_mut().scalar_int(i32::from(intercept))?;
    set_attr(ctx, result, "intercept", int)?;
    let response = ctx.session_mut().scalar_int(i32::from(lhs != SEXP_NIL))?;
    set_attr(ctx, result, "response", response)?;
    mark_formula(ctx, result, env, &["terms", "formula"])
}

fn substitute_dot(
    ctx: &mut Ctx,
    site: &Call,
    expr: SexpId,
    replacement: SexpId,
    depth: usize,
) -> EvalResult {
    if depth >= 128 {
        return Err(ctx.stop(site.err_call(), "formula nesting is too deep"));
    }
    let s = ctx.session();
    if s.sym_name_of(expr).ok() == Some(".") && replacement != SEXP_NIL {
        return Ok(replacement);
    }
    if s.type_of(expr)? != SexpType::Lang {
        return Ok(expr);
    }
    let mut parts = s.pairlist_iter(expr);
    let Some((_, head)) = parts.next() else {
        return Ok(expr);
    };
    let args = parts.collect::<Vec<_>>();
    let args = args
        .into_iter()
        .map(|(tag, value)| {
            let value = substitute_dot(ctx, site, value, replacement, depth + 1)?;
            Ok((tag.map_or(SEXP_NIL, |sym| sym.as_sexp()), value))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    call(ctx.session_mut(), head, args.into_iter())
}

pub(super) fn update(ctx: &mut Ctx, site: &Call, args: &mut Args) -> EvalResult {
    let old = args.get(0);
    let (old_lhs, old_rhs) = parts(ctx, site, old)?;
    let (new_lhs, new_rhs) = parts(ctx, site, args.get(1))?;
    let lhs = if new_lhs == SEXP_NIL {
        old_lhs
    } else {
        substitute_dot(ctx, site, new_lhs, old_lhs, 0)?
    };
    let rhs = substitute_dot(ctx, site, new_rhs, old_rhs, 0)?;
    let env = attr(ctx, old, ".Environment")?;
    let expr = build_formula(ctx.session_mut(), lhs, rhs)?;
    mark_formula(ctx, expr, env, &["formula"])
}

fn drop_first_call_arg(ctx: &mut Ctx, x: SexpId) -> EvalResult {
    if x == SEXP_NIL {
        return Ok(x);
    }
    let (head, tail, _) = ctx.session().cons_parts(x)?;
    let tail = if tail == SEXP_NIL {
        tail
    } else {
        ctx.session().cons_parts(tail)?.1
    };
    Ok(ctx.session_mut().lcons(head, tail, SEXP_NIL)?)
}

pub(super) fn delete_response(ctx: &mut Ctx, site: &Call, input: &mut Args) -> EvalResult {
    let source = input.get(0);
    let (lhs, rhs) = parts(ctx, site, source)?;
    if lhs == SEXP_NIL {
        return Ok(source);
    }
    let result = build_formula(ctx.session_mut(), SEXP_NIL, rhs)?;
    crate::distn::support::copy_attrs(ctx, source, result)?;
    for name in ["variables", "predvars"] {
        let from = attr(ctx, source, name)?;
        let to = drop_first_call_arg(ctx, from)?;
        set_attr(ctx, result, name, to)?;
    }
    let zero = ctx.session_mut().scalar_int(0)?;
    set_attr(ctx, result, "response", zero)?;
    let factors = attr(ctx, source, "factors")?;
    let dims = attr(ctx, factors, "dim")?;
    if let Ok(dims) = ctx.session().int(dims)
        && dims.len() == 2
    {
        let (rows, cols) = (dims.get(0) as usize, dims.get(1) as usize);
        if rows > 0 {
            let dim_names = attr(ctx, factors, "dimnames")?;
            let row_names = ctx.session().list_get(dim_names, 0)?;
            let col_names = ctx.session().list_get(dim_names, 1)?;
            let row_names = ctx
                .session()
                .str_slice(row_names)?
                .iter()
                .skip(1)
                .copied()
                .collect::<Vec<_>>();
            let row_names = ctx.session_mut().alloc_str_from(&row_names)?;
            let vals = ctx
                .session()
                .int(factors)?
                .iter()
                .enumerate()
                .filter_map(|(i, value)| (i % rows != 0).then_some(value))
                .collect::<Vec<_>>();
            let new_factors = ctx.session_mut().alloc_int_from(&vals)?;
            set_dim(ctx, site, new_factors, rows - 1, cols)?;
            set_dimnames(ctx, new_factors, row_names, col_names)?;
            set_attr(ctx, result, "factors", new_factors)?;
        }
    }
    let offsets = attr(ctx, source, "offset")?;
    if offsets != SEXP_NIL {
        let offsets = ctx
            .session()
            .int(offsets)?
            .iter()
            .map(|i| i - 1)
            .collect::<Vec<_>>();
        let offsets = ctx.session_mut().alloc_int_from(&offsets)?;
        set_attr(ctx, result, "offset", offsets)?;
    }
    let specials = attr(ctx, source, "specials")?;
    if specials != SEXP_NIL {
        let specs = ctx.session().pairlist_iter(specials).collect::<Vec<_>>();
        let variables = attr(ctx, source, "variables")?;
        let has_variables = !args(ctx.session(), variables).is_empty();
        let mut out = SEXP_NIL;
        for (tag, value) in specs.into_iter().rev() {
            let value = if value == SEXP_NIL && has_variables {
                ctx.session_mut().alloc_lgl_from(&[])?
            } else if value != SEXP_NIL {
                let values = ctx
                    .session()
                    .int(value)?
                    .iter()
                    .filter(|&i| i > 1)
                    .map(|i| i - 1)
                    .collect::<Vec<_>>();
                ctx.session_mut().alloc_int_from(&values)?
            } else {
                value
            };
            out = ctx
                .session_mut()
                .cons(value, out, tag.map_or(SEXP_NIL, |sym| sym.as_sexp()))?;
        }
        set_attr(ctx, result, "specials", out)?;
    }
    Ok(result)
}

pub(super) fn drop_terms(ctx: &mut Ctx, site: &Call, input: &mut Args) -> EvalResult {
    let source = input.get(0);
    let (lhs, _) = parts(ctx, site, source)?;
    let lhs = if logical(ctx, site, input.get(2))? == Some(true) {
        lhs
    } else {
        SEXP_NIL
    };
    let variables = attr(ctx, source, "variables")?;
    let vars = args(ctx.session(), variables);
    let factors = attr(ctx, source, "factors")?;
    let offsets = attr(ctx, source, "offset")?;
    let offsets = if offsets == SEXP_NIL {
        Vec::new()
    } else {
        ctx.session()
            .int(offsets)?
            .iter()
            .filter_map(|i| offset_index(i, vars.len()))
            .collect()
    };
    let (removed, _) = ctx.session_mut().coerce(input.get(1), SexpType::Int)?;
    let removed = ctx.session().int(removed)?.iter().collect::<HashSet<_>>();
    let codes = ctx.session().int(factors)?.iter().collect::<Vec<_>>();
    let mut terms = Vec::new();
    if !vars.is_empty() {
        for (i, column) in codes.chunks_exact(vars.len()).enumerate() {
            if !removed.contains(&((i + 1) as i32)) {
                terms.push(
                    column
                        .iter()
                        .enumerate()
                        .filter_map(|(j, &c)| (c != 0).then_some(j))
                        .collect(),
                );
            }
        }
    }
    let intercept = attr(ctx, source, "intercept")?;
    let intercept = ctx.session().int(intercept)?.iter().next() == Some(1);
    let rhs = simplified(ctx.session_mut(), &vars, &terms, &offsets, intercept)?;
    let env = attr(ctx, source, ".Environment")?;
    let expr = build_formula(ctx.session_mut(), lhs, rhs)?;
    mark_formula(ctx, expr, env, &["formula"])
}

fn offset_index(index: i32, length: usize) -> Option<usize> {
    usize::try_from(index)
        .ok()?
        .checked_sub(1)
        .filter(|&index| index < length)
}

pub(super) fn labels(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let names = ctx
        .session()
        .list_slice(args.get(0))?
        .iter()
        .map(|&value| match ctx.session().sym_name_of(value) {
            Ok(name) => Ok(name.to_owned()),
            Err(_) => label(ctx.session(), value),
        })
        .collect::<EvalResult<Vec<_>>>()?;
    strings(ctx.session_mut(), names.iter().map(String::as_str))
}

pub(super) fn all_symbols(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let mut output = Vec::new();
    symbols(ctx.session(), args.get(0), &mut output);
    let names = output
        .iter()
        .map(|&sym| Ok(ctx.session().sym_name_of(sym)?.to_owned()))
        .collect::<EvalResult<Vec<_>>>()?;
    strings(ctx.session_mut(), names.iter().map(String::as_str))
}
