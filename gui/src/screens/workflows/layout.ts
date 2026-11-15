import type { PlanMilestone } from "../../api/types";

/**
 * The milestone graph's layout, written by hand (no graph library): a layered layout with
 * longest-path ranks, a crossing-reducing order inside each rank, and centred rows so
 * parallel milestones sit side by side. Pure arithmetic, unit-tested; `Graph` only draws it.
 */

export const COL_WIDTH = 168;
export const ROW_HEIGHT = 92;

export interface GraphNode {
  id: string;
  title: string;
  /** The milestone's current state (`pending`, `merged`, …), empty when unknown. */
  state: string;
  /** Same rank = same row of the drawing. */
  rank: number;
  x: number;
  y: number;
  enabled: boolean;
}

export interface GraphEdge {
  from: string;
  to: string;
  /** Why the dependency is there, from the plan (`Depends on: M1 (needs the frame)`). */
  reason: string | null;
}

export interface GraphLayout {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** Longest-path rank: a milestone sits one row below its worst dependency. */
function ranks(milestones: PlanMilestone[]): Map<string, number> {
  const rank = new Map<string, number>();
  const onStack = new Set<string>();
  const visit = (m: PlanMilestone): number => {
    const known = rank.get(m.id);
    if (known !== undefined) return known;
    if (onStack.has(m.id)) return -1; // a cycle: ignore this edge, the plan is a DAG
    onStack.add(m.id);
    const deps = m.depends_on
      .map((d) => milestones.find((o) => o.id === d.id))
      .filter((o) => o !== undefined);
    let worst = -1;
    for (const d of deps) worst = Math.max(worst, visit(d));
    onStack.delete(m.id);
    const r = worst + 1;
    rank.set(m.id, r);
    return r;
  };
  for (const m of milestones) visit(m);
  return rank;
}

/**
 * Order inside each rank by repeated barycentre sweeps: a node goes towards the average
 * column of its neighbours, so parallel milestones line up instead of crossing.
 */
function orderInRanks(
  milestones: PlanMilestone[],
  rank: Map<string, number>,
  rows: number,
): string[][] {
  let columns: string[][] = Array.from({ length: rows }, () => []);
  for (const m of milestones) columns[rank.get(m.id) ?? 0]?.push(m.id);
  const colOf = new Map<string, number>();
  const reindex = () => {
    colOf.clear();
    for (const row of columns) row.forEach((id, i) => colOf.set(id, i));
  };
  reindex();
  const depsOf = new Map(milestones.map((m) => [m.id, m.depends_on.map((d) => d.id)]));
  const dependantsOf = new Map<string, string[]>(milestones.map((m) => [m.id, []]));
  for (const m of milestones)
    for (const d of m.depends_on) dependantsOf.get(d.id)?.push(m.id);
  const sweep = (pick: (m: PlanMilestone) => string[], down: boolean) => {
    const order = down ? milestones : [...milestones].reverse();
    for (const m of order) {
      const neighbours = pick(m);
      if (neighbours.length === 0) continue;
      const positions = neighbours
        .map((n) => colOf.get(n))
        .filter((p): p is number => p !== undefined);
      if (positions.length === 0) continue;
      const want = positions.reduce((a, b) => a + b, 0) / positions.length;
      const row = columns[rank.get(m.id) ?? 0];
      if (!row) continue;
      const from = colOf.get(m.id);
      if (from === undefined || row.length < 2) continue;
      const to = Math.max(0, Math.min(row.length - 1, Math.round(want)));
      if (to === from) continue;
      row.splice(from, 1);
      row.splice(to, 0, m.id);
      reindex();
    }
  };
  for (let i = 0; i < 4; i++) {
    sweep((m) => depsOf.get(m.id) ?? [], true);
    sweep((m) => dependantsOf.get(m.id) ?? [], false);
  }
  return columns;
}

/**
 * Lay the plan's milestones out: one row per rank, rows centred, columns left to right in
 * the crossing-reduced order. Edges keep the plan's dependency reasons.
 */
export function layoutGraph(milestones: PlanMilestone[], states: Map<string, string>): GraphLayout {
  const rank = ranks(milestones);
  const rows = milestones.length === 0 ? 0 : Math.max(...rank.values()) + 1;
  const columns = orderInRanks(milestones, rank, rows);
  const widest = Math.max(1, ...columns.map((c) => c.length));
  const nodes: GraphNode[] = [];
  for (const row of columns) {
    const offset = ((widest - row.length) * COL_WIDTH) / 2;
    row.forEach((id, i) => {
      const m = milestones.find((o) => o.id === id);
      if (!m) return;
      const r = rank.get(id) ?? 0;
      nodes.push({
        id,
        title: m.title,
        state: states.get(id) ?? "",
        rank: r,
        x: offset + i * COL_WIDTH,
        y: r * ROW_HEIGHT,
        enabled: m.enabled,
      });
    });
  }
  const edges: GraphEdge[] = [];
  for (const m of milestones)
    for (const d of m.depends_on)
      if (milestones.some((o) => o.id === d.id)) edges.push({ from: d.id, to: m.id, reason: d.reason });
  return { nodes, edges };
}

/** Break a title into short lines that fit inside a graph node (a hand-rolled wrap). */
export function wrapTitle(title: string, maxChars = 20, maxLines = 3): string[] {
  const words = title.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [""];
  const lines: string[] = [];
  let line = "";
  let i = 0;
  while (i < words.length) {
    const word = words[i] ?? "";
    const next = line ? `${line} ${word}` : word;
    if (next.length <= maxChars || !line) {
      line = next;
      i++;
    } else {
      lines.push(line);
      line = "";
      if (lines.length === maxLines) break; // word i and the rest did not fit
    }
  }
  let leftover = "";
  if (i < words.length) {
    leftover = words.slice(i).filter(Boolean).join(" ");
  } else if (line && lines.length < maxLines) {
    lines.push(line);
  } else if (line) {
    leftover = line;
  }
  if (leftover) {
    const combined = `${lines[lines.length - 1] ?? ""} ${leftover}`.trim();
    const cut = combined.length > maxChars ? combined.slice(0, Math.max(0, maxChars - 1)) : combined;
    lines[lines.length - 1] = `${cut.trim()}…`;
  }
  return lines;
}
