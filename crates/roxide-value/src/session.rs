//! [`RSession`] owning the GC heap, symbol/character intern tables, and permanent singletons.

use std::collections::HashMap;

use roxide_gc::{GcPolicy, GcStats, Heap, RootSet, SexpId};

use crate::alt::AltRegistry;
use crate::api::{ValueApi, ValueError};
use crate::na::RLogical;
use crate::object::{Body, CharId, Encoding, EnvData, RChar, RObject, RVec, Sym};

/// Specialized GC heap storing [`RObject`] values.
pub type RHeap = Heap<RObject>;

/// Permanent `NILSXP` (`NULL`, index 0).
pub const SEXP_NIL: SexpId = SexpId::permanent(0);
/// Permanent internal unbound sentinel (`R_UnboundValue`, index 1).
pub const SEXP_UNBOUND: SexpId = SexpId::permanent(1);
/// Permanent missing-argument sentinel (`R_MissingArg`, index 2).
pub const SEXP_MISSING: SexpId = SexpId::permanent(2);
/// Permanent `emptyenv()` (index 5).
pub const EMPTY_ENV: SexpId = SexpId::permanent(5);
/// Permanent `baseenv()` (index 6).
pub const BASE_ENV: SexpId = SexpId::permanent(6);
/// Permanent `.GlobalEnv` (index 7).
pub const GLOBAL_ENV: SexpId = SexpId::permanent(7);
/// Permanent base namespace environment (index 8).
pub const BASE_NAMESPACE: SexpId = SexpId::permanent(8);
/// Permanent shared `TRUE` scalar (index 9).
pub const TRUE_SCALAR: SexpId = SexpId::permanent(9);
/// Permanent shared `FALSE` scalar (index 10).
pub const FALSE_SCALAR: SexpId = SexpId::permanent(10);
/// Permanent shared `NA` logical scalar (index 11).
pub const NA_LGL_SCALAR: SexpId = SexpId::permanent(11);
/// Permanent S4 pseudo-NULL slot sentinel (`\x01NULL\x01`, index 12; ADR-0012).
pub const SEXP_S4_NULL: SexpId = SexpId::permanent(12);

/// Owns the Roxide GC heap, permanent symbol table, weak `CHARSXP` table, and ALTREP registry.
pub struct RSession {
    /// Main GC arena.
    pub heap: RHeap,
    symbols: HashMap<Box<[u8]>, Sym>,
    chars: HashMap<(Box<[u8]>, Encoding), CharId>,
    /// ALTREP class unserializer registry.
    pub alt: AltRegistry,
}

impl RSession {
    /// Initialize a fresh [`RSession`] with the fixed permanent singleton table (ADR-0002).
    pub fn new(policy: GcPolicy) -> Result<Self, ValueError> {
        let mut heap = Heap::new(policy);

        // Fixed permanent slot order (indices 0..=11, NEVER reorder):
        let nil = heap.alloc_permanent(RObject::new(Body::Nil))?;
        debug_assert_eq!(nil, SEXP_NIL);

        let unbound = heap.alloc_permanent(RObject::new(Body::Sym {
            name: CharId::BLANK,
        }))?;
        debug_assert_eq!(unbound, SEXP_UNBOUND);

        let missing = heap.alloc_permanent(RObject::new(Body::Sym {
            name: CharId::BLANK,
        }))?;
        debug_assert_eq!(missing, SEXP_MISSING);

        let na_char = heap.alloc_permanent(RObject::new(Body::Char(RChar::new(
            b"NA",
            Encoding::Native,
        ))))?;
        debug_assert_eq!(na_char, CharId::NA.as_sexp());

        let blank_char =
            heap.alloc_permanent(RObject::new(Body::Char(RChar::new(b"", Encoding::Native))))?;
        debug_assert_eq!(blank_char, CharId::BLANK.as_sexp());

        let empty_env =
            heap.alloc_permanent(RObject::new(Body::Env(Box::new(EnvData::new(SEXP_NIL)))))?;
        debug_assert_eq!(empty_env, EMPTY_ENV);

        let base_env =
            heap.alloc_permanent(RObject::new(Body::Env(Box::new(EnvData::new(EMPTY_ENV)))))?;
        debug_assert_eq!(base_env, BASE_ENV);

        let global_env =
            heap.alloc_permanent(RObject::new(Body::Env(Box::new(EnvData::new(BASE_ENV)))))?;
        debug_assert_eq!(global_env, GLOBAL_ENV);

        let base_ns =
            heap.alloc_permanent(RObject::new(Body::Env(Box::new(EnvData::new(GLOBAL_ENV)))))?;
        debug_assert_eq!(base_ns, BASE_NAMESPACE);

        let true_s = heap.alloc_permanent(RObject::new(Body::Lgl(RVec::from_vec(vec![
            RLogical::TRUE,
        ]))))?;
        debug_assert_eq!(true_s, TRUE_SCALAR);

        let false_s = heap.alloc_permanent(RObject::new(Body::Lgl(RVec::from_vec(vec![
            RLogical::FALSE,
        ]))))?;
        debug_assert_eq!(false_s, FALSE_SCALAR);

        let na_lgl =
            heap.alloc_permanent(RObject::new(Body::Lgl(RVec::from_vec(vec![RLogical::NA]))))?;
        debug_assert_eq!(na_lgl, NA_LGL_SCALAR);

        let s4_null = heap.alloc_permanent(RObject::new(Body::Sym {
            name: CharId::BLANK,
        }))?;
        debug_assert_eq!(s4_null, SEXP_S4_NULL);

        let s4_null_bytes: &[u8] = b"\x01NULL\x01";
        let s4_null_char = CharId(heap.alloc_permanent(RObject::new(Body::Char(RChar::new(
            s4_null_bytes,
            Encoding::Native,
        ))))?);
        if let Body::Sym { name } = &mut heap.get_mut(SEXP_S4_NULL)?.body {
            *name = s4_null_char;
        }

        let mut chars = HashMap::new();
        chars.insert((Box::from(&b""[..]), Encoding::Native), CharId::BLANK);
        chars.insert((Box::from(s4_null_bytes), Encoding::Native), s4_null_char);

        let mut symbols = HashMap::new();
        symbols.insert(Box::from(s4_null_bytes), Sym(SEXP_S4_NULL));

        Ok(Self {
            heap,
            symbols,
            chars,
            alt: AltRegistry::new(),
        })
    }

    /// Intern a symbol (`SYMSXP`) permanently.
    pub fn intern(&mut self, name: &str) -> Result<Sym, ValueError> {
        let bytes = name.as_bytes();
        if let Some(&sym) = self.symbols.get(bytes) {
            return Ok(sym);
        }
        if bytes.contains(&0) {
            return Err(ValueError::EmbeddedNul);
        }
        let enc = if name.is_ascii() {
            Encoding::Native
        } else {
            Encoding::Utf8
        };
        let char_id = self.mk_char_permanent(bytes, enc)?;
        let id = self
            .heap
            .alloc_permanent(RObject::new(Body::Sym { name: char_id }))?;
        let sym = Sym(id);
        self.symbols.insert(bytes.into(), sym);
        Ok(sym)
    }

    /// Look up the print name of a [`Sym`].
    pub fn sym_name(&self, sym: Sym) -> Result<&str, ValueError> {
        self.sym_name_of(sym.as_sexp())
    }

    /// Get the print name of a symbol directly from its handle.
    pub fn sym_name_of(&self, id: SexpId) -> Result<&str, ValueError> {
        let obj = self.heap.get(id)?;
        let Body::Sym { name } = obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "symbol",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        let bytes = self.char_bytes(name)?;
        Ok(std::str::from_utf8(bytes).unwrap_or(""))
    }

    /// Intern a `CHARSXP` in the weak character cache. Rejects interior NUL bytes.
    pub fn mk_char(&mut self, bytes: &[u8], enc: Encoding) -> Result<CharId, ValueError> {
        if bytes.contains(&0) {
            return Err(ValueError::EmbeddedNul);
        }
        if bytes.is_empty() && enc != Encoding::Bytes {
            return Ok(CharId::BLANK);
        }
        let norm_enc = if bytes.is_ascii() && enc != Encoding::Bytes {
            Encoding::Native
        } else {
            enc
        };
        // Fast lookup without allocating a Box<[u8]> if present via small scan or owned key.
        let key = (Box::<[u8]>::from(bytes), norm_enc);
        if let Some(&existing) = self.chars.get(&key) {
            return Ok(existing);
        }
        let id = self
            .heap
            .alloc(RObject::new(Body::Char(RChar::new(bytes, norm_enc))))?;
        let cid = CharId(id);
        self.chars.insert(key, cid);
        Ok(cid)
    }

    fn mk_char_permanent(&mut self, bytes: &[u8], enc: Encoding) -> Result<CharId, ValueError> {
        if bytes.is_empty() {
            return Ok(CharId::BLANK);
        }
        let norm_enc = if bytes.is_ascii() && enc != Encoding::Bytes {
            Encoding::Native
        } else {
            enc
        };
        let key = (Box::<[u8]>::from(bytes), norm_enc);
        if let Some(&existing) = self.chars.get(&key)
            && existing.as_sexp().is_permanent()
        {
            return Ok(existing);
        }
        let id = self
            .heap
            .alloc_permanent(RObject::new(Body::Char(RChar::new(bytes, norm_enc))))?;
        let cid = CharId(id);
        self.chars.insert(key, cid);
        Ok(cid)
    }

    /// Borrow the raw bytes (without trailing NUL) of a [`CharId`].
    pub fn char_bytes(&self, c: CharId) -> Result<&[u8], ValueError> {
        let obj = self.heap.get(c.as_sexp())?;
        let Body::Char(rc) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "char",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(rc.bytes())
    }

    /// Return the [`Encoding`] of a [`CharId`].
    pub fn char_encoding(&self, c: CharId) -> Result<Encoding, ValueError> {
        let obj = self.heap.get(c.as_sexp())?;
        let Body::Char(rc) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "char",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(rc.encoding())
    }

    /// Return the implicit class of a language object.
    pub fn call_class(&self, call: SexpId) -> Result<&'static str, ValueError> {
        let (head, _, _) = self.cons_parts(call)?;
        Ok(match self.sym_name_of(head) {
            Ok("if") => "if",
            Ok("for") => "for",
            Ok("while") => "while",
            Ok("{") => "{",
            Ok("(") => "(",
            Ok("<-") => "<-",
            Ok("=") => "=",
            _ => "call",
        })
    }

    /// Release references from a call frame that has no references outside its own bindings.
    pub fn release_call_frame(&mut self, env: SexpId, actuals: &[SexpId], retained: &[SexpId]) {
        let Ok(object) = self.heap.get(env) else {
            return;
        };
        let Body::Env(frame) = &object.body else {
            return;
        };
        let mut defaults = 0;
        for (_, binding) in &frame.entries {
            if self.heap.refcnt(binding.value) == 1
                && !retained.contains(&binding.value)
                && self
                    .promise_parts(binding.value)
                    .is_ok_and(|parts| parts.1 == env)
            {
                defaults += 1;
            }
        }
        let refs = self.heap.refcnt(env);
        if refs == roxide_gc::REFCNT_MAX || usize::from(refs) != defaults || retained.contains(&env)
        {
            return;
        }
        let Ok(object) = self.heap.get_mut(env) else {
            return;
        };
        let Body::Env(frame) = &mut object.body else {
            return;
        };
        let enclos = std::mem::replace(&mut frame.enclos, SEXP_NIL);
        let entries = std::mem::take(&mut frame.entries);
        frame.index.clear();
        self.heap.dec_ref(enclos);
        let mut candidates = actuals.to_vec();
        for (_, binding) in entries {
            self.heap.dec_ref(binding.value);
            let mut value = binding.value;
            while !self.heap.maybe_referenced(value) && !retained.contains(&value) {
                let Ok(object) = self.heap.get_mut(value) else {
                    break;
                };
                let Body::Dots(cell) = &mut object.body else {
                    break;
                };
                let (car, cdr, tag) = (cell.car, cell.cdr, cell.tag);
                cell.car = SEXP_NIL;
                cell.cdr = SEXP_NIL;
                cell.tag = SEXP_NIL;
                for edge in [car, cdr, tag] {
                    self.heap.dec_ref(edge);
                }
                candidates.push(car);
                value = cdr;
            }
            candidates.push(value);
        }
        for value in candidates {
            self.release_promise(value, retained);
        }
    }

    fn release_promise(&mut self, promise: SexpId, retained: &[SexpId]) {
        if self.heap.maybe_referenced(promise) || retained.contains(&promise) {
            return;
        }
        let Ok(object) = self.heap.get_mut(promise) else {
            return;
        };
        let Body::Promise {
            expr, env, value, ..
        } = &mut object.body
        else {
            return;
        };
        let edges = [expr, env, value].map(|edge| std::mem::replace(edge, SEXP_NIL));
        for edge in edges {
            self.heap.dec_ref(edge);
        }
    }

    /// Run a complete two-phase GC cycle: mark, purge unmarked weak `CHARSXP` entries, then sweep.
    pub fn collect(&mut self, roots: &dyn RootSet) -> GcStats {
        let marked = self.heap.mark(roots);
        self.chars.retain(|_, cid| marked.is_marked(cid.as_sexp()));
        marked.sweep()
    }
}
