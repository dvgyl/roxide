use roxide_gc::SexpId;
use roxide_value::{RSession, SexpType, Sym, ValueApi};

use super::super::{ACTIVE_BINDING, LOCKED_BINDING, NIL, TAG};
use super::{CodecSession, WireResult, Writer, invalid};

impl Writer<'_> {
    pub(super) fn environment(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        env: SexpId,
        attrs: &[(Sym, SexpId)],
        depth: usize,
    ) -> WireResult<()> {
        let locked = session.env_is_locked(env)?;
        self.word(SexpType::Env as u32)?;
        self.word(u32::from(locked))?;
        self.item(session, session.env_enclos(env)?, depth)?;
        // R stores non-hashed bindings in reverse insertion order.
        let entries = session.env_entries(env)?;
        for (_, binding) in &entries {
            session.root(binding.value)?;
        }
        for (tag, binding) in entries.into_iter().rev() {
            let locked = if binding.locked { LOCKED_BINDING } else { 0 };
            let active = if binding.active { ACTIVE_BINDING } else { 0 };
            self.word(
                SexpType::List as u32 | TAG | locked | active | (u32::from(binding.missing) << 12),
            )?;
            self.item(session, tag.as_sexp(), depth)?;
            self.item(session, binding.value, depth)?;
        }
        self.word(NIL)?;
        self.word(NIL)?;
        self.attributes(session, attrs, depth)
    }
}

pub(super) fn namespace_version(session: &mut RSession, env: SexpId) -> WireResult<String> {
    let info = session.intern(".__NAMESPACE__.")?;
    let spec = session.intern("spec")?;
    if let Some(info) = session.env_get(env, info)
        && let Some(spec) = session.env_get(info.value, spec)
        && let &[_, version] = session.str_slice(spec.value)?
    {
        return Ok(std::str::from_utf8(session.char_bytes(version)?)
            .map_err(invalid)?
            .to_owned());
    }
    Ok("4.6.2".to_owned())
}
