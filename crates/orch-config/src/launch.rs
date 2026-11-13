//! The one launch-command builder (Python's `launch_cmd`): the registry's `launch`
//! command for a kind, plus its configured extra arguments, on a given model under the
//! deny policy.

use crate::agents::{Registry, user_agents_file};
use crate::models::Catalog;
use orch_core::Error;
use orch_core::error::EXIT_USAGE;

/// Why a launch command could not be built. The message is Python's.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LaunchError(pub String);

impl From<LaunchError> for Error {
    fn from(e: LaunchError) -> Self {
        Error::wrap(e, EXIT_USAGE)
    }
}

/// A built command and the warnings the caller should print (to stderr).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub warnings: Vec<String>,
}

/// Quote one word for a POSIX shell (Python's `shlex.quote`).
pub fn shell_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// Python's `repr` of a plain string, as the messages show it.
pub fn repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', "\\'"))
    }
}

/// The shell command that starts agent `kind` in a pane, on `model` when given.
///
/// * a model the policy denies is refused, naming the matching pattern;
/// * a kind without `model_flag` refuses a model, unless `strict` is false: then the
///   kind starts on its default model (the caller says so);
/// * a model the catalog lists for other kinds only gives a warning;
/// * the kind's `extra_args` follow the launch command, each shell-quoted.
pub fn launch_command(
    registry: &Registry,
    catalog: &Catalog,
    kind: &str,
    model: Option<&str>,
    strict: bool,
) -> Result<Launch, LaunchError> {
    let ak = registry.get(kind).ok_or_else(|| {
        LaunchError(format!(
            "unknown agent kind {} — known: {} (add it in {})",
            repr(kind),
            registry.kinds().join(", "),
            user_agents_file().display()
        ))
    })?;
    let mut command = if ak.launch.is_empty() {
        ak.binary.clone()
    } else {
        ak.launch.clone()
    };
    for a in &ak.extra_args {
        command.push(' ');
        command.push_str(&shell_quote(a));
    }
    let mut warnings = Vec::new();
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        if let Some(pat) = catalog.denied(model) {
            return Err(LaunchError(format!(
                "{} is not allowed by the model policy (pattern {}) — see `orch models`",
                repr(model),
                repr(pat)
            )));
        }
        let Some(flag) = ak.model_flag.as_deref().filter(|f| !f.is_empty()) else {
            if !strict {
                return Ok(Launch { command, warnings });
            }
            return Err(LaunchError(format!(
                "the registry has no `model_flag` for {kind}, so orch can't build the command — choose the \
model inside {kind}, or set model_flag for it in {}",
                user_agents_file().display()
            )));
        };
        if let Some(m) = catalog.models.get(&model.to_lowercase())
            && !m.kinds.is_empty()
            && !m.kinds.iter().any(|k| k == kind)
        {
            warnings.push(format!(
                "WARN: the catalog lists {model} for {}, not {kind}",
                m.kinds.join(", ")
            ));
        }
        command.push_str(&format!(" {flag} {}", shell_quote(model)));
    }
    Ok(Launch { command, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agents, models};

    fn reg(user: &str) -> Registry {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("agents.toml");
        std::fs::write(&p, user).unwrap();
        agents::registry(agents::DEFAULT_TOML, &p).unwrap()
    }

    fn cat(user: &str) -> Catalog {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("models.toml");
        std::fs::write(&p, user).unwrap();
        models::catalog(models::DEFAULT_TOML, &p).unwrap()
    }

    const MODELS: &str = "[policy]\ndeny = [\"bad-*\"]\n\
[models.good-1]\nkinds = [\"claude\"]\ntier = \"high\"\nreasoning = 5\n";

    #[test]
    fn plain_launch_is_the_registry_command() {
        let l = launch_command(&reg(""), &cat(MODELS), "claude", None, true).unwrap();
        assert_eq!(l.command, "claude --permission-mode acceptEdits");
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn a_model_adds_the_flag_and_the_quoted_model() {
        let l = launch_command(&reg(""), &cat(MODELS), "claude", Some("good-1"), true).unwrap();
        assert_eq!(
            l.command,
            "claude --permission-mode acceptEdits --model good-1"
        );
        let l = launch_command(&reg(""), &cat(MODELS), "claude", Some("my model$"), true).unwrap();
        assert!(l.command.ends_with("--model 'my model$'"), "{}", l.command);
    }

    #[test]
    fn a_denied_model_is_refused_naming_the_pattern() {
        let e = launch_command(&reg(""), &cat(MODELS), "claude", Some("BAD-9"), true).unwrap_err();
        assert_eq!(
            e.0,
            "'BAD-9' is not allowed by the model policy (pattern 'bad-*') — see `orch models`"
        );
        // denied wins over a missing model_flag and over strict=false
        assert!(launch_command(&reg(""), &cat(MODELS), "amp", Some("bad-1"), false).is_err());
    }

    #[test]
    fn a_kind_without_model_flag_refuses_a_model_when_strict() {
        let e = launch_command(&reg(""), &cat(MODELS), "amp", Some("good-1"), true).unwrap_err();
        assert!(
            e.0.starts_with(
                "the registry has no `model_flag` for amp, so orch can't build the command"
            ),
            "{e}"
        );
        assert!(e.0.contains("agents.toml"));
    }

    #[test]
    fn a_kind_without_model_flag_uses_its_default_model_when_not_strict() {
        let l = launch_command(&reg(""), &cat(MODELS), "amp", Some("good-1"), false).unwrap();
        assert_eq!(l.command, reg("").get("amp").unwrap().launch);
    }

    #[test]
    fn a_model_listed_for_other_kinds_only_warns() {
        let l = launch_command(&reg(""), &cat(MODELS), "codex", Some("good-1"), true).unwrap();
        assert_eq!(l.command, "codex --approve-for-me --model good-1");
        assert_eq!(
            l.warnings,
            ["WARN: the catalog lists good-1 for claude, not codex"]
        );
        // a model not in the catalog: no warning
        let l = launch_command(&reg(""), &cat(MODELS), "codex", Some("unknown-1"), true).unwrap();
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn extra_args_follow_the_launch_command_before_the_model() {
        let r = reg("[claude]\nextra_args = [\"--verbose\", \"a b\"]\n");
        let l = launch_command(&r, &cat(MODELS), "claude", Some("good-1"), true).unwrap();
        assert_eq!(
            l.command,
            "claude --permission-mode acceptEdits --verbose 'a b' --model good-1"
        );
        let l = launch_command(&r, &cat(MODELS), "claude", None, true).unwrap();
        assert_eq!(
            l.command,
            "claude --permission-mode acceptEdits --verbose 'a b'"
        );
    }

    #[test]
    fn an_unknown_kind_lists_the_known_ones() {
        let e = launch_command(&reg(""), &cat(MODELS), "nope", None, true).unwrap_err();
        assert!(
            e.0.starts_with("unknown agent kind 'nope' — known: agy, amp, claude"),
            "{e}"
        );
        assert!(e.0.contains("(add it in "));
    }

    #[test]
    fn shell_quote_matches_shlex() {
        assert_eq!(shell_quote("abc-1.5/x"), "abc-1.5/x");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
    }
}
