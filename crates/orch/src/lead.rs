//! `orch lead plan|run` and `orch lead whoami`, as Python v4.10.1's `cmd_lead`.
//!
//! The playbooks are shipped inside the binary (`crates/orch/assets/playbooks/`), so any
//! agent can become the lead (§8) without the install; only the version line comes from
//! this build. `lead whoami` reaches herdr through the [`RuntimeAdapter`] like every other
//! command; the unit tests use the in-memory fake.

use crate::listing::die;
use orch_config::tunables::Tunables;
use orch_herdr::cli::CliHerdr;
use orch_herdr::error::HerdrError;
use orch_herdr::RuntimeAdapter;

/// The lead planning playbook (Python v4.10.1's `playbooks/plan.md`).
const PLAYBOOK_PLAN: &str = include_str!("../assets/playbooks/plan.md");
/// The lead run playbook (Python v4.10.1's `playbooks/run.md`).
const PLAYBOOK_RUN: &str = include_str!("../assets/playbooks/run.md");

/// `orch lead plan` / `orch lead run`: the header line with this build's version, then the
/// playbook text. Extra arguments are ignored, as in Python.
pub fn playbook(which: &str) -> i32 {
    let pb = match which {
        "plan" => PLAYBOOK_PLAN,
        "run" => PLAYBOOK_RUN,
        _ => return die("usage: orch lead plan | run | check | whoami"),
    };
    println!(
        "herdr-orchestra {} — lead playbook: {which}\n",
        env!("CARGO_PKG_VERSION")
    );
    print!("{pb}");
    0
}

/// `orch lead whoami`: the pane this agent runs in, and the command that names it `lead`.
pub fn whoami() -> i32 {
    let pane_env = std::env::var("HERDR_PANE_ID").ok().filter(|p| !p.is_empty());
    match whoami_text(&herdr(), pane_env) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(msg) => die(msg),
    }
}

/// The backend every herdr call goes through (the same construction as `lead check`).
fn herdr() -> CliHerdr {
    match Tunables::from_env() {
        Ok(t) => CliHerdr::from_tunables(&t),
        Err(_) => CliHerdr::new("herdr", 30),
    }
}

/// The stdout of `orch lead whoami`, or the message Python's `die` prints (exit 2).
/// `pane_env` is the shell's `HERDR_PANE_ID`, when it is set to something non-empty.
pub fn whoami_text(
    runtime: &dyn RuntimeAdapter,
    pane_env: Option<String>,
) -> Result<String, String> {
    let pane = match runtime.current_pane() {
        Ok(p) => p,
        Err(HerdrError::StalePane { message, .. }) => {
            // Python's die for a failing `herdr pane current`: this shell is not in a pane.
            return Err(format!(
                "not inside a herdr pane (`herdr pane current` failed) — start this agent from a Herdr pane: {message}"
            ));
        }
        Err(e) => return Err(format!("herdr pane current failed: {e}")),
    };
    let mut out = String::new();
    if pane_env.is_none() {
        out.push_str(
            "NOTE: HERDR_PANE_ID is not set in this shell, so herdr reported the FOCUSED pane. \
Check it is the pane you are running in before renaming it.\n",
        );
    }
    let id = &pane.pane_id;
    out.push_str(&format!("PANE {id} agent_status={}\n", pane.status));
    out.push_str(&format!(
        "This pane is YOU (the lead). Name it:  herdr agent rename {id} lead\n\
Write `Lead: lead (<your kind>)` in the plan header, and never prompt this pane.\n"
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_herdr::fake::FakeHerdr;
    use orch_herdr::model::{AgentInfo, PaneInfo};
    use orch_herdr::status::AgentStatus;

    fn pane(id: &str) -> PaneInfo {
        let mut p = PaneInfo::new(id, "w1", "w1:t1");
        p.status = AgentStatus::Idle;
        p
    }

    fn fake_with_current_pane() -> FakeHerdr {
        let f = FakeHerdr::new();
        f.add_pane(pane("w1:p1"));
        f.set_current_pane(Some("w1:p1"));
        f
    }

    // AC12: the pane comes from the adapter, and the output names it `lead`.
    #[test]
    fn whoami_reports_the_current_pane_and_the_rename_command() {
        let out = whoami_text(&fake_with_current_pane(), Some("w1:p1".into())).unwrap();
        assert_eq!(
            out,
            "PANE w1:p1 agent_status=idle\n\
This pane is YOU (the lead). Name it:  herdr agent rename w1:p1 lead\n\
Write `Lead: lead (<your kind>)` in the plan header, and never prompt this pane.\n"
        );
    }

    // Without HERDR_PANE_ID the focused-pane note comes first.
    #[test]
    fn without_pane_env_the_focused_pane_note_is_printed() {
        let out = whoami_text(&fake_with_current_pane(), None).unwrap();
        assert!(out.starts_with("NOTE: HERDR_PANE_ID is not set in this shell"));
        assert!(out.contains("PANE w1:p1 agent_status=idle"));
    }

    // Outside a herdr pane: Python's message and exit code 2.
    #[test]
    fn outside_a_herdr_pane_it_fails_like_python() {
        let e = whoami_text(&FakeHerdr::new(), Some("w1:p9".into())).unwrap_err();
        assert!(e.starts_with("not inside a herdr pane (`herdr pane current` failed)"), "{e}");
        assert!(e.contains("start this agent from a Herdr pane"));
    }

    #[test]
    fn an_unreachable_herdr_reports_the_call_failure() {
        let f = FakeHerdr::new();
        f.fail_next(
            orch_herdr::fake::CallKind::PaneCurrent,
            1,
            HerdrError::Unavailable { message: "down".into() },
        );
        let e = whoami_text(&f, None).unwrap_err();
        assert!(e.starts_with("herdr pane current failed: "), "{e}");
    }

    #[test]
    fn an_agentless_pane_still_names_itself() {
        let f = FakeHerdr::new();
        f.add_pane(pane("w1:p2"));
        f.set_current_pane(Some("w1:p2"));
        f.add_agent(AgentInfo::new("codex-1", "codex", "w1:p3", AgentStatus::Working));
        let out = whoami_text(&f, None).unwrap();
        assert!(out.contains("PANE w1:p2 agent_status=idle"));
    }

    #[test]
    fn the_playbooks_are_shipped_whole() {
        assert!(PLAYBOOK_PLAN.starts_with("You are the LEAD of a Herdr agent team"));
        assert!(PLAYBOOK_PLAN.ends_with("Do not dispatch before they say so.\n"));
        assert_eq!(PLAYBOOK_PLAN.lines().count(), 177);
        assert!(PLAYBOOK_RUN.starts_with("You are the LEAD, whatever agent you are"));
        assert!(PLAYBOOK_RUN.ends_with("tell the developer to run `orch autopilot` again.\n"));
        assert_eq!(PLAYBOOK_RUN.lines().count(), 61);
    }
}
