//! Test Makeconf parsing and command output.

use std::path::Path;

use roxide_host::{Host, PureHost};
use roxide_pkg::makeconf::{Makeconf, run_config_cli};

const CONFIG: &str = r#"
# The stage script writes this file.
LIBR = -L$(R_HOME)/lib -lR # Native linker flags.
R_HOME = /rhome
R_INCLUDE_DIR := ${R_HOME}/include
R_HOME = /staged
FLAGS = -Wall \
    -O2
HASH = a\#b
FROM_ENV = $(UNDEFINED)
A = $(B)
B = ${A}
"#;

#[test]
fn expands_make_assignments_and_bounds_cycles() -> Result<(), String> {
    let config = Makeconf::parse(CONFIG)?;
    assert_eq!(
        config.get("R_INCLUDE_DIR").as_deref(),
        Some("/rhome/include")
    );
    assert_eq!(config.get("LIBR").as_deref(), Some("-L/staged/lib -lR"));
    assert_eq!(config.get("FLAGS").as_deref(), Some("-Wall -O2"));
    assert_eq!(config.get("HASH").as_deref(), Some("a#b"));
    assert_eq!(config.get("FROM_ENV").as_deref(), Some(""));
    assert_eq!(config.get("A"), None);
    assert_eq!(config.get("B"), None);
    assert_eq!(config.get("UNDEFINED"), None);
    assert!(Makeconf::parse("LAST = unfinished \\").is_err());
    Ok(())
}

#[test]
fn queries_config_through_the_host() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = PureHost::new();
    host.set_env_var("R_HOME", Some(b"/rhome"))?;
    host.insert_file(Path::new("/rhome/etc/Makeconf"), CONFIG.as_bytes());
    for (option, expected) in [
        ("--all", Makeconf::parse(CONFIG)?.dump()),
        ("--cppflags", "-I/rhome/include\n".into()),
        ("--ldflags", "-L/staged/lib -lR\n".into()),
        ("R_HOME", "/staged\n".into()),
    ] {
        let before = host.stdout_str().len();
        assert_eq!(run_config_cli(&[option.into()], &mut host), 0);
        assert_eq!(host.stdout_str().get(before..), Some(expected.as_str()));
    }
    assert!(host.stdout_str().starts_with("A = $(B)\nB = ${A}\nFLAGS"));
    assert_eq!(run_config_cli(&["MISSING".into()], &mut host), 1);
    assert_eq!(
        host.stderr_str(),
        "ERROR: no information for variable 'MISSING'\n"
    );
    Ok(())
}
