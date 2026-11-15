/** Unix seconds as a local date and time. */
export function formatTime(ts: number | null | undefined): string {
  return ts === null || ts === undefined ? "–" : new Date(ts * 1000).toLocaleString();
}

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

const RUN_STATES: Record<string, string> = {
  running: "Running",
  idle: "Idle",
  all_done: "All done",
  stopped: "Stopped",
  broken_branch: "Broken branch",
};
export function runStateLabel(s: string): string {
  return RUN_STATES[s] ?? s;
}
