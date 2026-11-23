//! Copies of value objects with reference counts for the copied fields.

use std::collections::HashMap;

use roxide_gc::SexpId;

use crate::api::ValueError;
use crate::object::{Body, RObject};
use crate::session::RSession;

impl RSession {
    /// Copy an object and share its child values. Keep reference objects by identity.
    pub fn shallow_duplicate(&mut self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let body = match &obj.body {
            Body::List(cell) => Body::List(*cell),
            Body::Lang(cell) => Body::Lang(*cell),
            Body::Dots(cell) => Body::Dots(*cell),
            Body::Lgl(v) => Body::Lgl(v.clone()),
            Body::Int(v) => Body::Int(v.clone()),
            Body::Real(v) => Body::Real(v.clone()),
            Body::Cplx(v) => Body::Cplx(v.clone()),
            Body::Str(v) => Body::Str(v.clone()),
            Body::Raw(v) => Body::Raw(v.clone()),
            Body::Vec(v) => Body::Vec(v.clone()),
            Body::Expr(v) => Body::Expr(v.clone()),
            Body::Alt(v) => Body::Alt(v.clone()),
            Body::Closure { formals, body, env } => Body::Closure {
                formals: *formals,
                body: *body,
                env: *env,
            },
            Body::Obj => Body::Obj,
            _ => return Ok(x),
        };
        let mut copy = RObject {
            body,
            gp: obj.gp,
            flags: obj.flags,
            attrs: obj.attrs.clone(),
        };
        let mut edges = Vec::new();
        for_each_child(&mut copy, |child| edges.push(*child));
        match &copy.body {
            Body::Closure { env, .. } => edges.push(*env),
            Body::Alt(alt) => edges.extend(alt.wrapped_target()),
            _ => {}
        }
        let id = self.heap.alloc(copy)?;
        for edge in edges {
            self.heap.inc_ref(edge);
        }
        Ok(id)
    }

    /// Copy an object and its child values. Preserve cycles and reference object identities.
    pub fn deep_duplicate(&mut self, x: SexpId) -> Result<SexpId, ValueError> {
        let copy = self.shallow_duplicate(x)?;
        if copy == x {
            return Ok(copy);
        }
        let mut copies = HashMap::from([(x, copy)]);
        let mut pending = vec![copy];
        while let Some(parent) = pending.pop() {
            let mut children = Vec::new();
            for_each_child(self.heap.get_mut(parent)?, |child| children.push(*child));
            let mut replacements = Vec::with_capacity(children.len());
            for &child in &children {
                let replacement = if let Some(&id) = copies.get(&child) {
                    id
                } else {
                    let id = self.shallow_duplicate(child)?;
                    copies.insert(child, id);
                    if id != child {
                        pending.push(id);
                    }
                    id
                };
                replacements.push(replacement);
            }
            for (&child, &replacement) in children.iter().zip(&replacements) {
                if child != replacement {
                    self.heap.inc_ref(replacement);
                    self.heap.dec_ref(child);
                }
            }
            let mut replacements = replacements.into_iter();
            for_each_child(self.heap.get_mut(parent)?, |child| {
                if let Some(replacement) = replacements.next() {
                    *child = replacement;
                }
            });
        }
        Ok(copy)
    }
}

fn for_each_child(obj: &mut RObject, mut visit: impl FnMut(&mut SexpId)) {
    if let Some(attrs) = &mut obj.attrs.0 {
        for (_, child) in attrs.iter_mut() {
            visit(child);
        }
    }
    match &mut obj.body {
        Body::Vec(v) | Body::Expr(v) => v.as_mut_slice().iter_mut().for_each(visit),
        Body::List(cell) | Body::Lang(cell) | Body::Dots(cell) => {
            visit(&mut cell.car);
            visit(&mut cell.cdr);
        }
        Body::Closure { formals, body, .. } => {
            visit(formals);
            visit(body);
        }
        _ => {}
    }
}
