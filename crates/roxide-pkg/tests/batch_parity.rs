//! Run BATCH scripts with an in-memory filesystem.

use std::cell::RefCell;
use std::io::Read;
use std::path::Path;

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{OpenMode, PureHost};

fn run(args: &[&str], source: &str, workspace: Option<Vec<u8>>) -> (i32, String, Vec<u8>) {
    let mut host = PureHost::new();
    host.insert_file("scripts/input.R", source.as_bytes());
    let inspect_workspace = workspace.is_none();
    if let Some(workspace) = workspace {
        host.insert_file(".RData", workspace);
    }
    let output = RefCell::new(None);
    let saved = RefCell::new(None);
    let status = roxide_pkg::batch::run_with_host(
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        Box::new(host),
        &|mut host| {
            *output.borrow_mut() = Some(
                host.open(Path::new("input.Rout"), OpenMode::read_only())
                    .expect("open transcript"),
            );
            if inspect_workspace {
                host.open(Path::new(".RData"), OpenMode::write_truncate())
                    .expect("create workspace file for inspection");
                // Reopen for reading so the cursor remains at the start.
                *saved.borrow_mut() = Some(
                    host.open(Path::new(".RData"), OpenMode::read_only())
                        .expect("open workspace"),
                );
            }
            let mut ctx = Ctx::new(host, GcPolicy::default())?;
            roxide_base::register_all(&mut ctx)?;
            ctx.session_mut().heap.set_policy(GcPolicy::Torture);
            Ok(ctx)
        },
    );
    let mut transcript = String::new();
    output
        .into_inner()
        .expect("transcript")
        .read_to_string(&mut transcript)
        .expect("read transcript");
    let mut workspace = Vec::new();
    if let Some(mut file) = saved.into_inner() {
        file.read_to_end(&mut workspace).expect("read workspace");
    }
    (status, transcript, workspace)
}

#[test]
fn batch_echoes_script_and_restores_workspace() {
    let (status, transcript, workspace) = run(
        &[
            "--no-restore",
            "--no-timing",
            "scripts/input.R",
            "--args",
            "value",
        ],
        "answer <- 7L\n3L +\n 4L\ncat(commandArgs(TRUE), '\\n')\nmessage('stderr')\n",
        None,
    );
    assert_eq!(status, 0, "{transcript}");
    assert!(transcript.starts_with("Roxide "));
    assert!(transcript.contains("> 3L +\n+  4L\n[1] 7\n"));
    let stdout = transcript
        .lines()
        .position(|line| line.trim_end() == "value");
    let stderr = transcript.lines().position(|line| line == "stderr");
    assert!(
        matches!((stdout, stderr), (Some(out), Some(err)) if out < err),
        "{transcript}"
    );
    assert!(!transcript.contains("proc.time()"));
    assert!(!workspace.is_empty());

    let (status, transcript, _) = run(
        &["-q", "--no-save", "scripts/input.R"],
        "print(answer)\n",
        Some(workspace),
    );
    assert_eq!(status, 0, "{transcript}");
    assert!(transcript.starts_with("> print(answer)\n[1] 7\n"));
    assert!(transcript.contains("> proc.time()\n"));
}

#[test]
fn batch_failure_writes_error_and_halts() {
    let (status, transcript, workspace) = run(
        &["--vanilla", "--slave", "scripts/input.R"],
        "cat('out\\n')\nstop('broken')\ncat('unreachable\\n')\n",
        None,
    );
    assert_eq!(status, 1);
    assert!(transcript.starts_with("out\nError"));
    assert!(transcript.contains("broken"));
    assert!(transcript.ends_with("Execution halted\n"));
    assert!(!transcript.contains("unreachable"));
    assert!(workspace.is_empty());
}
