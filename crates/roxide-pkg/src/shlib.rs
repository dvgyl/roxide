//! Build shared libraries with the make rules in a staged R_HOME.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use roxide_host::{CommandSpec, Host};

use crate::makeconf::{Makeconf, absolute, env_text, is_file, r_home, valid_key};

const HELP: &str = "Usage: R CMD SHLIB [-o FILE] [-n] [--preclean] [--clean] [-d]\n\
                   \x20                  [--use-LTO | --no-use-LTO] [KEY=VALUE] FILE...\n";

/// Options for one shared library build.
#[derive(Debug, Default)]
pub struct ShlibOptions {
    /// Use this R_HOME instead of the host environment or executable location.
    pub r_home: Option<PathBuf>,
    output: Option<PathBuf>,
    dry_run: bool,
    preclean: bool,
    clean: bool,
    debug: bool,
    lto: Option<bool>,
    overrides: Vec<String>,
    inputs: Vec<PathBuf>,
    link_flags: Vec<String>,
}

impl ShlibOptions {
    /// Parse the arguments after `R CMD SHLIB`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter().map(String::as_str);
        let mut positional = false;
        while let Some(arg) = args.next() {
            if positional {
                options.inputs.push(arg.into());
                continue;
            }
            match arg {
                "--" => positional = true,
                "-o" => options.output = Some(output_path(args.next().unwrap_or_default())?),
                "-n" | "--dry-run" => options.dry_run = true,
                "--preclean" => options.preclean = true,
                "--clean" => options.clean = true,
                "-d" | "--debug" => options.debug = true,
                "-c" | "--use-LTO" => options.lto = Some(true),
                "--no-use-LTO" => options.lto = Some(false),
                "-framework" | "-l" | "-L" => {
                    let value = args
                        .next()
                        .filter(|value| !value.is_empty() && !value.starts_with('-'))
                        .ok_or_else(|| format!("missing argument for '{arg}'"))?;
                    options
                        .link_flags
                        .extend([arg.to_owned(), value.to_owned()]);
                }
                _ if arg.starts_with("--output=") => {
                    options.output = Some(output_path(&arg["--output=".len()..])?);
                }
                _ if ["-l", "-L", "-Wl,"]
                    .iter()
                    .any(|flag| arg.starts_with(flag)) =>
                {
                    options.link_flags.push(arg.to_owned());
                }
                _ if arg.starts_with('-') => return Err(format!("unknown SHLIB option '{arg}'")),
                _ if let Some((key, _)) = arg.split_once('=') => {
                    if !valid_key(key) || key.starts_with("ROXIDE_") || key == "SHLIB" {
                        return Err(format!("invalid SHLIB variable '{key}'"));
                    }
                    options.overrides.push(arg.to_owned());
                }
                _ => options.inputs.push(arg.into()),
            }
        }
        if options.inputs.is_empty() {
            return Err("no source or object files specified".into());
        }
        Ok(options)
    }

    /// Build the shared library in the host's current directory.
    pub fn run(&self, host: &mut dyn Host) -> Result<(), String> {
        let cwd = host.cwd().map_err(|error| error.to_string())?;
        let home = match &self.r_home {
            Some(path) => absolute(host, path)?,
            None => r_home(host)?,
        };
        let conf = Makeconf::read(host, &home)?;
        self.build(host, &cwd, &home, &conf, None, "")
    }

    pub(crate) fn for_package(inputs: Vec<PathBuf>, output: PathBuf) -> Self {
        Self {
            inputs,
            output: Some(output),
            preclean: true,
            ..Self::default()
        }
    }

    pub(crate) fn build(
        &self,
        host: &mut dyn Host,
        cwd: &Path,
        home: &Path,
        conf: &Makeconf,
        cxx_standard: Option<&str>,
        linking_flags: &str,
    ) -> Result<(), String> {
        let rules = home.join("share/make/shlib.mk");
        if !is_file(host, &rules) {
            return Err(format!("cannot find {}", rules.display()));
        }
        let inputs = BuildInputs::new(host, cwd, &self.inputs)?;
        let output = match &self.output {
            Some(path) => path.clone(),
            None => {
                let stem = self
                    .inputs
                    .first()
                    .and_then(|path| path.file_stem())
                    .and_then(|name| name.to_str())
                    .ok_or("missing input file name")?;
                PathBuf::from(format!("{stem}{}", shared_extension(conf)?))
            }
        };
        let target = make_path(&output)?;
        let output_file = cwd.join(&output);
        let existing_output = host.canonicalize(&output_file).ok();
        if inputs.objects.contains(&target)
            || existing_output.is_some_and(|path| inputs.sources.contains(&path))
        {
            return Err("SHLIB output must differ from the input files".into());
        }
        let mut args = make_args(cwd)?;
        args.extend([
            "-r".into(),
            "-f".into(),
            path_text(&home.join("etc/Makeconf"))?,
            "-f".into(),
            path_text(&rules)?,
            format!("SHLIB={target}"),
            format!("ROXIDE_DEFAULT_OBJECTS={}", inputs.objects.join(" ")),
            format!("ROXIDE_SOURCE_RULES={}", inputs.rules.join("\n")),
            format!("ROXIDE_CLEAN_OBJECTS={}", inputs.generated.join(" ")),
            format!("ROXIDE_HAS_CXX={}", if inputs.cxx { "yes" } else { "no" }),
            format!(
                "ROXIDE_DEFAULT_CXX_STD={}",
                cxx_standard.unwrap_or_default()
            ),
        ]);
        if let Some(vars) = package_makevars(host, cwd, &self.inputs) {
            args.push(format!("ROXIDE_PACKAGE_MAKEVARS={}", make_path(&vars)?));
        } else if self.inputs.is_empty() {
            return Err("no source or object files specified".into());
        }
        if let Some(vars) = user_makevars(host)? {
            args.push(format!("ROXIDE_USER_MAKEVARS={}", make_path(&vars)?));
        }
        if let Some(lto) = self.lto {
            args.push(format!("ROXIDE_USE_LTO={}", if lto { "yes" } else { "no" }));
        }
        if self.debug {
            args.push("--debug".into());
        }
        let flags = self
            .link_flags
            .iter()
            .map(|flag| make_shell_word(flag))
            .collect::<Vec<_>>();
        args.push(format!("ROXIDE_LINK_FLAGS={}", flags.join(" ")));
        args.extend(self.overrides.iter().cloned());
        if !linking_flags.is_empty() {
            args.push(format!("CLINK_CPPFLAGS={linking_flags}"));
        }
        if !self.dry_run
            && let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty())
        {
            host.create_dir(&cwd.join(parent), true, 0o755)
                .map_err(|error| error.to_string())?;
        }
        if self.dry_run {
            args.push("-n".into());
        }

        let mut targets = Vec::new();
        if self.preclean {
            targets.push("preclean");
            args.push("-B".into());
        }
        targets.push("all");
        if self.clean {
            targets.push("clean");
        }
        let make = make_program(host)?;
        if self.dry_run {
            let command = std::iter::once(make.as_str())
                .chain(args.iter().map(String::as_str))
                .chain(targets.iter().copied())
                .map(shell_word)
                .collect::<Vec<_>>()
                .join(" ");
            host.write_out(format!("make cmd is\n  {command}\n\nmake would use\n").as_bytes())
                .map_err(|error| error.to_string())?;
        }
        for target in targets {
            let mut args = args.clone();
            args.push(target.into());
            if let Err(error) = run_process(host, &make, args) {
                return Err(format!("SHLIB {target}: {error}"));
            }
        }
        if !self.dry_run && !is_file(host, &output_file) {
            return Err(format!("build did not produce '{}'", output.display()));
        }
        Ok(())
    }
}

#[derive(Default)]
struct BuildInputs {
    sources: HashSet<PathBuf>,
    objects: Vec<String>,
    generated: Vec<String>,
    rules: Vec<String>,
    cxx: bool,
}

impl BuildInputs {
    fn new(host: &dyn Host, cwd: &Path, paths: &[PathBuf]) -> Result<Self, String> {
        let mut inputs = Self::default();
        for path in paths {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default();
            if extension != "o" && !is_source(path) {
                return Err(format!("unsupported SHLIB input '{}'", path.display()));
            }
            let file = cwd.join(path);
            let canonical = host
                .canonicalize(&file)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if !is_file(host, &canonical) {
                return Err(format!("input '{}' is not a file", path.display()));
            }
            inputs.sources.insert(canonical);
            inputs.cxx |= matches!(extension, "cc" | "cpp" | "cxx" | "mm");
            let object = make_path(&path.with_extension("o"))?;
            if inputs.objects.contains(&object) {
                return Err(format!("duplicate SHLIB object '{object}'"));
            }
            if extension != "o" {
                let source = make_path(path)?;
                inputs.rules.push(format!(
                    "$(call roxide_rule,{},{},{extension})",
                    object.replace(',', "$(roxide_comma)"),
                    source.replace(',', "$(roxide_comma)"),
                ));
                inputs.generated.push(object.clone());
            }
            inputs.objects.push(object);
        }
        Ok(inputs)
    }
}

pub(crate) fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("c" | "cc" | "cpp" | "cxx" | "f" | "f90" | "f95" | "m" | "mm")
    )
}

pub(crate) fn shared_extension(conf: &Makeconf) -> Result<String, String> {
    let value = conf.get("SHLIB_EXT").unwrap_or_else(|| ".so".into());
    if value.strip_prefix('.').is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
    }) {
        Ok(value)
    } else {
        Err("invalid SHLIB_EXT in Makeconf".into())
    }
}

fn output_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.file_name().is_none() {
        return Err("missing SHLIB output file name".into());
    }
    Ok(path)
}

fn make_path(path: &Path) -> Result<String, String> {
    let mut path = path_text(path)?;
    if path.chars().any(|ch| {
        ch.is_control()
            || matches!(
                ch,
                '$' | '#'
                    | ':'
                    | '%'
                    | '='
                    | '\\'
                    | '"'
                    | '\''
                    | '`'
                    | ';'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '*'
                    | '?'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '!'
                    | '~'
                    | '{'
                    | '}'
            )
    }) {
        return Err(format!("unsupported path for make: '{path}'"));
    }
    if path.starts_with('-') {
        path.insert_str(0, "./");
    }
    Ok(path.replace(' ', "\\ "))
}

fn package_makevars(host: &dyn Host, cwd: &Path, paths: &[PathBuf]) -> Option<PathBuf> {
    let windows = host.sys_info().sysname.eq_ignore_ascii_case("windows");
    let names = [windows.then_some("Makevars.ucrt"), Some("Makevars")];
    std::iter::once(cwd.to_path_buf())
        .chain(
            paths
                .iter()
                .filter_map(|path| path.parent().map(|parent| cwd.join(parent))),
        )
        .flat_map(|dir| names.into_iter().flatten().map(move |name| dir.join(name)))
        .find(|path| is_file(host, path))
}

fn user_makevars(host: &dyn Host) -> Result<Option<PathBuf>, String> {
    if let Some(value) = env_text(host, "R_MAKEVARS_USER")? {
        return if value.is_empty() {
            Ok(None)
        } else {
            absolute(host, Path::new(&value)).map(|path| Some(path).filter(|p| is_file(host, p)))
        };
    }
    Ok(host
        .home_dir()
        .map(|dir| dir.join(".R/Makevars"))
        .filter(|path| is_file(host, path)))
}

pub(crate) fn path_text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid UTF-8 path: {}", path.display()))
}

pub(crate) fn make_args(cwd: &Path) -> Result<Vec<String>, String> {
    Ok(vec![
        "--no-print-directory".into(),
        "-C".into(),
        path_text(cwd)?,
    ])
}

pub(crate) fn make_program(host: &dyn Host) -> Result<String, String> {
    Ok(env_text(host, "MAKE")?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "make".into()))
}

pub(crate) fn shell_word(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./-=,+".contains(&b))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(crate) fn make_shell_word(value: &str) -> String {
    shell_word(value).replace('$', "$$")
}

pub(crate) fn run_process(
    host: &mut dyn Host,
    program: &str,
    args: Vec<String>,
) -> Result<(), String> {
    let spec = CommandSpec {
        program: program.into(),
        args,
        stdin: None,
        timeout: Some(Duration::from_secs(600)),
    };
    let output = host
        .spawn(&spec)
        .map_err(|error| format!("{program}: {error}"))?;
    host.write_out(&output.stdout)
        .map_err(|error| error.to_string())?;
    host.write_err(&output.stderr)
        .map_err(|error| error.to_string())?;
    if output.status == 0 {
        Ok(())
    } else {
        Err(format!("{program} exited with status {}", output.status))
    }
}

/// Run `R CMD SHLIB` and return its exit status.
pub fn run_cli(args: &[String], host: &mut dyn Host) -> i32 {
    if matches!(args, [arg] if matches!(arg.as_str(), "-h" | "--help")) {
        return i32::from(host.write_out(HELP.as_bytes()).is_err());
    }
    match ShlibOptions::parse(args).and_then(|options| options.run(host)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = host.write_err(format!("ERROR: {error}\n").as_bytes());
            1
        }
    }
}
