//! Creating a project: make the folder (when it is new), put a git repository in it when there
//! is none, and prepare it for a run. Refuses a folder that already holds a run, so nothing of
//! an earlier run is ever overwritten. Uses the `git` program; writes nothing but `.git/` and
//! an empty `.orchestrate/`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What was done, for the message the page shows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Created {
    pub path: String,
    pub made_folder: bool,
    pub git_initialised: bool,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e} (is git installed and on the PATH?)"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Has the folder a run (or what one leaves behind: its state database or its plan)?
fn holds_a_run(path: &Path) -> bool {
    let o = path.join(".orchestrate");
    o.join("state.db").exists() || o.join("plan.md").exists()
}

/// Make `path` a project ready for a run. An empty or new folder is fine; a folder with files
/// and no git repository gets one; a folder that already holds a run is refused.
pub fn create_project(path: &str) -> Result<Created, String> {
    if path.trim().is_empty() || path.contains(['\n', '\r']) {
        return Err("choose a folder for the new project".into());
    }
    let dir = PathBuf::from(path);
    if dir.exists() && !dir.is_dir() {
        return Err(format!("{path} is a file, not a folder: choose a folder"));
    }
    if holds_a_run(&dir) {
        return Err(format!(
            "{path} already holds a run (its .orchestrate/ has a plan or a state database): open it from the project list instead, so nothing of that run is overwritten"
        ));
    }
    let made_folder = !dir.exists();
    if made_folder {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {path}: {e}"))?;
    }
    let mut git_initialised = false;
    if !dir.join(".git").exists() {
        git(&dir, &["init", "-q"])?;
        git_initialised = true;
    }
    // A run needs a commit to branch from.
    if git(&dir, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
        git(
            &dir,
            &[
                "-c",
                "user.name=herdr-orchestra",
                "-c",
                "user.email=herdr-orchestra@localhost",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "Start the project",
            ],
        )?;
        git_initialised = true;
    }
    std::fs::create_dir_all(dir.join(".orchestrate"))
        .map_err(|e| format!("cannot prepare {path}/.orchestrate: {e}"))?;
    let canonical = std::fs::canonicalize(&dir)
        .map_err(|e| format!("cannot open {path}: {e}"))?
        .display()
        .to_string();
    Ok(Created {
        path: canonical,
        made_folder,
        git_initialised,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::scratch;

    #[test]
    fn a_new_folder_becomes_a_git_repository_with_a_first_commit_and_an_orchestrate_folder() {
        let parent = scratch("create-new");
        let target = parent.join("brand-new");
        let c = create_project(target.to_str().unwrap()).unwrap();
        assert!(c.made_folder && c.git_initialised);
        assert!(target.join(".git").exists());
        assert!(target.join(".orchestrate").is_dir());
        assert!(git(&target, &["rev-parse", "--verify", "HEAD"]).is_ok(), "needs a commit");
        assert_eq!(c.path, std::fs::canonicalize(&target).unwrap().display().to_string());
    }

    #[test]
    fn an_empty_existing_folder_is_fine() {
        let d = scratch("create-empty");
        let c = create_project(d.to_str().unwrap()).unwrap();
        assert!(!c.made_folder && c.git_initialised);
        assert!(d.join(".git").exists());
    }

    #[test]
    fn a_folder_with_files_keeps_them_and_gets_a_repository() {
        let d = scratch("create-files");
        std::fs::write(d.join("README.md"), "hello").unwrap();
        create_project(d.to_str().unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("README.md")).unwrap(), "hello");
        assert!(d.join(".git").exists());
    }

    #[test]
    fn an_existing_repository_is_kept_as_it_is() {
        let d = scratch("create-repo");
        git(&d, &["init", "-q"]).unwrap();
        git(
            &d,
            &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "mine"],
        )
        .unwrap();
        let head = git(&d, &["rev-parse", "HEAD"]).unwrap();
        let c = create_project(d.to_str().unwrap()).unwrap();
        assert!(!c.git_initialised);
        assert_eq!(git(&d, &["rev-parse", "HEAD"]).unwrap(), head, "no new commit");
    }

    #[test]
    fn a_folder_that_holds_a_run_is_refused_and_left_untouched() {
        let d = scratch("create-run");
        std::fs::create_dir_all(d.join(".orchestrate")).unwrap();
        std::fs::write(d.join(".orchestrate/plan.md"), "# Plan: x").unwrap();
        let e = create_project(d.to_str().unwrap()).unwrap_err();
        assert!(e.contains("already holds a run") && e.contains("overwritten"), "{e}");
        assert!(!d.join(".git").exists(), "nothing was created");
        assert_eq!(std::fs::read_to_string(d.join(".orchestrate/plan.md")).unwrap(), "# Plan: x");

        let d2 = scratch("create-run-db");
        std::fs::create_dir_all(d2.join(".orchestrate")).unwrap();
        std::fs::write(d2.join(".orchestrate/state.db"), "x").unwrap();
        assert!(create_project(d2.to_str().unwrap()).is_err());
    }

    #[test]
    fn a_file_or_an_empty_path_is_refused() {
        let d = scratch("create-file");
        let f = d.join("a-file");
        std::fs::write(&f, "x").unwrap();
        assert!(create_project(f.to_str().unwrap()).unwrap_err().contains("is a file"));
        assert!(create_project("  ").is_err());
        assert!(create_project("/a\nb").is_err());
    }
}
