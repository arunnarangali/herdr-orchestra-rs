import { useState } from "react";
import { isTaskState } from "../../api/states";
import type { PlanMilestone, TaskInfo } from "../../api/types";
import { layoutGraph, wrapTitle, type GraphEdge } from "./layout";

const NODE_W = 148;
const NODE_H = 56;

/** The tone colours come from the theme, the same ones the state badges use. */
function toneOf(state: string): string {
  switch (isTaskState(state) ? state : "pending") {
    case "merged":
      return "success";
    case "failed":
    case "conflict":
      return "danger";
    case "needs-answer":
    case "needs-fix":
    case "held":
    case "paused":
      return "warning";
    case "skipped":
      return "muted";
    case "pending":
      return "neutral";
    default:
      return "info";
  }
}

function edgeLabel(e: GraphEdge): string {
  return `${e.to} depends on ${e.from}`;
}

/**
 * The milestone dependency graph as an SVG the layout module computed. Nodes are buttons
 * (keyboard reachable, Enter opens the detail); edges are focusable too and their reason is
 * shown as text under the graph, not a tooltip that needs a mouse. A plain list with the
 * same information is rendered next to it by the screen for screen readers.
 */
export function MilestoneGraph({
  milestones,
  tasks,
  onSelect,
}: {
  milestones: PlanMilestone[];
  tasks: TaskInfo[];
  onSelect: (id: string) => void;
}) {
  const states = new Map(tasks.map((t) => [t.id, t.state]));
  const layout = layoutGraph(milestones, states);
  const width = Math.max(1, ...layout.nodes.map((n) => n.x)) + NODE_W + 16;
  const height = Math.max(1, ...layout.nodes.map((n) => n.y)) + NODE_H + 16;
  const [active, setActive] = useState<string | null>(null);
  const activeEdge = layout.edges.find((e) => `${e.from}->${e.to}` === active) ?? null;
  const byId = new Map(layout.nodes.map((n) => [n.id, n]));
  const nodeLabel = (id: string) => {
    const m = milestones.find((o) => o.id === id);
    const state = states.get(id) ?? "pending";
    return `${id} ${m?.title ?? ""} (${state})`;
  };
  return (
    <div className="graph-wrap">
      <svg
        className="graph"
        viewBox={`0 0 ${width} ${height}`}
        role="group"
        aria-label="Milestone dependency graph"
        style={{ aspectRatio: `${width} / ${height}` }}
      >
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" className="graph-arrow" />
          </marker>
        </defs>
        {layout.edges.map((e) => {
          const from = byId.get(e.from);
          const to = byId.get(e.to);
          if (!from || !to) return null;
          const x1 = from.x + NODE_W;
          const y1 = from.y + NODE_H / 2;
          const x2 = to.x;
          const y2 = to.y + NODE_H / 2;
          const bend = y2 > y1 ? Math.min(y1 + 28, (y1 + y2) / 2) : Math.max(y1 - 28, (y1 + y2) / 2);
          const d =
            y1 === y2
              ? `M ${x1} ${y1} L ${x2} ${y2}`
              : `M ${x1} ${y1} C ${x1} ${bend}, ${x2} ${bend}, ${x2} ${y2}`;
          const label = edgeLabel(e);
          return (
            <path
              key={`${e.from}->${e.to}`}
              d={d}
              className={`graph-edge${active === `${e.from}->${e.to}` ? " graph-edge--active" : ""}`}
              markerEnd="url(#arrow)"
              tabIndex={0}
              aria-label={label + (e.reason ? `, because ${e.reason}` : "")}
              onMouseEnter={() => setActive(`${e.from}->${e.to}`)}
              onMouseLeave={() => setActive(null)}
              onFocus={() => setActive(`${e.from}->${e.to}`)}
              onBlur={() => setActive(null)}
            >
              <title>{e.reason ? `Because: ${e.reason}` : label}</title>
            </path>
          );
        })}
        {layout.nodes.map((n) => {
          const lines = wrapTitle(n.title, 20, 2);
          const tone = toneOf(n.state);
          return (
            <g
              key={n.id}
              className={`graph-node graph-node--${tone}${n.enabled ? "" : " graph-node--disabled"}`}
              transform={`translate(${n.x + 8}, ${n.y + 8})`}
              role="button"
              tabIndex={0}
              aria-label={nodeLabel(n.id)}
              onClick={() => onSelect(n.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(n.id);
                }
              }}
            >
              <rect width={NODE_W} height={NODE_H} rx={8} className="graph-node-box" />
              <text className="graph-node-id" x={10} y={18}>
                {n.id}
              </text>
              {lines.map((line, i) => (
                <text key={i} className="graph-node-title" x={10} y={34 + i * 15}>
                  {line}
                </text>
              ))}
            </g>
          );
        })}
      </svg>
      <p className="graph-caption" aria-live="polite">
        {activeEdge?.reason
          ? `${activeEdge.to} depends on ${activeEdge.from}: ${activeEdge.reason}`
          : activeEdge
            ? `${activeEdge.to} depends on ${activeEdge.from}`
            : "Hover or focus a line to read why a milestone depends on another."}
      </p>
    </div>
  );
}
