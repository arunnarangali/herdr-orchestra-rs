import { isTaskState } from "../api/states";

type Tone = "neutral" | "info" | "success" | "warning" | "danger" | "muted";

const TONES: Record<string, Tone> = {
  pending: "neutral",
  coding: "info",
  verifying: "info",
  "awaiting-review": "info",
  reviewing: "info",
  reviewed: "info",
  committed: "info",
  merged: "success",
  "needs-answer": "warning",
  "needs-fix": "warning",
  held: "warning",
  paused: "warning",
  failed: "danger",
  conflict: "danger",
  skipped: "muted",
};

export function toneOf(state: string): Tone {
  return isTaskState(state) ? TONES[state] ?? "neutral" : "neutral";
}

/** A milestone state as a label (the text always says it; colour only adds to it). */
export function StatusBadge({ state }: { state: string }) {
  return (
    <span className={`badge badge--${toneOf(state)}`} data-state={state}>
      {state}
    </span>
  );
}
