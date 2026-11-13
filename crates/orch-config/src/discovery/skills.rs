//! The orch skills and slash commands shipped inside the binary (the text of Python
//! v4.10.1's `skills/`, `claude-commands/` and `opencode-commands/`), and the copying of
//! them into an agent's folders (`agents_sync`), with the uninstall manifest.

use crate::agents::AgentKind;
use std::io;
use std::path::{Path, PathBuf};

/// The two orch skills, by name.
pub const SKILL_NAMES: [&str; 2] = ["orch-plan", "orch-run"];

const PLAN_SKILL: &str = include_str!("../../assets/skills/orch-plan/SKILL.md");
const RUN_SKILL: &str = include_str!("../../assets/skills/orch-run/SKILL.md");
const CLAUDE_PLAN: &str = include_str!("../../assets/claude-commands/orch-plan.md");
const CLAUDE_RUN: &str = include_str!("../../assets/claude-commands/orch-run.md");
const OPENCODE_PLAN: &str = include_str!("../../assets/opencode-commands/orch-plan.md");
const OPENCODE_RUN: &str = include_str!("../../assets/opencode-commands/orch-run.md");

/// The `SKILL.md` text of a skill (`orch-plan` or `orch-run`).
pub fn skill_text(name: &str) -> Option<&'static str> {
    match name {
        "orch-plan" => Some(PLAN_SKILL),
        "orch-run" => Some(RUN_SKILL),
        _ => None,
    }
}

/// The slash-command text of one command in a flavour (`claude-commands` or
/// `opencode-commands`).
pub fn command_text(flavor: &str, name: &str) -> Option<&'static str> {
    match (flavor, name) {
        ("claude-commands", "orch-plan") => Some(CLAUDE_PLAN),
        ("claude-commands", "orch-run") => Some(CLAUDE_RUN),
        ("opencode-commands", "orch-plan") => Some(OPENCODE_PLAN),
        ("opencode-commands", "orch-run") => Some(OPENCODE_RUN),
        _ => None,
    }
}

/// The flavour a kind's commands come from when its registry entry says nothing.
pub const DEFAULT_FLAVOR: &str = "claude-commands";

/// The home folder (`$HOME`).
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Expand a leading `~` (or `~/`) to `home`, as Python's `expanduser`.
pub fn expand(p: &str, home: &Path) -> PathBuf {
    if p == "~" {
        home.to_path_buf()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(p)
    }
}

/// `~/path` for display (Python's `short`).
pub fn short(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

/// Whether one expected file is there and matches what this build ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetState {
    Current,
    Outdated,
    Missing,
}

/// One skill or command file a kind should have.
#[derive(Debug, Clone)]
pub struct AssetCheck {
    pub path: PathBuf,
    /// A `SKILL.md` (true) or a slash command (false).
    pub is_skill: bool,
    pub state: AssetState,
}

fn state_of(path: &Path, want: Option<&str>) -> AssetState {
    match std::fs::read_to_string(path) {
        Err(_) if !path.exists() => AssetState::Missing,
        Ok(text) if Some(text.as_str()) == want => AssetState::Current,
        // Present but different (or unreadable text): installed, not current.
        _ => AssetState::Outdated,
    }
}

/// Every skill and command file the registry says `ak` should have, with its state
/// (Python's `agent_assets`, plus whether the text is current).
pub fn agent_assets(ak: &AgentKind, home: &Path) -> Vec<AssetCheck> {
    let flavor = ak.commands_from.as_deref().unwrap_or(DEFAULT_FLAVOR);
    let mut out = Vec::new();
    for d in &ak.skills_dirs {
        for s in SKILL_NAMES {
            let path = expand(d, home).join(s).join("SKILL.md");
            let state = state_of(&path, skill_text(s));
            out.push(AssetCheck {
                path,
                is_skill: true,
                state,
            });
        }
    }
    for d in &ak.commands_dirs {
        for s in SKILL_NAMES {
            let path = expand(d, home).join(format!("{s}.md"));
            let state = state_of(&path, command_text(flavor, s));
            out.push(AssetCheck {
                path,
                is_skill: false,
                state,
            });
        }
    }
    out
}

/// Write `text` to `dst` (creating parents) unless it already has exactly that text.
/// True when the file was created or changed.
fn write_if_changed(dst: &Path, text: &str) -> io::Result<bool> {
    if std::fs::read(dst).is_ok_and(|b| b == text.as_bytes()) {
        return Ok(false);
    }
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(dst, text)?;
    Ok(true)
}

/// Copy orch-plan / orch-run into `<d>/<skill>/SKILL.md`. Returns (changed, paths to record).
pub fn sync_skills(d: &Path) -> io::Result<(bool, Vec<PathBuf>)> {
    let (mut changed, mut recs) = (false, Vec::new());
    for s in SKILL_NAMES {
        changed |= write_if_changed(
            &d.join(s).join("SKILL.md"),
            skill_text(s).unwrap_or_default(),
        )?;
        recs.push(d.join(s));
    }
    Ok((changed, recs))
}

/// Copy the orch-*.md slash commands of `flavor` into `d`. An unknown flavour copies
/// nothing (Python's `asset()` returning None).
pub fn sync_commands(d: &Path, flavor: &str) -> io::Result<(bool, Vec<PathBuf>)> {
    let (mut changed, mut recs) = (false, Vec::new());
    for s in SKILL_NAMES {
        let Some(text) = command_text(flavor, s) else {
            continue;
        };
        let dst = d.join(format!("{s}.md"));
        changed |= write_if_changed(&dst, text)?;
        recs.push(dst);
    }
    Ok((changed, recs))
}

/// The uninstall manifest: `~/.local/share/herdr-orchestra/manifest.txt`.
pub fn manifest_path(home: &Path) -> PathBuf {
    home.join(".local/share/herdr-orchestra/manifest.txt")
}

/// Append paths to the manifest (once each) so the uninstaller removes them.
pub fn manifest_record(home: &Path, paths: &[PathBuf]) -> io::Result<()> {
    let mf = manifest_path(home);
    if let Some(p) = mf.parent() {
        std::fs::create_dir_all(p)?;
    }
    let text = std::fs::read_to_string(&mf).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for p in paths {
        let p = p.display().to_string();
        if !lines.contains(&p) {
            lines.push(p);
        }
    }
    std::fs::write(
        &mf,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_text_is_python_4_10_1s() {
        assert!(skill_text("orch-plan").unwrap().contains("orch lead plan"));
        assert!(
            command_text("claude-commands", "orch-run")
                .unwrap()
                .contains("orch lead run")
        );
        assert_eq!(
            command_text("claude-commands", "orch-plan"),
            command_text("opencode-commands", "orch-plan")
        );
        assert!(command_text("nope", "orch-plan").is_none());
    }

    #[test]
    fn expand_and_short_round_trip() {
        let home = Path::new("/h");
        assert_eq!(expand("~/.x/y", home), Path::new("/h/.x/y"));
        assert_eq!(expand("/abs", home), Path::new("/abs"));
        assert_eq!(short(Path::new("/h/.x/y"), home), "~/.x/y");
        assert_eq!(short(Path::new("/other/y"), home), "/other/y");
    }

    #[test]
    fn manifest_has_each_path_once() {
        let t = tempfile::tempdir().unwrap();
        let a = PathBuf::from("/a/orch-plan");
        manifest_record(t.path(), &[a.clone(), PathBuf::from("/b")]).unwrap();
        manifest_record(t.path(), &[a, PathBuf::from("/c")]).unwrap();
        let text = std::fs::read_to_string(manifest_path(t.path())).unwrap();
        assert_eq!(text, "/a/orch-plan\n/b\n/c\n");
    }

    #[test]
    fn sync_writes_then_reports_nothing_changed() {
        let t = tempfile::tempdir().unwrap();
        let (c, recs) = sync_skills(t.path()).unwrap();
        assert!(c);
        assert_eq!(recs.len(), 2);
        assert!(!sync_skills(t.path()).unwrap().0);
        let (c, _) = sync_commands(t.path(), "claude-commands").unwrap();
        assert!(c);
        assert!(!sync_commands(t.path(), "claude-commands").unwrap().0);
        assert_eq!(sync_commands(t.path(), "unknown").unwrap(), (false, vec![]));
    }
}
