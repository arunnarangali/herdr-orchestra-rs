// The reference the Help screen shows. Commands, exit codes and tunables are the real `orch`
// ones (`orch --help`, crates/orch-core and crates/orch-config/src/tunables.rs), not samples.

export interface CliGroup {
  id: string;
  title: string;
  sub: string;
  rows: [command: string, comment: string][];
}

export const CLI: CliGroup[] = [
  {
    id: "lead",
    title: "Lead & Plan",
    sub: "Plan a feature, sync it and start the run.",
    rows: [
      ["orch lead plan", "# The lead's planning playbook"],
      ["orch plan show", "# The plan in plain words"],
      ["orch sync", "# plan.md -> state (checks deps and cycles)"],
      ["orch start", "# Create the feature branch and worktrees"],
      ["orch dispatch M1 M2", "# Start (or restart) milestones"],
    ],
  },
  {
    id: "control",
    title: "Control",
    sub: "Answer, approve, fix and escalate while the run goes.",
    rows: [
      ['orch answer M2 "text"', "# The developer's answer, back to the coder"],
      ["orch approve M4", "# Let a held milestone land"],
      ['orch fix M2 "text"', "# A follow-up fix for something M2 landed"],
      ["orch escalate M3", "# One more go on a stronger model"],
      ["orch stop", "# Stop the event manager cleanly"],
      ["orch resume", "# After a restart: pick the run up again"],
    ],
  },
  {
    id: "diag",
    title: "Diagnostics",
    sub: "Check the setup, read events and finish a run.",
    rows: [
      ["orch doctor", "# Check installed agents against the real tools"],
      ["orch status", "# Workers and milestones; ALL_DONE or STUCK"],
      ["orch events --peek", "# What the lead was woken for, without reading it"],
      ["orch report", "# Time, rounds, first-pass rate, wasted work"],
      ["orch finish", "# Final check, push and open a pull request"],
    ],
  },
];

/** Exit codes `orch` commands use. */
export const EXIT: [code: number, title: string, text: string][] = [
  [0, "All Done", "Finished; for `orch autopilot`, every milestone merged."],
  [1, "Error", "Something went wrong; the message says what."],
  [2, "Refused", "Wrong use, or a rule refused it (validation, policy, scope)."],
  [3, "Needs You / Cap", "A person is needed, a round limit was hit, or nobody is free (BUSY, NONE)."],
  [4, "Merge Conflict", "The milestone does not merge cleanly."],
  [5, "Not Delivered", "The prompt could not be delivered to the agent's pane."],
  [6, "Check Failed", "The check command failed on the merged result."],
  [7, "Held for Approval", "A risky change waits for `orch approve`."],
];

/** Common tunables and their defaults. */
export const ENV: [name: string, value: string, text: string][] = [
  ["ORCH_CODER_BUDGET_MIN", "45", "Minutes a coder may take before it is checked on"],
  ["ORCH_REVIEW_BUDGET_MIN", "15", "Minutes the reviewer may take"],
  ["ORCH_LEAD_WAIT_MIN", "15", "Minutes to wait for the lead"],
  ["ORCH_CHECK_TIMEOUT_MIN", "30", "Longest the check command may run"],
  ["ORCH_RISK_MAX_LINES", "1500", "Changed lines above which a milestone is held"],
];

export const GUARANTEES: [title: string, text: string][] = [
  ["Single Driver Lease", "Only one event manager drives a run at a time."],
  ["SQLite Transactions", "Plans, runs and events are stored durably and in order."],
  ["Hidden Git Refs (refs/orch/*)", "Parked work is kept in hidden refs, not in extra branches."],
  ["Zero Embedded Terminals", "No terminal inside the app: agents run in your Herdr panes."],
];
