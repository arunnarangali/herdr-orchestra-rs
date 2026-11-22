import type { AgentsResponse, Health } from "../../api/types";
import { herdrRunningText } from "../../app/HerdrSetup";
import type { HerdrInfo } from "../../app/shell";

export type StepState = "checking" | "ok" | "problem" | "unknown";

export interface StepResult {
  state: StepState;
  /** What was found or what is missing, in plain words. */
  found: string;
  /** What to do next. */
  next: string;
  /** Commands to copy. */
  commands: string[];
}

export const checking = (what: string): StepResult => ({ state: "checking", found: what, next: "", commands: [] });

export function orchResult(path: string | null, error: string | null, health: Health | null, hasShell: boolean): StepResult {
  if (path) return { state: "ok", found: `orch found at ${path}.`, next: "Nothing to do.", commands: [] };
  if (hasShell && error) {
    return {
      state: "problem",
      found: error,
      next: "Install orch, or tell the app where it is: set ORCH_BIN to its full path (or put it on your PATH) and open the app again.",
      commands: ["orch --version", "export ORCH_BIN=/full/path/to/orch"],
    };
  }
  if (health) {
    return { state: "ok", found: `A running orch ${health.version} answered, so orch is installed.`, next: "Nothing to do.", commands: [] };
  }
  return {
    state: "unknown",
    found: "A browser cannot look for programs on this computer.",
    next: "In a terminal, check that orch is installed. The desktop app can check this for you.",
    commands: ["orch --version"],
  };
}

/**
 * The Herdr step from the shell's view of Herdr (`null`: no shell). A running Herdr is done,
 * with no install asked for; anything else is a problem whose way out is the Herdr setup panel
 * (install, update, the official site), never a terminal command.
 */
export function herdrResult(info: HerdrInfo | null, hasShell: boolean): StepResult {
  if (info?.state === "running") {
    return { state: "ok", found: herdrRunningText(info), next: "Nothing to do: the app takes care of Herdr.", commands: [] };
  }
  if (hasShell && info) {
    return { state: "problem", found: info.error ?? "Herdr is not running.", next: "", commands: [] };
  }
  return {
    state: "unknown",
    found: "A browser cannot look for programs on this computer.",
    next: "In a terminal, check that Herdr is installed.",
    commands: ["herdr --version"],
  };
}

export function agentsResult(data: AgentsResponse | null, connected: boolean, error: string | null): StepResult {
  if (!connected) {
    return {
      state: "unknown",
      found: "Agents are found by the orch server, which starts once a project is chosen.",
      next: "Choose a project folder (the next step). This check then runs by itself.",
      commands: [],
    };
  }
  if (error) {
    return { state: "problem", found: `The agent list could not be read: ${error}`, next: "Check the connection (see the top of the window), then check again.", commands: ["orch doctor"] };
  }
  if (!data) return checking("Looking for agents…");
  const installed = data.agents.filter((a) => a.installed);
  const missing = data.agents.filter((a) => !a.installed);
  const names = (l: typeof missing) => l.map((a) => a.display_name ?? a.kind).join(", ");
  if (installed.length === 0) {
    return {
      state: "problem",
      found: `0 of ${data.agents.length} agents found. Missing: ${names(missing) || "none known"}.`,
      next: "Install at least one coding-agent CLI and log in to it (one is enough: the lead builds a team from what is installed). Then check again. This app never installs anything.",
      commands: ["orch doctor"],
    };
  }
  const problems = installed.filter((a) => a.login.state !== "authenticated");
  return {
    state: "ok",
    found:
      `${installed.length} of ${data.agents.length} agents found: ${names(installed)}.` +
      (missing.length ? ` Missing: ${names(missing)} (optional).` : "") +
      (problems.length ? ` Not confirmed logged in: ${names(problems)}.` : ""),
    next: problems.length
      ? "Log in to the agents listed as not confirmed, then run `orch doctor` to check. You do not need to install the missing ones."
      : "You do not need to install the missing ones: the lead builds a team from what is installed.",
    commands: problems.length ? ["orch doctor"] : [],
  };
}

export function finalCommands(installedKind: string | null): string[] {
  return [
    "orch sync",
    installedKind ? `orch team ${installedKind}` : "orch team <agent>",
    "orch team up",
  ];
}

/** Are the orch skills (the skill wrappers) in place for every installed agent? From `/agents`. */
export function skillsResult(data: AgentsResponse | null, connected: boolean, error: string | null): StepResult {
  if (!connected) {
    return {
      state: "unknown",
      found: "The skills are checked by the orch server, which starts once a project is chosen.",
      next: "Choose a project folder. This check then runs by itself.",
      commands: [],
    };
  }
  if (error) return { state: "problem", found: `The agent list could not be read: ${error}`, next: "Check the connection, then check again.", commands: ["orch agents check"] };
  if (!data) return checking("Looking at the skills…");
  const installed = data.agents.filter((a) => a.installed && a.skills.status !== "none");
  if (installed.length === 0) {
    return { state: "unknown", found: "No installed agent uses the orch skills yet.", next: "Install an agent first (the step before).", commands: [] };
  }
  const bad = installed.filter((a) => a.skills.status === "missing" || a.skills.status === "outdated");
  if (bad.length === 0) {
    return { state: "ok", found: `The orch skills are installed and current for ${installed.map((a) => a.display_name ?? a.kind).join(", ")}.`, next: "Nothing to do.", commands: [] };
  }
  return {
    state: "problem",
    found: bad
      .map((a) => `Skills ${a.skills.status} for ${a.display_name ?? a.kind}${a.skills.problems.length ? ` (${a.skills.problems.join(", ")})` : ""}`)
      .join(". ") + ".",
    next: "Install them with `orch agents sync` in a terminal (it records what it installs), then check again. The app does not install anything.",
    commands: ["orch agents sync"],
  };
}
