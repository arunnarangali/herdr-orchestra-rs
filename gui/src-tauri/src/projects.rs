//! The list of projects the app remembers, and what can be seen of each folder without the
//! server: whether it is a git repository, its branch, and its archive of earlier runs. Reads
//! project folders, never writes to them.

use crate::store::{self, Loaded};
use serde::Serialize;
use std::path::{Path, PathBuf};

const FILE: &str = "projects.txt";
const KEYS: &[&str] = &["project", "active"];

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProjectFile {
    pub projects: Vec<String>,
    pub active: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArchiveEntry {
    /// The phase folder's name in `.orchestrate/archive/`.
    pub name: String,
    pub files: Vec<String>,
    /// Seconds since the epoch.
    pub modified: Option<i64>,
    /// First line of the phase's summary file, as plain text.
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    /// The folder is still there.
    pub exists: bool,
    pub is_git: bool,
    pub branch: Option<String>,
    /// The folder has an `.orchestrate/` (a run, or the remains of one).
    pub has_orchestrate: bool,
    pub archive: Vec<ArchiveEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectList {
    pub projects: Vec<ProjectInfo>,
    pub active: Option<String>,
    pub warning: Option<String>,
}

pub struct ProjectStore {
    dir: PathBuf,
}

impl ProjectStore {
    pub fn new(dir: PathBuf) -> ProjectStore {
        ProjectStore { dir }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    pub fn load(&self) -> Loaded<ProjectFile> {
        let (entries, warning) = store::read_or_set_aside(&self.file(), KEYS);
        let mut out = ProjectFile::default();
        for (k, v) in entries.unwrap_or_default() {
            match k.as_str() {
                "project" if !out.projects.contains(&v) => out.projects.push(v),
                "active" => out.active = Some(v),
                _ => {}
            }
        }
        // An active project that is not in the list is dropped, not trusted.
        if out
            .active
            .as_ref()
            .is_some_and(|a| !out.projects.contains(a))
        {
            out.active = None;
        }
        Loaded {
            value: out,
            warning,
        }
    }

    fn save(&self, f: &ProjectFile) -> Result<(), String> {
        let mut entries: Vec<(&str, &str)> =
            f.projects.iter().map(|p| ("project", p.as_str())).collect();
        if let Some(a) = &f.active {
            entries.push(("active", a));
        }
        store::write_atomic(&self.file(), &store::render(&entries))
    }

    /// Remember `path`. It must be a folder holding a git repository.
    pub fn add(&self, path: &str) -> Result<ProjectFile, String> {
        store::check_value(path)?;
        let p = Path::new(path);
        if !p.is_dir() {
            return Err(format!("{path} is not a folder"));
        }
        if !p.join(".git").exists() {
            return Err(format!(
                "{path} is not a git repository: choose the folder that contains .git (run `git init` there first if it is new)"
            ));
        }
        let canonical = std::fs::canonicalize(p)
            .map_err(|e| format!("cannot open {path}: {e}"))?
            .display()
            .to_string();
        let mut f = self.load().value;
        if !f.projects.contains(&canonical) {
            f.projects.push(canonical);
        }
        self.save(&f)?;
        Ok(f)
    }

    /// Forget `path`. Only the list changes; the folder is not touched.
    pub fn remove(&self, path: &str) -> Result<ProjectFile, String> {
        let mut f = self.load().value;
        f.projects.retain(|p| p != path);
        if f.active.as_deref() == Some(path) {
            f.active = None;
        }
        self.save(&f)?;
        Ok(f)
    }

    /// Make `path` the active project. It must be in the list and still exist.
    pub fn set_active(&self, path: &str) -> Result<ProjectFile, String> {
        let mut f = self.load().value;
        if !f.projects.iter().any(|p| p == path) {
            return Err(format!("{path} is not in the project list"));
        }
        if !Path::new(path).is_dir() {
            return Err(format!(
                "{path} no longer exists: remove it from the list or restore the folder"
            ));
        }
        f.active = Some(path.to_string());
        self.save(&f)?;
        Ok(f)
    }

    pub fn list(&self) -> ProjectList {
        let Loaded { value, warning } = self.load();
        ProjectList {
            projects: value
                .projects
                .iter()
                .map(|p| inspect(Path::new(p)))
                .collect(),
            active: value.active,
            warning,
        }
    }
}

/// The current branch from `.git/HEAD` (following a worktree's `gitdir:` pointer).
fn branch_of(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        let t = PathBuf::from(target);
        if t.is_absolute() { t } else { root.join(t) }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(b) => Some(b.to_string()),
        None if head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(format!("(detached at {})", &head[..7]))
        }
        None => None,
    }
}

fn first_line(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

fn mtime(path: &Path) -> Option<i64> {
    let t = path.metadata().ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// `.orchestrate/archive/<phase>/`: the phases found, newest name first.
fn archive_of(root: &Path) -> Vec<ArchiveEntry> {
    let Ok(rd) = std::fs::read_dir(root.join(".orchestrate").join("archive")) else {
        return Vec::new();
    };
    let mut out: Vec<ArchiveEntry> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let dir = e.path();
            let mut files: Vec<String> = std::fs::read_dir(&dir)
                .map(|r| {
                    r.filter_map(Result::ok)
                        .map(|f| f.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            let summary = files
                .iter()
                .find(|f| f.to_lowercase().starts_with("summary"))
                .and_then(|f| first_line(&dir.join(f)));
            ArchiveEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                files,
                modified: mtime(&dir),
                summary,
            }
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

pub fn inspect(path: &Path) -> ProjectInfo {
    let exists = path.is_dir();
    ProjectInfo {
        path: path.display().to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        exists,
        is_git: exists && path.join(".git").exists(),
        branch: if exists { branch_of(path) } else { None },
        has_orchestrate: exists && path.join(".orchestrate").is_dir(),
        archive: if exists { archive_of(path) } else { Vec::new() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::scratch;

    fn repo(tag: &str, branch: &str) -> PathBuf {
        let d = scratch(tag);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".git/HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn store(tag: &str) -> ProjectStore {
        ProjectStore::new(scratch(tag).join("config"))
    }

    #[test]
    fn add_remembers_a_repository_once() {
        let s = store("add");
        let r = repo("add-r", "main");
        let f = s.add(r.to_str().unwrap()).unwrap();
        assert_eq!(f.projects, vec![r.display().to_string()]);
        s.add(r.to_str().unwrap()).unwrap();
        assert_eq!(s.load().value.projects.len(), 1);
        assert!(s.load().warning.is_none());
    }

    #[test]
    fn add_refuses_a_folder_that_is_not_a_repository_and_one_that_is_not_a_folder() {
        let s = store("refuse");
        let plain = scratch("plain");
        let e = s.add(plain.to_str().unwrap()).unwrap_err();
        assert!(e.contains("not a git repository"), "{e}");
        let e = s.add("/nonexistent/x").unwrap_err();
        assert!(e.contains("not a folder"), "{e}");
        assert!(s.load().value.projects.is_empty());
        assert!(s.add("/a\nb").is_err());
    }

    #[test]
    fn remove_only_changes_the_list() {
        let s = store("remove");
        let r = repo("remove-r", "main");
        let p = r.to_str().unwrap();
        s.add(p).unwrap();
        s.set_active(p).unwrap();
        let f = s.remove(p).unwrap();
        assert!(f.projects.is_empty() && f.active.is_none());
        assert!(r.join(".git/HEAD").exists(), "the folder must be untouched");
        s.remove("/not/listed").unwrap(); // removing what is not there is fine
    }

    #[test]
    fn switch_sets_the_active_project_and_survives_a_reload() {
        let s = store("switch");
        let (a, b) = (repo("sw-a", "main"), repo("sw-b", "dev"));
        s.add(a.to_str().unwrap()).unwrap();
        s.add(b.to_str().unwrap()).unwrap();
        s.set_active(b.to_str().unwrap()).unwrap();
        let reloaded = ProjectStore::new(s.dir.clone()).list();
        assert_eq!(reloaded.active, Some(b.display().to_string()));
        assert_eq!(reloaded.projects.len(), 2);
        assert!(
            s.set_active("/not/listed")
                .unwrap_err()
                .contains("not in the project list")
        );
    }

    #[test]
    fn a_folder_that_vanished_is_listed_as_missing_and_cannot_be_switched_to() {
        let s = store("vanish");
        let r = repo("vanish-r", "main");
        let p = r.to_str().unwrap().to_string();
        s.add(&p).unwrap();
        std::fs::remove_dir_all(&r).unwrap();
        let list = s.list();
        assert!(!list.projects[0].exists);
        assert!(!list.projects[0].is_git);
        assert!(list.projects[0].branch.is_none());
        assert!(s.set_active(&p).unwrap_err().contains("no longer exists"));
        s.remove(&p).unwrap(); // and it can still be removed
    }

    #[test]
    fn a_corrupt_file_is_set_aside_with_a_warning_and_the_app_starts_empty() {
        let s = store("corrupt");
        std::fs::create_dir_all(&s.dir).unwrap();
        std::fs::write(s.file(), "{ this is not our format").unwrap();
        let l = s.load();
        assert!(l.value.projects.is_empty());
        assert!(
            l.warning
                .as_deref()
                .unwrap()
                .contains("projects.txt.corrupt"),
            "{:?}",
            l.warning
        );
        assert!(s.dir.join("projects.txt.corrupt").exists());
        // and it is usable again
        let r = repo("corrupt-r", "main");
        s.add(r.to_str().unwrap()).unwrap();
        assert!(s.load().warning.is_none());
    }

    #[test]
    fn an_unknown_key_counts_as_corrupt_and_a_stray_active_is_dropped() {
        let s = store("odd");
        std::fs::create_dir_all(&s.dir).unwrap();
        std::fs::write(s.file(), format!("{}\nbogus=1\n", store::HEADER)).unwrap();
        assert!(s.load().warning.is_some());
        std::fs::write(
            s.file(),
            format!("{}\nproject=/a\nactive=/b\n", store::HEADER),
        )
        .unwrap();
        let l = s.load();
        assert_eq!(
            (l.value.projects, l.value.active),
            (vec!["/a".to_string()], None)
        );
    }

    #[test]
    fn inspect_reads_branch_and_archive_without_writing() {
        let r = repo("inspect", "feature/x");
        let arch = r.join(".orchestrate/archive/phase-1");
        std::fs::create_dir_all(&arch).unwrap();
        std::fs::write(arch.join("summary.md"), "\n# Phase 1 done\nmore").unwrap();
        std::fs::write(arch.join("plan.md"), "x").unwrap();
        std::fs::create_dir_all(r.join(".orchestrate/archive/phase-2")).unwrap();
        let i = inspect(&r);
        assert!(i.exists && i.is_git && i.has_orchestrate);
        assert_eq!(i.branch.as_deref(), Some("feature/x"));
        assert_eq!(
            i.archive
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["phase-2", "phase-1"]
        );
        assert_eq!(i.archive[1].summary.as_deref(), Some("# Phase 1 done"));
        assert_eq!(i.archive[1].files, ["plan.md", "summary.md"]);
        assert!(i.archive[0].summary.is_none());
    }

    #[test]
    fn branch_handles_detached_head_and_worktrees() {
        let r = repo("detached", "main");
        std::fs::write(
            r.join(".git/HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert_eq!(inspect(&r).branch.as_deref(), Some("(detached at 0123456)"));
        let main = repo("wt-main", "main");
        let gitdir = main.join(".git/worktrees/w");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/wt-branch\n").unwrap();
        let wt = scratch("wt");
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert_eq!(inspect(&wt).branch.as_deref(), Some("wt-branch"));
    }
}
