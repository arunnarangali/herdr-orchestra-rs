import { invoke } from "@tauri-apps/api/core";

/** `yes`, `no` (with the reason) or `unknown` (this build of the shell cannot say). */
export type GitCheck = { kind: "yes" } | { kind: "no"; reason: string } | { kind: "unknown" };

/**
 * Is `path` a git repository? The shell's project list refuses folders that are not (its
 * `add_project` command), and remembering the project is wanted anyway. A shell without that
 * command cannot say, which is reported as unknown rather than guessed.
 */
async function viaShell(path: string): Promise<GitCheck> {
  try {
    await invoke("add_project", { path });
    return { kind: "yes" };
  } catch (e) {
    const text = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
    if (/not a git repository/i.test(text)) return { kind: "no", reason: text };
    if (/not found|unknown command|not allowed/i.test(text)) return { kind: "unknown" };
    return { kind: "no", reason: text };
  }
}

let override: ((path: string) => Promise<GitCheck>) | undefined;

/** For tests. */
export function setGitCheckForTests(f: ((path: string) => Promise<GitCheck>) | undefined): void {
  override = f;
}

export function checkGitRepository(path: string): Promise<GitCheck> {
  return (override ?? viaShell)(path);
}
