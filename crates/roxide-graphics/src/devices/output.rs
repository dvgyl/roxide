use std::{
    io::Write,
    path::{Path, PathBuf},
};

use roxide_host::{Host, HostError, HostErrorKind, HostResult, OpenMode};

pub(super) type Point = (f64, f64);

pub(super) fn valid_point((x, y): Point) -> bool {
    x.is_finite() && y.is_finite()
}

pub(super) fn line_width(gc: &crate::engine::GContext) -> Option<f64> {
    (!gc.col.is_transparent() && gc.lwd.is_finite() && gc.lwd > 0.0).then_some(gc.lwd * 0.75)
}

pub(super) fn dashes(gc: &crate::engine::GContext, width: f64) -> impl Iterator<Item = f64> + '_ {
    gc.lty
        .iter()
        .map(move |d| d * width)
        .filter(|d| d.is_finite() && *d > 0.0)
}

pub(super) fn invalid(message: &str) -> HostError {
    HostError::new(HostErrorKind::InvalidInput, message)
}

pub(super) fn dimensions(width: f64, height: f64, pointsize: f64) -> Result<(), String> {
    if ![width * 72.0, height * 72.0, pointsize]
        .into_iter()
        .all(|n| n.is_finite() && n > 0.0)
    {
        return Err("invalid device dimensions or pointsize".into());
    }
    Ok(())
}

pub(super) fn path(host: &dyn Host, name: &str) -> PathBuf {
    if (name == "~" || name.starts_with("~/"))
        && let Some(home) = host.home_dir()
    {
        return match name.strip_prefix("~/") {
            Some(suffix) => home.join(suffix),
            None => home,
        };
    }
    PathBuf::from(name)
}

pub(super) fn write(host: &mut dyn Host, name: &str, bytes: &[u8]) -> HostResult<()> {
    let path = path(host, name);
    write_path(host, &path, bytes)
}

pub(super) fn write_path(host: &mut dyn Host, path: &Path, bytes: &[u8]) -> HostResult<()> {
    let mut file = host.open(path, OpenMode::write_truncate())?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(HostError::from_io)
}

pub(super) struct PageName {
    prefix: String,
    field: Option<(usize, bool, String)>,
}

impl PageName {
    pub(super) fn new(path: &str, numbered: bool) -> Result<Self, String> {
        if path.is_empty() || path.contains('\0') {
            return Err("invalid graphics filename".into());
        }
        let mut name = Self {
            prefix: path.into(),
            field: None,
        };
        if !numbered {
            return Ok(name);
        }
        name.prefix.clear();
        let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            let part = name
                .field
                .as_mut()
                .map_or(&mut name.prefix, |(_, _, suffix)| suffix);
            if c != '%' || chars.next_if_eq(&'%').is_some() {
                part.push(c);
                continue;
            }
            let mut width = 0;
            let zero_pad = chars.next_if_eq(&'0').is_some();
            while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                width = width * 10 + (digit as usize - '0' as usize);
                if width > 32 {
                    return Err("graphics filename has an invalid page number width".into());
                }
            }
            if chars.next_if_eq(&'d').is_none() || name.field.is_some() {
                return Err("invalid page number format in graphics filename".into());
            }
            name.field = Some((width, zero_pad, String::new()));
        }
        Ok(name)
    }

    pub(super) fn at(&self, page: usize) -> String {
        match &self.field {
            None => self.prefix.clone(),
            Some((width, true, suffix)) => format!("{}{page:0width$}{suffix}", self.prefix),
            Some((width, false, suffix)) => format!("{}{page:width$}{suffix}", self.prefix),
        }
    }
}

pub(super) fn number(n: f64) -> String {
    // Fixed notation is valid in both PDF and SVG.
    let mut s = format!("{n:.6}");
    s.truncate(s.trim_end_matches('0').trim_end_matches('.').len());
    if n != 0.0 && matches!(s.as_str(), "0" | "-0") {
        n.to_string()
    } else if s == "-0" {
        "0".into()
    } else {
        s
    }
}
