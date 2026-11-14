//! The scope check (§13, Python `path_allowed` / `changed_files`): the coder's real diff against
//! the milestone's `Likely files`. An entry matches a file when it is that exact path, a glob
//! that matches it (`*` also crosses `/`, as Python's `fnmatch` does), or a folder the file is
//! under (`src/` and `src` both work).

use crate::error::Result;
use crate::git;
use globset::Glob;
use std::collections::BTreeSet;
use std::path::Path;

/// Whether `path` is covered by one of the `Likely files` entries.
pub fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        let folder = format!("{}/", p.trim_end_matches('/'));
        path == p
            || path.starts_with(&folder)
            || Glob::new(p).is_ok_and(|g| g.compile_matcher().is_match(path))
    })
}

/// The changed files that no entry covers, sorted. With no entries nothing is out of scope (a
/// milestone without `Likely files` is not checked).
pub fn strays(changed: &[String], allowed: &[String]) -> Vec<String> {
    if allowed.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = changed.iter().filter(|f| !path_allowed(f, allowed)).cloned().collect();
    out.sort();
    out.dedup();
    out
}

/// Files nearly every milestone wants to touch: two of them editing one in parallel conflict
/// (Python `HOTSPOT_RE`).
pub fn is_hotspot(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    const EXACT: &[&str] = &[
        "package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb",
        "pubspec.yaml", "pubspec.lock", "go.mod", "go.sum", "pom.xml", "build.gradle",
        "build.gradle.kts", "settings.gradle", "cargo.toml", "cargo.lock", "requirements.txt",
        "pyproject.toml", "poetry.lock", "uv.lock", "docker-compose.yml", "docker-compose.yaml",
        "compose.yml", "compose.yaml", "makefile", ".env.example", "app.tsx", "app.jsx", "app.vue",
        "app.svelte", "main.ts", "main.tsx", "main.js", "main.dart", "main.go",
    ];
    let stem_is = |stem: &str| {
        name.strip_prefix(stem)
            .is_some_and(|rest| rest.len() > 1 && rest.starts_with('.') && rest[1..].chars().all(|c| c.is_alphanumeric() || c == '_'))
    };
    EXACT.contains(&name.as_str()) || stem_is("route") || stem_is("routes") || stem_is("router")
}

/// The files the coder changed in `dir` (committed on its saved work or not), without those that
/// differ only because other milestones landed on `branch` since. Shared mode: the working tree
/// against `HEAD`. Untracked files count; `.orch-task/` is git-excluded in every folder.
pub fn changed_files(dir: &Path, branch: &str, shared: bool) -> Result<Vec<String>> {
    let lines = |args: &[&str]| -> Result<BTreeSet<String>> {
        Ok(git::run(dir, args)?.out().lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    };
    let untracked = lines(&["ls-files", "--others", "--exclude-standard"])?;
    if shared {
        let mut all = lines(&["diff", "--name-only", "HEAD"])?;
        all.extend(untracked);
        return Ok(all.into_iter().collect());
    }
    let mb = git::run(dir, &["merge-base", branch, "HEAD"])?.out().trim().to_string();
    let base = if mb.is_empty() { branch.to_string() } else { mb };
    let vs_base = lines(&["diff", "--name-only", &base])?;
    let vs_tip = lines(&["diff", "--name-only", branch])?;
    let mut all: BTreeSet<String> = vs_base.intersection(&vs_tip).cloned().collect();
    all.extend(untracked);
    Ok(all.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exact_folder_and_glob_entries_cover_files() {
        let allowed = v(&["crates/a/src/x.rs", "crates/b/tests/", "docs/*.md", "web"]);
        for ok in ["crates/a/src/x.rs", "crates/b/tests/t.rs", "crates/b/tests/deep/t.rs", "docs/a.md", "web/index.html"] {
            assert!(path_allowed(ok, &allowed), "{ok}");
        }
        for bad in ["crates/a/src/y.rs", "crates/b/testsX/t.rs", "docs/a.txt", "webby/x", "Cargo.toml"] {
            assert!(!path_allowed(bad, &allowed), "{bad}");
        }
    }

    #[test]
    fn strays_are_sorted_and_empty_without_likely_files() {
        let allowed = v(&["src/"]);
        assert_eq!(strays(&v(&["b.rs", "src/a.rs", "a.rs"]), &allowed), v(&["a.rs", "b.rs"]));
        assert!(strays(&v(&["b.rs"]), &[]).is_empty());
    }

    #[test]
    fn hotspots() {
        for f in ["package.json", "a/Cargo.toml", "src/routes.ts", "x/main.go", "App.tsx"] {
            assert!(is_hotspot(f), "{f}");
        }
        for f in ["src/lib.rs", "routes", "route.", "README.md"] {
            assert!(!is_hotspot(f), "{f}");
        }
    }

    #[test]
    fn the_real_diff_ignores_what_landed_meanwhile_and_counts_untracked() {
        let (_t, root) = git::testutil::repo();
        let g = |args: &[&str]| git::testutil::g(&root, args);
        let branch = g(&["rev-parse", "--abbrev-ref", "HEAD"]);
        g(&["switch", "-q", "-c", "work"]);
        std::fs::write(root.join("mine.rs"), "x").unwrap();
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "mine"]);
        std::fs::write(root.join("loose.rs"), "y").unwrap();
        // another milestone lands on the branch meanwhile
        g(&["switch", "-q", &branch]);
        std::fs::write(root.join("theirs.rs"), "z").unwrap();
        g(&["add", "theirs.rs"]);
        g(&["commit", "-q", "-m", "theirs"]);
        g(&["switch", "-q", "work"]);
        let files = changed_files(&root, &branch, false).unwrap();
        assert_eq!(files, v(&["loose.rs", "mine.rs"]));
        // shared: working tree against HEAD
        std::fs::write(root.join("mine.rs"), "changed").unwrap();
        let files = changed_files(&root, "work", true).unwrap();
        assert_eq!(files, v(&["loose.rs", "mine.rs"]));
    }
}
