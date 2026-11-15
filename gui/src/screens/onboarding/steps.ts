import type { AgentsResponse, Health } from "../../api/types";

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

export function herdrResult(path: string | null, error: string | null, hasShell: boolean): StepResult {
  if (path) {
    return {
      state: "ok",
      found: `Herdr found at ${path}.`,
      next: "Make sure Herdr is running when you open a lead pane.",
      commands: ["herdr --version"],
    };
  }
  if (hasShell && error) {
    return {
      state: "problem",
      found: error,
      next: "Orchestra uses your existing Herdr to open the lead and coder panes. Install Herdr following its own documentation, or set HERDR_BIN to its full path, then check again. This app never installs it.",
      commands: ["herdr --version", "export HERDR_BIN=/full/path/to/herdr"],
    };
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
