import { invoke } from "@tauri-apps/api/core";
import type { Credentials } from "../api/client";

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
  stopServer(): Promise<void>;
  /** The running server's address and token, or null when none was started. */
  getConnection(): Promise<Credentials | null>;
  /** Path of the `orch` binary (`ORCH_BIN`, else the PATH); rejects with a clear message. */
  findOrch(): Promise<string>;
  /** Path of the Herdr executable; rejects with a clear message. */
  findHerdr(): Promise<string>;
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
    stopServer: () => call<void>("stop_server"),
    getConnection: () => call<Credentials | null>("get_connection"),
    findOrch: () => call<string>("find_orch"),
    findHerdr: () => call<string>("find_herdr"),
  };
}

export function detectShell(): Shell | null {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window ? createTauriShell() : null;
}
