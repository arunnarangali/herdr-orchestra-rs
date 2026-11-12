//! The agent registry: shipped `agents.default.toml` merged key by key with the user's
//! `agents.toml` (or `ORCH_AGENTS_FILE`), as Python's `registry()` does.

use orch_core::error::EXIT_USAGE;
use orch_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The shipped registry, embedded in the binary (v4.10.1, unchanged).
pub const DEFAULT_TOML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/agents.default.toml"));

/// Roles an agent kind can serve.
pub const ROLES: [&str; 3] = ["lead", "coder", "reviewer"];

#[derive(Debug, thiserror::Error)]
#[error("agent registry: {file}: {problem}")]
pub struct AgentsError {
    /// File the problem is in (`agents.default.toml` or the override path).
    pub file: String,
    /// What is wrong with it.
    pub problem: String,
}

impl From<AgentsError> for Error {
    fn from(e: AgentsError) -> Self {
        Error::wrap(e, EXIT_USAGE)
    }
}

/// One agent kind's registry entry (`[claude]`, `[codex]`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKind {
    /// Executable that must be on PATH for the kind to count as installed.
    pub binary: String,
    /// Command to start it in a herdr pane, with its auto-approve-edits option.
    pub launch: String,
    /// The launch option that picks the model, when the tool has a verified one.
    #[serde(default)]
    pub model_flag: Option<String>,
    /// Whether herdr can tell idle / working / blocked for this kind.
    #[serde(default)]
    pub herdr_detect: bool,
    /// Any of "lead", "coder", "reviewer".
    pub roles: Vec<String>,
    /// Global SKILL.md folders the installer puts the orch skills into.
    #[serde(default)]
    pub skills_dirs: Vec<String>,
    /// Slash-command folders the installer puts the orch commands into.
    #[serde(default)]
    pub commands_dirs: Vec<String>,
    /// Which command flavour to install there: "claude-commands" or "opencode-commands".
    #[serde(default)]
    pub commands_from: Option<String>,
    /// How the developer starts the plan playbook in that agent.
    #[serde(default)]
    pub invoke: Option<String>,
    /// True only when the launch option and skills folder were checked against the real tool.
    #[serde(default)]
    pub verified: bool,
    /// What was (or wasn't) verified.
    #[serde(default)]
    pub notes: Option<String>,
}

/// The whole registry: agent kind name -> its entry.
#[derive(Debug, Clone, Default)]
pub struct Registry(pub BTreeMap<String, AgentKind>);

impl Registry {
    pub fn get(&self, kind: &str) -> Option<&AgentKind> {
        self.0.get(kind)
    }
}

/// The user's override file: `ORCH_AGENTS_FILE` when set (and not empty), else
/// `~/.config/herdr-orchestra/agents.toml`, as Python's `user_agents_file()` does.
pub fn user_agents_file() -> PathBuf {
    match std::env::var_os("ORCH_AGENTS_FILE") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home_dir().join(".config").join("herdr-orchestra").join("agents.toml"),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Shipped defaults merged with `user_file` (missing file is fine, a broken one is an
/// error naming it), then validated. `None` uses [`user_agents_file`].
pub fn load(user_file: Option<&Path>) -> Result<Registry> {
    let path = user_file.map_or_else(user_agents_file, Path::to_path_buf);
    registry(DEFAULT_TOML, &path).map_err(Error::from)
}

/// Merge a default registry text with one user override file and validate the result.
/// Each kind's keys are merged key by key, the user winning (Python's `dict.update`).
pub fn registry(default_toml: &str, user_file: &Path) -> std::result::Result<Registry, AgentsError> {
    let mut table = parse(default_toml, "agents.default.toml")?;
    if user_file.exists() {
        let text = read(user_file)?;
        for (kind, vals) in parse(&text, &file_name(user_file))? {
            if let Some(vals) = vals.as_table() {
                let mut merged = table.get(&kind).and_then(|v| v.as_table().cloned()).unwrap_or_default();
                for (k, v) in vals {
                    merged.insert(k.clone(), v.clone());
                }
                table.insert(kind, toml::Value::Table(merged));
            }
        }
    }
    let mut kinds = BTreeMap::new();
    for (kind, vals) in table {
        if !vals.is_table() {
            return Err(AgentsError {
                file: file_name(user_file),
                problem: format!("agent kind {kind:?} must be a table"),
            });
        }
        let ak: AgentKind = AgentKind::deserialize(vals).map_err(|e| AgentsError {
            file: file_name(user_file),
            problem: format!("agent kind {kind:?}: {e}"),
        })?;
        validate(&ak).map_err(|problem| AgentsError {
            file: file_name(user_file),
            problem: format!("agent kind {kind:?}: {problem}"),
        })?;
        kinds.insert(kind, ak);
    }
    Ok(Registry(kinds))
}

fn validate(ak: &AgentKind) -> std::result::Result<(), String> {
    for role in &ak.roles {
        if !ROLES.contains(&role.as_str()) {
            return Err(format!("unknown role {role:?} (allowed: {})", ROLES.join(", ")));
        }
    }
    Ok(())
}

fn parse(text: &str, file: &str) -> std::result::Result<toml::Table, AgentsError> {
    text.parse::<toml::Table>()
        .map_err(|e: toml::de::Error| AgentsError { file: file.to_string(), problem: e.to_string() })
}

fn read(path: &Path) -> std::result::Result<String, AgentsError> {
    std::fs::read_to_string(path).map_err(|e| AgentsError {
        file: file_name(path),
        problem: format!("cannot read it: {e}"),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(t: &tempfile::TempDir, toml_text: &str) -> PathBuf {
        let p = t.path().join("agents.toml");
        std::fs::write(&p, toml_text).unwrap();
        p
    }

    #[test]
    fn shipped_defaults_parse_with_all_kinds() {
        let reg = registry(DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        for kind in ["claude", "codex", "amp", "opencode", "hermes", "agy"] {
            assert!(reg.0.contains_key(kind), "missing {kind}");
        }
        assert_eq!(reg.0.len(), 6);
    }

    #[test]
    fn shipped_keys_are_todays() {
        let reg = registry(DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let claude = reg.get("claude").unwrap();
        assert_eq!(claude.binary, "claude");
        assert_eq!(claude.launch, "claude --permission-mode acceptEdits");
        assert_eq!(claude.model_flag.as_deref(), Some("--model"));
        assert!(claude.herdr_detect);
        assert_eq!(claude.roles, ["lead", "coder", "reviewer"]);
        assert_eq!(claude.skills_dirs, ["~/.claude/skills"]);
        assert_eq!(claude.commands_dirs, ["~/.claude/commands"]);
        assert_eq!(claude.commands_from.as_deref(), Some("claude-commands"));
        assert_eq!(claude.invoke.as_deref(), Some("/orch-plan <feature>"));
        assert!(claude.verified);
        assert!(claude.notes.as_deref().unwrap().contains("2.1"));
        assert!(!reg.get("hermes").unwrap().verified);
        assert!(reg.get("amp").unwrap().model_flag.is_none());
        assert_eq!(
            reg.get("opencode").unwrap().commands_dirs,
            ["~/.config/opencode/commands"]
        );
    }

    #[test]
    fn override_one_key_of_one_kind() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[claude]\nlaunch = \"claude --foo\"\n");
        let reg = registry(DEFAULT_TOML, &p).unwrap();
        assert_eq!(reg.get("claude").unwrap().launch, "claude --foo");
        assert_eq!(reg.get("claude").unwrap().binary, "claude");
        assert_eq!(reg.get("codex").unwrap().launch, "codex --approve-for-me");
    }

    #[test]
    fn add_a_new_kind() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[acme]\nbinary = \"acme\"\nlaunch = \"acme\"\nroles = [\"coder\"]\n",
        );
        let reg = registry(DEFAULT_TOML, &p).unwrap();
        let acme = reg.get("acme").unwrap();
        assert_eq!(acme.binary, "acme");
        assert_eq!(acme.roles, ["coder"]);
        assert!(!acme.verified);
        assert_eq!(reg.0.len(), 7);
    }

    #[test]
    fn missing_override_file_is_fine() {
        let reg = registry(DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        assert_eq!(reg.0.len(), 6);
    }

    #[test]
    fn broken_toml_names_the_file() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[claude\nlaunch = \"x\"\n");
        let e = registry(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "agents.toml");
        assert!(e.to_string().contains("agent registry: agents.toml:"), "{e}");
    }

    #[test]
    fn wrong_type_names_the_file_and_the_problem() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[claude]\nherdr_detect = \"yes\"\n");
        let e = registry(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "agents.toml");
        assert!(e.problem.contains("claude"), "{e}");
    }

    #[test]
    fn unknown_role_is_a_validation_error() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[acme]\nbinary = \"a\"\nlaunch = \"a\"\nroles = [\"tester\"]\n");
        let e = registry(DEFAULT_TOML, &p).unwrap_err();
        assert!(e.problem.contains("unknown role \"tester\""), "{e}");
    }

    #[test]
    fn missing_required_key_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[acme]\nlaunch = \"a\"\nroles = [\"coder\"]\n");
        let e = registry(DEFAULT_TOML, &p).unwrap_err();
        assert!(e.problem.contains("acme"), "{e}");
    }

    #[test]
    fn non_table_kind_values_are_ignored_like_python() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "stray = 3\n[claude]\nlaunch = \"claude --foo\"\n");
        let reg = registry(DEFAULT_TOML, &p).unwrap();
        assert_eq!(reg.0.len(), 6);
        assert_eq!(reg.get("claude").unwrap().launch, "claude --foo");
    }

    #[test]
    fn load_uses_the_embedded_defaults_and_the_given_path() {
        let t = tempfile::tempdir().unwrap();
        let p = user(&t, "[claude]\nverified = false\n");
        let reg = load(Some(&p)).unwrap();
        assert!(!reg.get("claude").unwrap().verified);
        assert!(reg.get("agy").unwrap().verified);
        assert_eq!(reg.0.len(), 6);
    }

    #[test]
    fn user_agents_file_follows_the_env_var() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap();
        unsafe { std::env::set_var("ORCH_AGENTS_FILE", "/tmp/from-env/agents.toml") };
        assert_eq!(user_agents_file(), PathBuf::from("/tmp/from-env/agents.toml"));
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
        assert!(user_agents_file().ends_with("herdr-orchestra/agents.toml"));
        unsafe { std::env::set_var("ORCH_AGENTS_FILE", "") };
        assert!(user_agents_file().ends_with("herdr-orchestra/agents.toml"));
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
    }
}
