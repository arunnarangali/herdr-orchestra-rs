/** Milestone states, in the order `orch status` lists them. */
export const TASK_STATES = [
  "pending",
  "coding",
  "verifying",
  "awaiting-review",
  "reviewing",
  "reviewed",
  "committed",
  "merged",
  "needs-answer",
  "needs-fix",
  "held",
  "paused",
  "failed",
  "conflict",
  "skipped",
] as const;
export type TaskState = (typeof TASK_STATES)[number];

export function isTaskState(value: string): value is TaskState {
  return (TASK_STATES as readonly string[]).includes(value);
}
