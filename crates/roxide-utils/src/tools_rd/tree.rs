use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{EMPTY_ENV, RLogical, SEXP_NIL, SexpType, ValueApi};

use super::parser::{Body, Node, Parser};
use super::{attr, chars, first_text, set_attr, set_str};

pub(super) fn read_macros(
    ctx: &mut Ctx,
    value: SexpId,
    parser: &mut Parser,
) -> EvalResult<(bool, SexpId)> {
    match ctx.session().type_of(value)? {
        SexpType::Lgl => Ok((
            ctx.session().lgl(value)?.iter().next() != Some(RLogical::FALSE),
            EMPTY_ENV,
        )),
        SexpType::Env => {
            let mut current = value;
            let mut levels = ctx.host().limits().max_expressions;
            let mut remaining = ctx.host().limits().max_vector_bytes;
            while current != EMPTY_ENV && current != SEXP_NIL {
                levels = levels.checked_sub(1).ok_or_else(|| {
                    ctx.stop(SEXP_NIL, "Rd macro environment depth limit exceeded")
                })?;
                for (sym, binding) in ctx.session().env_entries(current)? {
                    let node = binding.value;
                    if ctx.session().type_of(node)? != SexpType::Int {
                        continue;
                    }
                    let arity = ctx
                        .session()
                        .int(node)?
                        .iter()
                        .next()
                        .unwrap_or(0)
                        .saturating_sub(280);
                    let def = attr(ctx, node, "definition")?;
                    if (0..=9).contains(&arity) && def != SEXP_NIL {
                        let name = ctx.session().sym_name(sym)?.to_owned();
                        let body = first_text(ctx, def)?.unwrap_or_default();
                        remaining = remaining
                            .checked_sub(name.len().saturating_add(body.len()))
                            .ok_or_else(|| ctx.stop(SEXP_NIL, "Rd size limit exceeded"))?;
                        parser.macros.entry(name).or_insert((body, arity as usize));
                    }
                }
                current = ctx.session().env_enclos(current)?;
            }
            Ok((true, value))
        }
        SexpType::Nil | SexpType::Str => Ok((true, EMPTY_ENV)),
        _ => Err(ctx.stop(SEXP_NIL, "invalid 'macros' argument")),
    }
}

pub(super) fn build(
    ctx: &mut Ctx,
    root: &Node,
    srcfile: SexpId,
    keep: bool,
    parent: SexpId,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let env = if keep {
            let env = ctx.session_mut().alloc_env(parent)?;
            ctx.protect(env);
            env
        } else {
            SEXP_NIL
        };
        let class = ctx.session_mut().scalar_str("srcref")?;
        ctx.protect(class);
        let value = build_node(ctx, root, srcfile, class, env, true)?;
        ctx.protect(value);
        if keep {
            set_attr(ctx, value, "macros", env)?;
        }
        Ok(value)
    })
}

fn build_node(
    ctx: &mut Ctx,
    node: &Node,
    srcfile: SexpId,
    class: SexpId,
    macros: SexpId,
    root: bool,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let value = match &node.body {
            Body::Text(text) => ctx.session_mut().scalar_str(text)?,
            Body::Definition { arity, .. } => ctx.session_mut().scalar_int(280 + *arity as i32)?,
            Body::Expansion { values, .. } => chars(ctx, values.iter().map(String::as_str))?,
            Body::Children(children) => {
                let value = ctx.session_mut().alloc_list(children.len())?;
                ctx.protect(value);
                for (i, child) in children.iter().enumerate() {
                    let child = build_node(ctx, child, srcfile, class, macros, false)?;
                    ctx.session_mut().list_set(value, i, child)?;
                }
                value
            }
        };
        ctx.protect(value);
        if root {
            set_str(ctx, value, "class", "Rd")?;
        } else if !node.tag.is_empty() {
            set_str(ctx, value, "Rd_tag", &node.tag)?;
        }
        if let Some(option) = &node.option {
            let option = build_node(ctx, option, srcfile, class, macros, false)?;
            ctx.protect(option);
            set_attr(ctx, value, "Rd_option", option)?;
        }
        match &node.body {
            Body::Definition { body, name, .. } => {
                set_str(ctx, value, "definition", body)?;
                if macros != SEXP_NIL {
                    let name = ctx.session_mut().intern(name)?;
                    ctx.session_mut().env_define(macros, name, value)?;
                }
            }
            Body::Expansion { name, .. } => set_str(ctx, value, "macro", name)?,
            _ => {}
        }
        if let Some(span) = node.span {
            let reference = ctx.session_mut().alloc_int_from(&span.integers())?;
            ctx.protect(reference);
            set_attr(ctx, reference, "srcfile", srcfile)?;
            set_attr(ctx, reference, "class", class)?;
            set_attr(ctx, value, "srcref", reference)?;
        }
        if node.dynamic != 0 {
            let dynamic = ctx.session_mut().scalar_int(node.dynamic)?;
            ctx.protect(dynamic);
            set_attr(ctx, value, "dynamicFlag", dynamic)?;
        }
        Ok(value)
    })
}

pub(super) fn decode(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Node> {
    let mut remaining = ctx.host().limits().max_vector_bytes;
    let depth = ctx.host().limits().max_expressions.min(128);
    decode_node(ctx, call, value, depth, &mut remaining)
}

fn decode_node(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    depth: usize,
    remaining: &mut usize,
) -> EvalResult<Node> {
    if depth == 0 {
        return Err(ctx.stop(call.err_call(), "Rd nesting depth limit exceeded"));
    }
    charge(ctx, call, remaining, size_of::<Node>())?;
    let tag = attr(ctx, value, "Rd_tag")?;
    let tag = if tag != SEXP_NIL {
        first_text(ctx, tag)?.unwrap_or_default()
    } else {
        String::new()
    };
    charge(ctx, call, remaining, tag.len())?;
    let body = match ctx.session().type_of(value)? {
        SexpType::Str => {
            let text = first_text(ctx, value)?.unwrap_or_default();
            charge(ctx, call, remaining, text.len())?;
            Body::Text(text)
        }
        SexpType::Vec => {
            let mut children = Vec::new();
            for i in 0..ctx.session().xlength(value)? {
                let child = ctx.session().list_get(value, i)?;
                children.push(decode_node(ctx, call, child, depth - 1, remaining)?);
            }
            Body::Children(children)
        }
        SexpType::Int if matches!(tag.as_str(), "\\newcommand" | "\\renewcommand") => {
            Body::Text(String::new())
        }
        _ => return Err(ctx.stop(call.err_call(), "invalid Rd node")),
    };
    let mut node = Node::new(&tag, body, None);
    let option = attr(ctx, value, "Rd_option")?;
    if option != SEXP_NIL {
        node.option = Some(Box::new(decode_node(
            ctx,
            call,
            option,
            depth - 1,
            remaining,
        )?));
    }
    Ok(node)
}

pub(super) fn charge(
    ctx: &mut Ctx,
    call: &Call,
    remaining: &mut usize,
    size: usize,
) -> EvalResult<()> {
    *remaining = remaining
        .checked_sub(size)
        .ok_or_else(|| ctx.stop(call.err_call(), "Rd size limit exceeded"))?;
    Ok(())
}

pub(super) fn as_character(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let root = decode(ctx, call, args.get(0))?;
    let mut tokens = Vec::new();
    for node in root.children() {
        tokenize(node, &mut tokens);
    }
    chars(ctx, tokens)
}

fn tokenize<'a>(node: &'a Node, tokens: &mut Vec<&'a str>) {
    match node.tag.as_str() {
        "\\newcommand" | "\\renewcommand" | "USERMACRO" => return,
        "TEXT" | "RCODE" | "VERB" | "COMMENT" | "UNKNOWN" => {
            tokens.push(node.text());
            return;
        }
        "LIST" | "" => tokens.push(""),
        _ => tokens.push(&node.tag),
    }
    if let Some(option) = &node.option {
        tokens.extend(["[", option.text(), "]"]);
    }
    let children = node.children();
    if matches!(node.tag.as_str(), "#ifdef" | "#ifndef") {
        for group in children {
            for child in group.children() {
                tokenize(child, tokens);
            }
        }
        tokens.push("#endif\n");
    } else if !matches!(
        node.tag.as_str(),
        "\\R" | "\\dots" | "\\ldots" | "\\cr" | "\\tab"
    ) && !(node.tag == "\\item" && children.is_empty())
    {
        let grouped = children.first().is_some_and(|n| n.tag.is_empty());
        if !grouped {
            tokens.push("{");
        }
        for child in children {
            tokenize(child, tokens);
        }
        if !grouped {
            tokens.push("}");
        }
    }
}
