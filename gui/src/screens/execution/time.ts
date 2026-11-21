// Time helpers for the Execution Monitor's "Elapsed / Budget" line.

/** Seconds as the prototype writes them: `5m 12s`, `1h 04m`. */
export function clock(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return `${m}m ${String(r).padStart(2, "0")}s`;
}

/**
 * The plan's `budget:` header in seconds (`60m`, `2h`, `1h30m`, `90 min`, `45`), or null when it
 * is missing or not a duration (a budget in money or tokens is shown as written, never guessed).
 */
export function budgetSeconds(budget: string | null | undefined): number | null {
  if (!budget) return null;
  const text = budget.trim().toLowerCase();
  if (/^\d+(\.\d+)?$/.test(text)) return Math.round(Number(text) * 60);
  const re = /(\d+(?:\.\d+)?)\s*(h|hr|hrs|hours?|m|min|mins|minutes?|s|sec|secs|seconds?)\b/g;
  let total = 0;
  let matched = "";
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const n = Number(m[1]);
    const unit = m[2]![0];
    total += unit === "h" ? n * 3600 : unit === "m" ? n * 60 : n;
    matched += m[0];
  }
  // Every non-space character must belong to a duration part.
  if (!matched || matched.replace(/\s/g, "").length !== text.replace(/\s/g, "").length) return null;
  return Math.round(total);
}

/** Unix seconds (or milliseconds) as a local `HH:MM:SS`. */
export function timeOfDay(ts: number): string {
  const d = new Date(ts > 1e11 ? ts : ts * 1000);
  if (Number.isNaN(d.getTime())) return "–";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}
