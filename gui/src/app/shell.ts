import { invoke } from "@tauri-apps/api/core";
import type { Credentials } from "../api/client";
import type { AgentCli, ProjectSettings, SaveProjectSettingsInput } from "../api/types";

/** Where the app's Herdr stands (`running`, `missing`, `too_old` or `failed`). */
export type HerdrState = "running" | "missing" | "too_old" | "failed";
/** A fix the app can offer: Herdr's official installer, or Herdr's own updater. */
export type HerdrFixAction = "install" | "update";
/** The app's Herdr, as the shell sees it after making sure its server runs. */
export interface HerdrInfo {
  state: HerdrState;
  installed: boolean;
  path: string | null;
  version: string | null;
  running: boolean;
  /** The app started this server and stops it when it closes. */
  managed: boolean;
  too_old: boolean;
  /** What went wrong and what to do, in plain words. */
  error: string | null;
  /** The fix to offer, if one helps. */
  fix: HerdrFixAction | null;
}
/** A fix ready to run: exactly what will run, shown before the developer's click. */
export interface HerdrFixPlan {
  action: HerdrFixAction;
  /** The command as it would be typed. */
  command: string;
  /** For the install: the official installer's full text, which is what runs. */
  script: string | null;
  /** Hand back to `runHerdrFix` to run this plan, and only this one. */
  token: string;
}

/**
 * The desktop shell's commands (`gui/src-tauri`). The shell starts `orch serve`, finds its
 * token and hands the page only `{url, token}`; it has no other knowledge of the run. In a
 * plain browser there is no shell and `detectShell()` returns null.
 */
export interface Shell {
  /** Ask the user for a project folder; null when they cancel. */
  chooseProjectFolder(): Promise<string | null>;
  /** Start `orch serve` in `project` (replacing a running one) and return how to reach it. */
  startServer(project: string): Promise<Credentials>;
  /** Stop one project's server, or every server when no project is named. */
  stopServer(project?: string): Promise<void>;
  /** A running server's address and token (the project's, else the first), or null. */
  getConnection(project?: string): Promise<Credentials | null>;
  /** Path of the `orch` binary (`ORCH_BIN`, else the PATH); rejects with a clear message. */
  findOrch(): Promise<string>;
  /** Path of the Herdr the app runs on, once its server runs; rejects with what is wrong. */
  findHerdr(): Promise<string>;
  /** How Herdr stands; its server is started first when Herdr is installed and not running. */
  getHerdrInfo?(): Promise<HerdrInfo>;
  /** Prepare Herdr's install or update for the developer to read; nothing runs yet. */
  prepareHerdrFix?(action: HerdrFixAction): Promise<HerdrFixPlan>;
  /** Run the plan the developer read and chose; rejects with what the installer printed. */
  runHerdrFix?(token: string): Promise<HerdrInfo>;
  /** Open Herdr's official site (herdr.dev) in the default browser. */
  openHerdrUrl?(): Promise<void>;
  /** Detect installed agent CLIs on this machine from orch's registry. */
  detectAgentClis(): Promise<AgentCli[]>;
  /** Read a project's saved settings (lead agent and folder) from the shell's config folder. */
  getProjectSettings(path: string): Promise<ProjectSettings>;
  /** Save a project's settings through the shell. */
  saveProjectSettings(path: string, settings: SaveProjectSettingsInput): Promise<ProjectSettings>;
}

/** Tauri rejects with the Rust `Err(String)`; make it an Error. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

export function createTauriShell(): Shell {
  return {
    chooseProjectFolder: () => call<string | null>("choose_project_folder"),
    startServer: (project) => call<Credentials>("start_server", { project }),
    stopServer: (project) => call<void>("stop_server", { project: project ?? null }),
    getConnection: (project) => call<Credentials | null>("get_connection", { project: project ?? null }),
    findOrch: () => call<string>("find_orch"),
    findHerdr: () => call<string>("find_herdr"),
    getHerdrInfo: () => call<HerdrInfo>("get_herdr_info"),
    prepareHerdrFix: (action) => call<HerdrFixPlan>("prepare_herdr_fix", { action }),
    runHerdrFix: (token) => call<HerdrInfo>("run_herdr_fix", { token }),
    openHerdrUrl: () => call<void>("open_herdr_url"),
    detectAgentClis: () => call<AgentCli[]>("detect_agent_clis"),
    getProjectSettings: (path) => call<ProjectSettings>("get_project_settings", { path }),
    saveProjectSettings: (path, settings) => call<ProjectSettings>("save_project_settings", { path, settings }),
  };
}

export function detectShell(): Shell | null {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window ? createTauriShell() : null;
}
