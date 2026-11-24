use std::borrow::Cow;
use std::io::BufRead;

use roxide_value::{NA_INTEGER, NA_REAL};

use super::{Reader, WireResult, invalid};

impl Reader<'_, '_> {
    pub(super) fn take(&mut self, n: usize) -> WireResult<&[u8]> {
        if n == 0 {
            return Ok(&[]);
        }
        match &mut self.input {
            Input::Slice(bytes) => {
                let (head, rest) = bytes.split_at_checked(n).ok_or_else(truncated)?;
                *bytes = rest;
                self.remaining = bytes.len();
                Ok(head)
            }
            Input::Stream(stream) => {
                if n > self.remaining {
                    return Err(invalid("serialized input exceeds the decompression limit"));
                }
                stream.field(&mut self.remaining, |bytes, used| {
                    let count = bytes.len().min(n - used);
                    (count, used + count == n)
                })
            }
        }
    }

    pub(super) fn line(&mut self) -> WireResult<&str> {
        let line = match &mut self.input {
            Input::Slice(bytes) => {
                let end = bytes
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .ok_or_else(truncated)?;
                let line = &bytes[..end];
                *bytes = &bytes[end + 1..];
                self.remaining = bytes.len();
                line
            }
            Input::Stream(stream) => {
                let bytes = stream.field(&mut self.remaining, |bytes, _| {
                    let end = bytes.iter().position(|&byte| byte == b'\n');
                    (end.map_or(bytes.len(), |i| i + 1), end.is_some())
                })?;
                bytes.strip_suffix(b"\n").ok_or_else(truncated)?
            }
        };
        std::str::from_utf8(line).map_err(invalid)
    }

    pub(super) fn string_bytes(&mut self, len: usize) -> WireResult<Cow<'_, [u8]>> {
        if self.ascii {
            return super::super::ascii::decode_string(self.line()?.as_bytes(), len)
                .map(Cow::Owned);
        }
        self.take(len).map(Cow::Borrowed)
    }

    pub(super) fn word(&mut self) -> WireResult<u32> {
        if self.ascii {
            return match self.line()? {
                "NA" => Ok(NA_INTEGER as u32),
                line => line.parse::<i32>().map(|n| n as u32).map_err(invalid),
            };
        }
        let b = self.take(4)?.try_into().map_err(invalid)?;
        Ok(u32::from_be_bytes(b))
    }

    pub(super) fn length(&mut self) -> WireResult<usize> {
        let n = self.word()?;
        match n {
            u32::MAX => {
                let hi = u64::from(self.word()?);
                let lo = u64::from(self.word()?);
                usize::try_from((hi << 32) | lo).map_err(invalid)
            }
            n if n <= i32::MAX as u32 => Ok(n as usize),
            _ => Err(invalid("invalid serialized length")),
        }
    }

    pub(super) fn vector_len<T>(&mut self, width: usize) -> WireResult<usize> {
        let len = self.length()?;
        self.check_vector::<T>(len, width)?;
        Ok(len)
    }

    pub(super) fn check_vector<T>(&self, len: usize, width: usize) -> WireResult<()> {
        if len > self.limits.max_vector_bytes / size_of::<T>() {
            return Err(invalid("serialized vector exceeds the allocation limit"));
        }
        let width = if self.ascii { 1 } else { width };
        if len > self.remaining / width {
            return Err(match self.input {
                Input::Slice(_) => truncated(),
                Input::Stream(..) => invalid("serialized input exceeds the decompression limit"),
            });
        }
        Ok(())
    }

    pub(super) fn f64(&mut self) -> WireResult<f64> {
        if self.ascii {
            return match self.line()? {
                "NA" => Ok(NA_REAL),
                line => line.parse().map_err(invalid),
            };
        }
        Ok(f64::from_be_bytes(
            self.take(8)?.try_into().map_err(invalid)?,
        ))
    }

    pub(super) fn vector<T>(
        &mut self,
        width: usize,
        mut get: impl FnMut(&mut Self) -> WireResult<T>,
    ) -> WireResult<Vec<T>> {
        let len = self.vector_len::<T>(width)?;
        let mut values = Vec::new();
        self.reserve_vector(&mut values, len)?;
        for _ in 0..len {
            let value = get(self)?;
            values.try_reserve(1).map_err(invalid)?;
            values.push(value);
        }
        Ok(values)
    }

    pub(super) fn reserve_vector<T>(&self, values: &mut Vec<T>, len: usize) -> WireResult<()> {
        if matches!(self.input, Input::Slice(_)) {
            values.try_reserve_exact(len).map_err(invalid)?;
        }
        Ok(())
    }
}

pub(super) enum Input<'a> {
    Slice(&'a [u8]),
    Stream(StreamInput<'a>),
}

pub(super) struct StreamInput<'a> {
    input: &'a mut dyn BufRead,
    pending: usize,
    capture: Option<Vec<u8>>,
    scratch: Vec<u8>,
}

impl<'a> StreamInput<'a> {
    pub(super) fn new(input: &'a mut dyn BufRead) -> Self {
        Self {
            input,
            pending: 0,
            capture: Some(Vec::new()),
            scratch: Vec::new(),
        }
    }

    pub(super) fn disable_replay(&mut self) {
        self.capture = None;
    }

    pub(super) fn replay(mut self) -> WireResult<Vec<u8>> {
        self.consume_pending()?;
        self.capture
            .take()
            .ok_or_else(|| invalid("serialization replay is unavailable"))
    }

    fn field(
        &mut self,
        remaining: &mut usize,
        scan: impl Fn(&[u8], usize) -> (usize, bool),
    ) -> WireResult<&[u8]> {
        let start = self.pending;
        let (count, complete) = scan(&self.input.fill_buf()?[start..], 0);
        if complete {
            use_bytes(remaining, count)?;
            self.pending += count;
            return Ok(&self.input.fill_buf()?[start..self.pending]);
        }

        self.consume_pending()?;
        self.scratch.clear();
        let buffer = self.capture.as_mut().unwrap_or(&mut self.scratch);
        let start = buffer.len();
        loop {
            let available = self.input.fill_buf()?;
            if available.is_empty() {
                return Err(truncated());
            }
            let (count, complete) = scan(available, buffer.len() - start);
            use_bytes(remaining, count)?;
            append(buffer, &available[..count])?;
            self.input.consume(count);
            if complete {
                return Ok(&buffer[start..]);
            }
        }
    }

    fn consume_pending(&mut self) -> WireResult<()> {
        if self.pending != 0 {
            if let Some(capture) = &mut self.capture {
                append(capture, &self.input.fill_buf()?[..self.pending])?;
            }
            self.input.consume(self.pending);
            self.pending = 0;
        }
        Ok(())
    }
}

impl Drop for StreamInput<'_> {
    fn drop(&mut self) {
        self.input.consume(self.pending);
    }
}

fn append(buffer: &mut Vec<u8>, bytes: &[u8]) -> WireResult<()> {
    buffer.try_reserve(bytes.len()).map_err(invalid)?;
    buffer.extend_from_slice(bytes);
    Ok(())
}

fn use_bytes(remaining: &mut usize, count: usize) -> WireResult<()> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| invalid("serialized input exceeds the decompression limit"))?;
    Ok(())
}

fn truncated() -> super::super::CodecError {
    invalid("truncated XDR stream")
}
