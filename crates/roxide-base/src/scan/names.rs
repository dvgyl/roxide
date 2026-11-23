use std::collections::{HashMap, HashSet};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{Encoding, ValueApi};

use crate::ops::common::flag;

pub(super) fn make_names(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let unique = flag(ctx, args.get(1), false)?;
    let allow_underscore = flag(ctx, args.get(2), true)?;
    let utf8 = ctx.host().locale().utf8;
    let input = super::r_strings(ctx, args.get(0))?;
    let mut names: Vec<_> = input
        .into_iter()
        .map(|name| syntactic(name.as_deref().unwrap_or("NA"), allow_underscore, utf8))
        .collect();
    if unique {
        make_unique(&mut names);
    }
    let s = ctx.session_mut();
    let ids = names
        .iter()
        .map(|name| s.mk_char(name.as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

fn syntactic(name: &str, allow_underscore: bool, utf8: bool) -> String {
    let alpha = |c: char| {
        if utf8 {
            c.is_alphabetic()
        } else {
            c.is_ascii_alphabetic()
        }
    };
    let mut chars = name.chars();
    let valid_start = match chars.next() {
        Some('.') => !chars.next().is_some_and(|c| c.is_ascii_digit()),
        Some(c) => alpha(c),
        None => false,
    };
    let mut result = String::new();
    if !valid_start {
        result.push('X');
    }
    result.extend(name.chars().map(|c| {
        if alpha(c) || c.is_ascii_digit() || c == '.' || (c == '_' && allow_underscore) {
            c
        } else {
            '.'
        }
    }));
    if matches!(
        result.as_str(),
        "if" | "else"
            | "repeat"
            | "while"
            | "function"
            | "for"
            | "in"
            | "next"
            | "break"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "Inf"
            | "NaN"
            | "NA"
            | "NA_integer_"
            | "NA_real_"
            | "NA_complex_"
            | "NA_character_"
    ) {
        result.push('.');
    }
    result
}

fn make_unique(names: &mut [String]) {
    let mut used: HashSet<_> = names.iter().cloned().collect();
    let mut next = HashMap::<String, usize>::new();
    for name in names {
        let suffix = next.entry(name.clone()).or_default();
        if *suffix != 0 {
            loop {
                let candidate = format!("{name}.{suffix}");
                *suffix += 1;
                if used.insert(candidate.clone()) {
                    *name = candidate;
                    break;
                }
            }
        } else {
            *suffix = 1;
        }
    }
}
