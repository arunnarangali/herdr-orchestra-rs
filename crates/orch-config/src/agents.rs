//! The agent registry: shipped `agents.default.toml` merged key by key with the user's
//! `agents.toml` (or `ORCH_AGENTS_FILE`), as Python's `registry()` does.

use orch_core::error::EXIT_USAGE;
use orch_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The shipped registry, embedded in the binary (v4.10.1, unchanged).
pub const DEFAULT_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/agents.default.toml"));

/// A role an agent kind can serve (§5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Lead,
    Coder,
    Reviewer,
    Tester,
    Researcher,
    Specialist,
}

impl Role {
    /// Every role, in the order the docs list them.
    pub const ALL: [Role; 6] = [
        Role::Lead,
        Role::Coder,
        Role::Reviewer,
        Role::Tester,
        Role::Researcher,
        Role::Specialist,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Lead => "lead",
            Role::Coder => "coder",
            Role::Reviewer => "reviewer",
            Role::Tester => "tester",
            Role::Researcher => "researcher",
            Role::Specialist => "specialist",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Role::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .ok_or_else(|| {
                let all: Vec<_> = Role::ALL.iter().map(|r| r.as_str()).collect();
                format!("unknown role {s:?} (allowed: {})", all.join(", "))
            })
    }
}

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
    #[serde(default = "default_true")]
    pub herdr_detect: bool,
    /// Any of lead, coder, reviewer, tester, researcher, specialist.
    pub roles: Vec<Role>,
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
    /// Human-readable name for tables and messages.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Extra launch arguments, appended to `launch` (each shell-quoted).
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Environment variables to start the agent with.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Folder to start the agent in.
    #[serde(default)]
    pub workdir: Option<String>,
    /// Free-form capability tags (e.g. "long-context").
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Most sessions of this kind running at once.
    #[serde(default)]
    pub max_sessions: Option<u32>,
    /// Seconds one execution may take before it counts as stuck.
    #[serde(default)]
    pub timeout_s: Option<u64>,
    /// Minutes the kind is benched after a failure.
    #[serde(default)]
    pub cooldown_min: Option<u64>,
    /// Custom instructions added to every prompt sent to this kind.
    #[serde(default)]
    pub instructions: Option<String>,
    /// Command that prints the tool's version (default `<binary> --version`).
    #[serde(default)]
    pub version_cmd: Option<String>,
    /// Command that prints the tool's help (default `<binary> --help`).
    #[serde(default)]
    pub help_cmd: Option<String>,
    /// Command whose exit status tells whether the account is logged in (0 = yes);
    /// its output is never read. Must be a non-destructive status command of the tool.
    #[serde(default)]
    pub auth_cmd: Option<String>,
    /// A file or folder whose presence means "logged in" (its content is never read).
    /// Used when `auth_cmd` is not set.
    #[serde(default)]
    pub auth_file: Option<String>,
}

fn default_true() -> bool {
    true
}

/// How each key's TOML value must look; checked before deserializing so that a wrong
/// type is reported with the key's name.
const KEY_TYPES: &[(&str, KeyType)] = &[
    ("binary", KeyType::Str),
    ("launch", KeyType::Str),
    ("model_flag", KeyType::Str),
    ("herdr_detect", KeyType::Bool),
    ("roles", KeyType::Roles),
    ("skills_dirs", KeyType::StrList),
    ("commands_dirs", KeyType::StrList),
    ("commands_from", KeyType::Str),
    ("invoke", KeyType::Str),
    ("verified", KeyType::Bool),
    ("notes", KeyType::Str),
    ("display_name", KeyType::Str),
    ("extra_args", KeyType::StrList),
    ("env", KeyType::StrMap),
    ("workdir", KeyType::Str),
    ("capabilities", KeyType::StrList),
    ("max_sessions", KeyType::Count),
    ("timeout_s", KeyType::Count),
    ("cooldown_min", KeyType::Count),
    ("instructions", KeyType::Str),
    ("version_cmd", KeyType::Str),
    ("help_cmd", KeyType::Str),
    ("auth_cmd", KeyType::Str),
    ("auth_file", KeyType::Str),
];

#[derive(Clone, Copy)]
enum KeyType {
    Str,
    Bool,
    StrList,
    StrMap,
    Roles,
    Count,
}

fn check_key(key: &str, ty: KeyType, v: &toml::Value) -> std::result::Result<(), String> {
    let bad = |expected: &str| {
        Err(format!(
            "key `{key}` must be {expected}, found {}",
            v.type_str()
        ))
    };
    match ty {
        KeyType::Str if v.is_str() => Ok(()),
        KeyType::Str => bad("a string"),
        KeyType::Bool if v.is_bool() => Ok(()),
        KeyType::Bool => bad("true or false"),
        KeyType::Count => match v.as_integer() {
            Some(n) if (0..=i64::from(u32::MAX)).contains(&n) => Ok(()),
            Some(_) => Err(format!("key `{key}` must be a non-negative whole number")),
            None => bad("a whole number"),
        },
        KeyType::StrList | KeyType::Roles => match v.as_array() {
            Some(a) if a.iter().all(toml::Value::is_str) => {
                if matches!(ty, KeyType::Roles) {
                    for r in a.iter().filter_map(toml::Value::as_str) {
                        r.parse::<Role>().map_err(|e| format!("key `{key}`: {e}"))?;
                    }
                }
                Ok(())
            }
            _ => bad("a list of strings"),
        },
        KeyType::StrMap => match v.as_table() {
            Some(t) if t.values().all(toml::Value::is_str) => Ok(()),
            _ => bad("a table of strings"),
        },
    }
}

/// The whole registry: agent kind name -> its entry.
#[derive(Debug, Clone, Default)]
pub struct Registry(pub BTreeMap<String, AgentKind>);

impl Registry {
    pub fn get(&self, kind: &str) -> Option<&AgentKind> {
        self.0.get(kind)
    }

    /// The kind names, sorted.
    pub fn kinds(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

impl AgentKind {
    /// Whether this kind's executable is on PATH (the "is this kind installed" check).
    pub fn is_installed(&self) -> bool {
        which(&self.binary).is_some()
    }
}

/// The executable `binary` resolves to on PATH (or itself when it has a slash), like
/// Python's `shutil.which`. Only looks, never runs it.
pub fn which(binary: &str) -> Option<PathBuf> {
    if binary.is_empty() {
        return None;
    }
    if binary.contains('/') {
        let p = PathBuf::from(binary);
        return is_executable(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(binary))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The user's override file: `ORCH_AGENTS_FILE` when set (and not empty), else
/// `~/.config/herdr-orchestra/agents.toml`, as Python's `user_agents_file()` does.
pub fn user_agents_file() -> PathBuf {
    match std::env::var_os("ORCH_AGENTS_FILE") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home_dir()
            .join(".config")
            .join("herdr-orchestra")
            .join("agents.toml"),
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
pub fn registry(
    default_toml: &str,
    user_file: &Path,
) -> std::result::Result<Registry, AgentsError> {
    let mut table = parse(default_toml, "agents.default.toml")?;
    if user_file.exists() {
        let text = read(user_file)?;
        for (kind, vals) in parse(&text, &file_name(user_file))? {
            if let Some(vals) = vals.as_table() {
                let mut merged = table
                    .get(&kind)
                    .and_then(|v| v.as_table().cloned())
                    .unwrap_or_default();
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
        if let Some(t) = vals.as_table() {
            for (key, ty) in KEY_TYPES {
                if let Some(v) = t.get(*key) {
                    check_key(key, *ty, v).map_err(|problem| AgentsError {
                        file: file_name(user_file),
                        problem: format!("agent kind {kind:?}: {problem}"),
                    })?;
                }
            }
        }
        let ak: AgentKind = AgentKind::deserialize(vals).map_err(|e| AgentsError {
            file: file_name(user_file),
            problem: format!("agent kind {kind:?}: {e}"),
        })?;
        kinds.insert(kind, ak);
    }
    Ok(Registry(kinds))
}

fn parse(text: &str, file: &str) -> std::result::Result<toml::Table, AgentsError> {
    text.parse::<toml::Table>()
        .map_err(|e: toml::de::Error| AgentsError {
            file: file.to_string(),
            problem: e.to_string(),
        })
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
        assert_eq!(claude.roles, [Role::Lead, Role::Coder, Role::Reviewer]);
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
        assert_eq!(acme.roles, [Role::Coder]);
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
        assert!(
            e.to_string().contains("agent registry: agents.toml:"),
            "{e}"
        );
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
        let p = user(
            &t,
            "[acme]\nbinary = \"a\"\nlaunch = \"a\"\nroles = [\"boss\"]\n",
        );
        let e = registry(DEFAULT_TOML, &p).unwrap_err();
        assert_eq!(e.file, "agents.toml");
        assert!(e.problem.contains("`roles`"), "{e}");
        assert!(e.problem.contains("unknown role \"boss\""), "{e}");
    }

    #[test]
    fn all_six_roles_are_accepted() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[acme]\nbinary = \"a\"\nlaunch = \"a\"\nroles = [\"lead\", \"coder\", \"reviewer\", \"tester\", \"researcher\", \"specialist\"]\n",
        );
        let reg = registry(DEFAULT_TOML, &p).unwrap();
        assert_eq!(reg.get("acme").unwrap().roles, Role::ALL);
    }

    #[test]
    fn new_optional_keys_load() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            r#"[claude]
display_name = "Claude Code"
extra_args = ["--verbose", "a b"]
env = { FOO = "1" }
workdir = "/work"
capabilities = ["long-context"]
max_sessions = 3
timeout_s = 900
cooldown_min = 15
instructions = "be brief"
version_cmd = "claude -v"
help_cmd = "claude -h"
"#,
        );
        let c = registry(DEFAULT_TOML, &p)
            .unwrap()
            .0
            .remove("claude")
            .unwrap();
        assert_eq!(c.display_name.as_deref(), Some("Claude Code"));
        assert_eq!(c.extra_args, ["--verbose", "a b"]);
        assert_eq!(c.env["FOO"], "1");
        assert_eq!(c.workdir.as_deref(), Some("/work"));
        assert_eq!(c.capabilities, ["long-context"]);
        assert_eq!(c.max_sessions, Some(3));
        assert_eq!(c.timeout_s, Some(900));
        assert_eq!(c.cooldown_min, Some(15));
        assert_eq!(c.instructions.as_deref(), Some("be brief"));
        assert_eq!(c.version_cmd.as_deref(), Some("claude -v"));
        assert_eq!(c.help_cmd.as_deref(), Some("claude -h"));
    }

    #[test]
    fn new_keys_are_absent_for_todays_files() {
        let reg = registry(DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let c = reg.get("claude").unwrap();
        assert!(c.display_name.is_none() && c.extra_args.is_empty() && c.env.is_empty());
        assert!(c.max_sessions.is_none() && c.version_cmd.is_none());
    }

    #[test]
    fn wrong_types_name_the_file_and_the_key() {
        for (line, key) in [
            ("display_name = 3", "display_name"),
            ("extra_args = \"x\"", "extra_args"),
            ("extra_args = [1]", "extra_args"),
            ("env = [\"A=1\"]", "env"),
            ("env = { A = 1 }", "env"),
            ("workdir = 1", "workdir"),
            ("capabilities = \"x\"", "capabilities"),
            ("max_sessions = \"3\"", "max_sessions"),
            ("max_sessions = -1", "max_sessions"),
            ("timeout_s = 1.5", "timeout_s"),
            ("cooldown_min = true", "cooldown_min"),
            ("instructions = 1", "instructions"),
            ("version_cmd = 1", "version_cmd"),
            ("help_cmd = [\"x\"]", "help_cmd"),
            ("herdr_detect = \"yes\"", "herdr_detect"),
            ("roles = \"lead\"", "roles"),
        ] {
            let t = tempfile::tempdir().unwrap();
            let p = user(&t, &format!("[claude]\n{line}\n"));
            let e = registry(DEFAULT_TOML, &p).unwrap_err();
            assert_eq!(e.file, "agents.toml", "{line}");
            assert!(e.problem.contains(&format!("`{key}`")), "{line}: {e}");
            assert!(e.problem.contains("claude"), "{line}: {e}");
        }
    }

    #[test]
    fn herdr_detect_defaults_to_true_like_python() {
        let t = tempfile::tempdir().unwrap();
        let p = user(
            &t,
            "[acme]\nbinary = \"a\"\nlaunch = \"a\"\nroles = [\"coder\"]\n",
        );
        assert!(
            registry(DEFAULT_TOML, &p)
                .unwrap()
                .get("acme")
                .unwrap()
                .herdr_detect
        );
    }

    #[test]
    fn is_installed_looks_on_path_only() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let bin = t.path().join("acme-bin");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(which(bin.to_str().unwrap()), Some(bin.clone()));
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
        assert!(which("").is_none());
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(which(bin.to_str().unwrap()).is_none());
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
        assert_eq!(
            user_agents_file(),
            PathBuf::from("/tmp/from-env/agents.toml")
        );
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
        assert!(user_agents_file().ends_with("herdr-orchestra/agents.toml"));
        unsafe { std::env::set_var("ORCH_AGENTS_FILE", "") };
        assert!(user_agents_file().ends_with("herdr-orchestra/agents.toml"));
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
    }
}
