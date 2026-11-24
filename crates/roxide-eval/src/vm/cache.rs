//! Inline caches for lexical lookup.

use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, BASE_NAMESPACE, RSession, Sym, ValueApi};

use super::CompiledCode;

pub(super) struct Caches {
    pub calls: Vec<Option<CallCache>>,
    pub vars: Vec<Option<VarCache>>,
}

impl Caches {
    pub fn new(code: &CompiledCode) -> Self {
        Self {
            calls: Vec::from_iter((0..code.num_call_caches).map(|_| None)),
            vars: Vec::from_iter((0..code.num_var_caches).map(|_| None)),
        }
    }
}

pub(super) struct CallCache {
    pub sym: Sym,
    pub path: EnvPath,
    pub fun: SexpId,
}

pub(super) struct VarCache {
    pub sym: Sym,
    pub path: EnvPath,
    pub binding_env: SexpId,
}

#[derive(Default)]
pub(super) struct EnvPath(Vec<(SexpId, u64)>);

impl EnvPath {
    pub fn push(&mut self, session: &RSession, env: SexpId) -> Option<()> {
        self.0
            .push((key(env), session.env_shape_version(env).ok()?));
        Some(())
    }

    pub fn matches(&self, session: &RSession, mut env: SexpId) -> bool {
        for (i, &(cached, version)) in self.0.iter().enumerate() {
            if key(env) != cached || session.env_shape_version(env) != Ok(version) {
                return false;
            }
            if i + 1 < self.0.len() {
                let Ok(parent) = session.env_enclos(env) else {
                    return false;
                };
                env = parent;
            }
        }
        true
    }
}

fn key(env: SexpId) -> SexpId {
    if env == BASE_NAMESPACE { BASE_ENV } else { env }
}
