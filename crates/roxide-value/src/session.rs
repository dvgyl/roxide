//! [`RSession`] owning the GC heap, symbol/character intern tables, and permanent singletons.

use std::collections::HashMap;

use roxide_gc::{GcPolicy, GcStats, Heap, RootSet, SexpId};

use crate::alt::AltDrops;
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
const S4_NULL_CHAR: CharId = CharId(SexpId::permanent(13));
const S4_NULL_BYTES: &[u8] = b"\x01NULL\x01";
/// Permanent 1-element blank character vector `""` (`R_BlankScalarString`, index 14; ADR-0017).
pub const BLANK_SCALAR_STRING: SexpId = SexpId::permanent(14);
/// Permanent namespace registry environment (`R_NamespaceRegistry`, index 15; ADR-0017).
pub const NAMESPACE_REGISTRY: SexpId = SexpId::permanent(15);
/// Permanent `namespace:methods` environment (`R_MethodsNamespace`, index 16; ADR-0017).
pub const METHODS_NAMESPACE: SexpId = SexpId::permanent(16);

/// Permanent `R_AsCharacterSymbol` (`"as.character"`, index 17).
pub const SYM_AS_CHARACTER: Sym = Sym(SexpId::permanent(17));
/// Permanent `R_AtsignSymbol` (`"@"`, index 18).
pub const SYM_ATSIGN: Sym = Sym(SexpId::permanent(18));
/// Permanent `R_BaseSymbol` / `R_baseSymbol` (`"base"`, index 19).
pub const SYM_BASE: Sym = Sym(SexpId::permanent(19));
/// Permanent `R_BraceSymbol` (`"{"`, index 20).
pub const SYM_BRACE: Sym = Sym(SexpId::permanent(20));
/// Permanent `R_Bracket2Symbol` (`"[["`, index 21).
pub const SYM_BRACKET2: Sym = Sym(SexpId::permanent(21));
/// Permanent `R_BracketSymbol` (`"["`, index 22).
pub const SYM_BRACKET: Sym = Sym(SexpId::permanent(22));
/// Permanent `R_ClassSymbol` (`"class"`, index 23).
pub const SYM_CLASS: Sym = Sym(SexpId::permanent(23));
/// Permanent `R_DeviceSymbol` (`".Device"`, index 24).
pub const SYM_DEVICE: Sym = Sym(SexpId::permanent(24));
/// Permanent `R_DimNamesSymbol` (`"dimnames"`, index 25).
pub const SYM_DIMNAMES: Sym = Sym(SexpId::permanent(25));
/// Permanent `R_DimSymbol` (`"dim"`, index 26).
pub const SYM_DIM: Sym = Sym(SexpId::permanent(26));
/// Permanent `R_DollarSymbol` (`"$"`, index 27).
pub const SYM_DOLLAR: Sym = Sym(SexpId::permanent(27));
/// Permanent `R_DotsSymbol` (`"..."`, index 28).
pub const SYM_DOTS: Sym = Sym(SexpId::permanent(28));
/// Permanent `R_DoubleColonSymbol` (`"::"`, index 29).
pub const SYM_DOUBLE_COLON: Sym = Sym(SexpId::permanent(29));
/// Permanent `R_DropSymbol` (`"drop"`, index 30).
pub const SYM_DROP: Sym = Sym(SexpId::permanent(30));
/// Permanent `R_EvalSymbol` (`"eval"`, index 31).
pub const SYM_EVAL: Sym = Sym(SexpId::permanent(31));
/// Permanent `R_FunctionSymbol` (`"function"`, index 32).
pub const SYM_FUNCTION: Sym = Sym(SexpId::permanent(32));
/// Permanent `R_LastvalueSymbol` (`".Last.value"`, index 33).
pub const SYM_LASTVALUE: Sym = Sym(SexpId::permanent(33));
/// Permanent `R_LevelsSymbol` (`"levels"`, index 34).
pub const SYM_LEVELS: Sym = Sym(SexpId::permanent(34));
/// Permanent `R_ModeSymbol` (`"mode"`, index 35).
pub const SYM_MODE: Sym = Sym(SexpId::permanent(35));
/// Permanent `R_NaRmSymbol` (`"na.rm"`, index 36).
pub const SYM_NA_RM: Sym = Sym(SexpId::permanent(36));
/// Permanent `R_NameSymbol` (`"name"`, index 37).
pub const SYM_NAME: Sym = Sym(SexpId::permanent(37));
/// Permanent `R_NamesSymbol` (`"names"`, index 38).
pub const SYM_NAMES: Sym = Sym(SexpId::permanent(38));
/// Permanent `R_NamespaceEnvSymbol` (`".__NAMESPACE__."`, index 39).
pub const SYM_NAMESPACE_ENV: Sym = Sym(SexpId::permanent(39));
/// Permanent `R_PackageSymbol` (`"package"`, index 40).
pub const SYM_PACKAGE: Sym = Sym(SexpId::permanent(40));
/// Permanent `R_PreviousSymbol` (`"previous"`, index 41).
pub const SYM_PREVIOUS: Sym = Sym(SexpId::permanent(41));
/// Permanent `R_QuoteSymbol` (`"quote"`, index 42).
pub const SYM_QUOTE: Sym = Sym(SexpId::permanent(42));
/// Permanent `R_RowNamesSymbol` (`"row.names"`, index 43).
pub const SYM_ROW_NAMES: Sym = Sym(SexpId::permanent(43));
/// Permanent `R_SeedsSymbol` (`".Random.seed"`, index 44).
pub const SYM_SEEDS: Sym = Sym(SexpId::permanent(44));
/// Permanent `R_SortListSymbol` (`"sort.list"`, index 45).
pub const SYM_SORT_LIST: Sym = Sym(SexpId::permanent(45));
/// Permanent `R_SourceSymbol` (`"source"`, index 46).
pub const SYM_SOURCE: Sym = Sym(SexpId::permanent(46));
/// Permanent `R_SpecSymbol` (`"spec"`, index 47).
pub const SYM_SPEC: Sym = Sym(SexpId::permanent(47));
/// Permanent `R_SrcfileSymbol` (`"srcfile"`, index 48).
pub const SYM_SRCFILE: Sym = Sym(SexpId::permanent(48));
/// Permanent `R_SrcrefSymbol` (`"srcref"`, index 49).
pub const SYM_SRCREF: Sym = Sym(SexpId::permanent(49));
/// Permanent `R_TripleColonSymbol` (`":::"`, index 50).
pub const SYM_TRIPLE_COLON: Sym = Sym(SexpId::permanent(50));
/// Permanent `R_TspSymbol` (`"tsp"`, index 51).
pub const SYM_TSP: Sym = Sym(SexpId::permanent(51));
/// Permanent `R_dot_Generic` (`".Generic"`, index 52).
pub const SYM_DOT_GENERIC: Sym = Sym(SexpId::permanent(52));
/// Permanent `R_dot_Method` (`".Method"`, index 53).
pub const SYM_DOT_METHOD: Sym = Sym(SexpId::permanent(53));
/// Permanent `R_dot_defined` (`".defined"`, index 54).
pub const SYM_DOT_DEFINED: Sym = Sym(SexpId::permanent(54));
/// Permanent `R_dot_packageName` (`".packageName"`, index 55).
pub const SYM_DOT_PACKAGE_NAME: Sym = Sym(SexpId::permanent(55));
/// Permanent `R_dot_target` (`".target"`, index 56).
pub const SYM_DOT_TARGET: Sym = Sym(SexpId::permanent(56));

const PERMANENT_SYMBOLS: &[(Sym, &str)] = &[
    (SYM_AS_CHARACTER, "as.character"),
    (SYM_ATSIGN, "@"),
    (SYM_BASE, "base"),
    (SYM_BRACE, "{"),
    (SYM_BRACKET2, "[["),
    (SYM_BRACKET, "["),
    (SYM_CLASS, "class"),
    (SYM_DEVICE, ".Device"),
    (SYM_DIMNAMES, "dimnames"),
    (SYM_DIM, "dim"),
    (SYM_DOLLAR, "$"),
    (SYM_DOTS, "..."),
    (SYM_DOUBLE_COLON, "::"),
    (SYM_DROP, "drop"),
    (SYM_EVAL, "eval"),
    (SYM_FUNCTION, "function"),
    (SYM_LASTVALUE, ".Last.value"),
    (SYM_LEVELS, "levels"),
    (SYM_MODE, "mode"),
    (SYM_NA_RM, "na.rm"),
    (SYM_NAME, "name"),
    (SYM_NAMES, "names"),
    (SYM_NAMESPACE_ENV, ".__NAMESPACE__."),
    (SYM_PACKAGE, "package"),
    (SYM_PREVIOUS, "previous"),
    (SYM_QUOTE, "quote"),
    (SYM_ROW_NAMES, "row.names"),
    (SYM_SEEDS, ".Random.seed"),
    (SYM_SORT_LIST, "sort.list"),
    (SYM_SOURCE, "source"),
    (SYM_SPEC, "spec"),
    (SYM_SRCFILE, "srcfile"),
    (SYM_SRCREF, "srcref"),
    (SYM_TRIPLE_COLON, ":::"),
    (SYM_TSP, "tsp"),
    (SYM_DOT_GENERIC, ".Generic"),
    (SYM_DOT_METHOD, ".Method"),
    (SYM_DOT_DEFINED, ".defined"),
    (SYM_DOT_PACKAGE_NAME, ".packageName"),
    (SYM_DOT_TARGET, ".target"),
];

/// Owns the Roxide GC heap, permanent symbol table, and weak `CHARSXP` table.
pub struct RSession {
    /// Main GC arena.
    pub heap: RHeap,
    symbols: HashMap<Box<[u8]>, Sym>,
    chars: HashMap<(Box<[u8]>, Encoding), CharId>,
    pub(crate) foreign_alt_seen: bool,
    pub(crate) alt_drops: AltDrops,
}

impl RSession {
    /// Initialize a fresh [`RSession`] with the fixed permanent singleton table (ADR-0002).
    pub fn new(policy: GcPolicy) -> Result<Self, ValueError> {
        let mut heap = Heap::new(policy);

        let symbol = |name| Body::Sym { name };
        let char_body = |bytes| Body::Char(RChar::new(bytes, Encoding::Native));
        let env = |parent| Body::Env(Box::new(EnvData::new(parent)));
        let logical = |value| Body::Lgl(RVec::from_vec(vec![value]));
        let singletons = [
            (SEXP_NIL, Body::Nil),
            (SEXP_UNBOUND, symbol(CharId::BLANK)),
            (SEXP_MISSING, symbol(CharId::BLANK)),
            (CharId::NA.as_sexp(), char_body(b"NA")),
            (CharId::BLANK.as_sexp(), char_body(b"")),
            (EMPTY_ENV, env(SEXP_NIL)),
            (BASE_ENV, env(EMPTY_ENV)),
            (GLOBAL_ENV, env(BASE_ENV)),
            (BASE_NAMESPACE, env(GLOBAL_ENV)),
            (TRUE_SCALAR, logical(RLogical::TRUE)),
            (FALSE_SCALAR, logical(RLogical::FALSE)),
            (NA_LGL_SCALAR, logical(RLogical::NA)),
            (SEXP_S4_NULL, symbol(S4_NULL_CHAR)),
            (S4_NULL_CHAR.as_sexp(), char_body(S4_NULL_BYTES)),
            (
                BLANK_SCALAR_STRING,
                Body::Str(RVec::from_vec(vec![CharId::BLANK])),
            ),
            (NAMESPACE_REGISTRY, env(EMPTY_ENV)),
            (METHODS_NAMESPACE, env(BASE_NAMESPACE)),
        ];

        let mut chars = HashMap::new();
        chars.insert((Box::from(&b""[..]), Encoding::Native), CharId::BLANK);
        chars.insert((Box::from(S4_NULL_BYTES), Encoding::Native), S4_NULL_CHAR);
        let mut symbols = HashMap::new();
        symbols.insert(Box::from(S4_NULL_BYTES), Sym(SEXP_S4_NULL));

        let symbol_name = |sym: Sym| {
            CharId(SexpId::permanent(
                sym.as_sexp().index() + PERMANENT_SYMBOLS.len() as u32,
            ))
        };
        let symbol_slots = PERMANENT_SYMBOLS
            .iter()
            .map(|&(sym, _)| (sym.as_sexp(), symbol(symbol_name(sym))));
        let name_slots = PERMANENT_SYMBOLS.iter().map(|&(sym, text)| {
            let name = symbol_name(sym);
            let bytes = text.as_bytes();
            chars.insert((Box::from(bytes), Encoding::Native), name);
            symbols.insert(Box::from(bytes), sym);
            (name.as_sexp(), char_body(bytes))
        });

        for (expected, body) in singletons.into_iter().chain(symbol_slots).chain(name_slots) {
            let actual = heap.alloc_permanent(RObject::new(body))?;
            if actual != expected {
                return Err(ValueError::InvalidAttribute(format!(
                    "permanent slot mismatch: expected {}, got {}",
                    expected.index(),
                    actual.index()
                )));
            }
        }

        let mut session = Self {
            heap,
            symbols,
            chars,
            foreign_alt_seen: false,
            alt_drops: AltDrops::default(),
        };
        session.env_define(NAMESPACE_REGISTRY, SYM_BASE, BASE_NAMESPACE)?;
        Ok(session)
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

    pub(crate) fn interned_symbol(&self, name: &str) -> Option<Sym> {
        self.symbols.get(name.as_bytes()).copied()
    }

    /// Look up the print name of a [`Sym`].
    pub fn sym_name(&self, sym: Sym) -> Result<&str, ValueError> {
        self.sym_name_of(sym.as_sexp())
    }

    /// Get the print name of a symbol directly from its handle.
    pub fn sym_name_of(&self, id: SexpId) -> Result<&str, ValueError> {
        let obj = self.heap.get(id)?;
        let Body::Sym { name } = obj.body else {
            return Err(obj.type_mismatch("symbol"));
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
            return Err(obj.type_mismatch("char"));
        };
        Ok(rc.bytes())
    }

    /// Return the [`Encoding`] of a [`CharId`].
    pub fn char_encoding(&self, c: CharId) -> Result<Encoding, ValueError> {
        let obj = self.heap.get(c.as_sexp())?;
        let Body::Char(rc) = &obj.body else {
            return Err(obj.type_mismatch("char"));
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
        frame.shape_version = frame.shape_version.wrapping_add(1);
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
        self.collect_with_hook(roots, |_| {})
    }

    /// Run a two-phase GC cycle, invoking `hook` between mark and ephemeron/weak-cache/sweep cleanup.
    pub fn collect_with_hook(
        &mut self,
        roots: &dyn RootSet,
        hook: impl FnOnce(&mut roxide_gc::Marked<'_, RObject>),
    ) -> GcStats {
        let mut marked = self.heap.mark(roots);
        hook(&mut marked);
        marked.resolve_ephemerons();
        self.chars.retain(|_, cid| marked.is_marked(cid.as_sexp()));
        let outcome = marked.sweep_outcome();
        for id in self.alt_drops.borrow_mut().drain(..) {
            self.heap.dec_ref(id);
        }
        for wr_id in outcome.cleared_weakrefs {
            if let Ok(obj) = self.heap.get_mut(wr_id)
                && let Body::WeakRef(wr) = &mut obj.body
            {
                wr.clear();
            }
        }
        outcome.stats
    }
}
