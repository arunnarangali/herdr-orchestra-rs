/** Unix seconds as a local date and time. */
export function formatTime(ts: number | null | undefined): string {
  return ts === null || ts === undefined ? "–" : new Date(ts * 1000).toLocaleString();
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "–";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  if (m < 60) return `${m}m ${Math.round(seconds % 60)}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** `needs_answer` and `needs-answer` are the same kind. */
export function kindKey(kind: string): string {
  return kind.replace(/_/g, "-").toLowerCase();
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
