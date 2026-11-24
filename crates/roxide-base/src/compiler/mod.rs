use std::collections::HashMap;
use std::rc::Rc;

use roxide_eval::vm::{self, CompiledCode};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, Package, PrimDef,
    PrimFn, PrimKind, Visibility,
};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{CharId, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::ops::common::{error, vector_limit};

const RUN: &str = ".rox_compiler_run";
const PROGRAM_CACHE_LIMIT: usize = 256;

pub(crate) const PACKAGE: Package = Package {
    name: "compiler",
    attach: false,
    exports: Some(&[
        "cmpfile",
        "cmpfun",
        "compile",
        "compilePKGS",
        "disassemble",
        "enableJIT",
        "getCompilerOption",
        "loadcmp",
        "setCompilerOptions",
    ]),
    modules: &[&Module {
        prims: &[
            internal("rox_compiler_jit", enable_jit, 1),
            internal("rox_compiler_pkgs", compile_pkgs, 1),
            internal("rox_compiler_compile", compile, 2),
            internal("rox_compiler_cmpfun", cmpfun, 1),
            internal("rox_compiler_disassemble", disassemble, 1),
            internal("rox_compiler_write", write_file, 4),
            internal("rox_compiler_load", load_file, 2),
            PrimDef {
                name: RUN,
                kind: PrimKind::Primitive,
                args: ArgEval::Unevaluated,
                arity: Arity::Exact(1),
                dispatch: Dispatch::None,
                visibility: Visibility::SetByFn,
                fun: run,
            },
        ],
        prelude: include_str!("compiler.R"),
    }],
};

pub(crate) const BASE: Module = Module {
    prims: &[PrimDef {
        name: "body",
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(1),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun: body,
    }],
    prelude: "",
};

const fn internal(name: &'static str, fun: PrimFn, arity: usize) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

struct State {
    compile_pkgs: RLogical,
    programs: HashMap<SexpId, Rc<CompiledCode>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            compile_pkgs: RLogical::FALSE,
            programs: HashMap::new(),
        }
    }
}

impl CtxExt for State {}

impl Trace for State {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for program in self.programs.values() {
            program.trace(tracer);
        }
    }
}

fn enable_jit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let level = ctx.session().int(args.get(0))?;
    let previous = match level.as_slice() {
        Some([-1]) => ctx.jit_level(),
        Some([level @ 0..=3]) => ctx.set_jit_level(*level as u8),
        _ => return Err(error(ctx, call, "invalid JIT level")),
    };
    Ok(ctx.session_mut().scalar_int(i32::from(previous))?)
}

fn compile_pkgs(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let enable = ctx
        .session()
        .lgl(args.get(0))?
        .iter()
        .next()
        .unwrap_or(RLogical::NA);
    let previous = std::mem::replace(&mut ctx.ext::<State>().compile_pkgs, enable);
    Ok(ctx.session_mut().scalar_lgl(previous)?)
}

fn check_env(ctx: &mut Ctx, call: &Call, env: SexpId) -> EvalResult<()> {
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(error(ctx, call, "argument is not an environment"));
    }
    Ok(())
}

fn compile(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    check_env(ctx, call, args.get(1))?;
    compile_object(ctx, args.get(0))
}

fn compile_object(ctx: &mut Ctx, expr: SexpId) -> EvalResult {
    if source(ctx, expr).is_some() {
        return Ok(expr);
    }
    let program = Rc::new(vm::compile_expr(ctx.session_mut(), expr)?);
    let ns = ctx
        .ns_lookup("compiler")
        .ok_or_else(|| ctx.stop(SEXP_NIL, "compiler namespace is unavailable"))?;
    let sym = ctx.session_mut().intern(RUN)?;
    let runner = ctx.find_fun(sym, ns)?;
    let session = ctx.session_mut();
    let args = session.cons(expr, SEXP_NIL, SEXP_NIL)?;
    let object = session.lcons(runner, args, SEXP_NIL)?;
    cache_program(ctx, object, program);
    Ok(object)
}

pub(crate) fn source(ctx: &Ctx, object: SexpId) -> Option<SexpId> {
    let session = ctx.session();
    if session.type_of(object).ok()? != SexpType::Lang {
        return None;
    }
    let (head, args, _) = session.cons_parts(object).ok()?;
    if ctx.primitive_name(head)? != RUN {
        return None;
    }
    let (expr, tail, tag) = session.cons_parts(args).ok()?;
    (tail == SEXP_NIL && tag == SEXP_NIL).then_some(expr)
}

fn cache_program(ctx: &mut Ctx, object: SexpId, program: Rc<CompiledCode>) {
    let state = ctx.ext::<State>();
    if state.programs.len() >= PROGRAM_CACHE_LIMIT {
        state.programs.clear();
    }
    state.programs.insert(object, program);
}

fn program(ctx: &mut Ctx, object: SexpId, expr: SexpId) -> EvalResult<Rc<CompiledCode>> {
    if let Some(program) = ctx.ext::<State>().programs.get(&object)
        && program.source == expr
    {
        return Ok(Rc::clone(program));
    }
    let program = Rc::new(vm::compile_expr(ctx.session_mut(), expr)?);
    cache_program(ctx, object, Rc::clone(&program));
    Ok(program)
}

fn run(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let expr = source(ctx, call.expr).ok_or_else(|| error(ctx, call, "invalid bytecode object"))?;
    let program = program(ctx, call.expr, expr)?;
    vm::run_compiled(ctx, &program, call.env)
}

fn cmpfun(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let fun = args.get(0);
    match ctx.session().type_of(fun)? {
        SexpType::Clo => {}
        SexpType::Builtin | SexpType::Special => return Ok(fun),
        _ => return Err(error(ctx, call, "cannot compile a non-function")),
    }
    let (formals, body, env) = ctx.session().closure_parts(fun)?;
    let compiled = compile_object(ctx, body)?;
    let session = ctx.session_mut();
    let result = session.alloc_closure(formals, compiled, env)?;
    for (name, value) in session.attrs(fun)? {
        session.set_attrib(result, name, value)?;
    }
    Ok(result)
}

fn body(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut fun = args.get(0);
    let name = match ctx.session().type_of(fun)? {
        SexpType::Str => {
            let [name] = ctx.session().str_slice(fun)? else {
                return Err(error(ctx, call, "invalid character argument"));
            };
            if *name == CharId::NA {
                return Err(error(ctx, call, "invalid character argument"));
            }
            Some(crate::ops::common::text(ctx.session(), *name))
        }
        SexpType::Sym => Some(ctx.session().sym_name_of(fun)?.to_owned()),
        _ => None,
    };
    if let Some(name) = name {
        let symbol = ctx.session_mut().intern(&name)?;
        fun = ctx.find_fun(symbol, call.env)?;
    }
    let Ok((_, body, _)) = ctx.session().closure_parts(fun) else {
        return Ok(SEXP_NIL);
    };
    Ok(source(ctx, body).unwrap_or(body))
}

fn disassemble(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let object = if let Ok((_, body, _)) = ctx.session().closure_parts(value) {
        body
    } else {
        value
    };
    let expr = if let Some(expr) = source(ctx, object) {
        expr
    } else if object != value && vm::is_compiled(ctx, value) {
        object
    } else {
        return Err(error(ctx, call, "function is not compiled"));
    };
    let program = program(ctx, object, expr)?;
    let text = program.disassemble(ctx.session());
    Ok(ctx.session_mut().scalar_str(&text)?)
}

fn write_file(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let parsed = args.get(0);
    let file = args.get(1);
    let ascii = crate::serialize::flag(ctx, call, args.get(2))?;
    let verbose = crate::serialize::flag(ctx, call, args.get(3))?;
    let n = ctx.session().xlength(parsed)?;
    vector_limit(ctx, call, n, size_of::<SexpId>())?;
    let mut exprs = Vec::with_capacity(n);
    for i in 0..n {
        let expr = ctx.session().list_get(parsed, i)?;
        if verbose {
            let lines = ctx.deparse_lines(expr, Default::default())?;
            output(
                ctx,
                call,
                &format!("compiling expression {} ...\n", lines.join("\n")),
            )?;
        }
        vm::compile_expr(ctx.session_mut(), expr)?;
        exprs.push(expr);
    }
    let payload = crate::ops::common::list(ctx.session_mut(), &exprs)?;
    ctx.protect(payload);
    let path = crate::serialize::path(ctx, call, file)?;
    output(
        ctx,
        call,
        &format!("saving to file \"{}\" ... ", path.display()),
    )?;
    crate::serialize::write_compiler_file(ctx, call, &path, payload, ascii)?;
    output(ctx, call, "done\n")?;
    Ok(SEXP_NIL)
}

fn load_file(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(1);
    check_env(ctx, call, env)?;
    let path = crate::serialize::path(ctx, call, args.get(0))?;
    let payload = crate::serialize::read_compiler_file(ctx, call, &path)?;
    ctx.protect(payload);
    if ctx.session().type_of(payload)? != SexpType::Vec {
        return Err(error(ctx, call, "invalid compiled file"));
    }
    for i in 0..ctx.session().xlength(payload)? {
        let expr = ctx.session().list_get(payload, i)?;
        let program = vm::compile_expr(ctx.session_mut(), expr)?;
        vm::run_compiled(ctx, &program, env)?;
    }
    Ok(SEXP_NIL)
}

fn output(ctx: &mut Ctx, call: &Call, text: &str) -> EvalResult<()> {
    ctx.host()
        .write_out(text.as_bytes())
        .map_err(|err| error(ctx, call, err.to_string()))
}
