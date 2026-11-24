use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::SEXP_NIL;

use super::ir::{CompiledCode, Reg};
use crate::{Ctx, CtxExt, FrameId};

#[derive(Default)]
pub(super) struct VmStack(pub Vec<VmFrame>);

impl CtxExt for VmStack {}

impl Trace for VmStack {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for frame in &self.0 {
            frame.trace(tracer);
        }
    }
}

pub(super) struct VmFrame {
    pub registers: Vec<SexpId>,
    pub cursors: Vec<ForCursor>,
    pub guards: Vec<GuardFrame>,
    pub loops: Vec<LoopFrame>,
    pub temp: SexpId,
    roots: Vec<SexpId>,
}

impl VmFrame {
    pub fn new(code: &CompiledCode, env: SexpId) -> Self {
        Self {
            registers: vec![SEXP_NIL; usize::from(code.num_regs)],
            cursors: vec![ForCursor::Empty; usize::from(code.num_regs)],
            guards: Vec::new(),
            loops: Vec::new(),
            temp: SEXP_NIL,
            roots: [code.source, env]
                .into_iter()
                .chain(code.consts.iter().chain(&code.syms).copied())
                .collect(),
        }
    }
}

impl Trace for VmFrame {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edges(&self.roots);
        tracer.edges(&self.registers);
        tracer.edge(self.temp);
        for cursor in &self.cursors {
            match *cursor {
                ForCursor::Pairlist(value) | ForCursor::Vector { value, .. } => tracer.edge(value),
                _ => {}
            }
        }
        for guard in &self.guards {
            tracer.edge(guard.fun);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ForCursor {
    Empty,
    Int {
        next: i128,
        step: i8,
        remaining: usize,
    },
    Real {
        start: f64,
        step: f64,
        index: usize,
        len: usize,
    },
    Pairlist(SexpId),
    Vector {
        value: SexpId,
        index: usize,
        len: usize,
    },
}

pub(super) struct LoopFrame {
    pub id: FrameId,
    pub break_target: usize,
    pub next_target: usize,
    pub cursor: Option<Reg>,
}

pub(super) struct GuardFrame {
    pub counted: bool,
    pub start: usize,
    pub end: usize,
    pub cache_idx: u16,
    pub fun: SexpId,
    pub lhs: Option<Reg>,
    pub retained: Option<SexpId>,
}

pub(super) fn active(ctx: &mut Ctx) -> bool {
    !ctx.ext::<VmStack>().0.is_empty()
}
