use roxide_gc::SexpId;
use roxide_host::{Host, HostError, HostErrorKind, HostResult, Limits, StdStream};
use roxide_value::SEXP_NIL;

use crate::api::{Call, EvalResult};

use super::{Args, Ctx};

type StreamSink = Box<dyn FnMut(&mut Ctx, &[u8]) -> EvalResult<()>>;

struct StreamCapture {
    id: u64,
    capture_out: bool,
    capture_err: bool,
    split: bool,
    persistent: bool,
    exceeded: bool,
    received: usize,
    bytes: Vec<u8>,
    sink: Option<StreamSink>,
    flushing: bool,
}
/// Host access with a stack of output captures and stream sinks.
pub(crate) struct HostStack {
    base: Box<dyn Host>,
    captures: Vec<StreamCapture>,
    next_capture_id: u64,
}

impl HostStack {
    pub(super) fn new(base: Box<dyn Host>) -> Self {
        Self {
            base,
            captures: Vec::new(),
            next_capture_id: 1,
        }
    }

    /// Write bytes to standard output.
    pub(crate) fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write(StdStream::Stdout, bytes)
    }

    /// Write bytes to standard error.
    pub(crate) fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write(StdStream::Stderr, bytes)
    }

    /// Return the host resource limits.
    pub(crate) fn limits(&self) -> &Limits {
        self.base.limits()
    }

    /// Check whether the host received an interrupt.
    pub(crate) fn poll_interrupt(&mut self) -> bool {
        self.base.poll_interrupt()
    }

    fn write(&mut self, stream: StdStream, bytes: &[u8]) -> HostResult<()> {
        let limit = self.base.limits().max_vector_bytes;
        for capture in self.captures.iter_mut().rev() {
            let selected = match stream {
                StdStream::Stdout => capture.capture_out,
                StdStream::Stderr => capture.capture_err,
                StdStream::Stdin => false,
            };
            if !selected || capture.flushing {
                continue;
            }
            if capture.exceeded || bytes.len() > limit.saturating_sub(capture.received) {
                capture.exceeded = true;
                return Err(HostError::new(
                    HostErrorKind::Other,
                    "output capture exceeds the vector size limit",
                ));
            }
            capture.received += bytes.len();
            capture.bytes.extend_from_slice(bytes);
            if !capture.split {
                return Ok(());
            }
        }
        match stream {
            StdStream::Stdout => self.base.write_out(bytes),
            StdStream::Stderr => self.base.write_err(bytes),
            StdStream::Stdin => Ok(()),
        }
    }
}

roxide_host::forward_host!(HostStack, base;
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write_out(bytes)
    }
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write_err(bytes)
    }
);

impl Ctx {
    /// Mutably borrow the underlying [`Host`].
    #[inline]
    #[must_use]
    pub fn host(&mut self) -> &mut dyn Host {
        &mut self.host
    }

    /// Capture the selected host streams while running `f`.
    ///
    /// With `split`, copy each write to the previous stream destination as well.
    /// The capture ends when `f` returns, including when it returns an R error.
    pub fn capture_streams<R>(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        f: impl FnOnce(&mut Self) -> EvalResult<R>,
    ) -> EvalResult<(R, Vec<u8>)> {
        self.with_stream_capture(capture_out, capture_err, split, None, f)
    }

    /// Send selected host output to `sink` before the next R primitive runs.
    ///
    /// Each capture limits the total number of bytes that it accepts.
    pub fn capture_streams_to(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: impl FnMut(&mut Self, &[u8]) -> EvalResult<()> + 'static,
        f: impl FnOnce(&mut Self) -> EvalResult<SexpId>,
    ) -> EvalResult {
        self.with_stream_capture(
            capture_out,
            capture_err,
            split,
            Some(Box::new(sink)),
            |ctx| {
                ctx.scoped(|ctx| {
                    let result = f(ctx);
                    ctx.protect_eval_result(&result);
                    let written = ctx.flush_stream_captures();
                    result.and_then(|value| written.map(|()| value))
                })
            },
        )
        .map(|(value, _)| value)
    }

    /// Push a persistent stream diversion (used by `base::sink()`).
    pub fn push_stream_sink(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: impl FnMut(&mut Self, &[u8]) -> EvalResult<()> + 'static,
    ) {
        let id = self.host.next_capture_id;
        self.host.next_capture_id += 1;
        self.host.captures.push(StreamCapture {
            id,
            capture_out,
            capture_err,
            split,
            persistent: true,
            exceeded: false,
            received: 0,
            bytes: Vec::new(),
            sink: Some(Box::new(sink)),
            flushing: false,
        });
    }

    /// Flush buffered output and pop the topmost persistent diversion for stdout (`message = false`)
    /// or stderr (`message = true`). Returns `true` if a matching sink was popped.
    pub fn pop_stream_sink(&mut self, message: bool) -> EvalResult<bool> {
        let flushed = self.flush_stream_captures();
        let pos = self.host.captures.iter().rposition(|capture| {
            capture.persistent
                && if message {
                    capture.capture_err
                } else {
                    capture.capture_out
                }
        });
        let removed = pos.map(|index| self.host.captures.remove(index)).is_some();
        flushed?;
        Ok(removed)
    }

    /// Flush all buffered bytes queued on active stream sinks.
    pub fn flush_stream_sinks(&mut self) -> EvalResult<()> {
        self.flush_stream_captures()
    }

    fn with_stream_capture<R>(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: Option<StreamSink>,
        f: impl FnOnce(&mut Self) -> EvalResult<R>,
    ) -> EvalResult<(R, Vec<u8>)> {
        let id = self.host.next_capture_id;
        self.host.next_capture_id += 1;
        self.host.captures.push(StreamCapture {
            id,
            capture_out,
            capture_err,
            split,
            persistent: false,
            exceeded: false,
            received: 0,
            bytes: Vec::new(),
            sink,
            flushing: false,
        });
        let result = f(self);
        let Some(pos) = self.host.captures.iter().rposition(|c| c.id == id) else {
            return Err(self.stop(SEXP_NIL, "missing output capture"));
        };
        let capture = self.host.captures.remove(pos);
        if capture.exceeded {
            return Err(self.stop(SEXP_NIL, "output capture exceeds the vector size limit"));
        }
        Ok((result?, capture.bytes))
    }

    /// Return whether a capture has an output sink.
    pub(crate) fn has_stream_sink(&self) -> bool {
        self.host
            .captures
            .iter()
            .any(|capture| capture.sink.is_some())
    }

    pub(super) fn flush_stream_captures(&mut self) -> EvalResult<()> {
        if self.host.captures.iter().any(|capture| capture.flushing) {
            return Ok(());
        }
        let ids: Vec<u64> = self
            .host
            .captures
            .iter()
            .rev()
            .map(|capture| capture.id)
            .collect();
        for id in ids {
            let Some(capture) = self.host.captures.iter_mut().find(|c| c.id == id) else {
                continue;
            };
            if capture.bytes.is_empty() {
                continue;
            }
            let Some(mut sink) = capture.sink.take() else {
                continue;
            };
            let bytes = std::mem::take(&mut capture.bytes);
            capture.flushing = true;
            let visible = self.visible;
            let result = sink(self, &bytes);
            self.visible = visible;
            if let Some(capture) = self.host.captures.iter_mut().find(|c| c.id == id) {
                capture.flushing = false;
                capture.sink = Some(sink);
            }
            result?;
        }
        Ok(())
    }

    /// Invoke a primitive and flush stream captures around it.
    pub(crate) fn invoke_streaming_primitive(
        &mut self,
        fun: crate::PrimFn,
        call: &Call,
        args: &mut Args,
    ) -> EvalResult {
        self.flush_stream_captures()?;
        let result = fun(self, call, args);
        self.scoped(|ctx| {
            ctx.protect_eval_result(&result);
            let written = ctx.flush_stream_captures();
            result.and_then(|value| written.map(|()| value))
        })
    }

    /// Check for pending user interrupts (`R_CheckUserInterrupt`).
    pub fn check_user_interrupt(&mut self) -> EvalResult<()> {
        if self.host.poll_interrupt() {
            return Err(self.stop(SEXP_NIL, "interrupted"));
        }
        Ok(())
    }
}
