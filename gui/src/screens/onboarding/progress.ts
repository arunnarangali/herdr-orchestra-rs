const KEY = "orch.onboarding";

export type Progress = "done" | "skipped" | null;

/** Whether the first run was finished or skipped. Only this flag is stored, never anything about the project. */
export function readProgress(): Progress {
  try {
    const v = window.localStorage.getItem(KEY);
    return v === "done" || v === "skipped" ? v : null;
  } catch {
    return null;
  }
}

export function writeProgress(p: Exclude<Progress, null>): void {
  try {
    window.localStorage.setItem(KEY, p);
  } catch {
    // without storage the flow shows again next time: harmless
  }
}
