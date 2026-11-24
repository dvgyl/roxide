#![forbid(unsafe_code)]

//! Byte inputs for Roxide parser robustness checks.

use roxide_base::serialize::unserialize_bytes;
use roxide_diag::SrcFileId;
use roxide_eval::{Ctx, InternalError};
use roxide_host::{Limits, PureHost, forward_host};
use roxide_pkg::{description::Description, makeconf::Makeconf};
use roxide_syntax::Parser;
use roxide_value::GLOBAL_ENV;

/// Parse R source. Replace invalid UTF-8 sequences before parsing.
pub fn check_r_syntax(bytes: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(bytes);
    Parser::new(&text, SrcFileId(0))
        .parse_all()
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Decode an XDR stream in a fresh session with resource limits.
pub fn check_xdr_deserialize(bytes: &[u8]) -> Result<(), String> {
    let mut ctx = context(PureHost::new()).map_err(|error| error.to_string())?;
    unserialize_bytes(&mut ctx, bytes, None)
        .map(drop)
        .map_err(|error| format!("{error:?}"))
}

/// Parse Makeconf and try to expand all variables.
pub fn check_makeconf(bytes: &[u8]) -> Result<(), String> {
    Makeconf::parse(&String::from_utf8_lossy(bytes)).map(|config| drop(config.dump()))
}

/// Parse DESCRIPTION with both Unix and Windows file orders.
pub fn check_description(bytes: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(bytes);
    let unix = Description::parse(&text, false);
    let windows = Description::parse(&text, true);
    unix.and(windows).map(drop)
}

/// Parse an Rd help file from the in-memory filesystem.
pub fn check_rd_help(bytes: &[u8]) -> Result<(), String> {
    let mut host = PureHost::new();
    host.insert_file("/work/input.Rd", bytes);
    let mut ctx = context(host).map_err(|error| error.to_string())?;
    ctx.register_module(&roxide_base::io_os::MODULE)
        .and_then(|()| ctx.register_package(&roxide_utils::TOOLS_PACKAGE))
        .map_err(|error| error.to_string())?;
    ctx.run_script(
        "<fuzz-rd>",
        r#"tools::parse_Rd("/work/input.Rd", macros = FALSE)"#,
        GLOBAL_ENV,
        false,
    )
    .map_err(|error| format!("{error:?}"))
}

struct BoundedHost {
    inner: PureHost,
}

forward_host!(BoundedHost, inner;
    fn limits(&self) -> &Limits {
        static LIMITS: Limits = Limits {
            max_vector_bytes: 64 * 1024,
            max_expressions: 48,
            max_decompress_bytes: 16 * 1024,
        };
        &LIMITS
    }
);

fn context(host: PureHost) -> Result<Ctx, InternalError> {
    Ctx::new(Box::new(BoundedHost { inner: host }), Default::default())
}
