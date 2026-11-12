//! The `ORCH_*` tunables with today's defaults (Python `bin/orch`, Appendix A), read from
//! a variable lookup so tests can pass values in instead of racing on the environment.

use orch_core::error::EXIT_USAGE;
use orch_core::{Error, Result};
use std::path::PathBuf;

/// Fixed limit: a milestone gets at most 3 fix rounds.
pub const MAX_FIX: u32 = 3;
/// Fixed limit: at most 2 review rounds.
pub const MAX_REVIEW: u32 = 2;
/// Fixed limit: at most 2 conflict rounds.
pub const MAX_CONFLICT: u32 = 2;

#[derive(Debug, thiserror::Error)]
#[error("{name}: {problem}")]
pub struct TunablesError {
    /// The `ORCH_*` (or `HERDR`) variable that was wrong.
    pub name: String,
    /// What is wrong with its value.
    pub problem: String,
}

impl From<TunablesError> for Error {
    fn from(e: TunablesError) -> Self {
        Error::wrap(e, EXIT_USAGE)
    }
}

/// Every tunable in Appendix A ("Tunables (environment)"), with today's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Tunables {
    pub coder_budget_min: i64,
    pub review_budget_min: i64,
    pub check_timeout_min: i64,
    pub check_timeout_s: i64,
    pub cooldown_min: i64,
    pub gone_cooldown_min: i64,
    pub idle_grace_s: i64,
    pub poll_step_s: f64,
    pub herdr_timeout_s: i64,
    pub send_retry_s: i64,
    pub confirm_s: i64,
    pub max_lost: i64,
    pub unreachable_after: i64,
    pub unreachable_min: i64,
    pub risk_max_lines: i64,
    pub max_tasks: i64,
    pub lead_wait_s: i64,
    pub lead_rewake_s: i64,
    pub lead_max_wakes: i64,
    pub lead_idle_max_s: i64,
    pub no_autostart: bool,
    pub team_wait_s: f64,
    pub agents_file: Option<PathBuf>,
    pub models_file: Option<PathBuf>,
    pub repo: Option<String>,
    pub version: Option<String>,
    pub fault: Option<String>,
    pub herdr: String,
}

impl Tunables {
    /// Read the environment (`ORCH_*`, `HERDR`), with today's defaults.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok()).map_err(Error::from)
    }

    /// Read tunables from any variable lookup, so tests (and callers with their own
    /// precedence rules) can pass values in.
    pub fn from_lookup(mut get: impl FnMut(&str) -> Option<String>) -> std::result::Result<Self, TunablesError> {
        let check_timeout_min = int(&mut get, "ORCH_CHECK_TIMEOUT_MIN", 30)?;
        let lead_wait_min = int(&mut get, "ORCH_LEAD_WAIT_MIN", 15)?;
        let lead_idle_max_min = int(&mut get, "ORCH_LEAD_IDLE_MAX_MIN", 720)?;
        let agents_file = nonempty(&mut get, "ORCH_AGENTS_FILE").map(PathBuf::from);
        let models_file = nonempty(&mut get, "ORCH_MODELS_FILE").map(PathBuf::from);
        Ok(Self {
            coder_budget_min: int(&mut get, "ORCH_CODER_BUDGET_MIN", 45)?,
            review_budget_min: int(&mut get, "ORCH_REVIEW_BUDGET_MIN", 15)?,
            check_timeout_min,
            check_timeout_s: check_timeout_min * 60,
            cooldown_min: int(&mut get, "ORCH_COOLDOWN_MIN", 60)?,
            gone_cooldown_min: int(&mut get, "ORCH_GONE_COOLDOWN_MIN", 10)?,
            idle_grace_s: int(&mut get, "ORCH_IDLE_GRACE_S", 120)?,
            poll_step_s: float(&mut get, "ORCH_POLL_STEP_S", 10.0)?,
            herdr_timeout_s: int(&mut get, "ORCH_HERDR_TIMEOUT_S", 30)?,
            send_retry_s: int(&mut get, "ORCH_SEND_RETRY_S", 120)?,
            confirm_s: int(&mut get, "ORCH_CONFIRM_S", 30)?,
            max_lost: int(&mut get, "ORCH_MAX_LOST", 4)?,
            unreachable_after: int(&mut get, "ORCH_UNREACHABLE_AFTER", 3)?,
            unreachable_min: int(&mut get, "ORCH_UNREACHABLE_MIN", 30)?,
            risk_max_lines: int(&mut get, "ORCH_RISK_MAX_LINES", 1500)?,
            max_tasks: int(&mut get, "ORCH_MAX_TASKS", 0)?,
            lead_wait_s: lead_wait_min * 60,
            lead_rewake_s: int(&mut get, "ORCH_LEAD_REWAKE_S", 300)?,
            lead_max_wakes: int(&mut get, "ORCH_LEAD_MAX_WAKES", 3)?,
            lead_idle_max_s: lead_idle_max_min * 60,
            no_autostart: get("ORCH_NO_AUTOSTART").is_some_and(|v| v == "1"),
            team_wait_s: float(&mut get, "ORCH_TEAM_WAIT_S", 90.0)?,
            agents_file,
            models_file,
            repo: nonempty(&mut get, "ORCH_REPO"),
            version: nonempty(&mut get, "ORCH_VERSION"),
            fault: get("ORCH_FAULT"),
            herdr: get("HERDR").unwrap_or_else(|| "herdr".to_string()),
        })
    }
}

/// `ORCH_MAX_TASKS` is the only tunable that treats an empty value as its default
/// (Python: `int(os.environ.get("ORCH_MAX_TASKS", "0") or 0)`).
fn int(get: &mut impl FnMut(&str) -> Option<String>, name: &str, default: i64) -> std::result::Result<i64, TunablesError> {
    let Some(raw) = get(name) else { return Ok(default) };
    let raw = raw.trim();
    if raw.is_empty() && name == "ORCH_MAX_TASKS" {
        return Ok(0);
    }
    raw.parse().map_err(|_| TunablesError {
        name: name.to_string(),
        problem: format!("{raw:?} is not an integer"),
    })
}

fn float(get: &mut impl FnMut(&str) -> Option<String>, name: &str, default: f64) -> std::result::Result<f64, TunablesError> {
    let Some(raw) = get(name) else { return Ok(default) };
    let raw = raw.trim();
    raw.parse().map_err(|_| TunablesError {
        name: name.to_string(),
        problem: format!("{raw:?} is not a number"),
    })
}

fn nonempty(get: &mut impl FnMut(&str) -> Option<String>, name: &str) -> Option<String> {
    get(name).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn from_map(map: &[(&str, &str)]) -> std::result::Result<Tunables, TunablesError> {
        let m: HashMap<String, String> =
            map.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Tunables::from_lookup(|name| m.get(name).cloned())
    }

    fn nothing(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_are_todays() {
        let t = Tunables::from_lookup(nothing).unwrap();
        assert_eq!(t.coder_budget_min, 45);
        assert_eq!(t.review_budget_min, 15);
        assert_eq!(t.check_timeout_min, 30);
        assert_eq!(t.check_timeout_s, 1800);
        assert_eq!(t.cooldown_min, 60);
        assert_eq!(t.gone_cooldown_min, 10);
        assert_eq!(t.idle_grace_s, 120);
        assert_eq!(t.poll_step_s, 10.0);
        assert_eq!(t.herdr_timeout_s, 30);
        assert_eq!(t.send_retry_s, 120);
        assert_eq!(t.confirm_s, 30);
        assert_eq!(t.max_lost, 4);
        assert_eq!(t.unreachable_after, 3);
        assert_eq!(t.unreachable_min, 30);
        assert_eq!(t.risk_max_lines, 1500);
        assert_eq!(t.max_tasks, 0);
        assert_eq!(t.lead_wait_s, 900);
        assert_eq!(t.lead_rewake_s, 300);
        assert_eq!(t.lead_max_wakes, 3);
        assert_eq!(t.lead_idle_max_s, 43200);
        assert!(!t.no_autostart);
        assert_eq!(t.team_wait_s, 90.0);
        assert!(t.agents_file.is_none());
        assert!(t.models_file.is_none());
        assert!(t.repo.is_none());
        assert!(t.version.is_none());
        assert!(t.fault.is_none());
        assert_eq!(t.herdr, "herdr");
    }

    #[test]
    fn fixed_limits() {
        assert_eq!((MAX_FIX, MAX_REVIEW, MAX_CONFLICT), (3, 2, 2));
    }

    #[test]
    fn values_come_from_their_variables() {
        let t = from_map(&[
            ("ORCH_CODER_BUDGET_MIN", "20"),
            ("ORCH_REVIEW_BUDGET_MIN", "5"),
            ("ORCH_CHECK_TIMEOUT_MIN", "10"),
            ("ORCH_COOLDOWN_MIN", "1"),
            ("ORCH_GONE_COOLDOWN_MIN", "2"),
            ("ORCH_IDLE_GRACE_S", "60"),
            ("ORCH_POLL_STEP_S", "2.5"),
            ("ORCH_HERDR_TIMEOUT_S", "15"),
            ("ORCH_SEND_RETRY_S", "10"),
            ("ORCH_CONFIRM_S", "5"),
            ("ORCH_MAX_LOST", "1"),
            ("ORCH_UNREACHABLE_AFTER", "2"),
            ("ORCH_UNREACHABLE_MIN", "20"),
            ("ORCH_RISK_MAX_LINES", "100"),
            ("ORCH_MAX_TASKS", "3"),
            ("ORCH_LEAD_WAIT_MIN", "1"),
            ("ORCH_LEAD_REWAKE_S", "10"),
            ("ORCH_LEAD_MAX_WAKES", "2"),
            ("ORCH_LEAD_IDLE_MAX_MIN", "30"),
            ("ORCH_NO_AUTOSTART", "1"),
            ("ORCH_TEAM_WAIT_S", "5"),
            ("ORCH_AGENTS_FILE", "/tmp/agents.toml"),
            ("ORCH_MODELS_FILE", "/tmp/models.toml"),
            ("ORCH_REPO", "/repo"),
            ("ORCH_VERSION", "4.10.1"),
            ("ORCH_FAULT", "after-load"),
            ("HERDR", "/opt/herdr"),
        ])
        .unwrap();
        assert_eq!(t.coder_budget_min, 20);
        assert_eq!(t.check_timeout_s, 600);
        assert_eq!(t.lead_wait_s, 60);
        assert_eq!(t.lead_idle_max_s, 1800);
        assert_eq!(t.poll_step_s, 2.5);
        assert_eq!(t.max_tasks, 3);
        assert!(t.no_autostart);
        assert_eq!(t.team_wait_s, 5.0);
        assert_eq!(t.agents_file, Some(PathBuf::from("/tmp/agents.toml")));
        assert_eq!(t.models_file, Some(PathBuf::from("/tmp/models.toml")));
        assert_eq!(t.repo.as_deref(), Some("/repo"));
        assert_eq!(t.version.as_deref(), Some("4.10.1"));
        assert_eq!(t.fault.as_deref(), Some("after-load"));
        assert_eq!(t.herdr, "/opt/herdr");
    }

    #[test]
    fn non_integer_names_the_variable() {
        let e = from_map(&[("ORCH_MAX_LOST", "many")]).unwrap_err();
        assert_eq!(e.name, "ORCH_MAX_LOST");
        assert!(e.to_string().contains("ORCH_MAX_LOST"), "{e}");
    }

    #[test]
    fn non_float_names_the_variable() {
        let e = from_map(&[("ORCH_POLL_STEP_S", "soon")]).unwrap_err();
        assert_eq!(e.name, "ORCH_POLL_STEP_S");
        assert!(e.problem.contains("soon"), "{e}");
    }

    #[test]
    fn empty_max_tasks_means_no_limit_like_python() {
        let t = from_map(&[("ORCH_MAX_TASKS", "")]).unwrap();
        assert_eq!(t.max_tasks, 0);
        let e = from_map(&[("ORCH_IDLE_GRACE_S", "")]).unwrap_err();
        assert_eq!(e.name, "ORCH_IDLE_GRACE_S");
    }

    #[test]
    fn other_no_autostart_values_are_off() {
        assert!(!from_map(&[("ORCH_NO_AUTOSTART", "yes")]).unwrap().no_autostart);
        assert!(!from_map(&[("ORCH_NO_AUTOSTART", "0")]).unwrap().no_autostart);
    }

    #[test]
    fn from_env_reads_the_environment() {
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();
        unsafe { std::env::set_var("ORCH_CODER_BUDGET_MIN", "33") };
        let t = Tunables::from_env().unwrap();
        assert_eq!(t.coder_budget_min, 33);
        unsafe { std::env::remove_var("ORCH_CODER_BUDGET_MIN") };
        let t = Tunables::from_env().unwrap();
        assert_eq!(t.coder_budget_min, 45);
    }
}
