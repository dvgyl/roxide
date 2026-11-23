//! Differential checks for the S4 dispatch acceptance cases.

#[path = "../src/dispatch/rank.rs"]
mod rank;

use std::path::PathBuf;
use std::process::Command;

use rank::{Ranked, distances};
use roxide_gc::GcPolicy;
use roxide_methods::registry::ClassInfo;
use roxide_value::SEXP_NIL;

struct Probe {
    target: &'static [&'static str],
    methods: &'static [&'static [&'static str]],
    chosen: usize,
    note: bool,
}

const PROBES: &[Probe] = &[
    Probe {
        target: &["D", "D"],
        methods: &[&["R", "ANY"], &["L", "ANY"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["E", "E"],
        methods: &[&["R", "ANY"], &["L", "ANY"]],
        chosen: 1,
        note: false,
    },
    Probe {
        target: &["D", "D"],
        methods: &[&["D", "Root"], &["L", "D"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["D", "D"],
        methods: &[&["L", "D"], &["D", "L"]],
        chosen: 2,
        note: true,
    },
    Probe {
        target: &["D", "D"],
        methods: &[&["D", "L"], &["L", "D"]],
        chosen: 1,
        note: true,
    },
    Probe {
        target: &["D", "D"],
        methods: &[&["L", "R"], &["R", "L"]],
        chosen: 1,
        note: true,
    },
    Probe {
        target: &["E", "E"],
        methods: &[&["L", "R"], &["R", "L"]],
        chosen: 2,
        note: true,
    },
    Probe {
        target: &["D", "D"],
        methods: &[&["D", "ANY"], &["L", "D"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["D", "missing"],
        methods: &[&["D", "ANY"], &["ANY", "missing"]],
        chosen: 1,
        note: false,
    },
    Probe {
        target: &["E", "missing"],
        methods: &[&["D", "ANY"], &["ANY", "missing"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["N6", "N6"],
        methods: &[&["N6", "N4"], &["N5", "N5"]],
        chosen: 1,
        note: true,
    },
    Probe {
        target: &["N6", "N6"],
        methods: &[&["N6", "N3"], &["N5", "N5"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["N6", "N6"],
        methods: &[&["N6", "N1"], &["N1", "N6"], &["N4", "N4"]],
        chosen: 3,
        note: false,
    },
    Probe {
        target: &["N6", "N6", "N6"],
        methods: &[&["N6", "N6", "N2"], &["N6", "N4", "N4"]],
        chosen: 1,
        note: true,
    },
    Probe {
        target: &["N6", "N6", "N6"],
        methods: &[&["N2", "N6", "N6"], &["N4", "N4", "N6"]],
        chosen: 2,
        note: true,
    },
    Probe {
        target: &["Wide", "Wide"],
        methods: &[&["Wide", "ANY"], &["W1", "W1"]],
        chosen: 1,
        note: true,
    },
    Probe {
        target: &["N6", "N6"],
        methods: &[&["ANY", "N6"], &["N3", "N3"]],
        chosen: 2,
        note: false,
    },
    Probe {
        target: &["N6", "N6"],
        methods: &[&["ANY", "N6"], &["N2", "N2"]],
        chosen: 1,
        note: false,
    },
];

fn class_info(class: &str) -> ClassInfo {
    let parents = match class {
        "L" | "R" => vec![("Root".into(), 1, 0)],
        "D" => vec![
            ("L".into(), 1, 0),
            ("R".into(), 1, 1),
            ("Root".into(), 2, 0),
        ],
        "E" => vec![
            ("R".into(), 1, 0),
            ("L".into(), 1, 1),
            ("Root".into(), 2, 0),
        ],
        "Wide" => (1..=6).map(|i| (format!("W{i}"), 1, i - 1)).collect(),
        _ => class
            .strip_prefix('N')
            .and_then(|n| n.parse::<usize>().ok())
            .map_or_else(Vec::new, |depth| {
                (0..depth)
                    .rev()
                    .map(|i| (format!("N{i}"), depth - i, 0))
                    .collect()
            }),
    };
    ClassInfo {
        name: class.to_owned(),
        package: ".GlobalEnv".into(),
        slots: Vec::new(),
        superclasses: parents,
        is_virtual: false,
        class_def: SEXP_NIL,
    }
}

fn r_vector(names: &[&str]) -> String {
    let quoted = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>();
    format!("c({})", quoted.join(","))
}

#[test]
fn all_resolver_probes() -> Result<(), Box<dyn std::error::Error>> {
    let mut expected = String::new();
    let mut script = String::from(
        r#"
setTimeLimit(elapsed = 15, transient = FALSE)
invisible(setClass("Root", slots = c(value = "numeric")))
invisible(setClass("L", contains = "Root"))
invisible(setClass("R", contains = "Root"))
invisible(setClass("D", contains = c("L", "R")))
invisible(setClass("E", contains = c("R", "L")))
invisible(setClass("N0", slots = c(value = "numeric")))
for (i in 1:6) {
    invisible(setClass(paste0("N", i), contains = paste0("N", i - 1)))
    invisible(setClass(paste0("W", i), slots = setNames("numeric", paste0("s", i))))
}
invisible(setClass("Wide", contains = paste0("W", 1:6)))
probe <- function(i, target, signatures) {
    name <- paste0("rxResolver", i)
    if (length(target) == 2L) {
        def <- function(x, y) standardGeneric(name)
    } else {
        def <- function(x, y, z) standardGeneric(name)
    }
    invisible(setGeneric(name, def, where = .GlobalEnv))
    for (j in seq_along(signatures)) {
        method <- local({
            choice <- j
            if (length(target) == 2L) function(x, y) choice else function(x, y, z) choice
        })
        setMethod(name, signatures[[j]], method, where = .GlobalEnv)
    }
    selected <- suppressMessages(selectMethod(name, target))
    values <- lapply(target[target != "missing"], new)
    cat(i, do.call(selected, values), "\n")
}
"#,
    );
    for (i, probe) in PROBES.iter().enumerate() {
        let maps = probe
            .target
            .iter()
            .map(|name| distances(name, class_info(name).superclasses))
            .collect::<Vec<_>>();
        let mut ranks = probe
            .methods
            .iter()
            .enumerate()
            .filter_map(|(j, sig)| {
                let signature = sig
                    .iter()
                    .map(|class| (*class).to_owned())
                    .collect::<Vec<_>>();
                Ranked::score(&signature, &maps).map(|rank| (rank, j + 1))
            })
            .collect::<Vec<_>>();
        ranks.sort_by(|(a, _), (b, _)| a.order(b));
        let Some(((rank, winner), alternatives)) = ranks.split_first() else {
            return Err(format!("Probe {} found no method", i + 1).into());
        };
        assert_eq!(*winner, probe.chosen, "probe {}", i + 1);
        assert_eq!(
            alternatives
                .iter()
                .any(|(other, _)| rank.total() == other.total() && rank.ambiguous_with(other)),
            probe.note,
            "probe {}",
            i + 1
        );
        expected.push_str(&format!("{} {} \n", i + 1, winner));
        let signatures = probe
            .methods
            .iter()
            .map(|sig| r_vector(sig))
            .collect::<Vec<_>>()
            .join(",");
        script.push_str(&format!(
            "probe({}, {}, list({signatures}))\n",
            i + 1,
            r_vector(probe.target)
        ));
    }
    if let Some(oracle) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") {
        let output = Command::new(oracle)
            .args(["--vanilla", "-e", &script])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout)?, expected);
    }
    Ok(())
}

#[test]
fn differential_scripts_under_torture_gc() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let default_bin = root.join("target/debug/roxide-harness");
    let harness = std::env::var_os("ROXIDE_TEST_HARNESS").map_or(default_bin, PathBuf::from);
    if !harness.is_file() || std::env::var_os("ROXIDE_ORACLE_RSCRIPT").is_none() {
        return Ok(());
    }
    let policy = GcPolicy::Torture;
    for file in [
        "04_s4_dispatch_resolver.rcase",
        "05_s4_callnextmethod_show.rcase",
    ] {
        let mut command = Command::new(&harness);
        if matches!(policy, GcPolicy::Torture) {
            command.env("ROXIDE_GC_TORTURE", "1");
        }
        let output = command
            .current_dir(&root)
            .args([
                "--file",
                &format!("conformance/layer3/{file}"),
                "--timeout-ms",
                "30000",
            ])
            .output()?;
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
