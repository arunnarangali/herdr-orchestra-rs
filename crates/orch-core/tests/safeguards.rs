//! Source-level guards for the §22 promises that must never regress silently: only
//! `orch finish` pushes, nothing force-pushes, and the only code that hands text to a shell
//! is the validated check and setup commands.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every non-test source file of every crate, with its text up to the first `#[cfg(test)]`.
fn all_sources() -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut files = Vec::new();
    for c in std::fs::read_dir(&crates).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let body = text.split("#[cfg(test)]").next().unwrap().to_string();
            (p.strip_prefix(&crates).unwrap().display().to_string(), body)
        })
        .collect()
}

#[test]
fn only_orch_finish_pushes_and_nothing_force_pushes() {
    for (file, text) in all_sources() {
        if !file.ends_with("orch-core/src/finish.rs") {
            // (`git stash push` is local; a remote push names the remote)
            let remote_push = text.lines().any(|l| {
                l.contains("\"push\"") && !l.contains("\"stash\"")
            });
            assert!(!remote_push, "{file} runs `git push`; only `orch finish` may");
        }
        // `orch set --force` is unrelated; what matters is the one file that pushes
        if file.ends_with("orch-core/src/finish.rs") {
            for flag in ["--force", "--force-with-lease", "\"-f\"", "+refs", "\"--mirror\""] {
                assert!(!text.contains(flag), "{file} uses {flag}: a push must never force");
            }
        }
    }
}

#[test]
fn only_the_check_and_setup_commands_go_through_a_shell() {
    let allowed = ["orch-core/src/check.rs", "orch-core/src/setup.rs"];
    for (file, text) in all_sources() {
        let uses_sh = text.contains("Command::new(\"sh\")") || text.contains("Command::new(\"bash\")");
        if uses_sh {
            assert!(
                allowed.iter().any(|a| file.ends_with(a)),
                "{file} hands text to a shell; only the validated check and setup commands may"
            );
        }
    }
}
