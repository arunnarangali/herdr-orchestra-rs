import type { PlanHeader } from "../../api/types";
import type { IconName } from "../../ui/Icon";

/** One row of a settings section, by where its value lives (and so whether the app can change it). */
export type Field =
  /** The app's own settings, saved by the desktop shell. */
  | { kind: "app"; id: "theme" | "notifications" | "orch" }
  /** A line of the plan header: read-only here, changed in plan.md. */
  | { kind: "plan"; key: Exclude<keyof PlanHeader, "extra">; label: string; desc: string; icon: IconName; code?: boolean }
  /** An `ORCH_*` tunable: read from orch's environment, shown with its default. */
  | { kind: "env"; name: string; def: string; unit?: string; label: string; desc: string; icon: IconName }
  /** A fact about how orch works, not a setting. */
  | { kind: "fact"; label: string; desc: string; icon: IconName; value: string }
  /** The fixed round limits. */
  | { kind: "caps" }
  /** The agent kinds `/agents` reports. */
  | { kind: "agents" }
  /** The tiers `/models` reports, and its deny policy. */
  | { kind: "tiers" }
  /** Plan header lines without a field of their own. */
  | { kind: "extra" }
  /** A setting the app cannot save yet: shown, disabled, with the reason. */
  | { kind: "none"; label: string; desc: string; icon: IconName; control: "switch" | "select" | "text" | "number" | "area" };

export interface Section {
  id: string;
  label: string;
  icon: IconName;
  title: string;
  desc: string;
  fields: Field[];
}

const off = (label: string, desc: string, icon: IconName, control: Extract<Field, { kind: "none" }>["control"]): Field => ({ kind: "none", label, desc, icon, control });

/** The prototype's sections, in its order. */
export const SECTIONS: Section[] = [
  {
    id: "general",
    label: "General",
    icon: "gear",
    title: "General",
    desc: "Appearance and everyday behaviour of the app.",
    fields: [
      { kind: "app", id: "theme" },
      off("Interface Density", "Spacing used in tables and lists.", "grid", "select"),
      off("Timestamps", "Time zone used for events, logs and reports.", "activity", "select"),
      off("Startup Page", "Screen opened when the app starts.", "grid", "select"),
      off("Reduce Motion", "Skip pulsing indicators and transitions in this app.", "pulse", "switch"),
    ],
  },
  {
    id: "runtime",
    label: "Herdr Runtime",
    icon: "keyboard",
    title: "Herdr Runtime",
    desc: "The programs the app starts and how orch talks to Herdr.",
    fields: [
      { kind: "app", id: "orch" },
      { kind: "env", name: "ORCH_HERDR_TIMEOUT_S", def: "30", unit: "seconds", label: "Herdr Timeout", desc: "How long orch waits for an answer from Herdr.", icon: "activity" },
      off("Herdr Socket", "Unix socket the orchestrator uses to control panes.", "keyboard", "text"),
      off("Pane Layout", "How new agent panes are arranged in a workspace.", "grid", "select"),
    ],
  },
  {
    id: "providers",
    label: "Agent Providers",
    icon: "robot",
    title: "Agent Providers",
    desc: "The agent CLIs orch knows, and which of them are installed.",
    fields: [{ kind: "agents" }],
  },
  {
    id: "roles",
    label: "Agent Roles",
    icon: "robot",
    title: "Agent Roles",
    desc: "Who fills each role in this run.",
    fields: [
      { kind: "plan", key: "lead", label: "Lead Agent", desc: "Plans, answers questions and relays decisions.", icon: "robot" },
      { kind: "plan", key: "workers", label: "Coders", desc: "The workers that build milestones.", icon: "robot" },
      { kind: "plan", key: "reviewer", label: "Reviewer", desc: "Reviews each milestone diff before merge.", icon: "check" },
    ],
  },
  {
    id: "models",
    label: "Models & Policy",
    icon: "cube",
    title: "Models & Policy",
    desc: "Model tiers and the deny policy, as the catalog reports them.",
    fields: [{ kind: "tiers" }, off("Fallback Model", "Used when the planned model is unavailable.", "robot", "select")],
  },
  {
    id: "lead",
    label: "Lead Mode",
    icon: "bolt",
    title: "Lead Mode",
    desc: "How much the lead may decide without asking you.",
    fields: [
      { kind: "plan", key: "lead_mode", label: "Lead Mode", desc: "How the lead runs: from the plan header.", icon: "bolt" },
      { kind: "env", name: "ORCH_LEAD_WAIT_MIN", def: "15", unit: "minutes", label: "Lead Wait", desc: "How long to wait for the lead before acting.", icon: "activity" },
      off("Auto-Approve Holds", "Release risk holds without asking. Not recommended for migrations.", "warning", "switch"),
    ],
  },
  {
    id: "scheduling",
    label: "Scheduling",
    icon: "activity",
    title: "Scheduling",
    desc: "How long work may take.",
    fields: [
      { kind: "plan", key: "budget", label: "Run Time Budget", desc: "From the plan header.", icon: "activity" },
      { kind: "env", name: "ORCH_CODER_BUDGET_MIN", def: "45", unit: "minutes", label: "Coder Budget", desc: "Minutes a coder may take before it is checked on.", icon: "activity" },
      { kind: "env", name: "ORCH_REVIEW_BUDGET_MIN", def: "15", unit: "minutes", label: "Review Budget", desc: "Minutes the reviewer may take.", icon: "activity" },
      off("Run Window", "Only dispatch new milestones inside a time window.", "activity", "switch"),
    ],
  },
  {
    id: "parallel",
    label: "Parallel Execution",
    icon: "grid",
    title: "Parallel Execution, Isolation & Risk Hold",
    desc: "How agents run in parallel, workspace isolation, scope enforcement and when to hold risky changes.",
    fields: [
      { kind: "plan", key: "isolation", label: "Isolation Mode", desc: "worktree: each coder works in its own folder.", icon: "folder" },
      { kind: "plan", key: "max_parallel", label: "Max Parallel Coders", desc: "Coders working at the same time.", icon: "robot" },
      { kind: "plan", key: "scope", label: "Scope Enforcement", desc: "What happens when a coder changes files outside the milestone's list.", icon: "warning" },
      { kind: "plan", key: "risk_hold", label: "Risk Hold Gate", desc: "Risky milestones wait for your approval.", icon: "warning" },
      { kind: "env", name: "ORCH_RISK_MAX_LINES", def: "1500", unit: "lines", label: "Line Change Limit", desc: "Changed lines above which a milestone is held.", icon: "warning" },
      { kind: "caps" },
    ],
  },
  {
    id: "git",
    label: "Git & Worktrees",
    icon: "branch",
    title: "Git & Worktrees",
    desc: "Branching and commit conventions.",
    fields: [
      { kind: "plan", key: "branch", label: "Feature Branch", desc: "The branch the run lands on.", icon: "branch", code: true },
      { kind: "plan", key: "base", label: "Base Branch", desc: "Where the feature branch starts and merges back.", icon: "branch", code: true },
      { kind: "plan", key: "commits", label: "Commit Style", desc: "How milestone work lands on the feature branch.", icon: "branch" },
      off("Clean Up Worktrees", "Remove milestone worktrees after they merge.", "folder", "switch"),
      off("Push on Finish", "Push the feature branch when a run finishes.", "external", "switch"),
    ],
  },
  {
    id: "verification",
    label: "Verification",
    icon: "check",
    title: "Verification",
    desc: "The check that must pass before a milestone moves to review.",
    fields: [
      { kind: "plan", key: "check_command", label: "Check Command", desc: "Must pass before a milestone is reviewed.", icon: "keyboard", code: true },
      { kind: "plan", key: "setup_command", label: "Setup Command", desc: "Prepares a fresh worktree.", icon: "keyboard", code: true },
      { kind: "plan", key: "setup_inputs", label: "Setup Inputs", desc: "Files whose change means setup runs again.", icon: "folder", code: true },
      { kind: "env", name: "ORCH_CHECK_TIMEOUT_MIN", def: "30", unit: "minutes", label: "Check Timeout", desc: "The longest the check may run.", icon: "activity" },
    ],
  },
  {
    id: "review",
    label: "Review",
    icon: "chat",
    title: "Review",
    desc: "Who reviews, and for how long.",
    fields: [
      { kind: "plan", key: "reviewer", label: "Reviewer", desc: "From the plan header; none means no review.", icon: "chat" },
      { kind: "env", name: "ORCH_REVIEW_BUDGET_MIN", def: "15", unit: "minutes", label: "Review Budget", desc: "Minutes the reviewer may take.", icon: "activity" },
      off("Review Strictness", "Blocking issues stop a merge. Suggestions never do.", "check", "select"),
    ],
  },
  {
    id: "risk",
    label: "Risk Hold & Scope",
    icon: "warning",
    title: "Risk Hold & Scope",
    desc: "Which changes are held for approval or flagged as out of scope.",
    fields: [
      { kind: "plan", key: "risk_hold", label: "Risk Hold Gate", desc: "Risky milestones wait for your approval.", icon: "warning" },
      { kind: "plan", key: "scope", label: "Scope Enforcement", desc: "off, warn or block.", icon: "warning" },
      { kind: "env", name: "ORCH_RISK_MAX_LINES", def: "1500", unit: "lines", label: "Line Change Limit", desc: "Changed lines above which a milestone is held.", icon: "warning" },
      off("Protected Paths", "One glob per line. Changes to these paths are held for approval.", "warning", "area"),
    ],
  },
  {
    id: "recovery",
    label: "Recovery & Retries",
    icon: "sync",
    title: "Recovery & Retries",
    desc: "Bounded retry limits and crash recovery.",
    fields: [
      { kind: "caps" },
      { kind: "env", name: "ORCH_MAX_LOST", def: "4", label: "Lost Attempts", desc: "Lost attempts before a milestone is parked.", icon: "sync" },
      off("Auto-Resume After Crash", "Resume an interrupted run by itself (today: run `orch resume`).", "sync", "switch"),
    ],
  },
  {
    id: "notifications",
    label: "Notifications",
    icon: "bell",
    title: "Notifications",
    desc: "Which events reach you outside the app.",
    fields: [{ kind: "app", id: "notifications" }, off("Play Sound", "Play a short sound with notifications.", "bell", "switch")],
  },
  {
    id: "storage",
    label: "Storage",
    icon: "cube",
    title: "Storage",
    desc: "Where orchestrator data lives and how long it is kept.",
    fields: [
      { kind: "fact", label: "State Directory", desc: "Plans, events and state live in the project's own folder.", icon: "folder", value: ".orchestrate/" },
      off("Log Retention", "Captured agent output older than this is deleted.", "activity", "number"),
    ],
  },
  {
    id: "toml",
    label: "Advanced TOML",
    icon: "keyboard",
    title: "Advanced TOML",
    desc: "Other plan header lines, and the raw configuration.",
    fields: [{ kind: "extra" }, off("config.toml", "The app does not read or write a configuration file.", "keyboard", "area")],
  },
];
