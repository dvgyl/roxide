use super::*;
use roxide_value::EMPTY_ENV;

enum Storage {
    Null,
    Object,
    Logical,
    Integer,
    Numeric,
    Complex,
    Character,
    Raw,
    List,
    Expression,
    Function,
    Environment,
    Name,
    Call,
    Matrix,
    Array,
}

struct Basic {
    name: &'static str,
    parents: &'static [&'static str],
    slots: &'static [(&'static str, &'static str)],
    storage: Storage,
    data: Option<&'static str>,
    virtual_: bool,
    sealed: bool,
}

const fn basic(
    name: &'static str,
    parents: &'static [&'static str],
    storage: Storage,
    data: Option<&'static str>,
    virtual_: bool,
) -> Basic {
    Basic {
        name,
        parents,
        slots: &[],
        storage,
        data,
        virtual_,
        sealed: true,
    }
}

const BUILTINS: &[Basic] = &[
    basic("ANY", &[], Storage::Null, None, true),
    basic("VIRTUAL", &[], Storage::Null, None, true),
    basic("missing", &[], Storage::Null, None, true),
    basic("S4", &[], Storage::Object, None, true),
    basic("vector", &[], Storage::Logical, None, true),
    basic("language", &[], Storage::Name, None, true),
    basic("refObject", &[], Storage::Environment, None, true),
    basic("data.frameRowLabels", &[], Storage::Character, None, true),
    basic("SuperClassMethod", &[], Storage::Character, None, true),
    basic("optionalMethod", &[], Storage::Null, None, true),
    basic("OptionalFunction", &[], Storage::Null, None, true),
    basic(
        "PossibleMethod",
        &["optionalMethod"],
        Storage::Function,
        None,
        true,
    ),
    basic(
        "NULL",
        &["OptionalFunction", "optionalMethod"],
        Storage::Null,
        None,
        false,
    ),
    basic(
        "logical",
        &["vector"],
        Storage::Logical,
        Some("logical"),
        false,
    ),
    basic(
        "numeric",
        &["vector"],
        Storage::Numeric,
        Some("numeric"),
        false,
    ),
    basic(
        "double",
        &["numeric", "vector"],
        Storage::Numeric,
        Some("double"),
        false,
    ),
    basic(
        "integer",
        &["double", "numeric", "vector", "data.frameRowLabels"],
        Storage::Integer,
        Some("integer"),
        false,
    ),
    basic(
        "complex",
        &["vector"],
        Storage::Complex,
        Some("complex"),
        false,
    ),
    basic(
        "character",
        &["vector", "data.frameRowLabels", "SuperClassMethod"],
        Storage::Character,
        Some("character"),
        false,
    ),
    basic("raw", &["vector"], Storage::Raw, Some("raw"), false),
    basic("list", &["vector"], Storage::List, Some("list"), false),
    basic(
        "expression",
        &["vector"],
        Storage::Expression,
        Some("expression"),
        false,
    ),
    basic(
        "function",
        &["OptionalFunction", "PossibleMethod"],
        Storage::Function,
        Some("function"),
        false,
    ),
    basic(
        "environment",
        &["refObject"],
        Storage::Environment,
        None,
        false,
    ),
    basic("externalptr", &["refObject"], Storage::Null, None, false),
    basic(
        "name",
        &["language", "refObject"],
        Storage::Name,
        None,
        false,
    ),
    basic("call", &["language"], Storage::Call, Some("call"), false),
    basic("structure", &["vector"], Storage::Numeric, None, true),
    basic(
        "array",
        &["structure"],
        Storage::Array,
        Some("array"),
        false,
    ),
    basic("matrix", &["array"], Storage::Matrix, Some("matrix"), false),
    Basic {
        slots: &[(".S3Class", "character")],
        sealed: false,
        ..basic("oldClass", &[], Storage::Object, None, true)
    },
    Basic {
        slots: &[
            (".Data", "list"),
            ("names", "character"),
            ("row.names", "data.frameRowLabels"),
            (".S3Class", "character"),
        ],
        ..basic(
            "data.frame",
            &["list", "oldClass"],
            Storage::List,
            Some("list"),
            false,
        )
    },
    Basic {
        slots: &[
            (".Data", "integer"),
            ("levels", "character"),
            (".S3Class", "character"),
        ],
        sealed: false,
        ..basic(
            "factor",
            &["integer", "oldClass"],
            Storage::Integer,
            Some("integer"),
            false,
        )
    },
    Basic {
        slots: &[
            (".Data", "integer"),
            ("levels", "character"),
            (".S3Class", "character"),
        ],
        sealed: false,
        ..basic(
            "ordered",
            &["factor"],
            Storage::Integer,
            Some("integer"),
            false,
        )
    },
    Basic {
        slots: &[(".S3Class", "character")],
        sealed: false,
        ..basic("formula", &["oldClass"], Storage::List, None, false)
    },
    Basic {
        slots: &[
            (".Data", "vector"),
            ("tsp", "numeric"),
            (".S3Class", "character"),
        ],
        ..basic(
            "ts",
            &["structure", "oldClass"],
            Storage::Logical,
            Some("vector"),
            false,
        )
    },
    Basic {
        slots: &[
            (".Data", "character"),
            ("names", "character"),
            ("package", "character"),
        ],
        ..basic(
            "signature",
            &["character"],
            Storage::Character,
            Some("character"),
            false,
        )
    },
    Basic {
        slots: &[(".Data", "character"), ("package", "character")],
        sealed: false,
        ..basic(
            "className",
            &["character"],
            Storage::Character,
            Some("character"),
            false,
        )
    },
    Basic {
        slots: &[(".Data", "character"), ("package", "character")],
        ..basic(
            "ObjectsWithPackage",
            &["character"],
            Storage::Character,
            Some("character"),
            false,
        )
    },
    Basic {
        slots: &[
            ("slots", "list"),
            ("contains", "list"),
            ("virtual", "logical"),
            ("prototype", "ANY"),
            ("validity", "OptionalFunction"),
            ("access", "list"),
            ("className", "character"),
            ("package", "character"),
            ("subclasses", "list"),
            ("versionKey", "externalptr"),
            ("sealed", "logical"),
        ],
        ..basic("classRepresentation", &[], Storage::Object, None, false)
    },
    Basic {
        slots: &[
            ("subClass", "character"),
            ("superClass", "character"),
            ("package", "character"),
            ("coerce", "function"),
            ("test", "function"),
            ("replace", "function"),
            ("simple", "logical"),
            ("by", "character"),
            ("dataPart", "logical"),
            ("distance", "numeric"),
        ],
        ..basic("SClassExtension", &[], Storage::Object, None, false)
    },
    Basic {
        slots: &[
            ("object", "ANY"),
            ("slots", "character"),
            ("dataPart", "logical"),
        ],
        ..basic("classPrototypeDef", &[], Storage::Object, None, false)
    },
    Basic {
        slots: &[
            (".Data", "function"),
            ("className", "character"),
            ("package", "character"),
        ],
        sealed: false,
        ..basic(
            "classGeneratorFunction",
            &["function"],
            Storage::Function,
            Some("function"),
            false,
        )
    },
];

fn zero(ctx: &mut Ctx, storage: &Storage, ns: SexpId) -> EvalResult {
    Ok(match storage {
        Storage::Null => SEXP_NIL,
        Storage::Object => ctx.session_mut().alloc_s4_object()?,
        Storage::Logical => ctx.session_mut().alloc_lgl_from(&[])?,
        Storage::Integer => ctx.session_mut().alloc_int_from(&[])?,
        Storage::Numeric | Storage::Matrix | Storage::Array => {
            ctx.session_mut().alloc_real_from(&[])?
        }
        Storage::Complex => ctx.session_mut().alloc_cplx_from(&[])?,
        Storage::Character => ctx.session_mut().alloc_str_from(&[])?,
        Storage::Raw => ctx.session_mut().alloc_raw_from(&[])?,
        Storage::List => ctx.session_mut().alloc_list(0)?,
        Storage::Expression => ctx.session_mut().alloc_expr_from(&[])?,
        Storage::Function => ctx.session_mut().alloc_closure(SEXP_NIL, SEXP_NIL, ns)?,
        Storage::Environment => ctx.session_mut().alloc_env(EMPTY_ENV)?,
        Storage::Name => ctx.session_mut().intern(".__S4_prototype__")?.as_sexp(),
        Storage::Call => {
            let head = ctx.session_mut().intern("quote")?;
            ctx.session_mut()
                .lcons(head.as_sexp(), SEXP_NIL, SEXP_NIL)?
        }
    })
}

pub(in crate::class) fn bootstrap(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(0);
    ctx.ext::<ClassState>().namespace = Some(env);
    for basic in BUILTINS {
        let prototype = zero(ctx, &basic.storage, env)?;
        if matches!(basic.storage, Storage::Matrix | Storage::Array) {
            let dims = if matches!(basic.storage, Storage::Matrix) {
                &[0, 0][..]
            } else {
                &[0][..]
            };
            let dim = ctx.session_mut().alloc_int_from(dims)?;
            set_attr(ctx, prototype, "dim", dim)?;
        }
        if !basic.slots.is_empty() {
            ctx.session_mut().set_s4_flag(prototype, true)?;
            for &(slot, typ) in basic.slots.iter().filter(|(slot, _)| *slot != ".Data") {
                let value = if slot == ".S3Class" && basic.name != "oldClass" {
                    let classes = if basic.name == "ordered" {
                        &["ordered", "factor"][..]
                    } else {
                        &[basic.name][..]
                    };
                    strings(ctx, classes.iter().copied())?
                } else if slot == "row.names" {
                    ctx.session_mut().alloc_int_from(&[])?
                } else {
                    default_value(ctx, typ, env)?
                };
                write_slot(ctx, prototype, slot, value)?;
            }
        }
        let def = Definition {
            info: ClassInfo {
                name: basic.name.into(),
                package: "methods".into(),
                slots: basic
                    .slots
                    .iter()
                    .map(|(slot, typ)| ((*slot).into(), (*typ).into()))
                    .collect(),
                superclasses: Vec::new(),
                is_virtual: basic.virtual_,
                class_def: SEXP_NIL,
            },
            record: Record {
                where_: env,
                edges: basic
                    .parents
                    .iter()
                    .map(|parent| Edge {
                        target: ((*parent).into(), "methods".into()),
                        methods: None,
                    })
                    .collect(),
                data: basic.data.map(str::to_owned),
            },
            prototype,
            validity: SEXP_NIL,
            access: make_list(ctx, &[], false)?,
            sealed: basic.sealed,
        };
        install(ctx, def)?;
    }
    refresh(ctx)?;
    let has_slot = ctx.session_mut().intern(".hasSlot")?;
    if let Some(binding) = ctx.session().env_get(env, has_slot) {
        ctx.session_mut()
            .env_define_binding(BASE_ENV, has_slot, binding)?;
        if let Some(exports) = ctx.ns_exports_env(env)? {
            let name = ctx.session_mut().scalar_str(".hasSlot")?;
            ctx.session_mut().env_define(exports, has_slot, name)?;
        }
    }
    Ok(SEXP_NIL)
}
