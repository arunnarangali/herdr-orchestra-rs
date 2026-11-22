import { invoke } from "@tauri-apps/api/core";

/** The shell's own copy of a project's settings (`get_project_settings`); only the Lead matters here. */
interface ProjectSettingsAnswer {
  leadAgent: string | null;
}

type Lookup = (projectPath: string) => Promise<string | null>;
let override: Lookup | undefined;

/** For tests: replace the shell lookup (null = a plain browser, which has no saved default). */
export function setDefaultLeadLookupForTests(lookup: Lookup | undefined): void {
  override = lookup;
}

/**
 * The agent the developer saved as this project's default Lead, or null when none is saved or
 * the page runs in a plain browser (no shell, so nothing is saved). Never throws: a shell that
 * cannot answer is the same as no saved default, and Chat then asks the developer to choose.
 */
export async function savedLeadAgent(projectPath: string | null): Promise<string | null> {
  if (!projectPath) return null;
  try {
    if (override) return await override(projectPath);
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return null;
    const s = await invoke<ProjectSettingsAnswer>("get_project_settings", { path: projectPath });
    return s.leadAgent?.trim() ? s.leadAgent : null;
  } catch {
    return null;
  }
}
