import { useMemo, useState } from "react";
import type { PlanMilestone, TaskInfo } from "../../api/types";
import { WfIcon } from "./icons";

interface EdgeCoord {
  from: string;
  to: string;
  d: string;
  polygon: string;
  hot: boolean;
  reason?: string | null;
}

const TIER_TONE: Record<string, string> = {
  small: "neutral",
  medium: "amber",
  high: "coral",
};

const STATE_CONFIG: Record<string, { label: string; tone: string }> = {
  merged: { label: "Merged", tone: "emerald" },
  coding: { label: "Coding", tone: "cyan" },
  verifying: { label: "Verifying", tone: "amber" },
  failed: { label: "Failed", tone: "coral" },
  pending: { label: "Pending", tone: "neutral" },
};

function stateBadge(state: string) {
  const st = STATE_CONFIG[state] ?? STATE_CONFIG["pending"]!;
  const cls = st.tone === "neutral" ? "badge" : `badge badge--${st.tone}`;
  return (
    <span className={cls}>
      {st.label}
      {state === "merged" ? " ✓" : ""}
    </span>
  );
}

function stateIcon(state: string, small?: boolean) {
  const iconName =
    state === "merged"
      ? "check"
      : state === "coding" || state === "verifying"
        ? "spinner"
        : state === "failed"
          ? "warning"
          : "circle";
  const label = (STATE_CONFIG[state] ?? STATE_CONFIG["pending"]!).label;
  return (
    <span
      className={`sicon sicon--${state}${small ? " sicon--sm" : ""}`}
      role="img"
      aria-label={label}
    >
      <WfIcon
        name={iconName}
        size={small ? 14 : 18}
        className={iconName === "spinner" ? "spin" : ""}
      />
    </span>
  );
}

function tierBadge(tier: string | null) {
  if (!tier) return null;
  const tone = TIER_TONE[tier] ?? "neutral";
  const cls = tone === "neutral" ? "badge" : `badge badge--${tone}`;
  return <span className={cls}>Tier: {tier}</span>;
}

export function MilestoneGraph({
  milestones,
  tasks,
  selected,
  onSelect,
  showDeps = true,
}: {
  milestones: PlanMilestone[];
  tasks: TaskInfo[];
  selected: string | null;
  onSelect: (id: string) => void;
  showDeps?: boolean;
}) {
  const [activeEdge, setActiveEdge] = useState<EdgeCoord | null>(null);

  const states = useMemo(() => new Map(tasks.map((t) => [t.id, t.state])), [tasks]);
  const workers = useMemo(
    () => new Map(tasks.map((t) => [t.id, t.worker ?? t.last_worker ?? null])),
    [tasks],
  );

  // Group milestones into columns by depth
  const cols = useMemo(() => {
    const memo: Record<string, number> = {};
    function depthOf(m: PlanMilestone): number {
      if (memo[m.id] !== undefined) return memo[m.id]!;
      memo[m.id] = 0;
      if (m.depends_on.length > 0) {
        let maxDep = 0;
        for (const d of m.depends_on) {
          const found = milestones.find((x) => x.id === d.id);
          if (found) maxDep = Math.max(maxDep, depthOf(found));
        }
        memo[m.id] = 1 + maxDep;
      }
      return memo[m.id]!;
    }

    const result: PlanMilestone[][] = [];
    milestones.forEach((m) => {
      const d = depthOf(m);
      if (!result[d]) result[d] = [];
      result[d]!.push(m);
    });
    return result;
  }, [milestones]);

  // Compute elbow connector edges from the column layout positions
  const edges = useMemo(() => {
    if (!showDeps || milestones.length === 0) return [];

    const nodePos = new Map<string, { l: number; r: number; y: number }>();
    cols.forEach((col, c) => {
      const colLeft = c * (250 + 64) + 28;
      const colRight = colLeft + 250;
      col.forEach((m, r) => {
        const centerY = 22 + r * (72 + 14) + 36;
        nodePos.set(m.id, { l: colLeft, r: colRight, y: centerY });
      });
    });

    const result: EdgeCoord[] = [];
    milestones.forEach((m) => {
      const to = nodePos.get(m.id);
      m.depends_on.forEach((dep) => {
        const from = nodePos.get(dep.id);
        if (!from || !to) return;
        const hot = m.id === selected || dep.id === selected;
        const x1 = from.r;
        const y1 = from.y;
        const x2 = to.l - 8;
        const y2 = to.y;
        const xm = x1 + (to.l - x1) / 2;
        const dy = y2 - y1;
        let d: string;
        if (Math.abs(dy) < 2) {
          d = `M${x1} ${y1} H${x2}`;
        } else {
          const r = Math.min(10, Math.abs(dy) / 2);
          const k = Math.sign(dy);
          d = `M${x1} ${y1} H${xm - r} Q${xm} ${y1} ${xm} ${y1 + k * r} V${y2 - k * r} Q${xm} ${y2} ${xm + r} ${y2} H${x2}`;
        }
        const polygon = `${x2},${y2 - 5} ${x2 + 8},${y2} ${x2},${y2 + 5}`;
        result.push({ from: dep.id, to: m.id, d, polygon, hot, reason: dep.reason });
      });
    });
    return result;
  }, [milestones, cols, selected, showDeps]);

  return (
    <div
      className="dag__body"
      id="dag-body"
      role="group"
      aria-label="Milestone dependency graph"
    >
      <svg className="dag__edges" aria-hidden="true" focusable="false">
        {showDeps &&
          edges.map((e) => {
            const label = `${e.to} depends on ${e.from}${e.reason ? `, because ${e.reason}` : ""}`;
            return (
              <g key={`${e.from}->${e.to}`}>
                <path
                  className={e.hot || activeEdge === e ? "on" : ""}
                  d={e.d}
                  tabIndex={0}
                  aria-label={label}
                  onFocus={() => setActiveEdge(e)}
                  onBlur={() => setActiveEdge(null)}
                  onMouseEnter={() => setActiveEdge(e)}
                  onMouseLeave={() => setActiveEdge(null)}
                >
                  <title>{e.reason ? `Because: ${e.reason}` : `${e.to} depends on ${e.from}`}</title>
                </path>
                <polygon className={e.hot || activeEdge === e ? "on" : ""} points={e.polygon} />
              </g>
            );
          })}
      </svg>
      <div className="dag__cols">
        {cols.map((col, ci) => (
          <div key={ci} className="dag__col">
            {col.map((m) => {
              const state = states.get(m.id) ?? "pending";
              const worker = workers.get(m.id);
              return (
                <button
                  key={m.id}
                  type="button"
                  className={`dnode dnode--${state}${m.enabled ? "" : " dnode--excluded"}`}
                  aria-pressed={selected === m.id}
                  aria-label={`${m.id} ${m.title} (${state})`}
                  data-id={m.id}
                  onClick={() => onSelect(m.id)}
                >
                  {stateIcon(state)}
                  <span>
                    <span className="dnode__title">{`${m.id}: ${m.title}`}</span>
                    <span className="dnode__badges">
                      {stateBadge(state)}
                      {worker && (
                        <span className="badge badge--mono">
                          <WfIcon name="user" size={12} />
                          {worker}
                        </span>
                      )}
                      {tierBadge(m.tier)}
                      {!m.enabled && <span className="badge">Excluded</span>}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
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
