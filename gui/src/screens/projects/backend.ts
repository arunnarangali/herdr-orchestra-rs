import { invoke } from "@tauri-apps/api/core";
import type { Credentials } from "../../api/client";
import { detectShell } from "../../app/shell";

export interface ArchiveEntry {
  name: string;
  files: string[];
  modified: number | null;
  summary: string | null;
}
export interface ProjectInfo {
  path: string;
  name: string;
  exists: boolean;
  is_git: boolean;
  branch: string | null;
  has_orchestrate: boolean;
  archive: ArchiveEntry[];
}
export interface ProjectList {
  projects: ProjectInfo[];
  active: string | null;
  /** The projects that are open (each has its own server). */
  open: string[];
  warning: string | null;
}
/** One project of the set restored at start-up: how to reach its server, or why it did not start. */
export interface RestoredProject {
  path: string;
  name: string;
  connection: Credentials | null;
  error: string | null;
}
export interface CreatedProject {
  created: { path: string; made_folder: boolean; git_initialised: boolean };
  connection: Credentials;
}
export interface AppSettings {
  theme: "system" | "light" | "dark";
  notifications: boolean;
  orch_path: string;
}

/** What the desktop shell stores for the app: its project list and settings. Absent in a browser. */
export interface AppBackend {
  chooseFolder(): Promise<string | null>;
  listProjects(): Promise<ProjectList>;
  addProject(path: string): Promise<ProjectList>;
  removeProject(path: string): Promise<ProjectList>;
  /** Make it the active project and make sure its server runs (others are not touched). */
  switchProject(path: string): Promise<Credentials>;
  /** Open a listed project: remember it as open and start its server. */
  openProject(path: string): Promise<Credentials>;
  /** Stop a failed or dead project's server and start a fresh one. */
  retryProject(path: string): Promise<Credentials>;
  /** Forget that it is open and stop its server (only its). */
  closeProject(path: string): Promise<ProjectList>;
  /** Start every project that was open last time, each on its own. */
  restoreProjects(): Promise<RestoredProject[]>;
  /** Make a new project in a folder (git repository, ready for a run), add and open it. */
  createProject(path: string): Promise<CreatedProject>;
  getSettings(): Promise<{ value: AppSettings; warning: string | null }>;
  saveSettings(settings: AppSettings): Promise<AppSettings>;
  findOrch(): Promise<string>;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

function tauriBackend(): AppBackend {
  const shell = detectShell();
  return {
    chooseFolder: () => call("choose_project_folder"),
    listProjects: () => call("list_projects"),
    addProject: (path) => call("add_project", { path }),
    removeProject: (path) => call("remove_project", { path }),
    switchProject: (path) => call("switch_project", { path }),
    openProject: (path) => call("open_project", { path }),
    retryProject: (path) => call("retry_project", { path }),
    closeProject: (path) => call("close_project", { path }),
    restoreProjects: () => call("restore_projects"),
    createProject: (path) => call("create_project", { path }),
    getSettings: () => call("get_app_settings"),
    saveSettings: (settingsIn) => call("save_app_settings", { settingsIn }),
    findOrch: () => (shell ? shell.findOrch() : Promise.reject(new Error("not in the desktop app"))),
  };
}

let override: AppBackend | null | undefined;

/** For tests: replace what `getBackend` returns (null = a plain browser). */
export function setBackendForTests(b: AppBackend | null | undefined): void {
  override = b;
}

/** The shell's storage, or null when the page runs in a plain browser. */
export function getBackend(): AppBackend | null {
  if (override !== undefined) return override;
  return detectShell() ? tauriBackend() : null;
}
