//! Link-time constants for permanent R objects and special numeric values.

#![allow(non_upper_case_globals)]

use roxide_value::{CharId, NA_INTEGER, NA_REAL, na::R_NAN, session::*};

use crate::abi::Sexp;

macro_rules! globals {
    ($($(#[$doc:meta])* $name:ident: $ty:ty = $value:expr),+ $(,)?) => {
        $($(#[$doc])* #[unsafe(no_mangle)] pub static $name: $ty = $value;)+
    };
}

globals! {
    /// R NULL object.
    R_NilValue: Sexp = Sexp::from_id(SEXP_NIL),
    /// Marker for an unbound name.
    R_UnboundValue: Sexp = Sexp::from_id(SEXP_UNBOUND),
    /// Marker for a missing argument.
    R_MissingArg: Sexp = Sexp::from_id(SEXP_MISSING),
    /// Missing interned string.
    R_NaString: Sexp = Sexp::from_id(CharId::NA.as_sexp()),
    /// Empty interned string.
    R_BlankString: Sexp = Sexp::from_id(CharId::BLANK.as_sexp()),
    /// Character vector that contains one empty string.
    R_BlankScalarString: Sexp = Sexp::from_id(BLANK_SCALAR_STRING),
    /// Empty environment.
    R_EmptyEnv: Sexp = Sexp::from_id(EMPTY_ENV),
    /// Base package environment.
    R_BaseEnv: Sexp = Sexp::from_id(BASE_ENV),
    /// Global environment.
    R_GlobalEnv: Sexp = Sexp::from_id(GLOBAL_ENV),
    /// Base namespace environment.
    R_BaseNamespace: Sexp = Sexp::from_id(BASE_NAMESPACE),
    /// Namespace registry environment.
    R_NamespaceRegistry: Sexp = Sexp::from_id(NAMESPACE_REGISTRY),
    /// Methods namespace environment.
    R_MethodsNamespace: Sexp = Sexp::from_id(METHODS_NAMESPACE),
    /// Logical TRUE scalar.
    R_TrueValue: Sexp = Sexp::from_id(TRUE_SCALAR),
    /// Logical FALSE scalar.
    R_FalseValue: Sexp = Sexp::from_id(FALSE_SCALAR),
    /// Missing logical scalar.
    R_LogicalNAValue: Sexp = Sexp::from_id(NA_LGL_SCALAR),
    /// Missing integer.
    R_NaInt: i32 = NA_INTEGER,
    /// Missing double.
    R_NaReal: f64 = NA_REAL,
    /// Double NaN.
    R_NaN: f64 = R_NAN,
    /// Positive infinity.
    R_PosInf: f64 = f64::INFINITY,
    /// Negative infinity.
    R_NegInf: f64 = f64::NEG_INFINITY,
}

macro_rules! symbols {
    ($($name:ident = $id:ident),+ $(,)?) => {
        $(
            #[doc = concat!("Permanent R symbol for `", stringify!($name), "`.")]
            #[unsafe(no_mangle)]
            pub static $name: Sexp = Sexp::from_id($id.as_sexp());
        )+
    };
}

symbols! {
    R_AsCharacterSymbol = SYM_AS_CHARACTER,
    R_AtsignSymbol = SYM_ATSIGN,
    R_BaseSymbol = SYM_BASE,
    R_BraceSymbol = SYM_BRACE,
    R_Bracket2Symbol = SYM_BRACKET2,
    R_BracketSymbol = SYM_BRACKET,
    R_ClassSymbol = SYM_CLASS,
    R_DeviceSymbol = SYM_DEVICE,
    R_DimNamesSymbol = SYM_DIMNAMES,
    R_DimSymbol = SYM_DIM,
    R_DollarSymbol = SYM_DOLLAR,
    R_DotsSymbol = SYM_DOTS,
    R_DoubleColonSymbol = SYM_DOUBLE_COLON,
    R_DropSymbol = SYM_DROP,
    R_EvalSymbol = SYM_EVAL,
    R_FunctionSymbol = SYM_FUNCTION,
    R_LastvalueSymbol = SYM_LASTVALUE,
    R_LevelsSymbol = SYM_LEVELS,
    R_ModeSymbol = SYM_MODE,
    R_NaRmSymbol = SYM_NA_RM,
    R_NameSymbol = SYM_NAME,
    R_NamesSymbol = SYM_NAMES,
    R_NamespaceEnvSymbol = SYM_NAMESPACE_ENV,
    R_PackageSymbol = SYM_PACKAGE,
    R_PreviousSymbol = SYM_PREVIOUS,
    R_QuoteSymbol = SYM_QUOTE,
    R_RowNamesSymbol = SYM_ROW_NAMES,
    R_SeedsSymbol = SYM_SEEDS,
    R_SortListSymbol = SYM_SORT_LIST,
    R_SourceSymbol = SYM_SOURCE,
    R_SpecSymbol = SYM_SPEC,
    R_SrcfileSymbol = SYM_SRCFILE,
    R_SrcrefSymbol = SYM_SRCREF,
    R_TripleColonSymbol = SYM_TRIPLE_COLON,
    R_TspSymbol = SYM_TSP,
    R_dot_Generic = SYM_DOT_GENERIC,
    R_dot_Method = SYM_DOT_METHOD,
    R_dot_defined = SYM_DOT_DEFINED,
    R_dot_packageName = SYM_DOT_PACKAGE_NAME,
    R_dot_target = SYM_DOT_TARGET,
    R_baseSymbol = SYM_BASE,
}
