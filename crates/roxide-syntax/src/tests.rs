use roxide_diag::{SourceMap, SrcFileId};

use crate::{Arg, ArgName, Ast, Const, Node, NodeId, ParseStep, Parser, StrEncoding, ops};

#[test]
fn string_escape_encoding_preserves_native_bytes() {
    for (source, bytes, enc) in [
        (r#""\xc3\xa9""#, &b"\xc3\xa9"[..], StrEncoding::Native),
        (r#""\xe9""#, &b"\xe9"[..], StrEncoding::Native),
        (r#""abc\xc3\xa9""#, &b"abc\xc3\xa9"[..], StrEncoding::Native),
        (r#""\303\251""#, &b"\xc3\xa9"[..], StrEncoding::Native),
        (r#""\351""#, &b"\xe9"[..], StrEncoding::Native),
        (r#""é\x21""#, "é!".as_bytes(), StrEncoding::Native),
        (r#""é\41""#, "é!".as_bytes(), StrEncoding::Native),
        (r#""\u00e9""#, "é".as_bytes(), StrEncoding::Utf8),
        (r#""\U0001f600""#, "😀".as_bytes(), StrEncoding::Utf8),
        (r#""\u0041""#, &b"A"[..], StrEncoding::Native),
        (r#""é""#, "é".as_bytes(), StrEncoding::Utf8),
    ] {
        let mut parser = Parser::new(source, SrcFileId(0));
        let nodes = parser.parse_all().unwrap();
        assert_eq!(
            parser.ast().node(nodes[0]),
            &Node::Const(Const::Str {
                bytes: bytes.into(),
                enc,
            }),
            "{source}"
        );
    }
}

fn shape(ast: &Ast, id: NodeId) -> String {
    match ast.node(id) {
        Node::Sym(symbol) => symbol.to_string(),
        Node::Empty => "_".into(),
        Node::Call { func, args } => {
            let mut parts = vec![shape(ast, *func)];
            parts.extend(ast.args(args.clone()).iter().map(|arg| argument(ast, arg)));
            format!("({})", parts.join(" "))
        }
        Node::Function { formals, body, .. } => {
            let formals: Vec<_> = ast
                .args(formals.clone())
                .iter()
                .map(|arg| argument(ast, arg))
                .collect();
            format!("(function ({}) {})", formals.join(" "), shape(ast, *body))
        }
        Node::Const(value) => match value {
            Const::Null => "NULL".into(),
            Const::Lgl(Some(true)) => "TRUE".into(),
            Const::Lgl(Some(false)) => "FALSE".into(),
            Const::Lgl(None) => "NA".into(),
            Const::Int(n) => format!("{n}L"),
            Const::Real(bits) => f64::from_bits(*bits).to_string(),
            Const::Complex { re_bits, im_bits } => {
                format!("{}+{}i", f64::from_bits(*re_bits), f64::from_bits(*im_bits))
            }
            Const::Str { bytes, .. } => format!("{:?}", String::from_utf8_lossy(bytes)),
            Const::NaChar => "NA_character_".into(),
        },
    }
}

fn argument(ast: &Ast, arg: &Arg) -> String {
    let value = arg.value.map_or_else(|| "_".into(), |id| shape(ast, id));
    match &arg.name {
        None => value,
        Some(ArgName::Sym(name)) => format!("{name}={value}"),
        Some(ArgName::Str(name)) => format!("{name:?}={value}"),
        Some(ArgName::Null) => format!("NULL={value}"),
    }
}

fn parsed_shapes(source: &str) -> Vec<String> {
    let mut parser = Parser::new(source, SrcFileId(0));
    parser
        .parse_all()
        .unwrap_or_else(|err| panic!("{source:?}: {err}"))
        .iter()
        .map(|&root| shape(parser.ast(), root))
        .collect()
}

const LAYER1: &str = include_str!("../../../conformance/layer1/01_syntax_deparse.rcase");

#[test]
fn layer1_syntax_cases_match_gnu_r_trees() {
    let expected: &[(&str, &[&str])] = &[
        (
            "pipe_first_argument",
            &[
                "(<- f (function (x=_ y=_) (- x y)))",
                "(<- x 12)",
                "(print (f x 5))",
            ],
        ),
        (
            "pipe_named_placeholder",
            &[
                "(<- f (function (x=_ y=_) (- x y)))",
                "(<- x 12)",
                "(print (f 5 y=x))",
            ],
        ),
        (
            "pipe_dollar_extraction",
            &[
                "(<- x (list a=17L b=23L))",
                "(dput ($ x a))",
                "(dput ($ x b))",
            ],
        ),
        (
            "pipe_double_bracket_extraction",
            &[
                "(<- x (list 17L 23L))",
                "(dput ([[ x 1))",
                "(dput ([[ x 2))",
            ],
        ),
        (
            "pipe_single_bracket_extraction",
            &["(<- x (c a=17L b=23L))", "(dput ([ x 1))", "(dput ([ x 2))"],
        ),
        (
            "pipe_chained_extraction",
            &[
                "(<- x (list a=(list (c first=17L second=23L))))",
                "(dput (unname ([ ([[ ($ x a) 1) 2)))",
                r#"(dput (sum ([[ ([[ x "a") 1)))"#,
            ],
        ),
        (
            "pipe_chained_calls",
            &[
                "(<- x (c 8 2 6 1))",
                "(print (head (sort x decreasing=TRUE) n=2))",
                "(print (sum (rev x)))",
            ],
        ),
        ("pipe_unnamed_placeholder_error", &[]),
        (
            "lambda_default_and_override",
            &[
                "(<- f (function (x=_ y=1) (+ x y)))",
                "(print (f 9))",
                "(print (f 9 y=7))",
            ],
        ),
        (
            "lambda_as_argument",
            &[
                "(<- x (c 2 5 8))",
                "(<- apply_one (function (f=_ x=_) (f x)))",
                "(print (apply_one (function (z=_) (^ z 2)) x))",
            ],
        ),
        (
            "raw_string_parentheses",
            &[
                r#"(<- x "line\\n \"quoted\" \\path")"#,
                "(dput x)",
                "(print (nchar x))",
            ],
        ),
        (
            "raw_string_brackets_and_dashes",
            &[
                r#"(<- x "left ]- right\\end")"#,
                "(dput x)",
                r#"(print (== x "left ]- right\\end"))"#,
            ],
        ),
        (
            "hexadecimal_floats",
            &["(dput 16)", "(dput 0.375)", "(print (typeof 16))"],
        ),
        (
            "hexadecimal_integer_literals",
            &["(dput 16L)", "(dput (+ 42L 1L))", "(print (typeof 16))"],
        ),
        (
            "complex_literals",
            &[
                "(<- z (+ 1 0+2i))",
                "(dput z)",
                "(dput (c (Re z) (Im z) (Im (- 0+3i))))",
            ],
        ),
        (
            "user_infix_precedence",
            &[
                r#"(assign "%op%" (function (x=_ y=_) (- x y)))"#,
                "(print (* 2 (%op% 5 3)))",
                "(print (%op% (%op% 16 4) 2))",
            ],
        ),
        (
            "null_coalescing",
            &[
                "(dput (%||% NULL 42L))",
                r#"(dput (%||% 0L (stop "Unexpected evaluation.")))"#,
                "(dput (%||% NA 42L))",
            ],
        ),
        (
            "left_and_equals_assignment",
            &["(= x (<- y 8L))", "(<- p (<- q 3L))", "(dput (c x y p q))"],
        ),
        (
            "right_assignment_precedence",
            &[
                "(<- y (<- x (+ 2 3)))",
                "(<- z (<- w 4))",
                "(dput (c x y z w))",
            ],
        ),
        (
            "superassignment_precedence",
            &[
                "(local ({ (<- target 0L) (<- f (function () (<- value (<<- target (+ 2L 5L))))) (dput (c (f) target))))",
            ],
        ),
        (
            "newline_inside_parentheses",
            &["(<- value (( (+ 4 9)))", "(dput value)"],
        ),
        (
            "newline_inside_braces",
            &["(<- value ({ 4 (+ 9)))", "(dput value)"],
        ),
        (
            "if_else_newline",
            &[
                "(<- x (if FALSE 4L 6L))",
                "(<- y ({ (if TRUE ({ 9L) ({ 12L))))",
                "(dput (c x y))",
            ],
        ),
        (
            "parse_and_deparse_precedence",
            &[
                r#"(<- expr (parse text="(2 + 3) * 4 ^ 2" keep.source=FALSE))"#,
                "(dput (deparse ([[ expr 1)))",
                "(print (eval expr))",
            ],
        ),
        (
            "parse_deparse_pipe_and_lambda",
            &[
                r#"(<- expr (parse text="6 |> (\\(a, b = 7) a * b)()" keep.source=FALSE))"#,
                "(dput (deparse ([[ expr 1)))",
                "(print (eval expr))",
            ],
        ),
    ];
    let cases: Vec<_> = LAYER1.split("=== case: ").skip(1).collect();
    assert_eq!(cases.len(), expected.len());
    for (case, &(name, trees)) in cases.into_iter().zip(expected) {
        let (header, body) = case.split_once('\n').unwrap();
        assert_eq!(header, format!("{name} ==="));
        if name == "pipe_unnamed_placeholder_error" {
            let err = Parser::new(body, SrcFileId(0)).parse_all().unwrap_err();
            assert!(err.message.contains("named argument"));
            assert_eq!(&body[err.span.lo as usize..err.span.hi as usize], "_");
        } else {
            assert_eq!(parsed_shapes(body), trees, "{name}");
        }
    }
    assert_eq!(
        parsed_shapes("(2 + 3) * 4 ^ 2"),
        ["(* (( (+ 2 3)) (^ 4 2))"]
    );
    assert_eq!(
        parsed_shapes(r"6 |> (\(a, b = 7) a * b)()"),
        ["((( (function (a=_ b=7) (* a b))) 6)"]
    );
}

#[test]
fn rewrites_only_valid_pipe_placeholders() {
    let cases = [
        ("x |> _@slot", "(@ x slot)"),
        ("x |> _[[1]]$a[]", "([ ($ ([[ x 1) a) _)"),
        ("x |> (function(y) y)()", "((( (function (y=_) y)) x)"),
        ("x |> base::identity()", "((:: base identity) x)"),
        ("x |> \"f\"(y)", "(f x y)"),
        ("x |> f(`_`)", "(f x _)"),
        ("x |> f(x |> g(a=_), b=_)", "(f (g a=x) b=x)"),
        ("x |> `+`(a=_)", "(+ a=x)"),
    ];
    for (input, expected) in cases {
        assert_eq!(parsed_shapes(input), [expected]);
    }
    for (input, message) in [
        ("_", "invalid use of pipe placeholder"),
        ("x |> _", "requires a function call"),
        ("x |> y", "requires a function call"),
        ("x |> f(_, 2)", "named argument"),
        ("x |> f(a = g(_))", "invalid use of pipe placeholder"),
        ("x |> f(a = (_))", "invalid use of pipe placeholder"),
        ("x |> f(a = _, b = _)", "may only appear once"),
        ("x |> _[_]", "may only appear once"),
        ("x |> _$a()", "cannot be used in the RHS function"),
        ("x |> a[1]", "not supported in RHS"),
        ("x |> (f())", "not supported in RHS"),
        ("x |> a * b", "requires a function call"),
        ("x |> `+`(1)", "not supported in RHS"),
        ("_abc", "unexpected"),
    ] {
        let error = Parser::new(input, SrcFileId(0)).parse_all().unwrap_err();
        assert!(error.message.contains(message), "{input}: {error}");
    }
}

#[test]
fn binds_members_calls_and_operators() {
    assert!(ops::prec::NS > ops::prec::EXTRACT);
    assert!(ops::prec::EXTRACT > ops::prec::SUBSET);
    for (input, expected) in [
        ("x$f(1)", "(($ x f) 1)"),
        ("x$f[1]", "([ ($ x f) 1)"),
        ("x[1]$f", "($ ([ x 1) f)"),
        ("pkg::fn(1)", "((:: pkg fn) 1)"),
        ("x[[1]][2]", "([ ([[ x 1) 2)"),
        (r#"x$"a""#, r#"($ x "a")"#),
        (r#"x@"a""#, r#"(@ x "a")"#),
        (r#"base::"+""#, r#"(:: base "+")"#),
        (r#""base":::`+`"#, r#"(::: "base" +)"#),
        ("pkg::x@`a b`", "(@ (:: pkg x) a b)"),
        ("x\n$y", ""),
        ("(pkg::\nfn)", "(( (:: pkg fn))"),
        ("x$\ny", "($ x y)"),
        ("-2 ** 3 ^ 2", "(- (^ 2 (^ 3 2)))"),
        (
            "!a < b & c && d | e || f",
            "(|| (| (&& (& (! (< a b)) c) d) e) f)",
        ),
        (
            "(-a:b %foo% c * d) + e",
            "(+ (( (* (%foo% (: (- a) b) c) d)) e)",
        ),
        ("~a~b~c", "(~ (~ (~ a) b) c)"),
        ("?x = y ? z", "(? (? (= x y)) z)"),
        ("x := y := z", "(:= x (:= y z))"),
        ("2 ->> x -> y", "(<- y (<<- x 2))"),
        ("(a < b) >= (c == d)", "(>= (( (< a b)) (( (== c d)))"),
        ("a < !b < c", "(< a (! (< b c)))"),
    ] {
        if expected.is_empty() {
            assert!(Parser::new(input, SrcFileId(0)).parse_all().is_err());
        } else {
            assert_eq!(parsed_shapes(input), [expected], "{input}");
        }
    }
    for input in [
        "x$y::z",
        "x::y::z",
        "(x)::y",
        "pkg::\nfn",
        "x$NA",
        "x$1",
        "a < b < c",
        "a == b >= c",
        "if(x=y) z",
        "while(x=y) z",
        "for(i in x=y) z",
        "f(a+b = c)",
        "f(a=b=c)",
    ] {
        assert!(
            Parser::new(input, SrcFileId(0)).parse_all().is_err(),
            "{input}"
        );
    }
    assert_eq!(
        parsed_shapes("f((x=y), if (a) b = c)"),
        ["(f (( (= x y)) (if a (= b c)))"]
    );
    assert_eq!(parsed_shapes("if(a) b ? c"), ["(? (if a b) c)"]);
}

#[test]
fn binds_else_in_blocks_and_parentheses() {
    assert_eq!(
        parsed_shapes("{if (a) if (b) x\n# comment\nelse y\nelse z}"),
        ["({ (if a (if b x y) z))"]
    );
    assert_eq!(parsed_shapes("(if (a) x\nelse y)"), ["(( (if a x y))"]);
    assert_eq!(parsed_shapes("f(if (a) x\nelse y)"), ["(f (if a x y))"]);
    assert_eq!(parsed_shapes("x[if (a) 1\nelse 2]"), ["([ x (if a 1 2))"]);
    assert_eq!(parsed_shapes("{if (a) b\n+ c}"), ["({ (if a b) (+ c))"]);
    for input in [
        "if (a) x\nelse y",
        "if (a) {x}\nelse y",
        "{if(a) x; else y}",
        "; x",
        "x;; y",
    ] {
        assert!(
            Parser::new(input, SrcFileId(0)).parse_all().is_err(),
            "{input}"
        );
    }
}

#[test]
fn handles_formals_and_missing_arguments() {
    assert_eq!(
        parsed_shapes(r#"function(`a\x20b`, ..., x = 1) f(`x\x20y` = x, , ...)"#),
        ["(function (a b=_ ...=_ x=1) (f x y=x _ ...))"]
    );
    assert_eq!(
        parsed_shapes("x[]; x[[]]; f(); f(,); f(x=)"),
        ["([ x _)", "([[ x _)", "(f)", "(f _ _)", "(f x=_)"]
    );
    for input in [
        "function(x, x) x",
        r"\(x, `\x78`) x",
        "function(..., ...) x",
    ] {
        let err = Parser::new(input, SrcFileId(0)).parse_all().unwrap_err();
        assert!(
            err.message.contains("repeated formal argument"),
            "{input}: {err}"
        );
    }
    for input in [
        "function(x,) x",
        "function(x=) x",
        "function(\"x\") x",
        "function(x=y=z) z",
    ] {
        assert!(
            Parser::new(input, SrcFileId(0)).parse_all().is_err(),
            "{input}"
        );
    }
}

#[test]
fn distinguishes_literals_and_reports_integer_warnings() {
    assert_eq!(
        parsed_shapes(r##"r'---{x}--'y}---'; R"[x]"; 0x1.8p-2i; .5L; 2147483648L; 1.0L"##),
        [
            r#""x}--'y""#,
            r#""x""#,
            "0+0.375i",
            "0.5",
            "2147483648",
            "1L"
        ]
    );
    let mut parser = Parser::new("2147483648L; 1.5L; 1.0L; 1.e2L; 0x10L", SrcFileId(0));
    parser.parse_all().unwrap();
    let warnings: Vec<_> = parser
        .warnings()
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    assert_eq!(
        warnings,
        [
            "non-integer value 2147483648L qualified with L; using numeric value",
            "integer literal 1.5L contains decimal; using numeric value",
            "integer literal 1.0L contains unnecessary decimal point",
        ]
    );
    let mut parser = Parser::new(
        "0x.8p1024; 0x1p-1024; 0x.8p-1023; 0x1.00000000000008p0; 0x1.000000000000081p0; 0x1.00000000000009p0; 0x1p99999999999999999999",
        SrcFileId(0),
    );
    let bits: Vec<_> = parser
        .parse_all()
        .unwrap()
        .into_iter()
        .map(|id| parser.ast().node(id))
        .collect();
    assert_eq!(
        bits,
        [
            &Node::Const(Const::Real(0x7fe0_0000_0000_0000)),
            &Node::Const(Const::Real(0)),
            &Node::Const(Const::Real(0x0004_0000_0000_0000)),
            &Node::Const(Const::Real(1.0f64.to_bits())),
            &Node::Const(Const::Real(1.0f64.to_bits())),
            &Node::Const(Const::Real(1.0f64.to_bits() + 1)),
            &Node::Const(Const::Real(f64::INFINITY.to_bits())),
        ]
    );
    assert_eq!(
        parsed_shapes("\"a\\\nb\"; å\u{2009}+1"),
        [r#""a\nb""#, "(+ å 1)"]
    );
    let mut parser = Parser::new(r#" "\xFF"; "\u00e5"; r'(å)' "#, SrcFileId(0));
    let nodes = parser.parse_all().unwrap();
    for (&node, (expected, enc)) in nodes.iter().zip([
        (&[255][..], StrEncoding::Native),
        ("å".as_bytes(), StrEncoding::Utf8),
        ("å".as_bytes(), StrEncoding::Utf8),
    ]) {
        assert_eq!(
            parser.ast().node(node),
            &Node::Const(Const::Str {
                bytes: expected.into(),
                enc
            })
        );
    }
    for input in [
        "0x",
        "0xp1",
        "0x1p-",
        "1e+",
        r#""\x41\u0042""#,
        r#""\400""#,
        r"`\u0041`",
        "``",
        "😀",
    ] {
        assert!(
            matches!(
                Parser::new(input, SrcFileId(0)).parse_next(),
                ParseStep::Error(_)
            ),
            "{input}"
        );
    }
}

#[test]
fn tracks_line_directives_without_changing_byte_spans() {
    let input = "#line 100 \"first.R\"\nx\n #line 7\ny\n#line5\nz\n#line 30 \"other.R\"\nw";
    let mut sources = SourceMap::new();
    let file = sources.add_file("test.R", input);
    let mut parser = Parser::new(input, file);
    let nodes = parser.parse_all().unwrap();
    let directives = parser.line_directives();
    assert_eq!(directives.len(), 3);
    assert_eq!(directives[0].filename.as_deref(), Some("first.R"));
    assert_eq!(directives[1].filename, None);
    assert_eq!(directives[2].filename.as_deref(), Some("other.R"));
    for directive in directives {
        sources
            .get_mut(file)
            .unwrap()
            .push_line_directive(directive.clone());
    }
    for (&node, (text, logical, physical)) in
        nodes
            .iter()
            .zip([("x", 100, 2), ("y", 102, 4), ("z", 5, 6), ("w", 30, 8)])
    {
        let span = parser.ast().span(node);
        assert_eq!(&input[span.lo as usize..span.hi as usize], text);
        assert_eq!(
            sources.srcref8(span),
            [logical, 1, logical, 1, 1, 1, physical, physical]
        );
    }
    assert!(
        Parser::new("r\"(x\n#line 4\ny)\"", file)
            .line_directives()
            .is_empty()
    );
}

#[test]
fn reports_incomplete_input_after_complete_statements() {
    for input in [
        "(",
        "(1",
        "x[",
        "x[[1]",
        "{",
        "{1",
        "f(1,",
        "1 +",
        "x |>",
        "x$",
        "x::",
        "function(",
        "if (a)",
        "if(a) b else",
        "\"open",
        "r\"(open",
        "`open",
    ] {
        let mut parser = Parser::new(input, SrcFileId(0));
        assert_eq!(parser.parse_next(), ParseStep::Incomplete, "{input}");
        assert_eq!(
            Parser::new(input, SrcFileId(0))
                .parse_all()
                .unwrap_err()
                .message,
            "unexpected end of input",
            "{input}"
        );
    }
    let mut parser = Parser::new("x\n1 + (", SrcFileId(0));
    assert!(matches!(parser.parse_next(), ParseStep::Expr(_)));
    assert_eq!(parser.parse_next(), ParseStep::Incomplete);
    let mut parser = Parser::new("x\n😀", SrcFileId(0));
    assert!(matches!(parser.parse_next(), ParseStep::Expr(_)));
    assert!(matches!(parser.parse_next(), ParseStep::Error(_)));
    for input in ["x[)", "{]", "if (a) )", "function(,", "x + ;", "x::\ny"] {
        assert!(
            matches!(
                Parser::new(input, SrcFileId(0)).parse_next(),
                ParseStep::Error(_)
            ),
            "{input}"
        );
    }
}

#[test]
fn rejects_excessive_nesting() {
    let input = format!("{}x{}", "(".repeat(1000), ")".repeat(1000));
    let error = Parser::new(&input, SrcFileId(0)).parse_all().unwrap_err();
    assert_eq!(error.message, "expression nested too deeply");
}
