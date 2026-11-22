//! The lead playbooks, shipped inside the binary (§8) so any agent can become the lead
//! without the install: `orch lead plan|run` prints them, and the GUI's Lead launcher hands
//! the planning one to the Lead it starts. The files live in `crates/orch/assets/playbooks/`.

/// The lead planning playbook (Python v4.10.1's `playbooks/plan.md`).
pub const PLAN: &str = include_str!("../../orch/assets/playbooks/plan.md");
/// The lead run playbook (Python v4.10.1's `playbooks/run.md`).
pub const RUN: &str = include_str!("../../orch/assets/playbooks/run.md");

/// The playbook `which` (`plan` or `run`) as `orch lead <which>` prints it: a header line
/// with this build's version, a blank line, then the playbook. `None` for any other name.
pub fn render(which: &str) -> Option<String> {
    let pb = match which {
        "plan" => PLAN,
        "run" => RUN,
        _ => return None,
    };
    Some(format!(
        "herdr-orchestra {} — lead playbook: {which}\n\n{pb}",
        env!("CARGO_PKG_VERSION")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_puts_the_version_header_before_the_playbook() {
        let text = render("plan").unwrap();
        assert!(text.starts_with(&format!(
            "herdr-orchestra {} — lead playbook: plan\n\nYou are the LEAD",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(text.ends_with(PLAN));
        assert!(render("run").unwrap().ends_with(RUN));
        assert_eq!(render("other"), None);
    }
}
