//! Value tests based on GNU R 4.6.2 black-box probes.

use roxide_diag::SrcFileId;
use roxide_gc::{GcPolicy, SexpId};
use roxide_syntax::Parser;
use roxide_value::{
    BASE_ENV, CharId, DeparseOpts, EMPTY_ENV, Encoding, GLOBAL_ENV, IdenticalOpts, KeepSource,
    NA_INTEGER, NA_REAL, NA_REAL_BITS, RComplex, RLogical, RSession, SEXP_NIL, SexpType, ValueApi,
    ValueError, deparse, dput, identical, is_na_real, lower,
};

fn session() -> RSession {
    RSession::new(GcPolicy::default()).expect("Session")
}

fn parse(s: &mut RSession, src: &str) -> SexpId {
    let mut parser = Parser::new(src, SrcFileId(0));
    let nodes = parser.parse_all().expect("Parse input");
    lower(parser.ast(), nodes[0], s, KeepSource::No).expect("Lower input")
}

fn display(s: &RSession, x: SexpId) -> String {
    dput(s, x, DeparseOpts::default())
        .expect("Deparse input")
        .join("\n")
}

fn equivalent(s: &RSession, x: SexpId, y: SexpId) -> bool {
    identical(s, x, y, IdenticalOpts::default()).expect("Compare values")
}

fn strings(s: &mut RSession, words: &[Option<&str>]) -> SexpId {
    let chars: Vec<_> = words
        .iter()
        .map(|word| {
            word.map_or(CharId::NA, |word| {
                s.mk_char(word.as_bytes(), Encoding::Utf8)
                    .expect("Intern text")
            })
        })
        .collect();
    s.alloc_str_from(&chars).expect("Allocate strings")
}

#[test]
fn deparses_r_atomic_vectors() {
    let mut s = session();
    for (values, expected) in [
        (&[1, 2, 3][..], "1:3"),
        (&[3, 2, 1][..], "3:1"),
        (&[1, 2][..], "1:2"),
        (&[NA_INTEGER, NA_INTEGER][..], "c(NA_integer_, NA_integer_)"),
        (&[1, NA_INTEGER][..], "c(1L, NA)"),
        (&[3, 1, 2][..], "c(3L, 1L, 2L)"),
    ] {
        let x = s.alloc_int_from(values).unwrap();
        assert_eq!(display(&s, x), expected);
    }
    let x = strings(&mut s, &[None, Some("NA"), Some("")]);
    assert_eq!(display(&s, x), r#"c(NA, "NA", "")"#);
    let raw = s.alloc_raw_from(&[0, 25, 255]).unwrap();
    assert_eq!(display(&s, raw), "as.raw(c(0x00, 0x19, 0xff))");
    let z = s
        .alloc_cplx_from(&[
            RComplex { re: 2.25, im: 0.0 },
            RComplex { re: 3.0, im: -4.0 },
            RComplex { re: 0.0, im: 2.0 },
            RComplex::NA,
        ])
        .unwrap();
    assert_eq!(display(&s, z), "c(2.25+0i, 3-4i, 0+2i, NA)");
    let z = s
        .alloc_cplx_from(&[RComplex {
            re: f64::NAN,
            im: 0.0,
        }])
        .unwrap();
    assert_eq!(display(&s, z), "complex(real=NaN, imaginary=0)");
    let z = s.alloc_cplx_from(&[RComplex::NA]).unwrap();
    assert_eq!(display(&s, z), "NA_complex_");
    let doubles = s.alloc_real_from(&[1e4, 1e5, 1e-4, 1.0 / 3.0]).unwrap();
    assert_eq!(
        display(&s, doubles),
        "c(10000, 1e+05, 1e-04, 0.333333333333333)"
    );
    let x = s.scalar_real(1.0 / 3.0).unwrap();
    let opts = DeparseOpts {
        digits17: true,
        ..DeparseOpts::default()
    };
    assert_eq!(deparse(&s, x, opts).unwrap(), ["0.33333333333333331"]);
}

#[test]
fn normalizes_names_and_expands_row_names() {
    let mut s = session();
    let names = s.intern("names").unwrap();
    let x = s.alloc_int_from(&[4, 8, 12]).unwrap();
    let short = strings(&mut s, &[Some("first"), Some("")]);
    s.set_attrib(x, names, short).unwrap();
    let normalized = s.attr(x, names);
    assert_eq!(s.str_slice(normalized).unwrap()[2], CharId::NA);
    assert_eq!(
        display(&s, x),
        r#"structure(c(4L, 8L, 12L), names = c("first", "", NA))"#
    );

    let long = strings(&mut s, &[Some("first"); 4]);
    assert!(matches!(
        s.set_attrib(x, names, long),
        Err(ValueError::InvalidAttribute(_))
    ));
    assert_eq!(s.attr(x, names), normalized);
    s.set_attrib(x, names, SEXP_NIL).unwrap();
    assert_eq!(s.attr(x, names), SEXP_NIL);

    let x = s.alloc_int_from(&[4, 8]).unwrap();
    let both = strings(&mut s, &[Some("a"), Some("b")]);
    s.set_attrib(x, names, both).unwrap();
    assert_eq!(display(&s, x), "c(a = 4L, b = 8L)");
    let partial = strings(&mut s, &[Some("x y"), Some("")]);
    s.set_attrib(x, names, partial).unwrap();
    assert_eq!(display(&s, x), "c(`x y` = 4L, 8L)");
    assert_eq!(
        deparse(&s, x, DeparseOpts::default()).unwrap(),
        [r#"c("x y" = 4L, 8L)"#]
    );

    let seq = s.alloc_compact_intseq(1, 2, 1).unwrap();
    s.set_attrib(seq, names, both).unwrap();
    assert_eq!(display(&s, seq), r#"structure(1:2, names = c("a", "b"))"#);

    let rn = s.intern("row.names").unwrap();
    let compact = s.alloc_int_from(&[NA_INTEGER, -3]).unwrap();
    s.set_attrib(x, rn, compact).unwrap();
    let expanded = s.attr_row_names(x).unwrap();
    assert_eq!(s.int(expanded).unwrap().to_vec(), [1, 2, 3]);
    assert_eq!(s.attr(x, rn), compact);
    assert_eq!(
        display(&s, x),
        "structure(c(`x y` = 4L, 8L), row.names = c(NA, -3L))"
    );
    s.clear_attrs(x).unwrap();
    assert!(s.attrs(x).unwrap().is_empty());
    assert_eq!(s.heap.refcnt(compact), 0);
}

#[test]
fn deparses_calls_and_closures() {
    let mut s = session();
    for (source, expected) in [
        ("(2 + 3) * 4 ^ 2", "(2 + 3) * 4^2"),
        (
            "6 |> (\\(a, b = 7) a * b)()",
            "(function(a, b = 7) a * b)(6)",
        ),
        ("x <- f(`odd name`, , y = )", "x <- f(`odd name`, , y = )"),
        ("a / b + -c ^ 2", "a/b + -c^2"),
        ("a <- base::x", "a <- base::x"),
        ("a <- x[[i]]$y", "a <- x[[i]]$y"),
        ("x -> y", "y <- x"),
        (
            "if (x) { f() } else { g() }",
            "if (x) {\n    f()\n} else {\n    g()\n}",
        ),
        ("for (i in 1:3) print(i)", "for (i in 1:3) print(i)"),
        ("while (!done) next", "while (!done) next"),
        ("repeat break", "repeat break"),
        (
            "f <- function(x, ..., y = 1L) { x + y }",
            "f <- function(x, ..., y = 1L) {\n    x + y\n}",
        ),
    ] {
        let x = parse(&mut s, source);
        assert_eq!(display(&s, x), expected, "{source}");
    }
    let op = s.intern("+").unwrap().as_sexp();
    assert_eq!(deparse(&s, op, DeparseOpts::default()).unwrap(), ["+"]);
    assert_eq!(display(&s, op), "`+`");
    let x = parse(&mut s, "function(x, y = 2L) { x + y }");
    let (_, args, _) = s.cons_parts(x).unwrap();
    let values: Vec<_> = s.pairlist_iter(args).map(|(_, value)| value).collect();
    let closure = s.alloc_closure(values[0], values[1], GLOBAL_ENV).unwrap();
    assert_eq!(
        display(&s, closure),
        "function (x, y = 2L) \n{\n    x + y\n}"
    );

    let default = s.scalar_int(1).unwrap();
    let a = s.intern("a").unwrap().as_sexp();
    let pair = s.cons(default, SEXP_NIL, a).unwrap();
    assert_eq!(display(&s, pair), "pairlist(a = 1L)");
    let call = parse(&mut s, "weight * count + offset");
    let expr = s.alloc_expr_from(&[call, default]).unwrap();
    assert_eq!(display(&s, expr), "expression(weight * count + offset, 1L)");
}

#[test]
fn wraps_vectors_lists_and_calls() {
    let mut s = session();
    let x = s.alloc_int_from(&[10; 10]).unwrap();
    let opts = DeparseOpts {
        width_cutoff: 22,
        ..DeparseOpts::default()
    };
    assert_eq!(
        deparse(&s, x, opts).unwrap(),
        [
            "c(10L, 10L, 10L, 10L, 10L, ",
            "10L, 10L, 10L, 10L, 10L",
            ")",
        ]
    );
    let x = parse(
        &mut s,
        "f(abcdefghijk, abcdefghijk, abcdefghijk, abcdefghijk, abcdefghijk, abcdefghijk)",
    );
    let opts = DeparseOpts {
        width_cutoff: 20,
        ..DeparseOpts::default()
    };
    assert_eq!(
        deparse(&s, x, opts).unwrap(),
        [
            "f(abcdefghijk, abcdefghijk, ",
            "    abcdefghijk, abcdefghijk, ",
            "    abcdefghijk, abcdefghijk)",
        ]
    );
    let x = strings(
        &mut s,
        &[
            Some("NULL"),
            Some("raw"),
            Some("logical"),
            Some("integer"),
            Some("double"),
            Some("complex"),
            Some("character"),
            Some("list"),
            Some("closure"),
        ],
    );
    assert_eq!(
        display(&s, x),
        "c(\"NULL\", \"raw\", \"logical\", \"integer\", \"double\", \"complex\", \"character\", \n\"list\", \"closure\")"
    );
}

#[test]
fn compares_bits_attributes_and_recursive_values() {
    let mut s = session();
    let pos = s.scalar_real(0.0).unwrap();
    let neg = s.scalar_real(-0.0).unwrap();
    assert!(equivalent(&s, pos, neg));
    let bits = IdenticalOpts {
        num_eq: false,
        single_na: false,
        ..IdenticalOpts::default()
    };
    assert!(!identical(&s, pos, neg, bits).unwrap());
    let na = s.scalar_real(NA_REAL).unwrap();
    let quiet = s
        .scalar_real(f64::from_bits(NA_REAL_BITS | (1 << 51)))
        .unwrap();
    let nan = s.scalar_real(f64::NAN).unwrap();
    assert!(equivalent(&s, na, quiet));
    assert!(!equivalent(&s, na, nan));
    assert!(!identical(&s, na, quiet, bits).unwrap());

    let x = s.alloc_list(1).unwrap();
    s.list_set(x, 0, x).unwrap();
    let y = s.deep_duplicate(x).unwrap();
    assert_ne!(x, y);
    assert_eq!(s.list_get(y, 0).unwrap(), y);
    assert!(equivalent(&s, x, y));
    let a = s.intern("a").unwrap();
    let b = s.intern("b").unwrap();
    s.set_attrib(x, a, na).unwrap();
    s.set_attrib(x, b, pos).unwrap();
    s.set_attrib(y, b, neg).unwrap();
    s.set_attrib(y, a, quiet).unwrap();
    assert!(equivalent(&s, x, y));
    let order = IdenticalOpts {
        attrib_as_set: false,
        ..IdenticalOpts::default()
    };
    assert!(!identical(&s, x, y, order).unwrap());
    s.set_s4_flag(y, true).unwrap();
    assert!(!equivalent(&s, x, y));

    let e1 = s.alloc_env(EMPTY_ENV).unwrap();
    let e2 = s.alloc_env(EMPTY_ENV).unwrap();
    assert!(!equivalent(&s, e1, e2));
    assert_eq!(s.deep_duplicate(e1).unwrap(), e1);
    let f1 = s.alloc_closure(SEXP_NIL, pos, e1).unwrap();
    let f2 = s.alloc_closure(SEXP_NIL, neg, e2).unwrap();
    assert!(!equivalent(&s, f1, f2));
    let srcref = s.intern("srcref").unwrap();
    s.set_attrib(f1, srcref, pos).unwrap();
    let opts = IdenticalOpts {
        ignore_environment: true,
        ..IdenticalOpts::default()
    };
    assert!(identical(&s, f1, f2, opts).unwrap());
    let opts = IdenticalOpts {
        ignore_srcref: false,
        ..opts
    };
    assert!(!identical(&s, f1, f2, opts).unwrap());

    let body1 = parse(&mut s, "{ x }");
    let body2 = s.shallow_duplicate(body1).unwrap();
    let whole = s.intern("wholeSrcref").unwrap();
    s.set_attrib(body2, whole, pos).unwrap();
    assert!(!equivalent(&s, body1, body2));
    let f1 = s.alloc_closure(SEXP_NIL, body1, e1).unwrap();
    let f2 = s.alloc_closure(SEXP_NIL, body2, e1).unwrap();
    assert!(equivalent(&s, f1, f2));
    assert!(!identical(&s, f1, f2, opts).unwrap());

    let a = parse(&mut s, "f(a = x + 1L)");
    let b = parse(&mut s, "f(a = x + 1L)");
    let c = parse(&mut s, "f(b = x + 1L)");
    assert!(equivalent(&s, a, b));
    assert!(!equivalent(&s, a, c));
}

#[test]
fn coerces_scalars_and_lists() {
    let mut s = session();
    let words = strings(&mut s, &[Some(" 0x15 "), Some("1e2"), Some("bad"), None]);
    let (ints, warn) = s.coerce(words, SexpType::Int).unwrap();
    assert_eq!(
        s.int(ints).unwrap().to_vec(),
        [21, 100, NA_INTEGER, NA_INTEGER]
    );
    assert!(warn.na_introduced);
    let words = strings(
        &mut s,
        &[Some("TRUE"), Some("F"), Some("1"), Some("0"), None],
    );
    let (logical, warn) = s.coerce(words, SexpType::Lgl).unwrap();
    assert_eq!(
        s.lgl(logical).unwrap().to_vec(),
        [
            RLogical::TRUE,
            RLogical::FALSE,
            RLogical::NA,
            RLogical::NA,
            RLogical::NA,
        ]
    );
    assert!(warn.na_introduced);

    let words = strings(
        &mut s,
        &[
            Some("1e-3-2i"),
            Some("0x1p2+0x2p0i"),
            Some("3i"),
            Some("1 + 2i"),
        ],
    );
    let (complex, warn) = s.coerce(words, SexpType::Cplx).unwrap();
    assert_eq!(display(&s, complex), "c(0.001-2i, 4+2i, 0+3i, NA)");
    assert!(warn.na_introduced);
    let (double, warn) = s.coerce(complex, SexpType::Real).unwrap();
    let vals = s.real(double).unwrap().to_vec();
    assert_eq!(vals[..3], [0.001, 4.0, 0.0]);
    assert!(is_na_real(vals[3]));
    assert!(warn.imaginary_discarded);

    let (logical, warn) = s.coerce(complex, SexpType::Lgl).unwrap();
    assert_eq!(
        s.lgl(logical).unwrap().to_vec(),
        [RLogical::TRUE, RLogical::TRUE, RLogical::TRUE, RLogical::NA],
    );
    assert!(!warn.imaginary_discarded);
    let words = strings(&mut s, &[Some("NaN")]);
    let (value, warn) = s.coerce(words, SexpType::Int).unwrap();
    assert_eq!(s.int(value).unwrap().get(0), 0);
    assert!(!warn.na_introduced);

    let x = s
        .alloc_real_from(&[
            2147483647.5,
            -2147483647.5,
            f64::INFINITY,
            f64::NAN,
            NA_REAL,
        ])
        .unwrap();
    let (integer, warn) = s.coerce(x, SexpType::Int).unwrap();
    assert_eq!(
        s.int(integer).unwrap().to_vec(),
        [i32::MAX, -i32::MAX, NA_INTEGER, NA_INTEGER, NA_INTEGER]
    );
    assert!(warn.na_introduced);
    let raw = s.alloc_raw_from(&[0, 25, 255]).unwrap();
    let (list, _) = s.coerce(raw, SexpType::Vec).unwrap();
    assert_eq!(
        display(&s, list),
        "list(as.raw(0x00), as.raw(0x19), as.raw(0xff))"
    );
    let (back, warn) = s.coerce(list, SexpType::Raw).unwrap();
    assert!(equivalent(&s, raw, back));
    assert!(!warn.na_introduced);
    let (empty, _) = s.coerce(SEXP_NIL, SexpType::Cplx).unwrap();
    assert_eq!(s.xlength(empty).unwrap(), 0);
    assert_eq!(s.coerce(raw, SexpType::Nil).unwrap().0, SEXP_NIL);
}

#[test]
fn alt_buffers_are_private_for_reads_and_writes() {
    let mut s = session();
    let compact = s.alloc_compact_intseq(3, 3, -1).unwrap();
    let dense = s.alloc_int_from(&[3, 2, 1]).unwrap();
    assert!(s.int(compact).unwrap().as_slice().is_none());
    assert!(equivalent(&s, compact, dense));
    assert!(s.alloc_compact_intseq(i32::MAX, 2, 1).is_err());
    assert_eq!(s.int_dataptr_ro(compact).unwrap(), [3, 2, 1]);

    let a = s.alloc_alt_wrapper(dense).unwrap();
    let b = s.alloc_alt_wrapper(dense).unwrap();
    assert_eq!(s.heap.refcnt(dense), 2);
    let original = s.int(dense).unwrap().as_slice().unwrap().as_ptr();
    assert!(s.int(a).unwrap().as_slice().is_none());
    let long = s.alloc_compact_intseq(1, 2_000_000_000, 1).unwrap();
    assert_eq!(display(&s, long), "1:2000000000");
    let read_only = s.int_dataptr_ro(a).unwrap().as_ptr();
    assert_ne!(read_only, original);
    assert_ne!(read_only, s.int_dataptr_ro(b).unwrap().as_ptr());
    assert_eq!(s.int_dataptr_ro(a).unwrap().as_ptr(), read_only);
    s.int_mut(b).unwrap()[0] = 77;
    assert_eq!(s.int_dataptr_ro(a).unwrap(), [3, 2, 1]);
    assert_eq!(s.int(dense).unwrap().to_vec(), [3, 2, 1]);
    assert_eq!(s.int(b).unwrap().to_vec(), [77, 2, 1]);

    let copy = s.shallow_duplicate(b).unwrap();
    s.int_mut(copy).unwrap()[1] = 99;
    assert_eq!(s.int(b).unwrap().to_vec(), [77, 2, 1]);
    assert_eq!(s.int(copy).unwrap().to_vec(), [77, 99, 1]);

    let reals = s.alloc_compact_realseq(2147483648.0, 3, 1.0).unwrap();
    let wrapper = s.alloc_alt_wrapper(reals).unwrap();
    s.real_mut(wrapper).unwrap()[0] = 0.0;
    assert_eq!(s.real(reals).unwrap().get(0), 2147483648.0);
    assert_eq!(s.real(wrapper).unwrap().get(0), 0.0);

    let str = strings(&mut s, &[Some("blue")]);
    let wrapped = s.alloc_alt_wrapper(str).unwrap();
    assert!(equivalent(&s, str, wrapped));
    assert_ne!(
        s.str_slice(str).unwrap().as_ptr(),
        s.str_slice(wrapped).unwrap().as_ptr()
    );
}

#[test]
fn edits_environment_frames_without_losing_bindings() {
    let mut s = session();
    let env = s.alloc_env(EMPTY_ENV).unwrap();
    let a = s.intern("a").unwrap();
    let b = s.intern("b").unwrap();
    let x = s.scalar_int(1).unwrap();
    s.env_define(env, a, x).unwrap();
    s.env_define(env, b, x).unwrap();
    assert_eq!(s.sym_name_of(a.as_sexp()).unwrap(), "a");
    assert!(s.env_remove(env, a).unwrap());
    assert_eq!(s.heap.refcnt(x), 1);
    assert_eq!(s.env_get(env, b).unwrap().value, x);
    assert_eq!(s.env_entries(env).unwrap()[0].0, b);
    s.env_set_locked(env, true).unwrap();
    assert!(s.env_remove(env, b).is_err());
    assert!(s.env_define(env, a, x).is_err());
    s.env_set_locked(env, false).unwrap();
    s.env_set_enclos(env, BASE_ENV).unwrap();
    assert_eq!(s.env_enclos(env).unwrap(), BASE_ENV);
}

#[test]
fn deparses_matrix_and_data_frame_attributes() {
    let mut s = session();
    let dim = s.intern("dim").unwrap();
    let dimnames = s.intern("dimnames").unwrap();
    let names = s.intern("names").unwrap();
    let class = s.intern("class").unwrap();
    let row_names = s.intern("row.names").unwrap();
    let x = s.alloc_int_from(&[2, 4, 6]).unwrap();
    let dims = s.alloc_int_from(&[1, 3]).unwrap();
    let rows = s.scalar_str("b").unwrap();
    let cols = strings(&mut s, &[Some("x"), Some("y"), Some("z")]);
    let labels = s.alloc_list(2).unwrap();
    s.list_set(labels, 0, rows).unwrap();
    s.list_set(labels, 1, cols).unwrap();
    s.set_attrib(x, dim, dims).unwrap();
    s.set_attrib(x, dimnames, labels).unwrap();
    assert_eq!(
        display(&s, x),
        "structure(c(2L, 4L, 6L), dim = c(1L, 3L), dimnames = list(\"b\", \n    c(\"x\", \"y\", \"z\")))",
    );

    let score = s.alloc_int_from(&[4, 8, 12]).unwrap();
    let df = s.alloc_list(1).unwrap();
    s.list_set(df, 0, score).unwrap();
    let column = s.scalar_str("score").unwrap();
    let typ = s.scalar_str("data.frame").unwrap();
    let compact = s.alloc_int_from(&[NA_INTEGER, -3]).unwrap();
    s.set_attrib(df, names, column).unwrap();
    s.set_attrib(df, class, typ).unwrap();
    s.set_attrib(df, row_names, compact).unwrap();
    assert!(s.is_object(df).unwrap());
    assert_eq!(
        display(&s, df),
        "structure(list(score = c(4L, 8L, 12L)), class = \"data.frame\", row.names = c(NA, \n-3L))",
    );
    let duplicate = s.shallow_duplicate(df).unwrap();
    let expanded = s.attr_row_names(df).unwrap();
    s.set_attrib(duplicate, row_names, expanded).unwrap();
    assert!(equivalent(&s, df, duplicate));
}
